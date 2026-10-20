//! Code intelligence in the editor and diffs (ARCHITECTURE §3.2 "Files,
//! editor, browser, scratchpad"; F-CD-23). Language servers run on this
//! machine only; a folder on an SSH host has none.

use std::path::PathBuf;

use tandem_core::IpcError;
use tandem_store::repo::{threads as threads_repo, workspaces};
use tauri::State;

use crate::services::files::FileContext;
use crate::services::lsp::{self, LspDocument, LspLocation, LspState};
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
    lsp::global()
        .hover(&t.root, &t.path, &text, line, character, &login)
        .await
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
}
