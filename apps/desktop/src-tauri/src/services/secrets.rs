//! Agent-proof secrets (F-TL-12, ARCHITECTURE §6.3c): names in the `secrets`
//! table, values in the keychain (`NS.secrets/<name>`). A folder's `.env`
//! refers to them as `vibezeo-secret://<name>`; Run with secrets resolves
//! them for one command in a new pane and masks them there.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zeroize::Zeroizing;

use tandem_core::error::{ErrorCode, IpcError};
use tandem_core::keychain::{self, Keychain, KeychainError, MAX_SECRET_BYTES};
use tandem_core::secrets::{self as refs, EnvVar};
use tandem_store::repo::{panes, secrets as repo, workspaces};
use tandem_store::Store;

/// The `.env` read for a run.
pub const ENV_FILE: &str = ".env";

/// A `.env` larger than this isn't read.
const MAX_ENV_BYTES: u64 = 256 * 1024;

/// Values this long also go to the global redactor (logs, uploads).
const REDACTOR_MIN: usize = 8;

/// A secret on the Secrets page: never its value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SecretInfo {
    pub name: String,
    /// Unix ms.
    #[ts(type = "number")]
    pub updated_at: i64,
}

/// One variable a run would set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunVar {
    pub name: String,
    /// The secret a reference names.
    pub secret: Option<String>,
    /// That secret doesn't exist.
    pub missing: bool,
}

/// What Run with secrets would use (`secrets_run_plan`): no values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunPlan {
    pub folder: String,
    /// The `.env` path, when the folder has one.
    pub env_file: Option<String>,
    pub vars: Vec<RunVar>,
}

/// A run waiting for its pane's first spawn.
pub struct PendingRun {
    pub command: String,
    /// Every `.env` variable, references resolved.
    pub env: Vec<(String, String)>,
    /// The values masked in the pane.
    pub mask: Vec<Zeroizing<Vec<u8>>>,
}

fn service() -> String {
    keychain::service("secrets")
}

fn unavailable(e: KeychainError) -> IpcError {
    match e {
        KeychainError::TooLarge => IpcError::new(ErrorCode::Invalid, "A value can be up to 16 KB"),
        e => IpcError::new(ErrorCode::Unavailable, e.to_string()),
    }
}

fn info(row: repo::SecretRow) -> SecretInfo {
    SecretInfo {
        name: row.name,
        updated_at: row.updated_at,
    }
}

pub async fn list(store: &Store) -> Result<Vec<SecretInfo>, IpcError> {
    Ok(repo::list(store).await?.into_iter().map(info).collect())
}

/// Adds a secret, or replaces its value.
pub async fn set(
    store: &Store,
    keychain: &dyn Keychain,
    name: &str,
    value: &str,
    now: i64,
) -> Result<SecretInfo, IpcError> {
    if !refs::valid_name(name) {
        return Err(IpcError::new(ErrorCode::Invalid, refs::NAME_RULES));
    }
    if value.is_empty() {
        return Err(IpcError::new(ErrorCode::Invalid, "Enter a value"));
    }
    if value.len() > MAX_SECRET_BYTES {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "A value can be up to 16 KB",
        ));
    }
    keychain
        .set(&service(), name, value.as_bytes())
        .map_err(unavailable)?;
    if value.len() >= REDACTOR_MIN {
        tandem_core::redact::Redactor::global().register_secret(value);
    }
    tracing::info!(code = "secrets.set", "a secret was saved");
    Ok(info(repo::upsert(store, name, now).await?))
}

pub async fn delete(store: &Store, keychain: &dyn Keychain, name: &str) -> Result<(), IpcError> {
    if !repo::exists(store, name).await? {
        return Err(IpcError::new(
            ErrorCode::NotFound,
            format!("No secret named {name}"),
        ));
    }
    keychain.delete(&service(), name).map_err(unavailable)?;
    repo::delete(store, name).await?;
    tracing::info!(code = "secrets.delete", "a secret was deleted");
    Ok(())
}

/// Every secret's keychain entry (Forget data, F-TL-05).
pub fn forget_all(keychain: &dyn Keychain, names: &[String]) {
    for name in names {
        let _ = keychain.delete(&service(), name);
    }
}

/// The pane's folder: a terminal pane in a folder on this computer.
async fn folder_of(store: &Store, pane: &tandem_core::ids::PaneId) -> Result<PathBuf, IpcError> {
    let record = panes::get(store, pane).await?;
    if record.kind != "terminal" {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "Run with secrets starts from a terminal pane",
        ));
    }
    let ws = workspaces::get(store, &record.workspace_id).await?;
    if ws.kind != "local" {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "Run with secrets needs a folder on this computer",
        ));
    }
    Ok(PathBuf::from(ws.root))
}

