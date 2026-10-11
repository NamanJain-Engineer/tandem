//! Command recall (F-EX-02): every command a pane's shell ran, redacted and
//! kept on this machine, searchable from the recall palette. Nothing here
//! makes a network request.

use std::sync::Arc;

use tandem_core::clock::Clock;
use tandem_core::ids::PaneId;
use tandem_core::redact::Redactor;
use tandem_core::settings::SettingKey;
use tandem_core::{ErrorCode, IpcError};
use tandem_pty::{PtyManager, Ran};
use tandem_store::repo::command_history::{self as history, HistoryEntry, NewRun};
use tandem_store::repo::{panes, settings};
use tandem_store::Store;

const DAY_MS: i64 = 86_400_000;

pub const NOT_A_SHELL: &str =
    "Run works in a terminal pane at its prompt. Insert the command instead.";
pub const SECRET: &str = "This command has a hidden secret. Insert it and fill in the secret.";

pub struct RecallService {
    store: Store,
    clock: Arc<dyn Clock>,
}

impl RecallService {
    pub fn new(store: Store, clock: Arc<dyn Clock>) -> Arc<Self> {
        Arc::new(Self { store, clock })
    }

    async fn enabled(&self) -> bool {
        settings::get(&self.store, SettingKey::CommandHistoryEnabled)
            .await
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
    }

    /// Stores a finished command, redacted, unless recall is off.
    pub async fn record(&self, pane: &str, ran: Ran) {
        if !self.enabled().await {
            return;
        }
        let redacted = Redactor::global().redact(&ran.command);
        let secret = redacted != ran.command;
        let command = redacted.into_owned();
        let record = panes::get(&self.store, &PaneId::from_db(pane.to_string()))
            .await
            .ok();
        let cwd = ran
            .cwd
            .or_else(|| record.as_ref().and_then(|r| r.cwd.clone()))
            .unwrap_or_default();
        let run = NewRun {
            pane_id: pane.to_string(),
            workspace_id: record.map(|r| r.workspace_id.as_str().to_string()),
            cwd,
            command,
            secret,
            exit_code: ran.exit,
            started_at: ran.started_at,
            duration_ms: ran.duration_ms,
        };
        if let Err(err) = history::insert(&self.store, run).await {
            tracing::warn!(code = "recall.store_failed", error = %err, "couldn't record a command");
        }
    }

    /// `history_search`, with each open pane's name.
    pub async fn search(
        &self,
        query: String,
        workspace_id: Option<String>,
        limit: u32,
    ) -> Result<Vec<HistoryEntry>, IpcError> {
        let mut rows = history::search(&self.store, query, workspace_id, limit).await?;
        for row in &mut rows {
            if let Ok(p) = panes::get(&self.store, &PaneId::from_db(row.pane_id.clone())).await {
                row.pane_name = p.human_name;
            }
        }
        Ok(rows)
    }

    /// `history_run`: the command and Enter, into a shell at its prompt.
    pub async fn run(&self, pty: &PtyManager, id: i64, pane: &PaneId) -> Result<(), IpcError> {
        let (command, secret) = history::command_of(&self.store, id).await?.ok_or_else(|| {
            IpcError::new(ErrorCode::NotFound, "That command is gone").with_stage("recall")
        })?;
        if secret {
            return Err(IpcError::new(ErrorCode::Invalid, SECRET).with_stage("recall"));
        }
        let record = panes::get_open(&self.store, pane).await?;
        if record.kind != "terminal" || !pty.at_prompt(pane.as_str()) {
            return Err(IpcError::new(ErrorCode::Invalid, NOT_A_SHELL).with_stage("recall"));
        }
        pty.write(pane.as_str(), format!("{command}\r").as_bytes())?;
        Ok(())
    }

    pub async fn delete(&self, id: i64) -> Result<(), IpcError> {
        Ok(history::delete_like(&self.store, id).await?)
    }

    pub async fn clear(&self) -> Result<u32, IpcError> {
        Ok(history::clear(&self.store).await?)
    }

    /// Drops what the retention setting no longer keeps (startup, hourly).
    pub async fn prune(&self) {
        let days = settings::get(&self.store, SettingKey::CommandHistoryDays)
            .await
            .ok()
            .and_then(|v| v.as_i64())
            .unwrap_or(90);
        let before = self.clock.now_ms() - days * DAY_MS;
        if let Err(err) = history::prune(&self.store, before).await {
            tracing::warn!(code = "recall.prune_failed", error = %err, "couldn't prune command history");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::clock::MockClock;
    use tandem_core::DataDir;

    async fn service() -> (tempfile::TempDir, Arc<RecallService>, MockClock) {
        let tmp = tempfile::tempdir().expect("tmp");
        let store =
            Store::open(&DataDir::at(tmp.path().to_path_buf()).expect("data")).expect("store");
        let clock = MockClock::new(chrono::Utc::now());
        (
            tmp,
            RecallService::new(store, Arc::new(clock.clone())),
            clock,
        )
    }

    fn ran(cmd: &str, at: i64) -> Ran {
        Ran {
            command: cmd.into(),
            cwd: Some("/w".into()),
            exit: Some(0),
            started_at: at,
            duration_ms: Some(5),
        }
    }

    #[tokio::test]
    async fn f_ex_02_2_secrets_are_redacted_before_they_are_stored() {
        let (tmp, svc, clock) = service().await;
        let token = "ghp_0123456789abcdefghijABCDEFGHIJ012345";
        svc.record(
            "pn_x",
            ran(&format!("export GITHUB_TOKEN={token}"), clock.now_ms()),
        )
        .await;
        let rows = svc.search(String::new(), None, 10).await.expect("search");
        assert_eq!(rows.len(), 1);
        assert!(rows[0].secret);
        assert!(
            rows[0].command.contains("[REDACTED]"),
            "{}",
            rows[0].command
        );
        assert!(!rows[0].command.contains(token));
        // Not in the database file either (WAL included).
        for entry in std::fs::read_dir(tmp.path()).expect("dir").flatten() {
            if entry.path().is_file() {
                let bytes = std::fs::read(entry.path()).expect("read");
                assert!(
                    !bytes.windows(token.len()).any(|w| w == token.as_bytes()),
                    "{:?} holds the token",
                    entry.path()
                );
            }
        }
        // Run refuses it.
        let pty = PtyManager::new();
        let err = svc
            .run(&pty, rows[0].id, &PaneId::new())
            .await
            .expect_err("refused");
        assert_eq!(err.message, SECRET);
    }

    #[tokio::test]
    async fn f_ex_02_5_off_records_nothing_and_old_commands_are_pruned() {
        let (_tmp, svc, clock) = service().await;
        settings::set(
            &svc.store,
            SettingKey::CommandHistoryEnabled,
            serde_json::json!(false),
        )
        .await
        .expect("set");
        svc.record("pn_x", ran("ls", clock.now_ms())).await;
        assert!(svc
            .search(String::new(), None, 10)
            .await
            .expect("s")
            .is_empty());
        settings::set(
            &svc.store,
            SettingKey::CommandHistoryEnabled,
            serde_json::json!(true),
        )
        .await
        .expect("set");
        svc.record("pn_x", ran("old", clock.now_ms() - 91 * DAY_MS))
            .await;
        svc.record("pn_x", ran("new", clock.now_ms())).await;
        svc.prune().await;
        let rows = svc.search(String::new(), None, 10).await.expect("s");
        assert_eq!(
            rows.iter().map(|r| r.command.as_str()).collect::<Vec<_>>(),
            ["new"]
        );
        assert_eq!(svc.clear().await.expect("clear"), 1);
    }
}
