//! `$DATA` layout (ARCHITECTURE §4.3). `TANDEM_HOME` overrides `~/.tandem`.

use std::io;
use std::path::{Path, PathBuf};

use crate::error::CoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDir {
    root: PathBuf,
}

impl DataDir {
    /// `TANDEM_HOME` if set, otherwise `~/.tandem`. Creates the root and the
    /// private subdirectories (0700 on Unix).
    pub fn resolve() -> Result<Self, CoreError> {
        Self::at(Self::default_root()?)
    }

    pub fn default_root() -> Result<PathBuf, CoreError> {
        if let Some(home) = std::env::var_os("TANDEM_HOME").filter(|v| !v.is_empty()) {
            return Ok(PathBuf::from(home));
        }
        user_home()
            .map(|home| home.join(".tandem"))
            .ok_or_else(|| CoreError::new("paths", "HOME is not set", "set TANDEM_HOME or HOME"))
    }

    /// Opens (and creates) a data dir at an explicit root. Tests use this with
    /// a temp dir.
    pub fn at(root: PathBuf) -> Result<Self, CoreError> {
        let dir = Self { root };
        for path in [
            dir.root.clone(),
            dir.run(),
            dir.logs(),
            dir.providers(),
            dir.auth_dir(),
            dir.cache(),
        ] {
            create_private_dir(&path).map_err(|err| {
                CoreError::io(
                    "paths",
                    &err,
                    format!("check that {} is writable", path.display()),
                )
            })?;
        }
        Ok(dir)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn db_path(&self) -> PathBuf {
        self.root.join("tandem.db")
    }
    pub fn backups(&self) -> PathBuf {
        self.root.join("backups")
    }
    pub fn window_state(&self) -> PathBuf {
        self.root.join("window-state.json")
    }
    pub fn update_state(&self) -> PathBuf {
        self.root.join("update-state.json")
    }
    pub fn auth_dir(&self) -> PathBuf {
        self.root.join("auth")
    }
    pub fn auth_credentials(&self) -> PathBuf {
        self.auth_dir().join("credentials.enc")
    }
    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }
    pub fn entitlement_cache(&self) -> PathBuf {
        self.cache().join("entitlement.jws")
    }
    pub fn config_cache(&self) -> PathBuf {
        self.cache().join("config.json")
    }
    pub fn run(&self) -> PathBuf {
        self.root.join("run")
    }
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }
    pub fn providers(&self) -> PathBuf {
        self.root.join("providers")
    }
    pub fn provider_home(&self, engine: &str, account: &str) -> PathBuf {
        self.providers().join(engine).join(account)
    }
    pub fn teammates(&self) -> PathBuf {
        self.root.join("teammates")
    }
    pub fn teammate_home(&self, id: &str) -> PathBuf {
        self.teammates().join(id)
    }
    pub fn trash(&self) -> PathBuf {
        self.root.join("trash")
    }
    pub fn scratch(&self, thread_id: &str) -> PathBuf {
        self.root.join("scratch").join(thread_id)
    }
    pub fn attachments(&self) -> PathBuf {
        self.root.join("attachments")
    }
    /// Non-git threads' starting manifests (F-TH-13).
    pub fn snapshots(&self) -> PathBuf {
        self.root.join("snapshots")
    }
    pub fn staging(&self) -> PathBuf {
        self.root.join("staging")
    }
    pub fn scrollback(&self) -> PathBuf {
        self.root.join("scrollback")
    }
    pub fn tasks(&self) -> PathBuf {
        self.root.join("tasks")
    }
    /// Plans with Lead, kept across restarts (F-TK-07).
    pub fn plans(&self) -> PathBuf {
        self.root.join("plans.json")
    }
    pub fn models(&self) -> PathBuf {
        self.root.join("models")
    }
    pub fn conductor(&self) -> PathBuf {
        self.root.join("conductor")
    }
    /// Checkpoint stores, one per folder (F-EX-01).
    pub fn checkpoints(&self) -> PathBuf {
        self.root.join("checkpoints")
    }
    pub fn scratchpad(&self) -> PathBuf {
        self.root.join("scratchpad")
    }
    pub fn browser_profile(&self) -> PathBuf {
        self.root.join("browser-profile")
    }
    pub fn ssh_known_hosts(&self) -> PathBuf {
        self.root.join("ssh").join("known_hosts")
    }
    pub fn shell_integration(&self) -> PathBuf {
        self.root.join("shell-integration")
    }
    /// The user's own prompt library (F-EX-05).
    pub fn prompts(&self) -> PathBuf {
        self.root.join("prompts")
    }
    pub fn mcp_configs(&self) -> PathBuf {
        self.root.join("mcp")
    }
    pub fn briefings(&self) -> PathBuf {
        self.root.join("briefings")
    }
    pub fn telemetry(&self) -> PathBuf {
        self.root.join("telemetry")
    }
    pub fn crashes(&self) -> PathBuf {
        self.root.join("crashes")
    }
    pub fn tmp(&self) -> PathBuf {
        self.root.join("tmp")
    }
}

pub fn user_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// `mkdir -p`. On Unix each folder it makes is 0700 from the start (never
/// briefly 0755), and `path` is chmod 0700 if it was already there.
pub fn create_private_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_layout_and_permissions() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(tmp.path().join("data")).expect("data dir");
        assert_eq!(data.db_path(), tmp.path().join("data/tandem.db"));
        assert_eq!(
            data.provider_home("claude", "acct_1"),
            tmp.path().join("data/providers/claude/acct_1")
        );
        assert!(data.run().is_dir());
        assert!(data.logs().is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(data.run())
                .expect("meta")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o700);
        }
    }

    #[cfg(unix)]
    #[test]
    fn f_tk_08_1_a_private_folder_and_the_folders_made_for_it_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).expect("meta").permissions().mode() & 0o777;
        let tmp = tempfile::tempdir().expect("tempdir");
        let run = tmp.path().join("run");
        std::fs::create_dir(&run).expect("run");
        std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let session = run.join("answers").join("ans_1");
        create_private_dir(&session).expect("create");
        // Made 0700, not chmod'd afterwards: `answers` was never asked for.
        assert_eq!(mode(&run.join("answers")), 0o700);
        assert_eq!(mode(&session), 0o700);
        // A folder that was there is left as it is, unless it's the one asked for.
        assert_eq!(mode(&run), 0o755);
        create_private_dir(&run).expect("again");
        assert_eq!(mode(&run), 0o700);
        // A file in the way is an error, not a folder.
        let file = tmp.path().join("file");
        std::fs::write(&file, "x").expect("file");
        assert!(create_private_dir(&file).is_err());
    }
}
