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

fn skill_err(e: tandem_store::skills::SkillError) -> IpcError {
    use tandem_core::ErrorCode;
    use tandem_store::skills::SkillError;
    let code = match &e {
        SkillError::Invalid(_) => ErrorCode::Invalid,
        SkillError::NotFound(_) => ErrorCode::NotFound,
        SkillError::Exists(_) => ErrorCode::Conflict,
        SkillError::Io(_) => ErrorCode::Io,
    };
    IpcError::new(code, e.to_string())
}

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
    tandem_store::skills::remove(std::path::Path::new(&teammate.home), &slug).map_err(skill_err)?;
    state.teammates.publish().await;
    Ok(())
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

/// `teammate_draft_with_ai` (F-AG-04): Pro, signed in and online; nothing
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
            "Create with AI is part of Pro.",
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
            let body = draft::body(&description, &engines, &plugins);
            let api = state.api.clone();
            state
                .auth
                .with_token(|token| {
                    let (api, body) = (api.clone(), body.clone());
                    async move { api.ai_teammate_draft(&token, body).await }
                })
                .await
                .map_err(|e| draft::api_error(&e))?
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
