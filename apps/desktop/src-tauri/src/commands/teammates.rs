//! Teammates (ARCHITECTURE §3.2 "Teammates, memory, skills, mail"; F-AG-01,
//! F-AG-02, F-AG-04, F-AG-05).

use tandem_core::ids::TeammateId;
use tandem_core::IpcError;
use tandem_store::repo::teammates::Teammate;
use tauri::State;

use crate::services::teammates::{TeammateInput, TeammatePatch, TeammateSummary};
use crate::state::AppState;

#[tauri::command]
pub async fn teammate_list(state: State<'_, AppState>) -> Result<Vec<TeammateSummary>, IpcError> {
    state.teammates.list().await
}

#[tauri::command]
pub async fn teammate_get(
    state: State<'_, AppState>,
    id: TeammateId,
) -> Result<Teammate, IpcError> {
    state.teammates.get(&id).await
}

#[tauri::command]
pub async fn teammate_create(
    state: State<'_, AppState>,
    input: TeammateInput,
) -> Result<Teammate, IpcError> {
    state.teammates.create(input).await
}

#[tauri::command]
pub async fn teammate_update(
    state: State<'_, AppState>,
    id: TeammateId,
    patch: TeammatePatch,
) -> Result<Teammate, IpcError> {
    let before = state.teammates.get(&id).await?;
    let updated = state.teammates.update(&id, patch).await?;
    // New built-ins (May message) or engine settings reach the next turn.
    if updated.settings != before.settings || updated.engine_id != before.engine_id {
        state.refresh_teammate_sessions(&id).await;
    }
    Ok(updated)
}

#[tauri::command]
pub async fn teammate_delete(state: State<'_, AppState>, id: TeammateId) -> Result<(), IpcError> {
    state.teammates.delete(&id).await
}

#[tauri::command]
pub async fn teammate_duplicate(
    state: State<'_, AppState>,
    id: TeammateId,
) -> Result<Teammate, IpcError> {
    state.teammates.duplicate(&id).await
}

#[tauri::command]
pub async fn teammate_reorder(
    state: State<'_, AppState>,
    ids: Vec<TeammateId>,
) -> Result<(), IpcError> {
    state.teammates.reorder(&ids).await
}

#[tauri::command]
pub async fn memory_get(
    state: State<'_, AppState>,
    teammate_id: TeammateId,
) -> Result<tandem_store::memory::Memory, IpcError> {
    state.teammates.memory(&teammate_id).await
}

/// The user's own edits (Add note, delete, Raw save): checked like the
/// teammate's writes (F-AG-07).
#[tauri::command]
pub async fn memory_write(
    state: State<'_, AppState>,
    teammate_id: TeammateId,
    file: tandem_store::memory::MemoryFile,
    markdown: String,
) -> Result<(), IpcError> {
    let teammate = state.teammates.get(&teammate_id).await?;
    state
        .teammates
        .write_memory(&teammate, file, &markdown)
        .await
        .map(|_| ())
}

#[tauri::command]
pub async fn memory_undo(state: State<'_, AppState>, change_id: String) -> Result<(), IpcError> {
    state.teammates.undo_memory(&change_id).await
}

use crate::services::teammates::skill_error as skill_err;

#[tauri::command]
pub async fn skills_list(
    state: State<'_, AppState>,
    teammate_id: TeammateId,
) -> Result<Vec<tandem_store::skills::SkillEntry>, IpcError> {
    let teammate = state.teammates.get(&teammate_id).await?;
    Ok(state.teammates.skills(&teammate).await)
}

