use crate::infrastructure::logging::redact;
use serde::Serializer;

const ERROR_CODE_CONTRACT: &str = include_str!("../../src/lib/schemas/error-codes.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    Internal,
    InvalidArgument,
    NotFound,
    Conflict,
    PreconditionFailed,
    Unavailable,
    Cancelled,
    Timeout,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Internal => "E_INTERNAL",
            Self::InvalidArgument => "E_INVALID_ARGUMENT",
            Self::NotFound => "E_NOT_FOUND",
            Self::Conflict => "E_CONFLICT",
            Self::PreconditionFailed => "E_PRECONDITION_FAILED",
            Self::Unavailable => "E_UNAVAILABLE",
            Self::Cancelled => "E_CANCELLED",
            Self::Timeout => "E_TIMEOUT",
        }
    }

    pub const fn all() -> [Self; 8] {
        [
            Self::Internal,
            Self::InvalidArgument,
            Self::NotFound,
            Self::Conflict,
            Self::PreconditionFailed,
            Self::Unavailable,
            Self::Cancelled,
            Self::Timeout,
        ]
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl serde::Serialize for ErrorCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{code}: {safe_message}")]
pub struct AppError {
    pub code: ErrorCode,
    pub safe_message: String,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            safe_message: redact(&message.into()),
        }
    }
}

pub fn verify_error_code_contract() -> Result<(), AppError> {
    let contract: Vec<String> = serde_json::from_str(ERROR_CODE_CONTRACT)
        .map_err(|_| AppError::new(ErrorCode::Internal, "error code contract is not json"))?;
    let actual: Vec<String> = ErrorCode::all()
        .iter()
        .map(|code| code.as_str().to_owned())
        .collect();
    if contract != actual {
        return Err(AppError::new(
            ErrorCode::Internal,
            "error code contract mismatch",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{verify_error_code_contract, AppError, ErrorCode, ERROR_CODE_CONTRACT};
    use serde_json::Value;

    #[test]
    fn error_codes_match_shared_contract() {
        verify_error_code_contract().expect("error code contract");
        let contract: Vec<String> =
            serde_json::from_str(ERROR_CODE_CONTRACT).expect("error code contract must be json");
        let actual: Vec<&str> = ErrorCode::all().iter().map(|code| code.as_str()).collect();
        assert_eq!(contract, actual);
    }

    #[test]
    fn error_display_uses_stable_code_and_redacts_secrets() {
        let error = AppError::new(ErrorCode::Unavailable, "api_key=sk-live-secretvalue");
        let rendered = error.to_string();
        assert!(rendered.starts_with("E_UNAVAILABLE:"));
        assert!(!rendered.contains("sk-live"));
        assert!(rendered.contains("[REDACTED]"));
    }

    #[test]
    fn error_code_serializes_as_variant_name() {
        let value = serde_json::to_value(ErrorCode::Timeout).expect("serialize");
        assert_eq!(value, Value::String("E_TIMEOUT".to_owned()));
    }
}
