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
    /// A jj working copy left behind by a rewrite elsewhere: the message
    /// names the folder and what to run. Never a Conflict, so nothing
    /// offers to force past it (F-CD-24).
    #[error("{0}")]
    Stale(String),
    #[error("{0}")]
    Invalid(String),
    /// The remote wants credentials git couldn't get without a prompt.
    #[error("{0}")]
    Auth(String),
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
            GitError::Invalid(_) | GitError::Stale(_) => ErrorCode::Invalid,
            GitError::Auth(_) => ErrorCode::Unauthenticated,
            GitError::LocalOnly => ErrorCode::PermissionDenied,
            GitError::Io(_) => ErrorCode::Io,
            GitError::Command { .. } | GitError::Lib(_) => ErrorCode::Internal,
        }
    }
}

/// Git refuses to commit (a merge, for one) until it has a name and an email.
pub const NO_IDENTITY: &str = "Git doesn't know who you are yet. Run git config --global user.name \"Your Name\" and git config --global user.email you@example.com, then try again.";

impl GitError {
    /// Git stopped because no user.name / user.email is set. A commit's
    /// message ends with "unable to auto-detect email address", which is
    /// all the shortened stderr keeps of it.
    pub fn is_missing_identity(&self) -> bool {
        matches!(self, GitError::Command { stderr, .. }
            if stderr.contains("Please tell me who you are")
                || stderr.contains("identity unknown")
                || stderr.contains("unable to auto-detect email address"))
    }
}

impl From<GitError> for IpcError {
    fn from(err: GitError) -> Self {
        if err.is_missing_identity() {
            return IpcError::new(ErrorCode::Invalid, NO_IDENTITY).with_stage("git");
        }
        IpcError::new(err.code(), err.to_string()).with_stage("git")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_cd_15_3_a_merge_without_a_git_identity_says_how_to_set_one() {
        let err = GitError::Command {
            op: "merge".into(),
            code: Some(128),
            stderr: "Committer identity unknown\n\n*** Please tell me who you are.\n".into(),
        };
        let ipc: IpcError = err.into();
        assert_eq!(ipc.code, ErrorCode::Invalid);
        assert!(ipc.message.contains("git config --global user.email"));
        let other: IpcError = GitError::Command {
            op: "merge".into(),
            code: Some(1),
            stderr: "boom".into(),
        }
        .into();
        assert_eq!(other.message, "git merge failed: boom");
    }

    #[test]
    fn f_cd_20_a_commit_without_a_git_identity_says_how_to_set_one() {
        // The last lines of what `git commit` prints, as the runner keeps them.
        let err = GitError::Command {
            op: "commit".into(),
            code: Some(128),
            stderr: "to set your account's default identity.\nOmit --global to set the identity only in this repository.\nfatal: unable to auto-detect email address (got 'me@host.(none)')".into(),
        };
        let ipc: IpcError = err.into();
        assert_eq!(ipc.code, ErrorCode::Invalid);
        assert_eq!(ipc.message, NO_IDENTITY);
    }
}
