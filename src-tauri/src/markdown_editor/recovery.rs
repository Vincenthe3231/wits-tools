use std::{fs, io::ErrorKind, path::Path};

use super::{EditorError, EditorResult, RecoveryDraft, files};

pub(super) fn read(path: &Path) -> EditorResult<Option<RecoveryDraft>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(serialization_error),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(EditorError::io(error)),
    }
}

pub(super) fn write(path: &Path, draft: &RecoveryDraft) -> EditorResult<()> {
    fs::create_dir_all(path.parent().expect("recovery file has a parent"))
        .map_err(EditorError::io)?;
    let bytes = serde_json::to_vec(draft).map_err(serialization_error)?;
    files::write_recovery(path, &bytes)
}

pub(super) fn clear(path: &Path) -> EditorResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(EditorError::io(error)),
    }
}

fn serialization_error(error: serde_json::Error) -> EditorError {
    EditorError::new(
        "recovery",
        format!("Could not process the recovery draft. {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_can_be_replaced_and_cleared() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("draft.json");
        let mut draft = RecoveryDraft {
            name: "Untitled".into(),
            markdown: "first".into(),
            original: None,
        };
        assert!(read(&path).unwrap().is_none());
        write(&path, &draft).unwrap();

        draft.markdown = "\u{feff}updated\r\n".into();
        write(&path, &draft).unwrap();
        assert_eq!(read(&path).unwrap().unwrap().markdown, draft.markdown);

        clear(&path).unwrap();
        clear(&path).unwrap();
        assert!(read(&path).unwrap().is_none());
    }
}
