//! Copies a Markdown snapshot and its local assets into an independently movable folder.

use std::{
    fs,
    path::{Component, Path},
};

use serde::Deserialize;

use super::{EditorError, EditorErrorCode, EditorResult, files, images};

#[derive(Deserialize)]
pub struct PackageAsset {
    source: String,
    target: String,
}

// Resolve only explicitly referenced files, using the original folder for relative links.
fn copy_asset(
    asset: &PackageAsset,
    original_folder: Option<&Path>,
    cache: &Path,
    image_folder: &Path,
) -> EditorResult<()> {
    let target = Path::new(&asset.target);
    let mut parts = target.components();
    if !matches!(parts.next(), Some(Component::Normal(_)))
        || parts.next().is_some()
        || asset.target.contains(':')
    {
        return Err(EditorError::new(
            EditorErrorCode::InvalidDestination,
            "An export image needs a single filename.",
        ));
    }
    let destination = image_folder.join(target);
    if destination.try_exists().map_err(EditorError::io)? {
        return Err(EditorError::new(
            EditorErrorCode::ImageConflict,
            "Two package images have the same name.",
        ));
    }

    let source = Path::new(&asset.source);
    let path = if source.is_absolute() {
        Some(source.to_owned())
    } else {
        original_folder.map(|parent| parent.join(source))
    };
    if let Some(path) = path
        && path.try_exists().map_err(EditorError::io)?
    {
        if !fs::metadata(&path).map_err(EditorError::io)?.is_file() {
            return Err(EditorError::new(
                EditorErrorCode::UnsupportedImageType,
                "An exported image must be a regular file.",
            ));
        }
        fs::copy(&path, &destination).map_err(EditorError::write)?;
        return Ok(());
    }

    // Unsaved clipboard images may exist only in the draft cache.
    let bytes = images::preview(cache, original_folder, &asset.source)?;
    fs::write(destination, bytes).map_err(EditorError::write)
}

// Publish the complete package only after all referenced files and Markdown are ready.
pub(super) fn write(
    destination: &Path,
    original_file: Option<&Path>,
    cache: &Path,
    markdown: &str,
    assets: &[PackageAsset],
) -> EditorResult<()> {
    files::validate_size(markdown)?;
    let filename = original_file
        .map(files::name)
        .unwrap_or_else(|| "Untitled.md".into());
    let stem = Path::new(&filename)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("Untitled");
    let folder_name = stem.split_whitespace().collect::<Vec<_>>().join("_");
    let folder = destination.join(folder_name);
    if folder.try_exists().map_err(EditorError::io)? {
        return Err(EditorError::new(
            EditorErrorCode::ExportConflict,
            "A package already exists at this location.",
        ));
    }
    let stage = tempfile::Builder::new()
        .prefix(".wits-export-")
        .tempdir_in(destination)
        .map_err(EditorError::write)?;
    let image_folder = stage.path().join("img");
    fs::create_dir(&image_folder).map_err(EditorError::write)?;

    for asset in assets {
        copy_asset(
            asset,
            original_file.and_then(Path::parent),
            cache,
            &image_folder,
        )?;
    }
    fs::write(stage.path().join(filename), markdown).map_err(EditorError::write)?;
    fs::rename(stage.path(), folder).map_err(EditorError::write)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verify that moving the package only requires the new folder contents.
    #[test]
    fn packages_manual_and_staged_images() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("Implementation Plan.md");
        let cache = root.path().join("cache");
        let png = b"\x89PNG\r\n\x1a\nfixture";
        let staged = images::stage(&cache, png).unwrap();
        fs::write(root.path().join("linked image.png"), png).unwrap();
        let assets = [
            PackageAsset {
                source: "linked image.png".into(),
                target: "linked.png".into(),
            },
            PackageAsset {
                source: staged,
                target: "staged.png".into(),
            },
        ];
        let markdown = "![A](img/linked.png) ![B](img/staged.png)";

        write(root.path(), Some(&source), &cache, markdown, &assets).unwrap();
        let target = root.path().join("Implementation_Plan");
        let moved = root.path().join("Moved");
        fs::rename(target, &moved).unwrap();
        assert_eq!(
            fs::read_to_string(moved.join("Implementation Plan.md")).unwrap(),
            markdown
        );
        assert_eq!(fs::read(moved.join("img/linked.png")).unwrap(), png);
        assert_eq!(fs::read(moved.join("img/staged.png")).unwrap(), png);
    }

    // A missing image must never publish a partial export or replace a prior package.
    #[test]
    fn leaves_destination_intact_when_an_image_is_missing() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("Implementation Plan.md");
        let assets = [PackageAsset {
            source: "missing.png".into(),
            target: "image.png".into(),
        }];
        let folder = root.path().join("Implementation_Plan");
        assert!(write(root.path(), Some(&source), root.path(), "", &assets).is_err());
        assert!(!folder.exists());
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("keep.md"), "original").unwrap();
        assert!(write(root.path(), Some(&source), root.path(), "", &[]).is_err());
        assert_eq!(
            fs::read_to_string(folder.join("keep.md")).unwrap(),
            "original"
        );
    }
}
