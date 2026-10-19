//! ACP agents the user adds (ARCHITECTURE §3.2 "Engines, accounts, usage",
//! F-EN-10, UI-163).

use std::path::{Path, PathBuf};

use tauri::{State, WebviewWindow};

use tandem_core::{ErrorCode, IpcError};
use tandem_engines::acp_agents::{self, AcpAgent, AcpAgentInput};
use tandem_engines::chat::acp::probe::{self, AgentInfo, PROBE_WAIT};

use crate::state::AppState;

fn invalid(e: acp_agents::AgentError) -> IpcError {
    IpcError::new(ErrorCode::Invalid, e.to_string())
}

fn main_only(window: &WebviewWindow) -> Result<(), IpcError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(IpcError::new(
            ErrorCode::PermissionDenied,
            "Adding an agent needs the main window",
        ))
    }
}

/// The command as an absolute path: as typed, or found on the login PATH.
async fn resolve(state: &AppState, command: &str) -> Result<PathBuf, IpcError> {
    let command = command.trim();
    let missing = || {
        IpcError::new(
            ErrorCode::NotFound,
            format!("Vibezeo can't find {command}. Type its full path, or install it first."),
        )
    };
    if Path::new(command).is_absolute() {
        return Path::new(command)
            .is_file()
            .then(|| PathBuf::from(command))
            .ok_or_else(missing);
    }
    let login = state.terminals.login_env().await;
    let path = login.vars.get("PATH").cloned().unwrap_or_default();
    let pathext = cfg!(windows)
        .then(|| std::env::var("PATHEXT").ok())
        .flatten();
    tandem_pty::env::resolve_on_path(command, &path, pathext.as_deref()).ok_or_else(missing)
}

/// Runs the agent's `initialize` with the login environment plus its own,
/// in the user's home folder.
async fn test_input(
    state: &AppState,
    input: &AcpAgentInput,
) -> Result<(PathBuf, AgentInfo), IpcError> {
    acp_agents::check(input).map_err(invalid)?;
    let program = resolve(state, &input.command).await?;
    let login = state.terminals.login_env().await;
    let mut env: Vec<(String, String)> = login
        .vars
        .iter()
        .filter(|(k, _)| !input.env.contains_key(*k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    env.extend(crate::services::fake_engine_env());
    env.extend(input.env.clone());
    let home = login
        .vars
        .get("HOME")
        .map(PathBuf::from)
        .or_else(tandem_core::paths::user_home)
        .unwrap_or_else(std::env::temp_dir);
    let info = probe::probe(program.clone(), input.args.clone(), env, home, PROBE_WAIT)
        .await
        .map_err(|e| {
            IpcError::new(ErrorCode::Unavailable, format!("Not an ACP agent: {e}"))
                .with_stage("acp_test")
        })?;
    tracing::info!(
        code = "acp_agents.tested",
        "an ACP agent answered initialize"
    );
    Ok((program, info))
}

/// The agents the user added.
#[tauri::command]
pub async fn acp_agents_list(state: State<'_, AppState>) -> Result<Vec<AcpAgent>, IpcError> {
    Ok(state.terminals.engines.acp_agents())
}

/// Test: `initialize` only, nothing saved.
#[tauri::command]
pub async fn acp_agent_test(
    window: WebviewWindow,
    state: State<'_, AppState>,
    input: AcpAgentInput,
) -> Result<AgentInfo, IpcError> {
    main_only(&window)?;
    Ok(test_input(&state, &input).await?.1)
}

/// Add: the same test, then saved with the command's absolute path.
#[tauri::command]
pub async fn acp_agent_add(
    window: WebviewWindow,
    state: State<'_, AppState>,
    input: AcpAgentInput,
) -> Result<AcpAgent, IpcError> {
    main_only(&window)?;
    let (program, _) = test_input(&state, &input).await?;
    let engines = &state.terminals.engines;
    let taken: Vec<String> = engines
        .registry()
        .all()
        .iter()
        .map(|e| e.id.clone())
        .collect();
    let agent = AcpAgent {
        id: acp_agents::id_for(&input.label, &taken),
        label: input.label.trim().to_string(),
        program: program.to_string_lossy().into_owned(),
        args: input.args.clone(),
        env: input.env.clone(),
    };
    agent
        .spec()
        .map_err(|e| IpcError::new(ErrorCode::Invalid, e.to_string()))?;
    engines.add_acp_agent(agent.clone())?;
    tracing::info!(code = "acp_agents.added", "an ACP agent was added");
    Ok(agent)
}

/// Remove: its threads stay, and say the engine is gone.
#[tauri::command]
pub async fn acp_agent_remove(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), IpcError> {
    main_only(&window)?;
    let engines = &state.terminals.engines;
    if engines.acp_agent(&id).is_none() {
        return Err(IpcError::new(ErrorCode::NotFound, "No agent with that id"));
    }
    engines.remove_acp_agent(&id)?;
    tracing::info!(code = "acp_agents.removed", "an ACP agent was removed");
    Ok(())
}
