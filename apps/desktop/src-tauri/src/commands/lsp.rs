//! Code intelligence in the editor and diffs (ARCHITECTURE §3.2 "Files,
//! editor, browser, scratchpad"; F-CD-23). Language servers run on this
//! machine only; a folder on an SSH host or in WSL has none, and none
//! starts before its repository is trusted (D-2580).

use std::path::{Path, PathBuf};

use tandem_core::origin::{LocalReview, Origin};
use tandem_core::{DataDir, ErrorCode, IpcError};
use tandem_store::repo::{threads as threads_repo, workspaces};
use tauri::{AppHandle, State, WebviewWindow};

use crate::commands::lanes::confirm_native;
use crate::services::files::FileContext;
use crate::services::lsp::{self, LspDocument, LspLocation, LspState, ServerSpec};
use crate::state::AppState;

const REMOTE: &str = "Code intelligence works on folders on this computer";
const OUTSIDE: &str = "Outside this folder: read only, without code intelligence";

/// A folder on this computer: not on an SSH host or in WSL.
async fn on_this_computer(app: &AppState, context: &FileContext) -> Result<bool, IpcError> {
    let ws_id = match context {
        FileContext::Workspace { workspace_id } => Some(workspace_id.clone()),
        FileContext::Thread { thread_id } => threads_repo::get(&app.store, thread_id)
            .await?
            .and_then(|t| t.workspace_id),
    };
    match ws_id {
        Some(id) => Ok(workspaces::get(&app.store, &id).await?.kind == "local"),
        None => Ok(true),
    }
}

/// Where a file's server runs: the context's folder, or the worktree a
/// path under `.tandem/worktrees/<slug>/` is in. The path is then relative
/// to it, and `prefix` turns the server's paths back into the context's.
struct Target {
    root: PathBuf,
    path: String,
    prefix: String,
}

fn target(root: PathBuf, path: String) -> Result<Target, IpcError> {
    // Checked whole first, so the worktree's name is a plain name.
    lsp::inside(&root, &path)?;
    let parts: Vec<&str> = path.splitn(4, '/').collect();
    if let [".tandem", "worktrees", slug, rest] = parts.as_slice() {
        if !slug.is_empty() && !rest.is_empty() {
            return Ok(Target {
                root: root.join(".tandem").join("worktrees").join(slug),
                prefix: format!(".tandem/worktrees/{slug}/"),
                path: (*rest).to_string(),
            });
        }
    }
    Ok(Target {
        root,
        path,
        prefix: String::new(),
    })
}

fn back(t: &Target, mut found: Vec<LspLocation>) -> Vec<LspLocation> {
    if !t.prefix.is_empty() {
        for l in found.iter_mut().filter(|l| l.in_folder) {
            l.path = format!("{}{}", t.prefix, l.path);
        }
    }
    found
}

/// Whose trust a folder's servers need (D-2580).
#[derive(Debug, Clone, PartialEq, Eq)]
struct TrustScope {
    /// The repository's main checkout, or the worktree itself.
    key: PathBuf,
    /// A worktree whose agents run in the F-TL-11 sandbox: it doesn't
    /// inherit its repository's trust, since what the agent wrote there
    /// would run outside the sandbox.
    sandboxed: bool,
}

/// The scope for a folder: its repository's (a worktree shares it), or its
/// own when agents run sandboxed and it's a worktree.
fn scope_for(root: &Path, sandbox_on: bool) -> TrustScope {
    let main = crate::services::worktree_setup::main_root(&root.to_string_lossy())
        .unwrap_or_else(|_| root.to_path_buf());
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let is_worktree = canon(&main) != canon(root);
    if sandbox_on && is_worktree {
        TrustScope {
            key: root.to_path_buf(),
            sandboxed: true,
        }
    } else {
        TrustScope {
            key: main,
            sandboxed: false,
        }
    }
}

