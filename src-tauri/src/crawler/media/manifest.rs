//! Parses HLS and static DASH into ordered, download-ready parts.

use std::collections::HashMap;

use serde::Deserialize;
use url::Url;

use super::Format;
use crate::crawler::{Error, Result, http_url};

const MAX_PARTS: usize = 100_000;

#[derive(Clone)]
pub(super) struct Encryption {
    pub key: Url,
    pub iv: [u8; 16],
}

#[derive(Clone)]
pub(super) struct Segment {
    pub url: Url,
    pub range: Option<(u64, u64)>,
    pub encryption: Option<Encryption>,
}

pub(super) struct Variant {
    pub format: Format,
    pub url: Option<Url>,
    pub segments: Vec<Segment>,
}

#[derive(Default)]
pub(super) struct Playlist {
    pub variants: Vec<Variant>,
    pub segments: Vec<Segment>,
}

/// Interpret playlist directives with segment-local key, sequence, and range state.
pub(super) fn hls(bytes: &[u8], base: &Url) -> Result<Playlist> {
    let data = std::str::from_utf8(bytes).map_err(|_| Error::InvalidManifest)?;
    let mut lines = data.trim_start_matches('\u{feff}').lines().map(str::trim);
    if lines.next() != Some("#EXTM3U") {
        return Err(Error::InvalidManifest);
    }
    let mut playlist = Playlist::default();
    let mut variant = None;
    let mut sequence = 0_u64;
    let mut key = None;
    let mut byte_range = None;
    let mut previous_range: Option<(Url, u64)> = None;
    for line in lines.filter(|line| !line.is_empty()) {
        if let Some(value) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            variant = Some(attributes(value));
        } else if let Some(value) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            sequence = value.parse().map_err(|_| Error::InvalidManifest)?;
        } else if let Some(value) = line.strip_prefix("#EXT-X-KEY:") {
            let values = attributes(value);
            key = match values.get("METHOD").copied() {
                Some("NONE") => None,
                Some("AES-128")
                    if values
                        .get("KEYFORMAT")
                        .is_none_or(|format| *format == "identity") =>
                {
                    let url = join(base, values.get("URI").ok_or(Error::InvalidManifest)?)?;
                    let iv = values.get("IV").map(|iv| parse_iv(iv)).transpose()?;
                    Some((url, iv))
                }
                _ => return Err(Error::UnsupportedMedia),
            };
        } else if let Some(value) = line.strip_prefix("#EXT-X-BYTERANGE:") {
            byte_range = Some(value);
        } else if line.starts_with("#EXT-X-MAP:") {
            return Err(Error::UnsupportedMedia);
        } else if !line.starts_with('#') {
            let url = join(base, line)?;
            if let Some(attributes) = variant.take() {
                let bandwidth = attributes
                    .get("BANDWIDTH")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or_default();
                let id = playlist.variants.len().to_string();
                playlist.variants.push(Variant {
                    format: Format { id, bandwidth },
                    url: Some(url),
                    segments: Vec::new(),
                });
                continue;
            }
            let range = byte_range
                .take()
                .map(|value| {
                    let (length, offset) = value
                        .split_once('@')
                        .map_or((value, None), |(length, offset)| (length, Some(offset)));
                    let length = length
                        .parse::<u64>()
                        .ok()
                        .filter(|length| *length > 0)
                        .ok_or(Error::InvalidManifest)?;
                    let start = match offset {
                        Some(offset) => {
                            offset.parse::<u64>().map_err(|_| Error::InvalidManifest)?
                        }
                        None => previous_range
                            .as_ref()
                            .filter(|(previous, _)| previous == &url)
                            .and_then(|(_, end)| end.checked_add(1))
                            .ok_or(Error::InvalidManifest)?,
                    };
                    Ok((
                        start,
                        start
                            .checked_add(length - 1)
                            .ok_or(Error::InvalidManifest)?,
                    ))
                })
                .transpose()?;
            previous_range = range.map(|(_, end)| (url.clone(), end));
            let encryption = key.as_ref().map(|(key, iv)| Encryption {
                key: key.clone(),
                iv: iv.unwrap_or_else(|| u128::from(sequence).to_be_bytes()),
            });
            playlist.segments.push(Segment {
                url,
                range,
                encryption,
            });
            sequence = sequence.checked_add(1).ok_or(Error::InvalidManifest)?;
            if playlist.segments.len() > MAX_PARTS {
                return Err(Error::InvalidManifest);
            }
        }
    }
    if variant.is_some() || (playlist.variants.is_empty() && playlist.segments.is_empty()) {
        return Err(Error::InvalidManifest);
    }
    Ok(playlist)
}

