use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorError {
    pub(super) code: &'static str,
    message: String,
}

impl EditorError {
    pub(super) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub(super) fn io(error: std::io::Error) -> Self {
        Self::new("io", error.to_string())
    }
}

pub(super) type EditorResult<T> = Result<T, EditorError>;
