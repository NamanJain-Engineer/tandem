//! The rest of Conductor's tools (F-CN-02's table) on the app's own
//! services: launching agents, threads, teammates, routines, tasks, usage
//! and updates. The executor has already checked each call against the
//! closed schema and asked first where a tool needs it; outputs go back
//! through the redactor there (F-CN-02 AC4).

use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use tauri::State;

use tandem_core::ids::{PaneId, RoutineId, TeammateId, ThreadId, WorkspaceId};
use tandem_core::source::SourceRef;
use tandem_core::status::{AgentStatus, SessionKey};
use tandem_store::repo::presets::{Isolation, Seat, SeatKind};
use tandem_store::repo::{threads, workspaces};
use tandem_voice::conductor::view::UiAction;

use crate::state::AppState;

/// What these tools change in the window: a `conductor:ui-action`, or
/// focusing a thread. The running app's `AppHost`; tests record them.
#[async_trait]
pub trait ToolWindow: Send + Sync {
    fn ui_action(&self, action: UiAction);
    async fn focus(&self, source: SourceRef) -> Result<(), tandem_core::IpcError>;
}

type Out = Result<String, String>;

fn s(args: &Value, k: &str) -> String {
    args[k].as_str().unwrap_or_default().trim().to_string()
}

fn ipc(e: tandem_core::IpcError) -> String {
    e.message
}

/// A workspace by id or name; "current" or nothing picks the only one.
async fn workspace(state: &AppState, wanted: &str) -> Result<WorkspaceId, String> {
    let all = workspaces::list(&state.store)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(w) = all
        .iter()
        .find(|w| w.id.as_str() == wanted || w.name.eq_ignore_ascii_case(wanted))
    {
        return Ok(w.id.clone());
    }
    match all.as_slice() {
        [only] => Ok(only.id.clone()),
        [] => Err("There's no workspace yet: open a folder first.".into()),
        _ => Err("Say which workspace.".into()),
    }
}

/// A thread by id or title.
async fn thread(state: &AppState, wanted: &str) -> Result<threads::ThreadRecord, String> {
    if wanted.is_empty() {
        return Err("Say which thread.".into());
    }
    let all = threads::list(&state.store, threads::ThreadKind::Thread, false)
        .await
        .map_err(|e| e.to_string())?;
    all.into_iter()
        .find(|t| t.id.as_str() == wanted || t.title.eq_ignore_ascii_case(wanted))
        .ok_or_else(|| format!("There's no thread {wanted}."))
}

/// A teammate by id or name.
async fn teammate(state: &AppState, wanted: &str) -> Result<TeammateId, String> {
    let all = state.teammates.list().await.map_err(ipc)?;
    all.into_iter()
        .map(|t| t.teammate)
        .find(|t| t.id.as_str() == wanted || t.name.eq_ignore_ascii_case(wanted))
        .map(|t| t.id)
        .ok_or_else(|| format!("There's no teammate {wanted}."))
}

/// The last thing said in a thread, one message per line.
async fn last_messages(state: &AppState, id: &ThreadId, n: usize) -> Vec<String> {
    let rows = threads::page(&state.store, id, None, 200)
        .await
        .unwrap_or_default();
    let mut lines: Vec<String> = rows
        .iter()
        .filter_map(|r| crate::services::find_work::message_text(&r.kind, &r.payload_json))
        .collect();
    let start = lines.len().saturating_sub(n);
    lines.drain(..start);
    lines
}