async fn scope(app: &AppState, root: &Path) -> TrustScope {
    let sandbox_on = tandem_store::repo::settings::get(
        &app.store,
        tandem_core::settings::SettingKey::AgentSandbox,
    )
    .await
    .ok()
    .and_then(|v| v.as_str().map(str::to_string))
    .is_some_and(|v| v == "workspace");
    scope_for(root, sandbox_on)
}

/// The installed server that waits for trust, or `None` when the request
/// may go ahead. A server that isn't installed says so instead.
fn waits_for_trust(
    data: &DataDir,
    scope: &TrustScope,
    path: &str,
    available: impl Fn(&ServerSpec) -> bool,
) -> Option<&'static ServerSpec> {
    let spec = lsp::spec_for(path)?;
    (available(spec) && !lsp::is_trusted(data, &scope.key)).then_some(spec)
}

/// Where a request goes.
enum Where {
    /// A folder on an SSH host or in WSL.
    Remote,
    /// An absolute path outside the folder (a dependency's source, opened
    /// read only from a definition).
    Outside,
    Here(Target),
}

async fn locate(app: &AppState, context: &FileContext, path: String) -> Result<Where, IpcError> {
    if !on_this_computer(app, context).await? {
        return Ok(Where::Remote);
    }
    let root = app.files.root(context).await?;
    Ok(match target(root, path) {
        Ok(t) => Where::Here(t),
        Err(_) => Where::Outside,
    })
}

fn quiet_doc(message: &str) -> LspDocument {
    LspDocument {
        language: None,
        server: None,
        state: LspState::None,
        message: Some(message.to_string()),
        diagnostics: Vec::new(),
        sandboxed: false,
    }
}

