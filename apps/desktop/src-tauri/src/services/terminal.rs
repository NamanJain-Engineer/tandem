//! Plain-shell terminals (F-CD-03, M2 slice): one login shell per pane with
//! the captured login environment, streaming over a per-pane channel.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::OnceCell;

use tandem_core::ids::PaneId;
use tandem_core::settings::SettingKey;
use tandem_core::{ErrorCode, IpcError};
use tandem_pty::env::{self, LoginEnv};
use tandem_pty::{OutputSink, PtyInfo, PtyManager, SpawnSpec};
use tandem_store::repo::{panes, settings, workspaces};
use tandem_store::Store;

pub const FOLDER_NOT_FOUND: &str = "Folder not found — opened at workspace root";

pub struct TerminalService {
    pub pty: PtyManager,
    store: Store,
    login: OnceCell<LoginEnv>,
    spawn_log: Option<PathBuf>,
}

/// One line of `TANDEM_SPAWN_LOG` (debug builds): what was started, with
/// env keys only.
#[derive(Debug, Clone, Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SpawnRecord {
    pub argv: Vec<String>,
    pub env_keys: Vec<String>,
    pub provider_home: Option<String>,
    pub cwd: String,
}

impl TerminalService {
    pub fn new(pty: PtyManager, store: Store) -> Arc<Self> {
        let spawn_log = if cfg!(debug_assertions) {
            std::env::var_os("TANDEM_SPAWN_LOG").map(PathBuf::from)
        } else {
            None
        };
        Arc::new(Self {
            pty,
            store,
            login: OnceCell::new(),
            spawn_log,
        })
    }

    /// Captured once per launch (§5.2); the first caller waits ≤3 s.
    pub async fn login_env(&self) -> &LoginEnv {
        self.login
            .get_or_init(|| async {
                let shell = env::default_shell();
                tokio::task::spawn_blocking(move || env::capture(&shell, env::CAPTURE_TIMEOUT))
                    .await
                    .unwrap_or_default()
            })
            .await
    }

    /// The plan's live-pane limit: at the limit no pane or process is
    /// created and the picker shows the upgrade card (F-CD-03).
    pub fn check_live_limit(&self, max_live: Option<u32>) -> Result<(), IpcError> {
        match max_live {
            Some(max) if self.pty.live_count() as u32 >= max => Err(IpcError::new(
                ErrorCode::LimitReached,
                format!("Free includes {max} live panes"),
            )
            .with_stage("panes")),
            _ => Ok(()),
        }
    }

    pub async fn spawn(
        &self,
        pane: &PaneId,
        cols: u16,
        rows: u16,
        max_live: Option<u32>,
        sink: Arc<dyn OutputSink>,
    ) -> Result<PtyInfo, IpcError> {
        self.check_live_limit(max_live)?;
        let record = panes::get(&self.store, pane).await?;
        let ws = workspaces::get(&self.store, &record.workspace_id).await?;
        let root = PathBuf::from(&ws.root);
        if !root.is_dir() {
            return Err(IpcError::new(
                ErrorCode::NotFound,
                "This workspace's folder is missing. Locate it to open terminals.",
            ));
        }
        let wanted = record
            .cwd
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| root.clone());
        let (cwd, notice) = if wanted.is_dir() {
            (wanted, None)
        } else {
            (root, Some(FOLDER_NOT_FOUND))
        };

        let login = self.login_env().await;
        let configured = settings::get(&self.store, SettingKey::DefaultShell).await?;
        let shell = configured
            .as_str()
            .filter(|s| !s.is_empty() && Path::new(s).is_absolute())
            .map(PathBuf::from)
            .unwrap_or_else(|| login.shell.clone());
        let scrollback = settings::get(&self.store, SettingKey::TermScrollback)
            .await?
            .as_u64()
            .unwrap_or(10_000) as usize;
        let spec = SpawnSpec {
            pane_id: pane.to_string(),
            args: env::login_args(&shell),
            program: shell,
            cwd,
            env: env::pane_env(login, pane.as_str(), env!("CARGO_PKG_VERSION")),
            cols,
            rows,
            scrollback_lines: scrollback,
        };
        self.log_spawn(&spec);
        let info = self.pty.spawn(spec, Some(sink))?;
        if let Some(text) = notice {
            let _ = self.pty.notice(pane.as_str(), text);
        }
        tracing::info!(code = "pty.spawned", pid = info.pid, "terminal started");
        Ok(info)
    }

    fn log_spawn(&self, spec: &SpawnSpec) {
        let Some(path) = &self.spawn_log else { return };
        let mut argv = vec![spec.program.to_string_lossy().into_owned()];
        argv.extend(spec.args.iter().cloned());
        let record = SpawnRecord {
            argv,
            env_keys: spec.env.keys().cloned().collect(),
            provider_home: None,
            cwd: spec.cwd.to_string_lossy().into_owned(),
        };
        if let Ok(line) = serde_json::to_string(&record) {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(f, "{line}");
            }
        }
    }

    pub fn spawn_records(&self) -> Vec<SpawnRecord> {
        let Some(path) = &self.spawn_log else {
            return Vec::new();
        };
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Duration;
    use tandem_core::DataDir;
    use tandem_pty::{wait_until, VecSink};
    use tandem_store::repo::tabs;

    #[tokio::test(flavor = "multi_thread")]
    async fn f_cd_03_shell_starts_in_the_workspace_and_echoes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let root = dir.path().join("ws1");
        std::fs::create_dir_all(&root).expect("mkdir");
        settings::set(
            &store,
            SettingKey::DefaultShell,
            serde_json::json!("/bin/sh"),
        )
        .await
        .expect("shell");
        let added = crate::services::workspaces::add(&store, root.to_str().expect("utf8"), Some(3))
            .await
            .expect("ws");
        let tab = tabs::list(&store, &added.workspace.id)
            .await
            .expect("tabs")
            .remove(0);
        let pane = panes::create(&store, &tab.id, "terminal", Some("/does/not/exist".into()))
            .await
            .expect("pane");

        let svc = TerminalService::new(PtyManager::new(), store.clone());
        let sink = Arc::new(VecSink::default());
        let info = svc
            .spawn(&pane.id, 80, 24, Some(4), sink.clone())
            .await
            .expect("spawn");
        assert!(info.pid > 0);
        svc.pty
            .write(pane.id.as_str(), b"echo hi-$((40+2)); pwd\n")
            .expect("write");
        let text = || String::from_utf8_lossy(&sink.data()).into_owned();
        assert!(
            wait_until(Duration::from_secs(5), || text().contains("hi-42")),
            "{}",
            text()
        );
        assert!(
            text().contains("ws1"),
            "starts in the workspace root when cwd is gone"
        );
        let notices: Vec<String> = sink
            .frames
            .lock()
            .expect("frames")
            .iter()
            .filter(|f| f.kind == tandem_pty::FrameKind::Notice)
            .filter_map(|f| serde_json::from_slice::<serde_json::Value>(&f.payload).ok())
            .filter_map(|v| v["message"].as_str().map(str::to_string))
            .collect();
        assert_eq!(notices, vec![FOLDER_NOT_FOUND.to_string()]);
        svc.pty.kill(pane.id.as_str()).expect("kill");
    }
}
