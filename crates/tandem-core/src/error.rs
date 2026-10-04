/// Library error. Every failure names the stage and a next step.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{message} (stage: {stage}). Next: {next_step}")]
    Failed {
        stage: &'static str,
        message: String,
        next_step: String,
    },
}

impl Error {
    pub fn new(
        stage: &'static str,
        message: impl Into<String>,
        next_step: impl Into<String>,
    ) -> Self {
        Self::Failed {
            stage,
            message: message.into(),
            next_step: next_step.into(),
        }
    }

    pub fn stage(&self) -> &'static str {
        match self {
            Self::Failed { stage, .. } => stage,
        }
    }
}