/// The folder's `.env`, parsed; `None` without one.
fn read_env(folder: &Path) -> Result<Option<(PathBuf, Vec<EnvVar>)>, IpcError> {
    let path = folder.join(ENV_FILE);
    let Ok(meta) = std::fs::metadata(&path) else {
        return Ok(None);
    };
    if !meta.is_file() {
        return Ok(None);
    }
    if meta.len() > MAX_ENV_BYTES {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "This folder's .env is larger than 256 KB",
        ));
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| IpcError::new(ErrorCode::Io, format!("Couldn't read .env: {e}")))?;
    Ok(Some((path, refs::parse_env(&text))))
}

pub async fn plan(store: &Store, pane: &tandem_core::ids::PaneId) -> Result<RunPlan, IpcError> {
    let folder = folder_of(store, pane).await?;
    let (env_file, vars) = match read_env(&folder)? {
        Some((path, vars)) => (Some(path.to_string_lossy().into_owned()), vars),
        None => (None, Vec::new()),
    };
    let mut out = Vec::new();
    for p in refs::plan(&vars) {
        let missing = match &p.secret {
            Some(name) => !repo::exists(store, name).await?,
            None => false,
        };
        out.push(RunVar {
            name: p.name,
            secret: p.secret,
            missing,
        });
    }
    Ok(RunPlan {
        folder: folder.to_string_lossy().into_owned(),
        env_file,
        vars: out,
    })
}

/// Resolves the folder's `.env` for `command`: the folder and the pending run.
pub async fn prepare(
    store: &Store,
    keychain: &dyn Keychain,
    pane: &tandem_core::ids::PaneId,
    command: &str,
) -> Result<(PathBuf, PendingRun), IpcError> {
    let command = command.trim();
    if command.is_empty() {
        return Err(IpcError::new(ErrorCode::Invalid, "Enter a command to run"));
    }
    if tandem_core::sandbox::in_flatpak() {
        // flatpak-spawn would carry the values on its command line.
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "Run with secrets isn't available in the Flatpak build yet",
        ));
    }
    let folder = folder_of(store, pane).await?;
    let vars = read_env(&folder)?.map(|(_, v)| v).unwrap_or_default();
    let mut failed: Option<KeychainError> = None;
    let resolved = refs::resolve(&vars, |name| match keychain.get(&service(), name) {
        Ok(Some(v)) => String::from_utf8(v.to_vec()).ok(),
        Ok(None) => None,
        Err(e) => {
            failed = Some(e);
            None
        }
    });
    if let Some(e) = failed {
        return Err(unavailable(e));
    }
    let resolved = resolved.map_err(|m| IpcError::new(ErrorCode::NotFound, m.to_string()))?;
    for v in &resolved.secrets {
        if v.len() >= REDACTOR_MIN {
            tandem_core::redact::Redactor::global().register_secret(v);
        }
    }
    let mask = resolved
        .secrets
        .iter()
        .map(|v| Zeroizing::new(v.as_bytes().to_vec()))
        .collect();
    Ok((
        folder,
        PendingRun {
            command: command.to_string(),
            env: resolved.env.into_iter().collect(),
            mask,
        },
    ))
}

