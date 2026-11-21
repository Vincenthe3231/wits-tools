//! Validates Markdown paths, tracks file fingerprints, and stages atomic writes.

use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use tempfile::NamedTempFile;

use super::{EditorError, EditorResult, FileVersion, OpenDocument};

// Keep the same bounded UTF-8 reads as WitsFlow's file editor.
const MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;

pub(super) fn name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".into())
}

fn validate_extension(path: &Path) -> EditorResult<()> {
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"))
    {
        return Ok(());
    }

    Err(EditorError::new(
        "extension",
        "Choose a .md or .markdown file.",
    ))
}

pub(super) fn validate_size(markdown: &str) -> EditorResult<()> {
    if markdown.len() <= MAX_TEXT_BYTES {
        return Ok(());
    }

    Err(EditorError::new(
        "size",
        "Markdown files can be up to 8 MiB.",
    ))
}

fn read_bytes(path: &Path) -> EditorResult<Vec<u8>> {
    let file = File::open(path).map_err(EditorError::io)?;
    if !file.metadata().map_err(EditorError::io)?.is_file() {
        return Err(EditorError::new("fileType", "Choose a regular file."));
    }

    let mut bytes = Vec::new();
    file.take((MAX_TEXT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(EditorError::io)?;
    if bytes.len() > MAX_TEXT_BYTES {
        return Err(EditorError::new(
            "size",
            "Markdown files can be up to 8 MiB.",
        ));
    }

    Ok(bytes)
}

/// Load the original text and fingerprint the same bytes for later conflict detection.
pub(super) fn open(path: &Path) -> EditorResult<(OpenDocument, FileVersion)> {
    validate_extension(path)?;
    let path = path.canonicalize().map_err(EditorError::io)?;
    let bytes = read_bytes(&path)?;
    let fingerprint = blake3::hash(&bytes).to_hex().to_string();
    let markdown = String::from_utf8(bytes)
        .map_err(|_| EditorError::new("encoding", "The selected file is not UTF-8 Markdown."))?;

    Ok((
        OpenDocument {
            file_backed: true,
            name: name(&path),
            markdown,
        },
        FileVersion { path, fingerprint },
    ))
}

/// Resolve the destination and check the original file version before replacing it.
pub(super) fn save(
    selected: Option<&Path>,
    original: Option<&FileVersion>,
    markdown: &str,
    prepare_assets: impl FnOnce(&Path) -> EditorResult<()>,
) -> EditorResult<FileVersion> {
    validate_size(markdown)?;
    let path = match selected {
        Some(path) => save_destination(path)?,
        None => original
            .map(|file| file.path.clone())
            .ok_or_else(|| EditorError::new("destination", "Choose a location with Save As."))?,
    };

    let same_file = original.filter(|file| file.path == path);
    let overwrite = same_file.is_some()
        || match selected {
            Some(selected) if selected.extension().is_some() => {
                path.try_exists().map_err(EditorError::io)?
            }
            _ => false,
        };
    let staged = stage(&path, markdown.as_bytes())?;
    if let Some(file) = same_file {
        // Check the bytes immediately before replacement, even when Save As picks the same file.
        let current = read_bytes(&path).ok();
        if path.canonicalize().ok().as_ref() != Some(&file.path)
            || !current
                .as_ref()
                .is_some_and(|bytes| blake3::hash(bytes).to_hex().as_str() == file.fingerprint)
        {
            return Err(EditorError::new(
                "externalChange",
                "The original file changed or moved. Use Save As to keep your edits in another file.",
            ));
        }
        if current.as_deref() == Some(markdown.as_bytes()) {
            return Ok(file.clone());
        }
    }

    // Publish attachments before making the Markdown visible at its destination.
    prepare_assets(path.parent().ok_or_else(invalid_destination)?)?;
    replace(staged, &path, overwrite)?;
    Ok(FileVersion {
        path,
        fingerprint: blake3::hash(markdown.as_bytes()).to_hex().to_string(),
    })
}

fn save_destination(path: &Path) -> EditorResult<PathBuf> {
    let path = if path.extension().is_none() {
        path.with_extension("md")
    } else {
        path.to_path_buf()
    };
    validate_extension(&path)?;
    if path.try_exists().map_err(EditorError::io)? {
        return path.canonicalize().map_err(EditorError::io);
    }

    let parent = path.parent().ok_or_else(invalid_destination)?;
    let name = path.file_name().ok_or_else(invalid_destination)?;
    Ok(parent.canonicalize().map_err(EditorError::io)?.join(name))
}

fn invalid_destination() -> EditorError {
    EditorError::new("destination", "Choose a valid Markdown file location.")
}

// Adapt WitsFlow's same-directory staging and flush before atomic replacement.
fn stage(path: &Path, bytes: &[u8]) -> EditorResult<NamedTempFile> {
    let parent = path.parent().ok_or_else(invalid_destination)?;
    let mut staged = NamedTempFile::new_in(parent).map_err(EditorError::io)?;
    staged.write_all(bytes).map_err(EditorError::io)?;
    staged.as_file().sync_all().map_err(EditorError::io)?;
    Ok(staged)
}

fn replace(staged: NamedTempFile, path: &Path, overwrite: bool) -> EditorResult<()> {
    let result = if overwrite {
        staged.persist(path)
    } else {
        staged.persist_noclobber(path)
    };

    result
        .map(|_| ())
        .map_err(|error| EditorError::io(error.error))
}

/// Use the same atomic staging as document saves for recovery snapshots.
pub(super) fn write_recovery(path: &Path, bytes: &[u8]) -> EditorResult<()> {
    replace(stage(path, bytes)?, path, true)
}

/// Publish a new attachment without replacing a file created by another process.
pub(super) fn write_attachment(path: &Path, bytes: &[u8]) -> EditorResult<()> {
    replace(stage(path, bytes)?, path, false)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn opening_preserves_utf8_bom_and_line_endings() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("note.md");
        let text = "\u{feff}# Note\r\n\r\nText\r\n";
        fs::write(&path, text).unwrap();

        let (document, version) = open(&path).unwrap();
        assert_eq!(document.markdown, text);
        assert_eq!(
            version.fingerprint,
            blake3::hash(text.as_bytes()).to_hex().as_str()
        );
    }

    #[test]
    fn external_edits_require_a_different_save_destination() {
        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("original.md");
        let copy = directory.path().join("copy.md");
        fs::write(&original, "initial").unwrap();
        let (_, version) = open(&original).unwrap();
        fs::write(&original, "external change").unwrap();

        for target in [None, Some(original.as_path())] {
            let error = save(target, Some(&version), "draft", |_| Ok(()))
                .err()
                .unwrap();
            assert_eq!(error.code, "externalChange");
        }

        save(Some(&copy), Some(&version), "draft", |_| Ok(())).unwrap();
        assert_eq!(fs::read_to_string(&copy).unwrap(), "draft");
        assert_eq!(fs::read_to_string(&original).unwrap(), "external change");
    }
}
