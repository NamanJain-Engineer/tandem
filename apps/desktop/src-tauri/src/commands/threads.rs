//! Threads (ARCHITECTURE §3.2 "Threads and teammate chats", F-TH-01,
//! F-TH-02, F-TH-03).

use serde::Deserialize;
use serde_json::{Map, Value};
use tandem_core::access::{Access, ThreadMode};
use tandem_core::ids::{AccountId, AttachmentId, TeammateId, ThreadId, WorkspaceId};
use tandem_core::settings::SettingKey;
use tandem_core::{ErrorCode, IpcError};
use tandem_engines::chat::InterruptOutcome;
use tandem_engines::events::ThreadEventEnvelope;
use tandem_store::repo::accounts as accounts_repo;
use tandem_store::repo::attachments::Attachment;
use tandem_store::repo::settings as settings_repo;
use tandem_store::repo::threads::{NewThread, ThreadKind, ThreadPatch, ThreadRecord};
use tandem_store::repo::workspaces;
use tauri::ipc::Channel;
use tauri::AppHandle;
use tauri::State;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tokio::sync::broadcast::error::RecvError;

use crate::services::models::{self, ModelEntry};
use crate::services::thread_files::PathEntry;
use crate::services::threads::{SendResult, ThreadDetail, ThreadEventPage};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadCreate {
    pub kind: ThreadKind,
    pub workspace_id: Option<WorkspaceId>,
    pub engine_id: Option<String>,
    pub account_id: Option<AccountId>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub access: Option<Access>,
    pub mode: Option<ThreadMode>,
    /// `teammate_chat`: whose chat.
    #[serde(default)]
    pub teammate_id: Option<TeammateId>,
    /// `teammate_chat`: the folder it works in (Home when omitted, or one of
    /// the teammate's places).
    #[serde(default)]
    pub place: Option<String>,
}

