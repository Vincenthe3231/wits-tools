//! Stages raster images for recovery and publishes content-addressed attachments on save.

use std::{
    fs::{self, File},
    io::Read,
    path::{Component, Path},
};

use super::{EditorError, EditorErrorCode, EditorResult, files};

const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;
const IMAGE_DIRECTORY: &str = "wits-images";

// Recognize only raster formats supported by the editor and Windows WebView.
fn extension(bytes: &[u8]) -> EditorResult<&'static str> {
    let ext = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "jpg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "gif"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "webp"
    } else {
        return Err(EditorError::new(
            EditorErrorCode::UnsupportedImageType,
            "Choose a PNG, JPEG, GIF, or WebP image.",
        ));
    };

    Ok(ext)
}

// Reject special files and bound the read even if the file grows during the operation.
fn read(path: &Path) -> EditorResult<Vec<u8>> {
    if !fs::metadata(path).map_err(EditorError::io)?.is_file() {
        return Err(EditorError::new(
            EditorErrorCode::UnsupportedImageType,
            "Choose a regular image file.",
        ));
    }

    let mut bytes = Vec::new();
    File::open(path)
        .map_err(EditorError::io)?
        .take((MAX_IMAGE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(EditorError::io)?;
    validate(&bytes)?;
    Ok(bytes)
}

// Apply identical limits to imported bytes and local previews.
fn validate(bytes: &[u8]) -> EditorResult<&'static str> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(EditorError::new(
            EditorErrorCode::ImageTooLarge,
            "Images can be up to 16 MiB each.",
        ));
    }
    extension(bytes)
}

// A hash and fixed extension make pasted references stable across undo, recovery, and Save As.
pub(super) fn stage(root: &Path, bytes: &[u8]) -> EditorResult<String> {
    let ext = validate(bytes)?;
    fs::create_dir_all(root).map_err(EditorError::io)?;
    let name = format!("{}.{ext}", blake3::hash(bytes).to_hex());
    files::write_recovery(&root.join(&name), bytes)?;
    Ok(format!("{IMAGE_DIRECTORY}/{name}"))
}

// Recognize only attachments owned by this editor for automatic copying during save.
fn managed_name(src: &str) -> Option<&str> {
    let name = src.strip_prefix("wits-images/")?;
    let (hash, ext) = name.split_once('.')?;
    (hash.len() == 64
        && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        && matches!(ext, "png" | "jpg" | "gif" | "webp"))
    .then_some(name)
}

// Resolve only paths inside the document directory, including after resolving links.
pub(super) fn preview(root: &Path, parent: Option<&Path>, src: &str) -> EditorResult<Vec<u8>> {
    let path = Path::new(src);
    if path
        .components()
        .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
        && !src.contains([':', '\\'])
        && let Some(parent) = parent
        && let Ok(path) = parent.join(path).canonicalize()
        && path.starts_with(parent)
    {
        return read(&path);
    }

    if let Some(name) = managed_name(src) {
        return read(&root.join(name));
    }

    Err(EditorError::new(
        EditorErrorCode::ImagePreviewUnavailable,
        "The local image is unavailable.",
    ))
}

// Publish each managed attachment before replacing the Markdown so a failed copy leaves the document intact.
pub(super) fn publish(
    root: &Path,
    original: Option<&Path>,
    destination: &Path,
    sources: &[String],
) -> EditorResult<()> {
    for src in sources {
        let Some(name) = managed_name(src) else {
            continue;
        };

        let folder = destination.join(IMAGE_DIRECTORY);
        fs::create_dir_all(&folder).map_err(EditorError::io)?;
        if folder.canonicalize().map_err(EditorError::io)? != folder {
            return Err(EditorError::new(
                EditorErrorCode::ImagePreviewUnavailable,
                "Choose a destination with a regular image folder.",
            ));
        }

        let target = folder.join(name);
        let bytes = preview(root, original, src)?;
        let expected = format!("{}.{}", blake3::hash(&bytes).to_hex(), extension(&bytes)?);
        if name != expected {
            return Err(EditorError::new(
                EditorErrorCode::ImageChanged,
                "An attached image has changed on disk.",
            ));
        }

        if target.try_exists().map_err(EditorError::io)? {
            if target.canonicalize().map_err(EditorError::io)? == target && read(&target)? == bytes
            {
                continue;
            }
            return Err(EditorError::new(
                EditorErrorCode::ImageConflict,
                "A different image already exists at the destination.",
            ));
        }

        files::write_attachment(&target, &bytes)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // An untitled draft can preview its staging copy, then publish to two separate save locations.
    #[test]
    fn publishes_staged_and_reopened_attachments() {
        let cache = tempfile::tempdir().unwrap();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let first = first.path().canonicalize().unwrap();
        let second = second.path().canonicalize().unwrap();
        // Only the file signature matters to the storage layer.
        let png = b"\x89PNG\r\n\x1a\nfixture";
        let src = stage(cache.path(), png).unwrap();

        assert_eq!(preview(cache.path(), None, &src).unwrap(), png);
        publish(cache.path(), None, &first, std::slice::from_ref(&src)).unwrap();

        fs::remove_file(cache.path().join(managed_name(&src).unwrap())).unwrap();
        publish(
            cache.path(),
            Some(&first),
            &second,
            std::slice::from_ref(&src),
        )
        .unwrap();
        assert_eq!(fs::read(second.join(src)).unwrap(), png);
    }
}