/// Opening a skill marks it seen ("New" goes away, F-AG-09).
#[tauri::command]
pub async fn skill_get(
    state: State<'_, AppState>,
    teammate_id: TeammateId,
    slug: String,
) -> Result<tandem_store::skills::Skill, IpcError> {
    let teammate = state.teammates.get(&teammate_id).await?;
    state
        .teammates
        .mark_skills_seen(&teammate_id, std::slice::from_ref(&slug))
        .await;
    let allow = state.teammates.teammates_write_skills().await;
    let seen = state.teammates.seen_skills(&teammate_id).await;
    let skill =
        tandem_store::skills::read(std::path::Path::new(&teammate.home), &slug, &seen, allow)
            .map_err(skill_err)?;
    state.teammates.publish().await;
    Ok(skill)
}

#[tauri::command]
pub async fn skill_save(
    state: State<'_, AppState>,
    teammate_id: TeammateId,
    slug: String,
    markdown: String,
) -> Result<(), IpcError> {
    let teammate = state.teammates.get(&teammate_id).await?;
    if crate::services::teammates::has_secret(&markdown) {
        return Err(IpcError::new(
            tandem_core::ErrorCode::Invalid,
            "Skills can't hold credentials",
        ));
    }
    tandem_store::skills::write(std::path::Path::new(&teammate.home), &slug, &markdown)
        .map_err(skill_err)?;
    state
        .teammates
        .mark_skills_seen(&teammate_id, std::slice::from_ref(&slug))
        .await;
    // Engines it's installed for read the saved text (F-AG-10).
    state.teammates.resync_skill(&teammate, &slug, true).await;
    state.teammates.publish().await;
    Ok(())
}

/// The folder moves to the teammate's own `.trash`.
#[tauri::command]
pub async fn skill_remove(
    state: State<'_, AppState>,
    teammate_id: TeammateId,
    slug: String,
) -> Result<(), IpcError> {
    let teammate = state.teammates.get(&teammate_id).await?;
    // Its synced copies go first (F-AG-10).
    state.teammates.resync_skill(&teammate, &slug, false).await;
    tandem_store::skills::remove(std::path::Path::new(&teammate.home), &slug).map_err(skill_err)?;
    state.teammates.publish().await;
    Ok(())
}

/// `skill_sync` (F-AG-10): "Also install for …" on or off for one engine
/// account. A same-named skill the engine already has is left as it is.
#[tauri::command]
pub async fn skill_sync(
    state: State<'_, AppState>,
    teammate_id: TeammateId,
    slug: String,
    engine_id: String,
    account_id: tandem_core::ids::AccountId,
    on: bool,
) -> Result<tandem_store::skills::SyncOutcome, IpcError> {
    let teammate = state.teammates.get(&teammate_id).await?;
    let st = state
        .teammates
        .sync_skill(&teammate, &slug, &engine_id, &account_id, on)
        .await?;
    state.teammates.publish().await;
    Ok(tandem_store::skills::SyncOutcome { state: st })
}

#[tauri::command]
pub fn skills_library() -> Vec<tandem_store::skills::SkillEntry> {
    tandem_store::skills::library()
}

#[tauri::command]
pub async fn skill_add_from_library(
    state: State<'_, AppState>,
    teammate_id: TeammateId,
    slug: String,
) -> Result<(), IpcError> {
    let teammate = state.teammates.get(&teammate_id).await?;
    tandem_store::skills::add_from_library(std::path::Path::new(&teammate.home), &slug)
        .map_err(skill_err)?;
    state
        .teammates
        .mark_skills_seen(&teammate_id, std::slice::from_ref(&slug))
        .await;
    state.teammates.publish().await;
    Ok(())
}

/// Empty-chat chips from the brief, made locally (F-AG-03).
#[tauri::command]
pub async fn teammate_suggestions(
    state: State<'_, AppState>,
    id: TeammateId,
) -> Result<Vec<String>, IpcError> {
    let teammate = state.teammates.get(&id).await?;
    Ok(crate::services::teammates::suggestions(&teammate.brief))
}

