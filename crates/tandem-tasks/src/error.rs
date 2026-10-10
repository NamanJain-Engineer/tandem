use serde::Serialize;

/// The tracker's error codes (ARCHITECTURE §9.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskErrorCode {
    RevisionConflict,
    NotFound,
    Invalid,
    ConnectionRevoked,
    Unavailable,
}

#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    #[error("{id} changed since you read it (now at revision {current_revision}); read it again and retry")]
    RevisionConflict { id: String, current_revision: u64 },
    #[error("{0} doesn't exist")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("this connection was revoked; the session's task access has ended")]
    ConnectionRevoked,
    #[error("{0}")]
    Unavailable(String),
    #[error("couldn't read or write the task files: {0}")]
    Io(#[from] std::io::Error),
    #[error("a task file is damaged: {0}")]
    Corrupt(String),
}

impl TaskError {
    pub fn code(&self) -> TaskErrorCode {
        match self {
            TaskError::RevisionConflict { .. } => TaskErrorCode::RevisionConflict,
            TaskError::NotFound(_) => TaskErrorCode::NotFound,
            TaskError::Invalid(_) => TaskErrorCode::Invalid,
            TaskError::ConnectionRevoked => TaskErrorCode::ConnectionRevoked,
            TaskError::Unavailable(_) | TaskError::Io(_) | TaskError::Corrupt(_) => {
                TaskErrorCode::Unavailable
            }
        }
    }

    pub fn invalid(msg: impl Into<String>) -> Self {
        TaskError::Invalid(msg.into())
    }
}