/// Thread mode's remembered defaults (F-TH-07 AC4): `last_engine`, and per
/// engine `<engine>.model`, `<engine>.effort` and `<engine>.access`.
async fn remembered(state: &AppState) -> Map<String, Value> {
    settings_repo::get(&state.store, SettingKey::ThreadDefaults)
        .await
        .ok()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn text(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Remembers a thread's engine and its settings for the next new thread.
async fn remember(state: &AppState, thread: &ThreadRecord) {
    if thread.kind != ThreadKind::Thread {
        return;
    }
    let mut map = remembered(state).await;
    let e = &thread.engine_id;
    map.insert("last_engine".into(), Value::String(e.clone()));
    map.insert(
        format!("{e}.model"),
        Value::String(thread.model.clone().unwrap_or_default()),
    );
    map.insert(
        format!("{e}.effort"),
        Value::String(thread.effort.clone().unwrap_or_default()),
    );
    map.insert(
        format!("{e}.access"),
        Value::String(thread.access.as_str().to_string()),
    );
    if let Err(e) =
        settings_repo::set(&state.store, SettingKey::ThreadDefaults, Value::Object(map)).await
    {
        tracing::warn!(code = "threads.defaults_failed", error = %e, "couldn't remember thread defaults");
    }
}

/// The engine a thread starts with: Claude Code when it's installed, else
/// the first installed engine (here, or on the thread's host) that has a
/// chat adapter and the plan runs.
fn default_engine(state: &AppState, installed: &dyn Fn(&str) -> bool) -> Option<String> {
    let engines = &state.terminals.engines;
    let registry = engines.registry();
    let limits = state.entitlements.limits();
    let chat: Vec<&str> = registry
        .all()
        .iter()
        .filter(|e| {
            crate::services::threads::has_chat_adapter(e)
                && installed(&e.id)
                && limits.allows_chat_engine(&e.id)
        })
        .map(|e| e.id.as_str())
        .collect();
    chat.iter()
        .find(|id| **id == "claude")
        .or_else(|| chat.first())
        .map(|id| id.to_string())
}

/// `thread_create`. A thread without a folder works in its own scratch
/// folder with Read access (F-TH-02); omitted settings take the defaults.
#[tauri::command]
pub async fn thread_create(
    state: State<'_, AppState>,
    args: ThreadCreate,
) -> Result<ThreadRecord, IpcError> {
    create_thread(&state, args).await
}

/// A new thread with this engine's remembered settings (also a handoff's
/// Thread target, F-CD-16).
pub(crate) async fn create_thread(
    state: &AppState,
    args: ThreadCreate,
) -> Result<ThreadRecord, IpcError> {
    if args.kind == ThreadKind::TeammateChat {
        return create_teammate_chat(state, args).await;
    }
    let defaults = remembered(state).await;
    // A remote workspace's thread runs one of the host's engines (F-RM-01).
    let host_report = match &args.workspace_id {
        Some(ws) => match workspaces::get(&state.store, ws).await {
            Ok(w) if w.kind == "ssh" => Some(
                state
                    .remote
                    .report(&tandem_core::ids::HostId::from_db(w.host_id))
                    .await
                    .unwrap_or_default(),
            ),
            _ => None,
        },
        None => None,
    };
    let installed = |e: &str| match &host_report {
        Some(r) => r.get(e).is_some(),
        None => state.terminals.engines.installed(e).is_some(),
    };
    // The last engine, unless the limits no longer run it (a token that
    // lists chat engines).
    let last = text(&defaults, "last_engine").filter(|e| {
        state.entitlements.limits().allows_chat_engine(e)
            && installed(e)
            && state
                .terminals
                .engines
                .get(e)
                .is_some_and(|spec| crate::services::threads::has_chat_adapter(&spec))
    });
    let engine_id = match args.engine_id.or(last) {
        Some(id) => id,
        None => default_engine(state, &installed).ok_or_else(|| {
            IpcError::new(
                ErrorCode::EngineMissing,
                "No coding agent that works in Thread mode is installed",
            )
        })?,
    };
    // This engine's last model, effort and access (F-TH-07 AC4).
    let model = args
        .model
        .or_else(|| text(&defaults, &format!("{engine_id}.model")));
    let effort = args
        .effort
        .or_else(|| text(&defaults, &format!("{engine_id}.effort")));
    let remembered_access = text(&defaults, &format!("{engine_id}.access"))
        .and_then(|a| serde_json::from_value::<Access>(Value::String(a)).ok());
    if state.terminals.engines.get(&engine_id).is_none() {
        return Err(IpcError::new(
            ErrorCode::EngineMissing,
            "This engine isn't known to Vibezeo",
        ));
    }
    let id = ThreadId::new();
    let (cwd, default_access) = match &args.workspace_id {
        Some(ws) => {
            let ws = workspaces::get(&state.store, ws)
                .await
                .map_err(|e| IpcError::new(ErrorCode::NotFound, e.to_string()))?;
            (ws.root, remembered_access.unwrap_or(Access::Edits))
        }
        None => {
            let dir = state.data.scratch(id.as_str());
            std::fs::create_dir_all(&dir).map_err(|e| {
                IpcError::new(
                    ErrorCode::Io,
                    format!("Couldn't create the thread's folder: {e}"),
                )
            })?;
            (dir.to_string_lossy().into_owned(), Access::Read)
        }
    };
    state
        .threads
        .create(NewThread {
            id,
            kind: args.kind,
            workspace_id: args.workspace_id,
            cwd,
            engine_id,
            account_id: args.account_id,
            model,
            effort,
            access: args.access.unwrap_or(default_access),
            mode: args.mode.unwrap_or(ThreadMode::Act),
            attended: true,
        })
        .await
}

/// A teammate chat (F-AG-03): the teammate's engine, model, preferred
/// account (F-AC-03) and default access, in its Home or one of its places.
async fn create_teammate_chat(
    state: &AppState,
    args: ThreadCreate,
) -> Result<ThreadRecord, IpcError> {
    let id = args
        .teammate_id
        .ok_or_else(|| IpcError::new(ErrorCode::Invalid, "Which teammate's chat?"))?;
    let teammate = state.teammates.get(&id).await?;
    let chat_capable = state
        .terminals
        .engines
        .get(&teammate.engine_id)
        .is_some_and(|s| crate::services::threads::has_chat_adapter(&s));
    if !chat_capable {
        return Err(IpcError::new(
            ErrorCode::EngineUnsupported,
            "Only chat engines carry memory and skills.",
        ));
    }
    let cwd =
        crate::services::teammates::TeammatesService::place_for(&teammate, args.place.as_deref())?;
    std::fs::create_dir_all(&cwd).map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
    let thread = state
        .threads
        .create_chat(
            NewThread {
                id: ThreadId::new(),
                kind: ThreadKind::TeammateChat,
                workspace_id: None,
                cwd,
                engine_id: teammate.engine_id.clone(),
                account_id: args.account_id.or(teammate.preferred_account_id.clone()),
                model: teammate.model.clone(),
                effort: teammate.effort.clone(),
                access: args.access.unwrap_or(teammate.settings.default_access),
                mode: args.mode.unwrap_or(ThreadMode::Act),
                attended: true,
            },
            tandem_store::repo::threads::ChatExtra {
                teammate_id: Some(teammate.id.clone()),
                ..Default::default()
            },
        )
        .await?;
    state.teammates.publish().await;
    Ok(thread)
}

#[tauri::command]
pub async fn thread_list(
    state: State<'_, AppState>,
    kind: ThreadKind,
    teammate_id: Option<TeammateId>,
    include_archived: Option<bool>,
) -> Result<Vec<ThreadRecord>, IpcError> {
    if let (ThreadKind::TeammateChat, Some(teammate)) = (kind, &teammate_id) {
        return Ok(tandem_store::repo::threads::teammate_chats(
            &state.store,
            teammate,
            include_archived.unwrap_or(false),
        )
        .await?);
    }
    state
        .threads
        .list(kind, include_archived.unwrap_or(false))
        .await
}

#[tauri::command]
pub async fn thread_get(
    state: State<'_, AppState>,
    thread_id: ThreadId,
) -> Result<ThreadDetail, IpcError> {
    state.threads.get(&thread_id).await
}

#[tauri::command]
pub async fn thread_events(
    state: State<'_, AppState>,
    thread_id: ThreadId,
    before_id: Option<i64>,
    limit: Option<u32>,
) -> Result<ThreadEventPage, IpcError> {
    state
        .threads
        .events(&thread_id, before_id, limit.unwrap_or(200))
        .await
}

/// Streams a thread's events to `channel`: the running turn so far, then
/// live events, until the webview drops the channel.
#[tauri::command]
pub async fn thread_subscribe(
    state: State<'_, AppState>,
    thread_id: ThreadId,
    channel: Channel<ThreadEventEnvelope>,
) -> Result<(), IpcError> {
    let threads = state.threads.clone();
    let (snapshot, mut rx) = threads.subscribe(&thread_id).await?;
    tauri::async_runtime::spawn(async move {
        for env in snapshot {
            if channel.send(env).is_err() {
                return;
            }
        }
        loop {
            match rx.recv().await {
                Ok(env) if env.thread_id == thread_id => {
                    if channel.send(env).is_err() {
                        return;
                    }
                }
                Ok(_) => {}
                // Fell behind: send the running turn again (the view
                // replaces events by seq).
                Err(RecvError::Lagged(_)) => {
                    let Ok((again, fresh)) = threads.subscribe(&thread_id).await else {
                        return;
                    };
                    rx = fresh;
                    for env in again {
                        if channel.send(env).is_err() {
                            return;
                        }
                    }
                }
                Err(RecvError::Closed) => return,
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn thread_send(
    state: State<'_, AppState>,
    thread_id: ThreadId,
    text: String,
    attachment_ids: Option<Vec<AttachmentId>>,
    queue_if_busy: Option<bool>,
) -> Result<SendResult, IpcError> {
    if text.trim().is_empty() {
        return Err(IpcError::new(ErrorCode::Invalid, "Type a message first"));
    }
    state
        .threads
        .send(
            &thread_id,
            text,
            attachment_ids.unwrap_or_default(),
            queue_if_busy.unwrap_or(false),
        )
        .await
}

#[tauri::command]
pub async fn thread_stop(
    state: State<'_, AppState>,
    thread_id: ThreadId,
) -> Result<InterruptOutcome, IpcError> {
    state.threads.stop(&thread_id).await
}

#[tauri::command]
pub async fn thread_retry(
    state: State<'_, AppState>,
    thread_id: ThreadId,
) -> Result<SendResult, IpcError> {
    state.threads.retry(&thread_id).await
}

/// `"default"` (or empty) means the engine's own default.
fn choice(v: Option<String>) -> Option<Option<String>> {
    v.map(|s| Some(s).filter(|s| !s.is_empty() && s != models::DEFAULT_MODEL))
}

/// `thread_update` (F-TH-01, F-TH-09). Omitted fields stay as they are.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn thread_update(
    state: State<'_, AppState>,
    thread_id: ThreadId,
    title: Option<String>,
    access: Option<Access>,
    mode: Option<ThreadMode>,
    model: Option<String>,
    effort: Option<String>,
    pinned: Option<bool>,
) -> Result<ThreadRecord, IpcError> {
    let settings_changed = access.is_some() || model.is_some() || effort.is_some();
    let thread = state
        .threads
        .update(
            &thread_id,
            ThreadPatch {
                title,
                access,
                mode,
                model: choice(model),
                effort: choice(effort),
                pinned,
            },
        )
        .await?;
    if settings_changed {
        remember(&state, &thread).await;
    }
    Ok(thread)
}

#[tauri::command]
pub async fn thread_archive(
    state: State<'_, AppState>,
    thread_id: ThreadId,
    archived: bool,
) -> Result<(), IpcError> {
    state.threads.archive(&thread_id, archived).await
}

#[tauri::command]
pub async fn thread_delete(
    state: State<'_, AppState>,
    thread_id: ThreadId,
) -> Result<(), IpcError> {
    state.threads.delete(&thread_id).await
}

/// `thread_switch_engine` (F-TH-16): between turns only.
#[tauri::command]
pub async fn thread_switch_engine(
    state: State<'_, AppState>,
    thread_id: ThreadId,
    engine_id: String,
    account_id: Option<AccountId>,
) -> Result<ThreadRecord, IpcError> {
    let spec = state.terminals.engines.get(&engine_id).ok_or_else(|| {
        IpcError::new(
            ErrorCode::EngineMissing,
            "This engine isn't known to Vibezeo",
        )
    })?;
    if !crate::services::threads::has_chat_adapter(&spec) {
        return Err(IpcError::new(
            ErrorCode::EngineUnsupported,
            format!(
                "{} can't run in Thread mode yet. Use it in Code mode.",
                spec.label
            ),
        ));
    }
    let thread = state
        .threads
        .switch_engine(
            &thread_id,
            &engine_id,
            account_id.as_ref(),
            switch_divider(&state, &spec.label, &engine_id, account_id.as_ref()).await,
        )
        .await?;
    remember(&state, &thread).await;
    Ok(thread)
}

/// "Continued on Codex (Work) with a summary" (F-TH-16): the account is
/// the one chosen, else the engine's default.
async fn switch_divider(
    state: &AppState,
    engine: &str,
    engine_id: &str,
    account_id: Option<&AccountId>,
) -> String {
    let account = match account_id {
        Some(id) => accounts_repo::get(&state.store, id).await.ok(),
        None => accounts_repo::default_for(&state.store, engine_id)
            .await
            .ok()
            .flatten(),
    };
    let label = account.map_or_else(|| accounts_repo::DEFAULT_LABEL.to_string(), |a| a.label);
    format!("Continued on {engine} ({label}) with a summary")
}

/// `thread_move_to_folder` (F-TH-02 "Add folder…").
#[tauri::command]
pub async fn thread_move_to_folder(
    state: State<'_, AppState>,
    thread_id: ThreadId,
    workspace_id: WorkspaceId,
) -> Result<ThreadRecord, IpcError> {
    let ws = workspaces::get(&state.store, &workspace_id)
        .await
        .map_err(|e| IpcError::new(ErrorCode::NotFound, e.to_string()))?;
    state
        .threads
        .move_to_folder(&thread_id, &workspace_id, &ws.root)
        .await
}

#[tauri::command]
pub async fn thread_save_draft(
    state: State<'_, AppState>,
    thread_id: ThreadId,
    draft: Value,
) -> Result<(), IpcError> {
    state.threads.save_draft(&thread_id, draft).await
}

#[tauri::command]
pub async fn thread_attach_paths(
    state: State<'_, AppState>,
    thread_id: ThreadId,
    paths: Vec<String>,
) -> Result<Vec<Attachment>, IpcError> {
    state.threads.attach_paths(&thread_id, paths).await
}

/// A pasted screenshot, read from the system clipboard and saved as a PNG
/// in `$DATA/attachments/<thread>/` (F-TH-07 AC5).
#[tauri::command]
pub async fn thread_attach_clipboard_image(
    app: AppHandle,
    state: State<'_, AppState>,
    thread_id: ThreadId,
) -> Result<Attachment, IpcError> {
    let image = app
        .clipboard()
        .read_image()
        .map_err(|_| IpcError::new(ErrorCode::NotFound, "There's no image on the clipboard"))?;
    let (width, height, rgba) = (image.width(), image.height(), image.rgba().to_vec());
    state
        .threads
        .attach_png(&thread_id, width, height, rgba)
        .await
}

#[tauri::command]
pub async fn attachment_remove(
    state: State<'_, AppState>,
    attachment_id: AttachmentId,
) -> Result<(), IpcError> {
    state.threads.remove_attachment(&attachment_id).await
}

#[tauri::command]
pub async fn thread_mention_search(
    state: State<'_, AppState>,
    thread_id: ThreadId,
    query: String,
) -> Result<Vec<PathEntry>, IpcError> {
    state.threads.mention_search(&thread_id, query).await
}

/// `models_list` (F-TH-08): Default first, then what the engine reports,
/// the remote catalog and the bundled one.
#[tauri::command]
pub async fn models_list(
    state: State<'_, AppState>,
    engine_id: String,
    #[allow(unused_variables)] account_id: Option<AccountId>,
) -> Result<Vec<ModelEntry>, IpcError> {
    Ok(models::catalog_for(
        &engine_id,
        &state.models,
        &state.config,
    ))
}

/// `thread_pick_files`: the native picker, then the picked files attached.
#[tauri::command]
pub async fn thread_pick_files(
    app: AppHandle,
    state: State<'_, AppState>,
    thread_id: ThreadId,
) -> Result<Vec<Attachment>, IpcError> {
    let picked: Vec<String> = match std::env::var("TANDEM_FAKE_PICK_FILES") {
        Ok(fake) if cfg!(debug_assertions) => fake
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect(),
        _ => {
            use tauri_plugin_dialog::DialogExt;
            let (tx, rx) = tokio::sync::oneshot::channel();
            app.dialog().file().pick_files(move |picked| {
                let paths = picked
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|p| p.into_path().ok())
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect::<Vec<_>>();
                let _ = tx.send(paths);
            });
            rx.await.unwrap_or_default()
        }
    };
    if picked.is_empty() {
        return Ok(Vec::new());
    }
    state.threads.attach_paths(&thread_id, picked).await
}

/// F-TH-13 Files: a text file in the thread's folder, contained.
#[tauri::command]
pub async fn thread_file_read(
    state: State<'_, AppState>,
    thread_id: ThreadId,
    path: String,
) -> Result<crate::services::thread_files::FileText, IpcError> {
    state.threads.read_file(&thread_id, path).await
}

/// F-TH-13: what changed in the thread's folder since its first turn.
#[tauri::command]
pub async fn thread_changes(
    state: State<'_, AppState>,
    thread_id: ThreadId,
) -> Result<crate::services::thread_changes::ThreadChanges, IpcError> {
    state.threads.changes(&thread_id).await
}

/// F-TH-10: Act at `access` (or the thread's), then send plan `version`.
#[tauri::command]
pub async fn thread_implement_plan(
    state: State<'_, AppState>,
    thread_id: ThreadId,
    version: u32,
    access: Option<Access>,
) -> Result<SendResult, IpcError> {
    state
        .threads
        .implement_plan(&thread_id, version, access)
        .await
}

#[tauri::command]
pub async fn thread_unqueue(
    state: State<'_, AppState>,
    thread_id: ThreadId,
) -> Result<Option<String>, IpcError> {
    state.threads.unqueue(&thread_id).await
}

/// A teammate's chat tabs in a new order (F-AG-03 AC1: it persists).
#[tauri::command]
pub async fn thread_reorder(
    state: State<'_, AppState>,
    ids: Vec<ThreadId>,
) -> Result<(), IpcError> {
    tandem_store::repo::threads::reorder(&state.store, &ids).await?;
    state.teammates.publish().await;
    Ok(())
}