/// `teammate_draft_with_ai` (F-AG-04): hosted, signed in and online; nothing
/// is saved.
#[tauri::command]
pub async fn teammate_draft_with_ai(
    state: State<'_, AppState>,
    description: String,
) -> Result<crate::services::teammate_draft::TeammateDraft, IpcError> {
    use crate::services::teammate_draft as draft;
    use tandem_core::ErrorCode;
    if description.trim().is_empty() {
        return Err(IpcError::new(ErrorCode::Invalid, "Describe the job first"));
    }
    let view = state.entitlements.view();
    if view.plan == tandem_core::entitlement::Plan::Free {
        return Err(IpcError::new(
            ErrorCode::ForbiddenPlan,
            "Create with AI comes with Vibezeo accounts.",
        ));
    }
    if !state.endpoints.hosted || !view.online || !state.auth.is_signed_in() {
        return Err(IpcError::new(ErrorCode::Unavailable, draft::COULDNT_DRAFT));
    }
    let engines: Vec<String> = {
        let e = &state.terminals.engines;
        e.registry()
            .all()
            .iter()
            .filter(|s| {
                crate::services::threads::has_chat_adapter(s) && e.installed(&s.id).is_some()
            })
            .map(|s| s.id.clone())
            .collect()
    };
    let plugins: Vec<String> = tandem_store::repo::plugins::connections(&state.store)
        .await?
        .into_iter()
        .map(|c| c.plugin_id)
        .collect();
    let raw = match draft::fixture() {
        Some(v) => v,
        None => {
            let job = draft::job(&description);
            let api = state.api.clone();
            let got = state
                .auth
                .with_token(|token| {
                    let (api, job, engines, plugins) =
                        (api.clone(), job.clone(), engines.clone(), plugins.clone());
                    async move {
                        api.ai_teammate_draft(&token, &job, &engines, &plugins)
                            .await
                    }
                })
                .await
                .map_err(|e| draft::api_error(&e))?;
            serde_json::to_value(got)
                .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?
        }
    };
    draft::sanitize(&raw, &engines, &plugins)
        .ok_or_else(|| IpcError::new(ErrorCode::Unavailable, draft::COULDNT_DRAFT))
}

#[tauri::command]
pub fn teammate_templates() -> Vec<crate::services::teammate_draft::TeammateTemplate> {
    crate::services::teammate_draft::templates()
}

/// Keep the chosen memory suggestions (F-AG-08 Ask); an empty `keep`
/// discards the set. Returns the change id (for Undo) when memory changed.
#[tauri::command]
pub async fn memory_suggestions_resolve(
    state: State<'_, AppState>,
    suggestion_set_id: String,
    keep: Vec<u32>,
) -> Result<Option<String>, IpcError> {
    state.learn.resolve(&suggestion_set_id, &keep).await
}

/// `mail_list` (F-AG-12): a teammate's mail, sent or received, newest first.
#[tauri::command]
pub async fn mail_list(
    state: State<'_, AppState>,
    teammate_id: TeammateId,
) -> Result<Vec<tandem_store::repo::mail::MailRow>, IpcError> {
    state.mail.list(&teammate_id).await
}

/// `mail_get`: one mail with its body.
#[tauri::command]
pub async fn mail_get(
    state: State<'_, AppState>,
    mail_id: tandem_core::ids::MailId,
) -> Result<tandem_store::repo::mail::Mail, IpcError> {
    state.mail.get(&mail_id).await
}

/// `mail_send`: the composer's `@name` (the chat's teammate is the sender).
#[tauri::command]
pub async fn mail_send(
    state: State<'_, AppState>,
    from_teammate_id: Option<TeammateId>,
    to_teammate_id: TeammateId,
    body: String,
    from_thread_id: Option<tandem_core::ids::ThreadId>,
) -> Result<tandem_store::repo::mail::MailRow, IpcError> {
    // The sender's thread of work is its chat's.
    let conversation_ref = match &from_thread_id {
        Some(t) => tandem_store::repo::threads::get(&state.store, t)
            .await
            .ok()
            .flatten()
            .and_then(|r| r.conversation_ref),
        None => None,
    };
    state
        .mail
        .send(crate::services::mail::Outgoing {
            from: from_teammate_id,
            from_thread: from_thread_id,
            conversation_ref,
            to: crate::services::mail::Recipient::Id(to_teammate_id),
            body,
        })
        .await
}

