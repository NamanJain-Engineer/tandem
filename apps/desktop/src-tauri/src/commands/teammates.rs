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