/// The run pane's name: the command, shortened.
pub fn pane_name(command: &str) -> String {
    let command = command.trim();
    let mut name: String = command.chars().take(40).collect();
    if command.chars().count() > 40 {
        name.push('…');
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::keychain::MemoryKeychain;

    async fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("dir");
        let data = tandem_core::DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        (dir, store)
    }

    #[tokio::test]
    async fn f_tl_12_1_the_value_goes_to_the_keychain_and_only_the_name_to_the_db() {
        let (_d, store) = store().await;
        let kc = MemoryKeychain::new();
        let got = set(&store, &kc, "stripe-test", "sk_test_42abc", 5)
            .await
            .expect("set");
        assert_eq!(
            got,
            SecretInfo {
                name: "stripe-test".into(),
                updated_at: 5
            }
        );
        assert_eq!(
            kc.get(&service(), "stripe-test")
                .expect("get")
                .map(|v| v.to_vec()),
            Some(b"sk_test_42abc".to_vec())
        );
        assert_eq!(list(&store).await.expect("list"), vec![got]);
        assert!(set(&store, &kc, "Bad Name", "x", 5).await.is_err());
        assert!(set(&store, &kc, "empty", "", 5).await.is_err());
    }

    #[tokio::test]
    async fn f_tl_12_6_delete_removes_the_entry_and_the_row() {
        let (_d, store) = store().await;
        let kc = MemoryKeychain::new();
        set(&store, &kc, "a", "value-1", 1).await.expect("set");
        set(&store, &kc, "b", "value-2", 1).await.expect("set");
        delete(&store, &kc, "a").await.expect("delete");
        assert_eq!(kc.get(&service(), "a").expect("get"), None);
        assert_eq!(list(&store).await.expect("list").len(), 1);
        assert_eq!(
            delete(&store, &kc, "a").await.err().map(|e| e.code),
            Some(ErrorCode::NotFound)
        );
        forget_all(&kc, &["b".to_string()]);
        assert_eq!(kc.get(&service(), "b").expect("get"), None);
    }

    /// A workspace whose `.env` refers to `stripe-test`, and a terminal pane in it.
    #[cfg(unix)]
    async fn folder(
        store: &Store,
        dir: &Path,
        env: &str,
    ) -> (PathBuf, tandem_store::repo::panes::PaneRecord) {
        let root = dir.join("app");
        std::fs::create_dir_all(&root).expect("mkdir");
        std::fs::write(root.join(ENV_FILE), env).expect(".env");
        tandem_store::repo::settings::set(
            store,
            tandem_core::settings::SettingKey::DefaultShell,
            serde_json::json!("/bin/sh"),
        )
        .await
        .expect("shell");
        let added = crate::services::workspaces::add(store, root.to_str().expect("utf8"), Some(3))
            .await
            .expect("ws");
        let tab = tandem_store::repo::tabs::list(store, &added.workspace.id)
            .await
            .expect("tabs")
            .remove(0);
        let pane = panes::create(store, &tab.id, "terminal", None)
            .await
            .expect("pane");
        (root, pane)
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn f_tl_12_2_f_tl_12_3_f_tl_12_5_a_run_gets_the_value_masks_it_and_the_shell_after_it_doesnt(
    ) {
        use std::sync::Arc;
        use std::time::Duration;
        use tandem_pty::{wait_until, PtyManager, VecSink};
        let (dir, store) = store().await;
        let kc = MemoryKeychain::new();
        set(&store, &kc, "stripe-test", "sk_test_4242xyz", 1)
            .await
            .expect("set");
        let env = "API_KEY=vibezeo-secret://stripe-test\nPORT=3000\n";
        let (root, pane) = folder(&store, dir.path(), env).await;
        let plan = plan(&store, &pane.id).await.expect("plan");
        assert_eq!(
            plan.vars,
            vec![
                RunVar {
                    name: "API_KEY".into(),
                    secret: Some("stripe-test".into()),
                    missing: false
                },
                RunVar {
                    name: "PORT".into(),
                    secret: None,
                    missing: false
                },
            ]
        );
        let (folder, run) = prepare(
            &store,
            &kc,
            &pane.id,
            "test \"$API_KEY\" = sk_test_4242xyz && echo \"ok $PORT $API_KEY\"",
        )
        .await
        .expect("prepared");
        assert_eq!(folder, root);
        let tab = pane.tab_id.clone();
        let run_pane = panes::create_with(
            &store,
            &tab,
            "terminal",
            Some(root.to_string_lossy().into_owned()),
            None,
        )
        .await
        .expect("run pane");
        let svc = crate::services::terminal::TerminalService::new(PtyManager::new(), store.clone());
        svc.set_pending_run(&run_pane.id, run);
        let sink = Arc::new(VecSink::default());
        svc.spawn(&run_pane.id, 120, 24, None, sink.clone())
            .await
            .expect("spawn");
        let text = || String::from_utf8_lossy(&sink.data()).into_owned();
        assert!(
            wait_until(Duration::from_secs(10), || text()
                .contains("ok 3000 [REDACTED]")),
            "{}",
            text()
        );
        assert!(!text().contains("sk_test_4242xyz"));
        std::thread::sleep(Duration::from_millis(500));
        svc.pty
            .write(run_pane.id.as_str(), b"echo \"after=${API_KEY:-none}\"\n")
            .expect("write");
        assert!(
            wait_until(Duration::from_secs(10), || text().contains("after=none")),
            "{}",
            text()
        );
        // The file still holds the reference.
        assert_eq!(
            std::fs::read_to_string(root.join(ENV_FILE)).expect("read"),
            env
        );
        let _ = svc.pty.kill(run_pane.id.as_str());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn f_tl_12_4_a_missing_secret_is_named_and_nothing_is_prepared() {
        let (dir, store) = store().await;
        let kc = MemoryKeychain::new();
        let (_root, pane) = folder(&store, dir.path(), "A=vibezeo-secret://gone\n").await;
        let plan = plan(&store, &pane.id).await.expect("plan");
        assert!(plan.vars[0].missing);
        let err = prepare(&store, &kc, &pane.id, "true")
            .await
            .err()
            .expect("refused");
        assert_eq!(
            (err.code, err.message.as_str()),
            (ErrorCode::NotFound, "No secret named gone")
        );
    }

    #[test]
    fn run_panes_are_named_after_the_command() {
        assert_eq!(pane_name("  pnpm dev "), "pnpm dev");
        assert_eq!(pane_name(&"x".repeat(50)).chars().count(), 41);
    }
}
