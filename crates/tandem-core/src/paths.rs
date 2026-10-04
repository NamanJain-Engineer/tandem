use std::path::{Path, PathBuf};

use crate::error::Error;

/// Resolved on-disk layout for one Tandem profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    pub data: PathBuf,
    pub logs: PathBuf,
    pub providers: PathBuf,
    pub run: PathBuf,
}

impl Paths {
    /// `TANDEM_HOME` wins. Otherwise `~/.tandem`.
    pub fn resolve() -> Result<Self, Error> {
        if let Some(home) = std::env::var_os("TANDEM_HOME") {
            return Ok(Self::from_data(PathBuf::from(home)));
        }
        let user = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .ok_or_else(|| Error::new("paths", "HOME is not set", "set TANDEM_HOME or HOME"))?;
        Ok(Self::from_data(user.join(".tandem")))
    }

    pub fn from_data(data: PathBuf) -> Self {
        Self {
            logs: data.join("logs"),
            providers: data.join("providers"),
            run: data.join("run"),
            data,
        }
    }

    pub fn provider_home(&self, engine: &str, account: &str) -> PathBuf {
        self.providers.join(engine).join(account)
    }

    pub fn ensure(&self) -> Result<(), Error> {
        for dir in [&self.data, &self.logs, &self.providers, &self.run] {
            std::fs::create_dir_all(dir).map_err(|err| {
                Error::new(
                    "paths",
                    format!("could not create {}: {err}", dir.display()),
                    "check that the parent directory is writable",
                )
            })?;
        }
        Ok(())
    }
}

pub fn is_under(root: &Path, path: &Path) -> bool {
    path.starts_with(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tandem_home_overrides_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let prev = std::env::var_os("TANDEM_HOME");
        std::env::set_var("TANDEM_HOME", dir.path());
        let paths = Paths::resolve().expect("resolve");
        if let Some(value) = prev {
            std::env::set_var("TANDEM_HOME", value);
        } else {
            std::env::remove_var("TANDEM_HOME");
        }
        assert_eq!(paths.data, dir.path());
        assert_eq!(paths.logs, dir.path().join("logs"));
        assert_eq!(
            paths.provider_home("claude", "work"),
            dir.path().join("providers").join("claude").join("work")
        );
    }
}
