//! Code intelligence in the editor and diffs (ARCHITECTURE §3.2 "Files,
//! editor, browser, scratchpad"; F-CD-23). Language servers run on this
//! machine only; a folder on an SSH host has none.

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

/// The repository whose trust a folder's servers need: its main checkout
/// (a worktree shares its repository's trust), or the folder itself.
fn repository_of(root: &Path) -> PathBuf {
    crate::services::worktree_setup::main_root(&root.to_string_lossy())
        .unwrap_or_else(|_| root.to_path_buf())
}

/// The installed server that waits for the repository's trust (D-2580), or
/// `None` when the request may go ahead.
fn waits_for_trust(
    data: &DataDir,
    t: &Target,
    available: impl Fn(&ServerSpec) -> bool,
) -> Option<&'static ServerSpec> {
    let spec = lsp::spec_for(&t.path)?;
    (available(spec) && !lsp::is_trusted(data, &repository_of(&t.root))).then_some(spec)
}

fn back(t: &Target, mut found: Vec<LspLocation>) -> Vec<LspLocation> {
    if !t.prefix.is_empty() {
        for l in found.iter_mut().filter(|l| l.in_folder) {
            l.path = format!("{}{}", t.prefix, l.path);
        }
    }
    found
}

/// The file's server target, or `None` for a folder on an SSH host or in WSL.
async fn local(
    app: &AppState,
    context: &FileContext,
    path: String,
) -> Result<Option<Target>, IpcError> {
    if !on_this_computer(app, context).await? {
        return Ok(None);
    }
    let root = app.files.root(context).await?;
    target(root, path).map(Some)
}

#[tauri::command]
pub async fn lsp_sync(
    app: State<'_, AppState>,
    context: FileContext,
    path: String,
    text: String,
) -> Result<LspDocument, IpcError> {
    let Some(t) = local(&app, &context, path).await? else {
        return Ok(LspDocument {
            language: None,
            server: None,
            state: LspState::None,
            message: Some(REMOTE.to_string()),
            diagnostics: Vec::new(),
        });
    };
    let login = app.terminals.login_env().await;
    if let Some(spec) = waits_for_trust(&app.data, &t, |s| lsp::global().available(s, &login)) {
        return Ok(lsp::untrusted_doc(spec));
    }
    lsp::global().sync(&t.root, &t.path, &text, &login).await
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
    if !on_this_computer(&app, &context).await? {
        return Ok(lsp::fallback_definition(&path, &text, line, character));
    }
    let Some(t) = local(&app, &context, path).await? else {
        return Ok(Vec::new());
    };
    let login = app.terminals.login_env().await;
    if waits_for_trust(&app.data, &t, |s| lsp::global().available(s, &login)).is_some() {
        return Ok(back(
            &t,
            lsp::fallback_definition(&t.path, &text, line, character),
        ));
    }
    let found = lsp::global()
        .definition(&t.root, &t.path, &text, line, character, &login)
        .await?;
    Ok(back(&t, found))
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
    let Some(t) = local(&app, &context, path).await? else {
        return Ok(Vec::new());
    };
    let login = app.terminals.login_env().await;
    if waits_for_trust(&app.data, &t, |s| lsp::global().available(s, &login)).is_some() {
        return Ok(Vec::new());
    }
    let found = lsp::global()
        .references(&t.root, &t.path, &text, line, character, &login)
        .await?;
    Ok(back(&t, found))
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
    let Some(t) = local(&app, &context, path).await? else {
        return Ok(None);
    };
    let login = app.terminals.login_env().await;
    if waits_for_trust(&app.data, &t, |s| lsp::global().available(s, &login)).is_some() {
        return Ok(None);
    }
    lsp::global()
        .hover(&t.root, &t.path, &text, line, character, &login)
        .await
}

/// Trusts the file's repository for language servers (D-2580): from the
/// main window, after the native confirm.
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
    let Some(t) = local(&app, &context, path).await? else {
        return Err(IpcError::new(ErrorCode::Invalid, REMOTE).with_stage("lsp"));
    };
    let repo = repository_of(&t.root);
    let name = repo
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| repo.to_string_lossy().into_owned());
    let body = format!(
        "rust-analyzer, typescript-language-server, pyright and gopls can run {name}'s build \
         scripts, macros, plugins or Python interpreter when they load it, and its worktrees' too. \
         Trust it only if you trust its code."
    );
    if !confirm_native(
        &app_handle,
        &format!("Trust {name} for language servers?"),
        &body,
        "Trust",
    )
    .await
    {
        return Err(IpcError::new(ErrorCode::Cancelled, "Not trusted"));
    }
    lsp::trust(&app.data, &repo)?;
    tracing::info!(
        code = "lsp.trusted",
        "repository trusted for language servers"
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
        let t = |p: &str| target(repo.clone(), p.into()).expect("target");
        // Every server waits, pyright too.
        for p in ["src/main.rs", "web/app.ts", "cmd/main.go", "main.py"] {
            assert!(waits_for_trust(&data, &t(p), installed).is_some(), "{p}");
        }
        // An uninstalled server says it's missing instead; other files have
        // no server.
        assert!(waits_for_trust(&data, &t("src/main.rs"), |_| false).is_none());
        assert!(waits_for_trust(&data, &t("notes.md"), installed).is_none());
        let doc = lsp::untrusted_doc(lsp::spec_for("a.rs").expect("spec"));
        assert_eq!(doc.state, LspState::Untrusted);
        assert_eq!(
            doc.message.as_deref(),
            Some("rust-analyzer is off until you trust this repository")
        );
        // Trusting the repository lets its servers start, its worktrees' too.
        lsp::trust(&data, &repo).expect("trust");
        assert!(waits_for_trust(&data, &t("src/main.rs"), installed).is_none());
        assert!(
            waits_for_trust(&data, &t(".tandem/worktrees/tidy/src/main.rs"), installed).is_none()
        );
        let lane = target(repo.join(".tandem/worktrees/tidy"), "src/main.rs".into()).expect("lane");
        assert!(waits_for_trust(&data, &lane, installed).is_none());
        // Another repository isn't covered.
        let other = tmp.path().join("other");
        std::fs::create_dir_all(&other).expect("mkdir");
        let elsewhere = target(other, "src/main.rs".into()).expect("other");
        assert!(waits_for_trust(&data, &elsewhere, installed).is_some());
    }
}