/// The file's target, if its server may run now; else what to answer.
enum Ready {
    Go(Target, std::sync::Arc<tandem_pty::env::LoginEnv>),
    Untrusted(&'static ServerSpec, bool),
    Elsewhere(Where),
}

async fn ready(app: &AppState, context: &FileContext, path: String) -> Result<Ready, IpcError> {
    let t = match locate(app, context, path).await? {
        Where::Here(t) => t,
        other => return Ok(Ready::Elsewhere(other)),
    };
    let login = app.terminals.login_env().await;
    let scope = scope(app, &t.root).await;
    if let Some(spec) = waits_for_trust(&app.data, &scope, &t.path, |s| {
        lsp::global().available(s, &login)
    }) {
        return Ok(Ready::Untrusted(spec, scope.sandboxed));
    }
    Ok(Ready::Go(t, login))
}

#[tauri::command]
pub async fn lsp_sync(
    app: State<'_, AppState>,
    context: FileContext,
    path: String,
    text: String,
) -> Result<LspDocument, IpcError> {
    match ready(&app, &context, path).await? {
        Ready::Go(t, login) => lsp::global().sync(&t.root, &t.path, &text, &login).await,
        Ready::Untrusted(spec, sandboxed) => Ok(lsp::untrusted_doc(spec, sandboxed)),
        Ready::Elsewhere(Where::Outside) => Ok(quiet_doc(OUTSIDE)),
        Ready::Elsewhere(_) => Ok(quiet_doc(REMOTE)),
    }
}

#[tauri::command]
pub async fn lsp_definition(
    app: State<'_, AppState>,
    context: FileContext,
    path: String,
    text: String,
    line: u32,
    character: u32,
) -> Result<Vec<LspLocation>, IpcError> {
    match ready(&app, &context, path.clone()).await? {
        Ready::Go(t, login) => {
            let found = lsp::global()
                .definition(&t.root, &t.path, &text, line, character, &login)
                .await?;
            Ok(back(&t, found))
        }
        // The same-file fallback needs no server.
        _ => Ok(lsp::fallback_definition(&path, &text, line, character)),
    }
}

#[tauri::command]
pub async fn lsp_references(
    app: State<'_, AppState>,
    context: FileContext,
    path: String,
    text: String,
    line: u32,
    character: u32,
) -> Result<Vec<LspLocation>, IpcError> {
    match ready(&app, &context, path).await? {
        Ready::Go(t, login) => {
            let found = lsp::global()
                .references(&t.root, &t.path, &text, line, character, &login)
                .await?;
            Ok(back(&t, found))
        }
        _ => Ok(Vec::new()),
    }
}

#[tauri::command]
pub async fn lsp_hover(
    app: State<'_, AppState>,
    context: FileContext,
    path: String,
    text: String,
    line: u32,
    character: u32,
) -> Result<Option<String>, IpcError> {
    match ready(&app, &context, path).await? {
        Ready::Go(t, login) => {
            lsp::global()
                .hover(&t.root, &t.path, &text, line, character, &login)
                .await
        }
        _ => Ok(None),
    }
}

/// The editor closed the file (`textDocument/didClose`).
#[tauri::command]
pub async fn lsp_close(
    app: State<'_, AppState>,
    context: FileContext,
    path: String,
) -> Result<(), IpcError> {
    if let Where::Here(t) = locate(&app, &context, path).await? {
        lsp::global().close(&t.root, &t.path).await;
    }
    Ok(())
}

/// Files changed on disk in the context's folder (the Files watcher's
/// batch): its servers hear `workspace/didChangeWatchedFiles`.
#[tauri::command]
pub async fn lsp_files_changed(
    app: State<'_, AppState>,
    context: FileContext,
    paths: Vec<String>,
) -> Result<(), IpcError> {
    if !on_this_computer(&app, &context).await? {
        return Ok(());
    }
    let root = app.files.root(&context).await?;
    let mut by_root: std::collections::BTreeMap<PathBuf, Vec<String>> = Default::default();
    for p in paths.into_iter().take(2000) {
        if let Ok(t) = target(root.clone(), p) {
            by_root.entry(t.root).or_default().push(t.path);
        }
    }
    for (r, rels) in by_root {
        lsp::global().changed(&r, &rels).await;
    }
    Ok(())
}

/// Trusts the file's repository for language servers (D-2580), or a
/// sandboxed worktree on its own: from the main window, after the native
/// confirm.
#[tauri::command]
pub async fn lsp_trust(
    app_handle: AppHandle,
    window: WebviewWindow,
    app: State<'_, AppState>,
    context: FileContext,
    path: String,
) -> Result<(), IpcError> {
    let origin = Origin::Local {
        window: window.label().to_string(),
    };
    LocalReview::check(&origin).ok_or_else(|| {
        IpcError::new(
            ErrorCode::PermissionDenied,
            "Trusting a repository needs the main window",
        )
    })?;
    let Where::Here(t) = locate(&app, &context, path).await? else {
        return Err(IpcError::new(ErrorCode::Invalid, REMOTE).with_stage("lsp"));
    };
    let scope = scope(&app, &t.root).await;
    let name = scope
        .key
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| scope.key.to_string_lossy().into_owned());
    let (title, body) = if scope.sandboxed {
        (
            format!("Trust the worktree {name} for language servers?"),
            format!(
                "The agent's code will run outside its sandbox. rust-analyzer, \
                 typescript-language-server, pyright and gopls can run {name}'s build scripts, \
                 macros, plugins or Python interpreter when they load it. Trust it only if you've \
                 read what the agent wrote."
            ),
        )
    } else {
        (
            format!("Trust {name} for language servers?"),
            format!(
                "rust-analyzer, typescript-language-server, pyright and gopls can run {name}'s \
                 build scripts, macros, plugins or Python interpreter when they load it, and its \
                 worktrees' too. Trust it only if you trust its code."
            ),
        )
    };
    if !confirm_native(&app_handle, &title, &body, "Trust").await {
        return Err(IpcError::new(ErrorCode::Cancelled, "Not trusted"));
    }
    lsp::trust(&app.data, &scope.key)?;
    tracing::info!(
        code = "lsp.trusted",
        sandboxed = scope.sandboxed,
        "trusted for language servers"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_cd_23_a_worktree_file_gets_the_worktree_server() {
        let t = target(
            PathBuf::from("/code/shop"),
            ".tandem/worktrees/tidy/src/a.py".into(),
        )
        .expect("target");
        assert_eq!(t.root, PathBuf::from("/code/shop/.tandem/worktrees/tidy"));
        assert_eq!(t.path, "src/a.py");
        let found = back(
            &t,
            vec![LspLocation {
                path: "src/b.py".into(),
                in_folder: true,
                line: 0,
                character: 0,
                end_line: 0,
                end_character: 1,
                preview: String::new(),
            }],
        );
        assert_eq!(found[0].path, ".tandem/worktrees/tidy/src/b.py");
        let plain = target(PathBuf::from("/code/shop"), "src/a.py".into()).expect("plain");
        assert_eq!(plain.root, PathBuf::from("/code/shop"));
        assert!(target(
            PathBuf::from("/code/shop"),
            ".tandem/worktrees/../x.py".into()
        )
        .is_err());
    }

    #[test]
    fn f_cd_23_servers_that_run_the_repositorys_code_wait_for_its_trust() {
        let tmp = tempfile::tempdir().expect("tmp");
        let data = DataDir::at(tmp.path().join("data")).expect("data dir");
        let repo = std::fs::canonicalize(tmp.path())
            .expect("canonical")
            .join("shop");
        std::fs::create_dir_all(&repo).expect("mkdir");
        let git = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .status()
                .expect("git")
                .success();
            assert!(ok, "git {args:?}");
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["commit", "-q", "--allow-empty", "-m", "first"]);
        git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            "tandem/tidy",
            ".tandem/worktrees/tidy",
        ]);
        let installed = |_: &ServerSpec| true;
        let waits = |p: &str, sandbox: bool, avail: &dyn Fn(&ServerSpec) -> bool| {
            let t = target(repo.clone(), p.into()).expect("target");
            waits_for_trust(&data, &scope_for(&t.root, sandbox), &t.path, avail).is_some()
        };
        // Every server waits, pyright too.
        for p in ["src/main.rs", "web/app.ts", "cmd/main.go", "main.py"] {
            assert!(waits(p, false, &installed), "{p}");
        }
        // An uninstalled server says it's missing instead; other files have
        // no server.
        assert!(!waits("src/main.rs", false, &|_| false));
        assert!(!waits("notes.md", false, &installed));
        let doc = lsp::untrusted_doc(lsp::spec_for("a.rs").expect("spec"), false);
        assert_eq!(doc.state, LspState::Untrusted);
        assert_eq!(
            doc.message.as_deref(),
            Some("rust-analyzer is off until you trust this repository")
        );
        // Trusting the repository lets its servers start, its worktrees' too.
        lsp::trust(&data, &repo).expect("trust");
        assert!(!waits("src/main.rs", false, &installed));
        assert!(!waits(
            ".tandem/worktrees/tidy/src/main.rs",
            false,
            &installed
        ));
        let lane = repo.join(".tandem/worktrees/tidy");
        assert!(
            waits_for_trust(&data, &scope_for(&lane, false), "src/main.rs", installed).is_none()
        );
        // Another repository isn't covered.
        let other = tmp.path().join("other");
        std::fs::create_dir_all(&other).expect("mkdir");
        assert!(
            waits_for_trust(&data, &scope_for(&other, false), "src/main.rs", installed).is_some()
        );
        // With sandboxed agents, a worktree needs its own trust, and says
        // so; the repository itself keeps its trust.
        let sandboxed = scope_for(&lane, true);
        assert!(sandboxed.sandboxed);
        assert!(waits_for_trust(&data, &sandboxed, "src/main.rs", installed).is_some());
        assert!(!waits("src/main.rs", true, &installed));
        let doc = lsp::untrusted_doc(lsp::spec_for("a.rs").expect("spec"), true);
        assert_eq!(
            doc.message.as_deref(),
            Some("rust-analyzer is off until you trust this worktree")
        );
        lsp::trust(&data, &lane).expect("trust the worktree");
        assert!(waits_for_trust(&data, &sandboxed, "src/main.rs", installed).is_none());
    }
}
