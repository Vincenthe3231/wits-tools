//! Classifies native Markdown editor failures and keeps diagnostic details out of IPC responses.
use std::io;

use serde::Serialize;

// Keep wire identifiers stable so Vue can choose messages independently of Rust diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum EditorErrorCode {
    InvalidPath,
    InvalidDestination,
    FileNotFound,
    PermissionDenied,
    IoFailed,
    WriteFailed,
    UnsupportedFileType,
    FileTooLarge,
    InvalidEncoding,
    ExternalChange,
    ExportConflict,
    UnsupportedImageType,
    ImageTooLarge,
    ImagePreviewUnavailable,
    ImageChanged,
    ImageConflict,
    RecoveryFailed,
    SessionUnavailable,
}

// Expose the stable code while retaining the underlying reason for native diagnostics.
#[derive(Debug, Serialize)]
pub struct EditorError {
    pub(super) code: EditorErrorCode,
    #[serde(skip)]
    detail: String,
}

impl EditorError {
    // Report the native detail locally so the UI never needs to render a raw OS message.
    pub(crate) fn new(code: EditorErrorCode, detail: impl Into<String>) -> Self {
        let error = Self {
            code,
            detail: detail.into(),
        };
        eprintln!("Markdown editor [{:?}]: {}", error.code, error.detail);
        error
    }

    // Preserve actionable permission and missing-file failures across read and write operations.
    pub(crate) fn io(error: io::Error) -> Self {
        Self::classify_io(error, EditorErrorCode::IoFailed)
    }

    pub(crate) fn write(error: io::Error) -> Self {
        Self::classify_io(error, EditorErrorCode::WriteFailed)
    }

    fn classify_io(error: io::Error, fallback: EditorErrorCode) -> Self {
        let code = match error.kind() {
            io::ErrorKind::NotFound => EditorErrorCode::FileNotFound,
            io::ErrorKind::PermissionDenied => EditorErrorCode::PermissionDenied,
            _ => fallback,
        };
        Self::new(code, error.to_string())
    }
}

// Use one failure contract throughout the editor module.
pub(crate) type EditorResult<T> = Result<T, EditorError>;

#[cfg(test)]
mod tests {
    use super::*;

    // Check the wire identifier and keep OS diagnostics out of the response.
    #[test]
    fn serializes_only_the_classified_code() {
        let error = EditorError::io(io::Error::new(io::ErrorKind::PermissionDenied, "detail"));
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({ "code": "permissionDenied" })
        );
    }
}
