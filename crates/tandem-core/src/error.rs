use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Every failure that crosses the IPC boundary carries one of these codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ErrorCode {
    NotFound,
    Invalid,
    Conflict,
    RevisionConflict,
    LimitReached,
    ForbiddenPlan,
    CreditsExhausted,
    Unauthenticated,
    Unavailable,
    Timeout,
    Cancelled,
    PermissionDenied,
    PathEscape,
    UnsafeFilename,
    EngineMissing,
    EngineUnsupported,
    Waiting,
    Io,
    Internal,
}

/// The error every Tauri command returns. Messages are user-safe and never
/// carry prompt text, file contents or tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
#[error("{message}")]
pub struct IpcError {
    pub code: ErrorCode,
    pub message: String,
    pub stage: Option<String>,
    pub hint: Option<String>,
    pub retryable: bool,
}

impl IpcError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            stage: None,
            hint: None,
            retryable: matches!(
                code,
                ErrorCode::Timeout | ErrorCode::Unavailable | ErrorCode::Io
            ),
        }
    }

    pub fn with_stage(mut self, stage: impl Into<String>) -> Self {
        self.stage = Some(stage.into());
        self
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// Library error: what failed, at which stage, and what the user can do next
/// (PRODUCT_SPEC §5.2 "Errors carry a stage and a next step").
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{stage}: {message} (next: {next_step})")]
pub struct CoreError {
    pub code: ErrorCode,
    pub stage: String,
    pub message: String,
    pub next_step: String,
}

impl CoreError {
    pub fn new(
        stage: impl Into<String>,
        message: impl Into<String>,
        next_step: impl Into<String>,
    ) -> Self {
        Self {
            code: ErrorCode::Internal,
            stage: stage.into(),
            message: message.into(),
            next_step: next_step.into(),
        }
    }

    pub fn with_code(mut self, code: ErrorCode) -> Self {
        self.code = code;
        self
    }

    pub fn io(
        stage: impl Into<String>,
        err: &std::io::Error,
        next_step: impl Into<String>,
    ) -> Self {
        Self::new(stage, err.to_string(), next_step).with_code(ErrorCode::Io)
    }
}

impl From<CoreError> for IpcError {
    fn from(err: CoreError) -> Self {
        let mut out = IpcError::new(err.code, err.message);
        out.stage = Some(err.stage);
        out.hint = Some(err.next_step);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_error_maps_to_ipc_error_with_stage_and_hint() {
        let err = CoreError::new("store", "disk full", "free some space").with_code(ErrorCode::Io);
        let ipc: IpcError = err.into();
        assert_eq!(ipc.code, ErrorCode::Io);
        assert_eq!(ipc.stage.as_deref(), Some("store"));
        assert_eq!(ipc.hint.as_deref(), Some("free some space"));
        assert!(ipc.retryable);
    }

    #[test]
    fn error_code_serializes_snake_case() {
        let json = serde_json::to_string(&ErrorCode::ForbiddenPlan).expect("json");
        assert_eq!(json, "\"forbidden_plan\"");
    }
}
