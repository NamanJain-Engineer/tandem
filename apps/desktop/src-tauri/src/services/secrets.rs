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
    /// The value looks like a reference but isn't a valid one.
    pub malformed: bool,
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
    /// Why Run can't start here (the pane's terminals open in WSL, or its
    /// shell can't keep the values to one command).
    pub unavailable: Option<String>,
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
    // The row goes first: a keychain entry without a row would be invisible
    // on the Secrets page and never deleted.
    let existed = repo::exists(store, name).await?;
    let row = repo::upsert(store, name, now).await?;
    if let Err(e) = keychain.set(&service(), name, value.as_bytes()) {
        if !existed {
            let _ = repo::delete(store, name).await;
        }
        return Err(unavailable(e));
    }
    if value.len() >= REDACTOR_MIN {
        tandem_core::redact::Redactor::global().register_secret(value);
    }
    tracing::info!(code = "secrets.set", "a secret was saved");
    Ok(info(row))
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

/// The folder's `.env`, parsed; `None` without one. A symlink is followed
/// only to a file inside the folder, and the file is read through the one
/// handle that was checked, no further than the size limit.
pub(crate) fn read_env(folder: &Path) -> Result<Option<(PathBuf, Vec<EnvVar>)>, IpcError> {
    use std::io::Read;
    let path = folder.join(ENV_FILE);
    let Ok(link) = std::fs::symlink_metadata(&path) else {
        return Ok(None);
    };
    if link.file_type().is_symlink() {
        let inside = match (std::fs::canonicalize(&path), std::fs::canonicalize(folder)) {
            (Ok(target), Ok(root)) => target.starts_with(&root),
            _ => false,
        };
        if !inside {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "This folder's .env links to a file outside the folder",
            ));
        }
    }
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(read_failed(e)),
    };
    if !file.metadata().map_err(read_failed)?.is_file() {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    file.take(MAX_ENV_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(read_failed)?;
    if bytes.len() as u64 > MAX_ENV_BYTES {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "This folder's .env is larger than 256 KB",
        ));
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| IpcError::new(ErrorCode::Invalid, "This folder's .env isn't UTF-8 text"))?;
    Ok(Some((path, refs::parse_env(&text))))
}

fn read_failed(e: std::io::Error) -> IpcError {
    IpcError::new(ErrorCode::Io, format!("Couldn't read .env: {e}"))
}

pub async fn plan(
    store: &Store,
    pane: &tandem_core::ids::PaneId,
    unavailable: Option<&str>,
) -> Result<RunPlan, IpcError> {
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
            malformed: p.malformed,
        });
    }
    Ok(RunPlan {
        folder: folder.to_string_lossy().into_owned(),
        env_file,
        vars: out,
        unavailable: unavailable.map(str::to_string),
    })
}

/// `tab` is in the same workspace as `pane`: a run lands beside its source.
pub async fn check_tab(
    store: &Store,
    pane: &tandem_core::ids::PaneId,
    tab: &tandem_core::ids::TabId,
) -> Result<(), IpcError> {
    let source = panes::get(store, pane).await?;
    let tab = tandem_store::repo::tabs::get(store, tab).await?;
    if tab.workspace_id != source.workspace_id {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "The run opens in its source pane's workspace",
        ));
    }
    Ok(())
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
    let resolved = resolve(keychain, &vars)?;
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