/// Split attributes outside double quotes so codes and URLs may contain commas.
fn attributes(line: &str) -> HashMap<&str, &str> {
    let mut fields = HashMap::new();
    let mut quoted = false;
    let mut start = 0;
    for (index, character) in line
        .char_indices()
        .chain(std::iter::once((line.len(), ',')))
    {
        if character == '"' {
            quoted = !quoted;
        }
        if character == ',' && !quoted {
            if let Some((key, value)) = line[start..index].split_once('=') {
                fields.insert(key.trim(), value.trim().trim_matches('"'));
            }
            start = index + 1;
        }
    }
    fields
}

fn parse_iv(value: &str) -> Result<[u8; 16]> {
    u128::from_str_radix(
        value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
            .ok_or(Error::InvalidManifest)?,
        16,
    )
    .map(u128::to_be_bytes)
    .map_err(|_| Error::InvalidManifest)
}

/// Deserialize only the DASH XML structures used by the current downloader.
#[derive(Default, Deserialize)]
#[serde(default)]
struct Mpd {
    #[serde(rename = "@type")]
    kind: String,
    #[serde(rename = "@mediaPresentationDuration")]
    duration: String,
    #[serde(rename = "BaseURL")]
    base: String,
    #[serde(rename = "Period")]
    periods: Vec<Period>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Period {
    #[serde(rename = "@duration")]
    duration: String,
    #[serde(rename = "BaseURL")]
    base: String,
    #[serde(rename = "AdaptationSet")]
    adaptations: Vec<Adaptation>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Adaptation {
    #[serde(rename = "@mimeType")]
    mime: String,
    #[serde(rename = "@contentType")]
    content: String,
    #[serde(rename = "BaseURL")]
    base: String,
    #[serde(rename = "SegmentTemplate")]
    template: Option<Template>,
    #[serde(rename = "SegmentList")]
    list: Option<SegmentList>,
    #[serde(rename = "ContentProtection")]
    protected: Vec<Protection>,
    #[serde(rename = "Representation")]
    representations: Vec<Representation>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Representation {
    #[serde(rename = "@id")]
    id: String,
    #[serde(rename = "@bandwidth")]
    bandwidth: u64,
    #[serde(rename = "@mimeType")]
    mime: String,
    #[serde(rename = "BaseURL")]
    base: String,
    #[serde(rename = "SegmentTemplate")]
    template: Option<Template>,
    #[serde(rename = "SegmentList")]
    list: Option<SegmentList>,
    #[serde(rename = "ContentProtection")]
    protected: Vec<Protection>,
}

#[derive(Deserialize)]
struct Protection {}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Template {
    #[serde(rename = "@initialization")]
    initialization: String,
    #[serde(rename = "@media")]
    media: String,
    #[serde(rename = "@startNumber")]
    start: Option<u64>,
    #[serde(rename = "@duration")]
    duration: Option<u64>,
    #[serde(rename = "@timescale")]
    timescale: Option<u64>,
    #[serde(rename = "@presentationTimeOffset")]
    offset: Option<u64>,
    #[serde(rename = "SegmentTimeline")]
    timeline: Option<Timeline>,
}

#[derive(Deserialize)]
struct Timeline {
    #[serde(rename = "S")]
    entries: Vec<TimelineEntry>,
}

#[derive(Deserialize)]
struct TimelineEntry {
    #[serde(rename = "@t")]
    time: Option<u64>,
    #[serde(rename = "@d")]
    duration: u64,
    #[serde(rename = "@r", default)]
    repeat: i64,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct SegmentList {
    #[serde(rename = "Initialization")]
    initialization: Option<SegmentAddress>,
    #[serde(rename = "SegmentURL")]
    media: Vec<SegmentAddress>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct SegmentAddress {
    #[serde(rename = "@sourceURL", alias = "@media")]
    url: String,
    #[serde(rename = "@range", alias = "@mediaRange")]
    range: String,
}

/// Resolve the MPD hierarchy and retain selectable representations from video adaptations.
pub(super) fn dash(bytes: &[u8], base: &Url) -> Result<Vec<Variant>> {
    let data = std::str::from_utf8(bytes).map_err(|_| Error::InvalidManifest)?;
    let mut reader = quick_xml::Reader::from_str(data);
    loop {
        use quick_xml::events::Event;
        match reader.read_event().map_err(|_| Error::InvalidManifest)? {
            Event::Start(name) | Event::Empty(name) if name.local_name().as_ref() == "MPD" => break,
            Event::Start(_) | Event::Empty(_) | Event::DocType(_) | Event::Eof => {
                return Err(Error::InvalidManifest);
            }
            _ => {}
        }
    }
    let mpd: Mpd = quick_xml::de::from_str(data).map_err(|_| Error::InvalidManifest)?;
    if mpd.kind == "dynamic" {
        return Err(Error::UnsupportedMedia);
    }
    let base = join(base, &mpd.base)?;
    let mut formats = Vec::new();
    let period_count = mpd.periods.len();
    for period in mpd.periods {
        let period_base = join(&base, &period.base)?;
        let period_duration = duration(&period.duration).or_else(|| {
            (period_count == 1)
                .then(|| duration(&mpd.duration))
                .flatten()
        });
        for set in period.adaptations {
            let set_base = join(&period_base, &set.base)?;
            for track in set.representations {
                let mime = if track.mime.is_empty() {
                    &set.mime
                } else {
                    &track.mime
                };
                if !mime.starts_with("video/") && set.content != "video" {
                    continue;
                }
                if !set.protected.is_empty() || !track.protected.is_empty() {
                    continue;
                }
                let track_base = join(&set_base, &track.base)?;
                let segments =
                    if let Some(template) = track.template.as_ref().or(set.template.as_ref()) {
                        template_segments(
                            template,
                            &track_base,
                            &track.id,
                            track.bandwidth,
                            period_duration,
                        )?
                    } else if let Some(list) = track.list.as_ref().or(set.list.as_ref()) {
                        list.initialization
                            .iter()
                            .chain(&list.media)
                            .map(|address| {
                                Ok(Segment {
                                    url: join(&track_base, &address.url)?,
                                    range: dash_range(&address.range)?,
                                    encryption: None,
                                })
                            })
                            .collect::<Result<Vec<_>>>()?
                    } else if !track.base.is_empty() {
                        vec![Segment {
                            url: track_base,
                            range: None,
                            encryption: None,
                        }]
                    } else {
                        continue;
                    };
                if !segments.is_empty() && segments.len() <= MAX_PARTS {
                    formats.push(Variant {
                        format: Format {
                            id: track.id,
                            bandwidth: track.bandwidth,
                        },
                        url: None,
                        segments,
                    });
                }
            }
        }
    }
    if formats.is_empty() {
        return Err(Error::UnsupportedMedia);
    }
    Ok(formats)
}

/// Expand explicit timelines and numbered templates while bounding segment counts.
fn template_segments(
    template: &Template,
    base: &Url,
    id: &str,
    bandwidth: u64,
    seconds: Option<f64>,
) -> Result<Vec<Segment>> {
    if template.media.is_empty() {
        return Err(Error::InvalidManifest);
    }
    let start = template.start.unwrap_or(1);
    let offset = template.offset.unwrap_or(0);
    let timescale = template.timescale.unwrap_or(1);
    if timescale == 0 {
        return Err(Error::InvalidManifest);
    }
    let end =
        seconds.map(|seconds| offset.saturating_add((seconds * timescale as f64).ceil() as u64));
    let mut parts = Vec::new();
    let to_part = |pattern: &str, number, time| -> Result<Segment> {
        Ok(Segment {
            url: join(base, &substitute(pattern, id, bandwidth, number, time)?)?,
            range: None,
            encryption: None,
        })
    };
    if !template.initialization.is_empty() {
        parts.push(to_part(&template.initialization, start, offset)?);
    }
    if let Some(timeline) = &template.timeline {
        let mut time = offset;
        let mut number = start;
        for (index, entry) in timeline.entries.iter().enumerate() {
            if entry.duration == 0 {
                return Err(Error::InvalidManifest);
            }
            time = entry.time.unwrap_or(time);
            let count = if entry.repeat >= 0 {
                entry.repeat as u64 + 1
            } else {
                let limit = timeline
                    .entries
                    .get(index + 1)
                    .and_then(|next| next.time)
                    .or(end)
                    .ok_or(Error::InvalidManifest)?;
                limit
                    .checked_sub(time)
                    .ok_or(Error::InvalidManifest)?
                    .div_ceil(entry.duration)
            };
            if count > (MAX_PARTS - parts.len()) as u64 {
                return Err(Error::InvalidManifest);
            }
            for _ in 0..count {
                parts.push(to_part(&template.media, number, time)?);
                time = time
                    .checked_add(entry.duration)
                    .ok_or(Error::InvalidManifest)?;
                number = number.checked_add(1).ok_or(Error::InvalidManifest)?;
            }
        }
    } else {
        let duration = template
            .duration
            .filter(|value| *value > 0)
            .ok_or(Error::InvalidManifest)?;
        let count = end
            .ok_or(Error::InvalidManifest)?
            .saturating_sub(offset)
            .div_ceil(duration);
        if count == 0 || count > (MAX_PARTS - parts.len()) as u64 {
            return Err(Error::InvalidManifest);
        }
        for index in 0..count {
            let number = start.checked_add(index).ok_or(Error::InvalidManifest)?;
            let time = index
                .checked_mul(duration)
                .and_then(|time| offset.checked_add(time))
                .ok_or(Error::InvalidManifest)?;
            parts.push(to_part(&template.media, number, time)?);
        }
    }
    Ok(parts)
}

fn substitute(pattern: &str, id: &str, bandwidth: u64, number: u64, time: u64) -> Result<String> {
    let mut output = String::new();
    let mut parts = pattern.split('$');
    output.push_str(parts.next().unwrap_or_default());
    while let Some(token) = parts.next() {
        let suffix = parts.next().ok_or(Error::InvalidManifest)?;
        let (name, width) = if let Some((name, width)) = token.split_once("%0") {
            let width = width
                .strip_suffix('d')
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|value| *value <= 20)
                .ok_or(Error::InvalidManifest)?;
            (name, width)
        } else {
            (token, 0)
        };
        match name {
            "" => output.push('$'),
            "RepresentationID" => output.push_str(id),
            "Bandwidth" => output.push_str(&format!("{bandwidth:0width$}")),
            "Number" => output.push_str(&format!("{number:0width$}")),
            "Time" => output.push_str(&format!("{time:0width$}")),
            _ => return Err(Error::InvalidManifest),
        }
        output.push_str(suffix);
    }
    Ok(output)
}

fn duration(value: &str) -> Option<f64> {
    let mut rest = value.strip_prefix("PT")?;
    let mut seconds = 0.0;
    for (unit, scale) in [('H', 3600.0), ('M', 60.0), ('S', 1.0)] {
        if let Some((value, tail)) = rest.split_once(unit) {
            seconds += value.parse::<f64>().ok()? * scale;
            rest = tail;
        }
    }
    (rest.is_empty() && seconds.is_finite() && seconds > 0.0).then_some(seconds)
}

fn dash_range(value: &str) -> Result<Option<(u64, u64)>> {
    if value.is_empty() {
        return Ok(None);
    }
    let (start, end) = value.split_once('-').ok_or(Error::InvalidManifest)?;
    let start = start.parse().map_err(|_| Error::InvalidManifest)?;
    let end = end.parse().map_err(|_| Error::InvalidManifest)?;
    if start > end {
        return Err(Error::InvalidManifest);
    }
    Ok(Some((start, end)))
}

fn join(base: &Url, value: &str) -> Result<Url> {
    let url = base
        .join(value.trim())
        .map_err(|_| Error::InvalidManifest)?;
    http_url(url.as_str()).map_err(|_| Error::InvalidManifest)
}
