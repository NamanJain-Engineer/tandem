use tandem_core::error::{ErrorCode, IpcError};

/// Messages are user-safe: they name the operation and git's own short
/// error, never file contents.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("Git isn't installed")]
    NotInstalled,
    #[error("Not a git repository")]
    NotARepo,
    #[error("git {op} failed: {stderr}")]
    Command {
        op: String,
        code: Option<i32>,
        stderr: String,
    },
    #[error("Uncommitted changes")]
    Dirty,
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Invalid(String),
    /// TL-06 rule 5: only a person at this machine can confirm this.
    #[error("This needs a confirmation on this computer")]
    LocalOnly,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("git: {0}")]
    Lib(#[from] git2::Error),
}

impl GitError {
    pub fn code(&self) -> ErrorCode {
        match self {
            GitError::NotInstalled => ErrorCode::EngineMissing,
            GitError::NotARepo => ErrorCode::NotFound,
            GitError::Dirty | GitError::Conflict(_) => ErrorCode::Conflict,
            GitError::Invalid(_) => ErrorCode::Invalid,
            GitError::LocalOnly => ErrorCode::PermissionDenied,
            GitError::Io(_) => ErrorCode::Io,
            GitError::Command { .. } | GitError::Lib(_) => ErrorCode::Internal,
        }
    }
}

impl From<GitError> for IpcError {
    fn from(err: GitError) -> Self {
        IpcError::new(err.code(), err.to_string()).with_stage("git")
    }
}