/// `vars` with their references replaced from the keychain; values of 8+
/// bytes also go to the global redactor.
pub(crate) fn resolve(
    keychain: &dyn Keychain,
    vars: &[EnvVar],
) -> Result<refs::Resolved, IpcError> {
    let mut failed: Option<KeychainError> = None;
    let resolved = refs::resolve(vars, |name| match keychain.get(&service(), name) {
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
    let resolved = resolved.map_err(|e| match e {
        refs::ResolveError::Missing(_) => IpcError::new(ErrorCode::NotFound, e.to_string()),
        refs::ResolveError::Malformed(_) => IpcError::new(ErrorCode::Invalid, e.to_string()),
    })?;
    for v in &resolved.secrets {
        if v.len() >= REDACTOR_MIN {
            tandem_core::redact::Redactor::global().register_secret(v);
        }
    }
    Ok(resolved)
}

/// The paragraph "Tell agents in this folder" adds to `AGENTS.md`, between
/// its markers (F-TL-12 slice 2). No secret names.
pub const AGENTS_START: &str = "<!-- vibezeo:run -->";
pub const AGENTS_END: &str = "<!-- /vibezeo:run -->";
pub const AGENTS_PARAGRAPH: &str = "Commands that need secrets: run them as `vibezeo run -- <command>`; the values stay out of your context.";

/// The instructions file the paragraph goes in.
pub const AGENTS_FILE: &str = "AGENTS.md";

/// What "Tell agents in this folder" would change: shown as a diff first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentsMdPlan {
    pub path: String,
    /// The file as it is (empty when there's none).
    pub before: String,
    pub after: String,
    /// `before` → `after` as a unified diff, one hunk (for DiffView).
    pub patch: String,
    /// The paragraph is there already, as it would be written.
    pub unchanged: bool,
}

/// A unified diff of `before` → `after` as one hunk, with up to 3 lines of
/// context: what changes between their common first and last lines.
pub fn one_hunk(name: &str, before: &str, after: &str) -> String {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    if a == b {
        return String::new();
    }
    let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[head..]
        .iter()
        .rev()
        .zip(b[head..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let start = head.saturating_sub(3);
    let (a_end, b_end) = (a.len() - tail, b.len() - tail);
    let after_ctx = tail.min(3);
    let mut out = format!(
        "diff --git a/{name} b/{name}\n--- a/{name}\n+++ b/{name}\n@@ -{},{} +{},{} @@\n",
        start + 1,
        a_end + after_ctx - start,
        start + 1,
        b_end + after_ctx - start
    );
    for l in &a[start..head] {
        out.push_str(&format!(" {l}\n"));
    }
    for l in &a[head..a_end] {
        out.push_str(&format!("-{l}\n"));
    }
    for l in &b[head..b_end] {
        out.push_str(&format!("+{l}\n"));
    }
    for l in &a[a_end..a_end + after_ctx] {
        out.push_str(&format!(" {l}\n"));
    }
    out
}

/// `text` with the paragraph in it: the marked block replaced where it is,
/// or added at the end.
pub fn with_paragraph(text: &str) -> String {
    let block = format!("{AGENTS_START}\n{AGENTS_PARAGRAPH}\n{AGENTS_END}");
    if let (Some(a), Some(b)) = (text.find(AGENTS_START), text.find(AGENTS_END)) {
        if a < b {
            return format!("{}{block}{}", &text[..a], &text[b + AGENTS_END.len()..]);
        }
    }
    if text.trim().is_empty() {
        return format!("{block}\n");
    }
    let sep = if text.ends_with("\n\n") {
        ""
    } else if text.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    format!("{text}{sep}{block}\n")
}

/// The pane's folder's `AGENTS.md`, read the way `.env` is (a link must
/// stay inside the folder, 256 KB at most).
fn read_agents(folder: &Path) -> Result<(PathBuf, String), IpcError> {
    let path = folder.join(AGENTS_FILE);
    match std::fs::symlink_metadata(&path) {
        Err(_) => return Ok((path, String::new())),
        Ok(m) if m.file_type().is_symlink() => {
            let inside = matches!(
                (std::fs::canonicalize(&path), std::fs::canonicalize(folder)),
                (Ok(t), Ok(r)) if t.starts_with(&r)
            );
            if !inside {
                return Err(IpcError::new(
                    ErrorCode::Invalid,
                    "This folder's AGENTS.md links to a file outside the folder",
                ));
            }
        }
        Ok(_) => {}
    }
    use std::io::Read;
    let file = std::fs::File::open(&path).map_err(read_failed)?;
    let mut bytes = Vec::new();
    file.take(MAX_ENV_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(read_failed)?;
    if bytes.len() as u64 > MAX_ENV_BYTES {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "This folder's AGENTS.md is larger than 256 KB",
        ));
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| IpcError::new(ErrorCode::Invalid, "AGENTS.md isn't UTF-8 text"))?;
    Ok((path, text))
}

/// `secrets_agents_plan`.
pub async fn agents_plan(
    store: &Store,
    pane: &tandem_core::ids::PaneId,
) -> Result<AgentsMdPlan, IpcError> {
    let folder = folder_of(store, pane).await?;
    let (path, before) = read_agents(&folder)?;
    let after = with_paragraph(&before);
    Ok(AgentsMdPlan {
        path: path.to_string_lossy().into_owned(),
        patch: one_hunk(AGENTS_FILE, &before, &after),
        unchanged: after == before,
        before,
        after,
    })
}

/// `secrets_agents_apply`: writes the plan's `after`, if the file is still
/// what the person saw.
pub async fn agents_apply(
    store: &Store,
    pane: &tandem_core::ids::PaneId,
    before: &str,
) -> Result<AgentsMdPlan, IpcError> {
    let plan = agents_plan(store, pane).await?;
    if plan.before != before {
        return Err(IpcError::new(
            ErrorCode::Conflict,
            "AGENTS.md changed since you looked. Review it again",
        ));
    }
    if !plan.unchanged {
        tandem_core::atomic::atomic_write(Path::new(&plan.path), plan.after.as_bytes(), None)
            .map_err(|e| IpcError::new(ErrorCode::Io, format!("Couldn't write AGENTS.md: {e}")))?;
        tracing::info!(
            code = "secrets.agents_md",
            "AGENTS.md tells agents about vibezeo run"
        );
    }
    Ok(AgentsMdPlan {
        before: plan.after.clone(),
        patch: String::new(),
        unchanged: true,
        ..plan
    })
}

/// One repository's rules for `vibezeo run` (`run_rules_list`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunRuleRepo {
    /// The main checkout's folder.
    pub repo: String,
    pub name: String,
    pub commands: Vec<String>,
}

/// Every folder on this computer that isn't a worktree, with its rules.
pub async fn rules_list(store: &Store, data: &Path) -> Result<Vec<RunRuleRepo>, IpcError> {
    let rules = tandem_core::run_rules::RunRules::load(data);
    let mut out: Vec<RunRuleRepo> = workspaces::list(store)
        .await?
        .into_iter()
        .filter(|w| w.kind == "local" && w.parent_id.is_none())
        .map(|w| RunRuleRepo {
            commands: rules.repos.get(&w.root).cloned().unwrap_or_default(),
            repo: w.root,
            name: w.name,
        })
        .collect();
    out.sort_by_key(|a| a.name.to_lowercase());
    Ok(out)
}

/// Replaces `repo`'s rules: `repo` must be a folder on this computer.
pub async fn rules_set(
    store: &Store,
    data: &Path,
    repo: &str,
    commands: &[String],
) -> Result<RunRuleRepo, IpcError> {
    let ws = workspaces::list(store)
        .await?
        .into_iter()
        .find(|w| w.kind == "local" && w.parent_id.is_none() && w.root == repo)
        .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "That folder isn't in Vibezeo"))?;
    let mut rules = tandem_core::run_rules::RunRules::load(data);
    rules
        .set(repo, commands)
        .map_err(|e| IpcError::new(ErrorCode::Invalid, e.to_string()))?;
    rules
        .save(data)
        .map_err(|e| IpcError::new(ErrorCode::Io, format!("Couldn't save the rules: {e}")))?;
    Ok(RunRuleRepo {
        commands: rules.repos.get(repo).cloned().unwrap_or_default(),
        repo: ws.root,
        name: ws.name,
    })
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

    struct LockedKeychain;

    impl Keychain for LockedKeychain {
        fn get(&self, _: &str, _: &str) -> Result<Option<Zeroizing<Vec<u8>>>, KeychainError> {
            Err(KeychainError::Unavailable("locked".into()))
        }
        fn set(&self, _: &str, _: &str, _: &[u8]) -> Result<(), KeychainError> {
            Err(KeychainError::Unavailable("locked".into()))
        }
        fn delete(&self, _: &str, _: &str) -> Result<(), KeychainError> {
            Err(KeychainError::Unavailable("locked".into()))
        }
    }

    #[tokio::test]
    async fn f_tl_12_1_a_keychain_failure_leaves_no_row() {
        let (_d, store) = store().await;
        assert!(set(&store, &LockedKeychain, "a", "value-1", 1)
            .await
            .is_err());
        assert!(list(&store).await.expect("list").is_empty());
        // Replacing an existing secret keeps its row when the keychain fails.
        let kc = MemoryKeychain::new();
        set(&store, &kc, "b", "value-2", 1).await.expect("set");
        assert!(set(&store, &LockedKeychain, "b", "value-3", 2)
            .await
            .is_err());
        assert_eq!(list(&store).await.expect("list").len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn f_tl_12_2_a_dotenv_link_outside_the_folder_is_refused() {
        let dir = tempfile::tempdir().expect("dir");
        let root = dir.path().join("repo");
        std::fs::create_dir_all(root.join("conf")).expect("mkdir");
        std::fs::write(dir.path().join("outside.env"), "A=1\n").expect("write");
        std::os::unix::fs::symlink(dir.path().join("outside.env"), root.join(ENV_FILE))
            .expect("link");
        assert_eq!(
            read_env(&root).err().map(|e| e.code),
            Some(ErrorCode::Invalid)
        );
        std::fs::remove_file(root.join(ENV_FILE)).expect("rm");
        std::fs::write(root.join("conf/dev.env"), "B=2\n").expect("write");
        std::os::unix::fs::symlink(root.join("conf/dev.env"), root.join(ENV_FILE)).expect("link");
        let (_, vars) = read_env(&root).expect("read").expect("some");
        assert_eq!(vars[0].name, "B");
    }

    #[test]
    fn f_tl_12_2_a_dotenv_over_the_limit_is_refused() {
        let dir = tempfile::tempdir().expect("dir");
        std::fs::write(
            dir.path().join(ENV_FILE),
            vec![b'#'; MAX_ENV_BYTES as usize + 1],
        )
        .expect("write");
        assert_eq!(
            read_env(dir.path()).err().map(|e| e.code),
            Some(ErrorCode::Invalid)
        );
        std::fs::write(dir.path().join(ENV_FILE), "A=1\n").expect("write");
        assert!(read_env(dir.path()).expect("read").is_some());
        std::fs::remove_file(dir.path().join(ENV_FILE)).expect("rm");
        assert!(read_env(dir.path()).expect("read").is_none());
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

    #[test]
    fn f_tl_12_agents_md_gets_one_marked_paragraph() {
        let block = format!("{AGENTS_START}\n{AGENTS_PARAGRAPH}\n{AGENTS_END}");
        assert_eq!(with_paragraph(""), format!("{block}\n"));
        assert_eq!(with_paragraph("# Rules"), format!("# Rules\n\n{block}\n"));
        assert_eq!(with_paragraph("# Rules\n"), format!("# Rules\n\n{block}\n"));
        let once = with_paragraph("# Rules\n");
        assert_eq!(with_paragraph(&once), once);
        let old = format!("a\n{AGENTS_START}\nold words\n{AGENTS_END}\nb\n");
        assert_eq!(with_paragraph(&old), format!("a\n{block}\nb\n"));
        assert!(!AGENTS_PARAGRAPH.contains("vibezeo-secret://"));
    }

    #[test]
    fn f_tl_12_the_agents_md_change_is_one_hunk() {
        assert_eq!(one_hunk("A", "x\n", "x\n"), "");
        assert_eq!(
            one_hunk("AGENTS.md", "", "a\nb\n"),
            "diff --git a/AGENTS.md b/AGENTS.md\n--- a/AGENTS.md\n+++ b/AGENTS.md\n@@ -1,0 +1,2 @@\n+a\n+b\n"
        );
        let before = "1\n2\n3\n4\nold\n5\n6\n7\n8\n";
        let after = "1\n2\n3\n4\nnew\nmore\n5\n6\n7\n8\n";
        assert_eq!(
            one_hunk("f", before, after),
            "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -2,7 +2,8 @@\n 2\n 3\n 4\n-old\n+new\n+more\n 5\n 6\n 7\n"
        );
    }

    #[tokio::test]
    async fn f_tl_12_tell_agents_shows_the_change_first_and_writes_only_what_was_seen() {
        let (dir, store) = store().await;
        let (root, pane) = folder(&store, dir.path(), "A=1\n").await;
        let plan = agents_plan(&store, &pane.id).await.expect("plan");
        assert_eq!(plan.before, "");
        assert!(!plan.unchanged);
        assert!(
            !root.join(AGENTS_FILE).exists(),
            "nothing written by a plan"
        );
        std::fs::write(root.join(AGENTS_FILE), "# Mine\n").expect("edit");
        assert_eq!(
            agents_apply(&store, &pane.id, &plan.before)
                .await
                .err()
                .map(|e| e.code),
            Some(ErrorCode::Conflict)
        );
        let plan = agents_plan(&store, &pane.id).await.expect("plan");
        agents_apply(&store, &pane.id, &plan.before)
            .await
            .expect("apply");
        let text = std::fs::read_to_string(root.join(AGENTS_FILE)).expect("read");
        assert!(text.starts_with("# Mine\n\n<!-- vibezeo:run -->"), "{text}");
        assert!(agents_plan(&store, &pane.id).await.expect("plan").unchanged);
    }

    #[tokio::test]
    async fn f_tl_12_9_rules_are_set_per_folder_on_this_computer() {
        let (dir, store) = store().await;
        let (root, _) = folder(&store, dir.path(), "A=1\n").await;
        let data = dir.path().join("data");
        let list = rules_list(&store, &data).await.expect("list");
        assert_eq!(list.len(), 1);
        assert!(list[0].commands.is_empty());
        let set = rules_set(&store, &data, &list[0].repo, &["pnpm deploy".into()])
            .await
            .expect("set");
        assert_eq!(set.commands, vec!["pnpm deploy"]);
        assert_eq!(
            rules_list(&store, &data).await.expect("list")[0].commands,
            vec!["pnpm deploy"]
        );
        assert_eq!(
            rules_set(&store, &data, &list[0].repo, &["make; ls".into()])
                .await
                .err()
                .map(|e| e.code),
            Some(ErrorCode::Invalid)
        );
        assert_eq!(
            rules_set(&store, &data, "/elsewhere", &["make".into()])
                .await
                .err()
                .map(|e| e.code),
            Some(ErrorCode::NotFound)
        );
        assert!(root.is_dir());
    }

    #[tokio::test]
    async fn f_tl_12_2_a_run_opens_only_in_its_source_panes_workspace() {
        let (dir, store) = store().await;
        let (_, pane) = folder(&store, dir.path(), "A=1\n").await;
        check_tab(&store, &pane.id, &pane.tab_id)
            .await
            .expect("own tab");
        let other = dir.path().join("other");
        std::fs::create_dir_all(&other).expect("mkdir");
        let added =
            crate::services::workspaces::add(&store, other.to_str().expect("utf8"), Some(4))
                .await
                .expect("ws");
        let foreign = tandem_store::repo::tabs::list(&store, &added.workspace.id)
            .await
            .expect("tabs")
            .remove(0);
        assert_eq!(
            check_tab(&store, &pane.id, &foreign.id)
                .await
                .err()
                .map(|e| e.code),
            Some(ErrorCode::Invalid)
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn f_tl_12_5_run_is_refused_where_the_shell_cant_keep_the_values_to_the_command() {
        use tandem_core::settings::SettingKey;
        let (_d, store) = store().await;
        let svc = crate::services::terminal::TerminalService::new(
            tandem_pty::PtyManager::new(),
            store.clone(),
        );
        tandem_store::repo::settings::set(
            &store,
            SettingKey::DefaultShell,
            serde_json::json!("/bin/sh"),
        )
        .await
        .expect("shell");
        assert_eq!(svc.run_blocked().await, None);
        tandem_store::repo::settings::set(
            &store,
            SettingKey::DefaultShell,
            serde_json::json!("/opt/cmd.exe"),
        )
        .await
        .expect("shell");
        assert_eq!(
            svc.run_blocked().await,
            Some(crate::services::terminal::RUN_NEEDS_SHELL)
        );
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
        let plan = plan(&store, &pane.id, None).await.expect("plan");
        assert_eq!(
            plan.vars,
            vec![
                RunVar {
                    name: "API_KEY".into(),
                    secret: Some("stripe-test".into()),
                    missing: false,
                    malformed: false,
                },
                RunVar {
                    name: "PORT".into(),
                    secret: None,
                    missing: false,
                    malformed: false,
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
        let plan = plan(&store, &pane.id, None).await.expect("plan");
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
