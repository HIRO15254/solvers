//! Stable common-input diagnostics, independent of product implementations.
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum Code {
    NLH001,
    NLH002,
    NLH003,
    NLH004,
    NLH005,
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, thiserror::Error)]
#[error("{code}: {location}{message}", location = self.key.as_ref().map(|key| format!("{key}: ")).unwrap_or_default())]
pub struct SpotError {
    pub code: Code,
    pub key: Option<String>,
    pub message: String,
}

impl SpotError {
    pub fn new(code: Code, key: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code,
            key: Some(key.into()),
            message: message.into(),
        }
    }
}

pub(crate) fn value_error(key: impl Into<String>, message: impl Into<String>) -> SpotError {
    SpotError::new(Code::NLH003, key, message)
}
