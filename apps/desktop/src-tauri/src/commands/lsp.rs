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
    // Checked whole first, so every component is a plain name.
    lsp::inside(&root, &path)?;
    let parts: Vec<&std::ffi::OsStr> = Path::new(&path)
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(n) => Some(n),
            _ => None,
        })
        .collect();
    if let [tandem, worktrees, slug, rest @ ..] = parts.as_slice() {
        if *tandem == ".tandem" && *worktrees == "worktrees" && !rest.is_empty() {
            let slug = slug.to_string_lossy();
            let rest: PathBuf = rest.iter().collect();
            return Ok(Target {
                root: root.join(".tandem").join("worktrees").join(slug.as_ref()),
                prefix: format!(".tandem/worktrees/{slug}/"),
                path: rest.to_string_lossy().replace('\\', "/"),
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
    /// The repository's main checkout (or a multi-repo folder), or the
    /// worktree itself.
    key: PathBuf,
    /// A worktree whose agents run in the F-TL-11 sandbox: it doesn't
    /// inherit its repository's trust, since what the agent wrote there
    /// would run outside the sandbox.
    sandboxed: bool,
    /// The sandbox is on: the repository's trust doesn't reach its
    /// worktrees.
    sandbox_on: bool,
}

/// The worktree folder `root` is or is inside, by Vibezeo's layout
/// (`<owner>/.tandem/worktrees/<slug>`), with its owner folder.
fn worktree_of(root: &Path) -> Option<(PathBuf, PathBuf)> {
    let comps: Vec<std::path::Component> = root.components().collect();
    for i in 0..comps.len().saturating_sub(2) {
        let is =
            |j: usize, name: &str| matches!(comps[j], std::path::Component::Normal(n) if n == name);
        if is(i, ".tandem") && is(i + 1, "worktrees") {
            // The slug is a plain name, and nothing after it climbs out.
            let plain = |c: &std::path::Component| matches!(c, std::path::Component::Normal(_));
            if !plain(&comps[i + 2]) || !comps[i + 2..].iter().all(plain) {
                return None;
            }
            let owner: PathBuf = comps[..i].iter().collect();
            let lane: PathBuf = comps[..i + 3].iter().collect();
            return Some((owner, lane));
        }
    }
    None
}

/// The scope for a folder: its repository's (a worktree shares it, a
/// multi-repo worktree its folder's), or its own when agents run sandboxed
/// and it's a worktree.
fn scope_for(root: &Path, sandbox_on: bool) -> TrustScope {
    let main_of = |p: &Path| {
        crate::services::worktree_setup::main_root(&p.to_string_lossy())
            .unwrap_or_else(|_| p.to_path_buf())
    };
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let (main, lane) = match worktree_of(root) {
        // Vibezeo's worktree, single- or multi-repo: its owner's trust.
        Some((owner, lane)) => (main_of(&owner), Some(lane)),
        None => {
            let main = main_of(root);
            // A git worktree elsewhere.
            let lane = (canon(&main) != canon(root)).then(|| root.to_path_buf());
            (main, lane)
        }
    };
    match lane {
        Some(lane) if sandbox_on => TrustScope {
            key: lane,
            sandboxed: true,
            sandbox_on,
        },
        _ => TrustScope {
            key: main,
            sandboxed: false,
            sandbox_on,
        },
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

/// Whether a file's server may start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gate {
    /// No server for the file, or trusted: ask.
    Go,
    /// Not trusted (whether or not the server is installed, so a server
    /// installed meanwhile never starts untrusted).
    Untrusted(&'static ServerSpec),
    /// Not trusted and not installed: say what to install.
    Missing(&'static ServerSpec),
}

fn gate(data: &DataDir, scope: &TrustScope, path: &str, installed: bool) -> Gate {
    let Some(spec) = lsp::spec_for(path) else {
        return Gate::Go;
    };
    if lsp::is_trusted(data, &scope.key) {
        Gate::Go
    } else if installed {
        Gate::Untrusted(spec)
    } else {
        Gate::Missing(spec)
    }
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
    Missing(&'static ServerSpec),
    Elsewhere(Where),
}

async fn ready(app: &AppState, context: &FileContext, path: String) -> Result<Ready, IpcError> {
    let t = match locate(app, context, path).await? {
        Where::Here(t) => t,
        other => return Ok(Ready::Elsewhere(other)),
    };
    let login = app.terminals.login_env().await;
    let scope = scope(app, &t.root).await;
    // The probe (`rust-analyzer --version`) blocks: off the runtime. It's
    // cached, so the server's own start finds the answer.
    let installed = match lsp::spec_for(&t.path) {
        Some(spec) => {
            let login = login.clone();
            tokio::task::spawn_blocking(move || lsp::global().available(spec, &login))
                .await
                .unwrap_or(false)
        }
        None => false,
    };
    Ok(match gate(&app.data, &scope, &t.path, installed) {
        Gate::Go => Ready::Go(t, login),
        Gate::Untrusted(spec) => Ready::Untrusted(spec, scope.sandboxed),
        Gate::Missing(spec) => Ready::Missing(spec),
    })
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
        Ready::Missing(spec) => Ok(lsp::missing_doc(spec)),
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
        // The same trust as a request: an untrusted folder's servers hear
        // nothing (one that started under a trust since withdrawn included).
        if !lsp::is_trusted(&app.data, &scope(&app, &r).await.key) {
            continue;
        }
        lsp::global().changed(&r, &rels).await;
    }
    Ok(())
}

/// Trusts the file's repository for language servers (D-2580), or a
/// sandboxed worktree on its own: from the main window, after the native
/// confirm.
/// The native confirm's title and body for trusting `name` (D-2580).
fn confirm_text(name: &str, sandboxed: bool, sandbox_on: bool) -> (String, String) {
    const RUNS: &str = "rust-analyzer, typescript-language-server, pyright and gopls can run";
    if sandboxed {
        return (
            format!("Trust the worktree {name} for language servers?"),
            format!(
                "The agent's code will run outside its sandbox. {RUNS} {name}'s build scripts, \
                 macros, plugins or Python interpreter when they load it. Trust it only if you've \
                 read what the agent wrote."
            ),
        );
    }
    let body = if sandbox_on {
        // Agents in the main checkout run sandboxed too; its servers don't.
        format!(
            "{RUNS} {name}'s build scripts, macros, plugins or Python interpreter when they load \
             it. Language servers run outside the agent sandbox: code an agent writes in {name} \
             itself can run outside it when they load it. Its worktrees aren't covered: with \
             agents sandboxed, each worktree asks on its own. Trust it only if you trust its code \
             and read what agents change in it."
        )
    } else {
        format!(
            "{RUNS} {name}'s build scripts, macros, plugins or Python interpreter when they load \
             it, and its worktrees' too. Trust it only if you trust its code."
        )
    };
    (format!("Trust {name} for language servers?"), body)
}

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
    let (title, body) = confirm_text(&name, scope.sandboxed, scope.sandbox_on);
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
        let gate_at = |root: &Path, p: &str, sandbox: bool, installed: bool| {
            let t = target(root.to_path_buf(), p.into()).expect("target");
            gate(&data, &scope_for(&t.root, sandbox), &t.path, installed)
        };
        let waits =
            |p: &str, sandbox: bool| matches!(gate_at(&repo, p, sandbox, true), Gate::Untrusted(_));
        // Every server waits, pyright too.
        for p in ["src/main.rs", "web/app.ts", "cmd/main.go", "main.py"] {
            assert!(waits(p, false), "{p}");
        }
        // Untrusted and not installed: it says what to install, and never
        // starts (even if it's installed by the next request).
        assert!(matches!(
            gate_at(&repo, "src/main.rs", false, false),
            Gate::Missing(_)
        ));
        assert_eq!(gate_at(&repo, "notes.md", false, true), Gate::Go);
        let doc = lsp::untrusted_doc(lsp::spec_for("a.rs").expect("spec"), false);
        assert_eq!(doc.state, LspState::Untrusted);
        assert_eq!(
            doc.message.as_deref(),
            Some("rust-analyzer is off until you trust this repository")
        );
        // Trusting the repository lets its servers start, its worktrees' too.
        lsp::trust(&data, &repo).expect("trust");
        assert!(!waits("src/main.rs", false));
        assert!(!waits(".tandem/worktrees/tidy/src/main.rs", false));
        let lane = repo.join(".tandem/worktrees/tidy");
        assert_eq!(gate_at(&lane, "src/main.rs", false, true), Gate::Go);
        // Another repository isn't covered.
        let other = tmp.path().join("other");
        std::fs::create_dir_all(&other).expect("mkdir");
        assert!(matches!(
            gate_at(&other, "src/main.rs", false, true),
            Gate::Untrusted(_)
        ));
        // With sandboxed agents, a worktree needs its own trust, and says
        // so; the repository itself keeps its trust.
        let sandboxed = scope_for(&lane, true);
        assert!(sandboxed.sandboxed);
        assert!(matches!(
            gate_at(&lane, "src/main.rs", true, true),
            Gate::Untrusted(_)
        ));
        assert!(!waits("src/main.rs", true));
        let doc = lsp::untrusted_doc(lsp::spec_for("a.rs").expect("spec"), true);
        assert_eq!(
            doc.message.as_deref(),
            Some("rust-analyzer is off until you trust this worktree")
        );
        lsp::trust(&data, &lane).expect("trust the worktree");
        assert_eq!(gate_at(&lane, "src/main.rs", true, true), Gate::Go);
        // Discarding the worktree forgets its trust: the next worktree at
        // that path asks again.
        lsp::untrust(&data, &lane);
        assert!(matches!(
            gate_at(&lane, "src/main.rs", true, true),
            Gate::Untrusted(_)
        ));
        assert_eq!(
            gate_at(&repo, "src/main.rs", true, true),
            Gate::Go,
            "the repository keeps its own"
        );
    }

    #[test]
    fn f_cd_23_with_the_sandbox_on_the_main_checkouts_confirm_says_servers_run_outside_it() {
        let (_, on) = confirm_text("app", false, true);
        assert!(
            on.contains("Language servers run outside the agent sandbox"),
            "{on}"
        );
        assert!(
            on.contains("code an agent writes in app itself can run outside it"),
            "{on}"
        );
        let (_, off) = confirm_text("app", false, false);
        assert!(!off.contains("sandbox"), "{off}");
        assert!(off.contains("and its worktrees' too"));
        let (title, wt) = confirm_text("fix", true, true);
        assert_eq!(title, "Trust the worktree fix for language servers?");
        assert!(wt.starts_with("The agent's code will run outside its sandbox."));
    }

    #[test]
    fn f_cd_23_a_worktree_is_found_by_a_plain_slug_only() {
        let w = |p: &str| worktree_of(Path::new(p));
        assert_eq!(
            w("/r/.tandem/worktrees/fix/src"),
            Some((
                PathBuf::from("/r"),
                PathBuf::from("/r/.tandem/worktrees/fix")
            ))
        );
        assert_eq!(w("/r/.tandem/worktrees/../../../other"), None);
        assert_eq!(w("/r/.tandem/worktrees/fix/../../x"), None);
        assert_eq!(w("/r/src"), None);
    }

    #[test]
    fn f_cd_23_a_multi_repo_worktree_is_a_worktree_for_trust() {
        let tmp = tempfile::tempdir().expect("tmp");
        let data = DataDir::at(tmp.path().join("data")).expect("data dir");
        // A folder of repositories (not itself one), and its worktree.
        let folder = std::fs::canonicalize(tmp.path())
            .expect("canonical")
            .join("shop");
        let lane = folder.join(".tandem/worktrees/tidy");
        std::fs::create_dir_all(lane.join("api")).expect("mkdir");
        // Without the sandbox, the folder's trust covers it.
        let plain = scope_for(&lane, false);
        assert_eq!(plain.key, folder);
        assert!(!plain.sandboxed);
        lsp::trust(&data, &folder).expect("trust");
        assert_eq!(gate(&data, &plain, "api/main.rs", true), Gate::Go);
        // With it, the worktree asks on its own, with the warning.
        let sandboxed = scope_for(&lane, true);
        assert_eq!(sandboxed.key, lane);
        assert!(sandboxed.sandboxed);
        assert!(matches!(
            gate(&data, &sandboxed, "api/main.rs", true),
            Gate::Untrusted(_)
        ));
        // A repository's folder inside it counts as the same worktree.
        assert_eq!(scope_for(&lane.join("api"), true).key, lane);
    }
}