/// `mail_retry`: a failed or waiting mail goes back in the queue.
#[tauri::command]
pub async fn mail_retry(
    state: State<'_, AppState>,
    mail_id: tandem_core::ids::MailId,
) -> Result<(), IpcError> {
    state.mail.retry(&mail_id).await
}

/// `teammate_launch_in_terminal` (F-AG-13): a Code-mode agent pane with the
/// teammate's engine and account in one of its places (or its Home), named
/// after it, whose first prompt is its brief. Without `tab_id` the place
/// opens as a workspace (an open one is reused) and the pane joins its
/// first tab.
#[tauri::command]
pub async fn teammate_launch_in_terminal(
    state: State<'_, AppState>,
    id: TeammateId,
    place_path: String,
    tab_id: Option<tandem_core::ids::TabId>,
) -> Result<tandem_store::repo::panes::PaneRecord, IpcError> {
    use crate::services::teammates::{pane_name, TeammatesService};
    use tandem_core::ErrorCode;
    use tandem_store::repo::{accounts, panes, tabs};
    let teammate = state.teammates.get(&id).await?;
    let place = TeammatesService::place_for(&teammate, Some(&place_path))?;
    let engines = &state.terminals.engines;
    let engine = engines.get(&teammate.engine_id).ok_or_else(|| {
        IpcError::new(
            ErrorCode::EngineMissing,
            "This engine isn't known to Vibezeo",
        )
    })?;
    if engines.installed(&teammate.engine_id).is_none() {
        return Err(IpcError::new(
            ErrorCode::EngineMissing,
            format!("{} isn't installed", engine.label),
        ));
    }
    state
        .terminals
        .check_live_limit(state.entitlements.limits().panes)?;
    let tab = match tab_id {
        Some(tab) => tab,
        None => {
            let added = crate::services::workspaces::add(
                &state.store,
                &place,
                state.entitlements.limits().workspaces,
            )
            .await?;
            tabs::list(&state.store, &added.workspace.id)
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "The workspace has no tab"))?
                .id
        }
    };
    // Its preferred account, while it's still one of the engine's.
    let account = match &teammate.preferred_account_id {
        Some(a) => accounts::get(&state.store, a)
            .await
            .ok()
            .filter(|acc| acc.engine_id == teammate.engine_id)
            .map(|acc| acc.id),
        None => None,
    };
    let mut pane = crate::commands::terminal::create_pane(
        &state,
        &tab,
        "agent",
        Some(place),
        Some(teammate.engine_id.clone()),
        account,
    )
    .await?;
    // Named after the teammate, so the Dashboard shows it under its name.
    let name = pane_name(&teammate.name);
    let named = match panes::rename(&state.store, &pane.id, &name).await {
        Ok(n) => n,
        Err(_) => panes::assign_human_name(&state.store, &pane.id, &[name], |_| 0).await?,
    };
    pane.human_name = Some(named);
    // The brief is typed whole (at most 8,000 characters, D-1301).
    let prompt = teammate.brief.trim().to_string();
    let terminals = state.terminals.clone();
    let pane_id = pane.id.clone();
    tauri::async_runtime::spawn(async move {
        if prompt.is_empty() {
            return;
        }
        if let Err(e) = terminals
            .type_prompt_when_live(
                &pane_id,
                &prompt,
                crate::services::launch::FIRST_PROMPT_WAIT,
            )
            .await
        {
            tracing::warn!(code = "teammates.terminal_prompt", error = %e.message, "the brief wasn't typed");
        }
    });
    tracing::info!(
        code = "teammates.launch_in_terminal",
        "teammate launched in a terminal"
    );
    Ok(pane)
}