/// Tools beyond the executor's own and `AppHost::run_tool`'s basics.
pub async fn run(state: State<'_, AppState>, host: &dyn ToolWindow, name: &str, a: &Value) -> Out {
    match name {
        "launch_agents" => {
            let ws = workspace(&state, &s(a, "workspace")).await?;
            let engine = s(a, "engine");
            let count = a["count"].as_u64().unwrap_or(1).clamp(1, 8) as u8;
            let seats = vec![Seat {
                kind: SeatKind::Agent,
                engine_id: Some(engine.clone()),
                account_id: None,
                role: None,
                task: None,
                command: None,
                url: None,
                count,
            }];
            let isolation = if s(a, "isolation") == "worktree" {
                Isolation::Worktree
            } else {
                Isolation::Shared
            };
            let task = s(a, "task");
            let report = crate::services::launch::launch(
                &state,
                &ws,
                &seats,
                (!task.is_empty()).then_some(task.as_str()),
                isolation,
            )
            .await
            .map_err(ipc)?;
            // The window puts the new panes in their tabs.
            host.ui_action(UiAction {
                action: "place_launched".into(),
                args: json!({ "launched": report.launched, "sharedTabId": report.shared_tab_id }),
            });
            let failed = report.failed.len();
            Ok(match failed {
                0 => format!("Launched {} {engine} agent(s).", report.launched.len()),
                n => format!("Launched {}; {n} didn't start.", report.launched.len()),
            })
        }
        "wait_for_pane" => {
            let pane = s(a, "pane");
            let timeout = a["timeout"].as_u64().unwrap_or(60).min(300);
            let until = tokio::time::Instant::now() + Duration::from_secs(timeout);
            loop {
                let status = state
                    .terminals
                    .status
                    .store
                    .get(&SessionKey::pane(&pane))
                    .map(|r| r.status);
                if status != Some(AgentStatus::Working) {
                    return Ok(format!(
                        "It's {}.",
                        match status {
                            Some(AgentStatus::Waiting) => "waiting on the user",
                            Some(AgentStatus::Done) => "done",
                            Some(AgentStatus::Failed) => "failed",
                            _ => "idle",
                        }
                    ));
                }
                if tokio::time::Instant::now() >= until {
                    return Ok("It's still working.".into());
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
        "handoff" => {
            let pane = PaneId::from_db(s(a, "pane"));
            let target = s(a, "target");
            let brief = crate::services::handoff::brief(&state, &pane, &target, None)
                .await
                .map_err(ipc)?;
            crate::services::handoff::start(
                &state,
                &pane,
                &target,
                None,
                &brief.text,
                crate::services::handoff::HandoffPlace::Same,
                false,
            )
            .await
            .map_err(ipc)?;
            Ok(format!("Handed it off to {target}."))
        }
        "open_thread" => {
            let t = thread(&state, &s(a, "thread")).await?;
            host.focus(SourceRef::Thread {
                thread_id: t.id.as_str().to_string(),
            })
            .await
            .map_err(ipc)?;
            Ok(format!("Opened {}.", t.title))
        }
        "new_thread" => {
            let folder = s(a, "folder");
            let ws = if folder.is_empty() {
                None
            } else {
                Some(workspace(&state, &folder).await?)
            };
            let engine = s(a, "engine");
            let args: crate::commands::threads::ThreadCreate = serde_json::from_value(json!({
                "kind": "thread",
                "workspaceId": ws,
                "engineId": (!engine.is_empty()).then_some(engine),
            }))
            .map_err(|e| e.to_string())?;
            let t = crate::commands::threads::thread_create(state.clone(), args)
                .await
                .map_err(ipc)?;
            let text = s(a, "text");
            if !text.is_empty() {
                crate::commands::threads::thread_send(
                    state.clone(),
                    t.id.clone(),
                    text,
                    None,
                    Some(true),
                )
                .await
                .map_err(ipc)?;
            }
            let _ = host
                .focus(SourceRef::Thread {
                    thread_id: t.id.as_str().to_string(),
                })
                .await;
            Ok("Started a new thread.".into())
        }
        "send_to_thread" => {
            let t = thread(&state, &s(a, "thread")).await?;
            let source = SourceRef::Thread {
                thread_id: t.id.as_str().to_string(),
            };
            if state.approvals.broker.pending_for(&source).is_some() {
                return Err(format!(
                    "{} is waiting for approval, so nothing was sent. Tell the user to answer it first.",
                    t.title
                ));
            }
            let text = tandem_voice::conductor::exec::strip_controls(&s(a, "text"));
            if a["send"].as_bool().unwrap_or(false) {
                crate::commands::threads::thread_send(
                    state.clone(),
                    t.id.clone(),
                    text,
                    None,
                    Some(true),
                )
                .await
                .map_err(ipc)?;
                Ok(format!("Sent to {}.", t.title))
            } else {
                crate::commands::threads::thread_save_draft(
                    state.clone(),
                    t.id.clone(),
                    json!({ "text": text }),
                )
                .await
                .map_err(ipc)?;
                Ok(format!("Left it in {}'s composer.", t.title))
            }
        }
        "read_thread" => {
            let t = thread(&state, &s(a, "thread")).await?;
            let n = a["last"].as_u64().unwrap_or(5).clamp(1, 50) as usize;
            let lines = last_messages(&state, &t.id, n).await;
            Ok(if lines.is_empty() {
                format!("{} has no messages yet.", t.title)
            } else {
                lines.join("\n")
            })
        }
        "stop_thread" => {
            let t = thread(&state, &s(a, "thread")).await?;
            crate::commands::threads::thread_stop(state.clone(), t.id.clone())
                .await
                .map_err(ipc)?;
            Ok(format!("Stopped {}.", t.title))
        }
        "set_thread_mode" => {
            let t = thread(&state, &s(a, "thread")).await?;
            let mode = serde_json::from_value(json!(s(a, "mode"))).map_err(|e| e.to_string())?;
            crate::commands::threads::thread_update(
                state.clone(),
                t.id.clone(),
                None,
                None,
                Some(mode),
                None,
                None,
                None,
            )
            .await
            .map_err(ipc)?;
            Ok(format!("{} is in {} mode.", t.title, s(a, "mode")))
        }
        "rename_thread" => {
            let t = thread(&state, &s(a, "thread")).await?;
            let title = s(a, "title");
            crate::commands::threads::thread_update(
                state.clone(),
                t.id.clone(),
                Some(title.clone()),
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .map_err(ipc)?;
            Ok(format!("Renamed it {title}."))
        }
        "archive_thread" => {
            let t = thread(&state, &s(a, "thread")).await?;
            crate::commands::threads::thread_archive(state.clone(), t.id.clone(), true)
                .await
                .map_err(ipc)?;
            Ok(format!("Archived {}.", t.title))
        }
        "open_teammate" => {
            let id = teammate(&state, &s(a, "teammate")).await?;
            host.ui_action(UiAction {
                action: "open_teammate".into(),
                args: json!({ "teammate": id }),
            });
            Ok("Opened.".into())
        }
        "message_teammate" => {
            let id = teammate(&state, &s(a, "teammate")).await?;
            let chats = threads::teammate_chats(&state.store, &id, false)
                .await
                .map_err(|e| e.to_string())?;
            let chat = match chats.into_iter().next() {
                Some(c) => c.id,
                None => {
                    let args: crate::commands::threads::ThreadCreate = serde_json::from_value(
                        json!({ "kind": "teammate_chat", "teammateId": id }),
                    )
                    .map_err(|e| e.to_string())?;
                    crate::commands::threads::thread_create(state.clone(), args)
                        .await
                        .map_err(ipc)?
                        .id
                }
            };
            let text = tandem_voice::conductor::exec::strip_controls(&s(a, "text"));
            crate::commands::threads::thread_send(state.clone(), chat, text, None, Some(true))
                .await
                .map_err(ipc)?;
            Ok("Sent.".into())
        }
        "create_teammate" => {
            // Saving happens in the sheet, with the user's own review.
            host.ui_action(UiAction {
                action: "new_teammate".into(),
                args: json!({ "description": s(a, "description") }),
            });
            Ok("Opened the New teammate sheet with that description; save it there.".into())
        }
        "pin_teammate" => {
            let id = teammate(&state, &s(a, "teammate")).await?;
            let pinned = a["pinned"].as_bool().unwrap_or(true);
            state
                .teammates
                .update(
                    &id,
                    crate::services::teammates::TeammatePatch {
                        pinned: Some(pinned),
                        ..Default::default()
                    },
                )
                .await
                .map_err(ipc)?;
            Ok(if pinned { "Pinned." } else { "Unpinned." }.into())
        }
        "read_teammate_memory" => {
            let id = teammate(&state, &s(a, "teammate")).await?;
            let m = state.teammates.memory(&id).await.map_err(ipc)?;
            let notes = m.notes.trim();
            Ok(if notes.is_empty() {
                "Its memory is empty.".into()
            } else {
                notes.chars().take(4_000).collect()
            })
        }
        "list_routines" => {
            let all = state.routines.list().await.map_err(ipc)?;
            Ok(Value::Array(
                all.iter()
                    .map(|r| json!({"id": r.routine.id, "name": r.routine.name, "when": r.description}))
                    .collect(),
            )
            .to_string())
        }
        "run_routine" => {
            let wanted = s(a, "routine");
            let all = state.routines.list().await.map_err(ipc)?;
            let r = all
                .iter()
                .find(|r| {
                    r.routine.id.as_str() == wanted || r.routine.name.eq_ignore_ascii_case(&wanted)
                })
                .ok_or_else(|| format!("There's no routine {wanted}."))?;
            state
                .routines
                .run_now(&RoutineId::from_db(r.routine.id.as_str().to_string()))
                .await
                .map_err(ipc)?;
            Ok(format!("Started {}.", r.routine.name))
        }
        "create_task" => {
            let input = serde_json::from_value(json!({
                "title": s(a, "title"),
                "description": (!s(a, "description").is_empty()).then(|| s(a, "description")),
            }))
            .map_err(|e| e.to_string())?;
            let issue = crate::commands::tasks::task_save(state.clone(), input)
                .await
                .map_err(ipc)?;
            Ok(format!("Created {}.", issue.id))
        }
        "execute_task" => {
            let ws = workspace(&state, &s(a, "workspace")).await?;
            let engine = s(a, "engine");
            let input = serde_json::from_value(json!({
                "issueId": s(a, "task"),
                "workspaceId": ws,
                "engineId": (!engine.is_empty()).then_some(engine),
                "isolation": "lane",
                "access": "edits",
                "review": true,
            }))
            .map_err(|e| e.to_string())?;
            crate::commands::task_exec::task_execute(state.clone(), input)
                .await
                .map_err(ipc)?;
            Ok(format!("Started {} in a new lane.", s(a, "task")))
        }
        "list_tasks" => {
            let page = crate::commands::tasks::tasks_list(state.clone(), Default::default(), None)
                .await
                .map_err(ipc)?;
            Ok(serde_json::to_value(&page.items)
                .map(|v| v.to_string())
                .unwrap_or_default())
        }
        "get_usage" => {
            let r = crate::commands::usage::usage_report(state.clone())
                .await
                .map_err(ipc)?;
            let today = r.days.last().map(|d| d.tokens).unwrap_or(0);
            Ok(format!(
                "{today} tokens today, {} since {} across your engines.",
                r.total, r.since
            ))
        }
        "check_updates" => {
            let u = crate::commands::updates::updates_check_now(state.clone())
                .await
                .map_err(ipc)?;
            Ok(match u.version {
                Some(v) => format!(
                    "Version {v} is available ({}). You have {}.",
                    u.stage, u.current
                ),
                None => format!("You're up to date on {}.", u.current),
            })
        }
        other => Err(format!(
            "{} isn't available by voice in this version yet; it's in the app.",
            other.replace('_', " ")
        )),
    }
}

#[cfg(test)]
mod tests;
