//! The launch sheet's lineups (F-CD-14, UI-12): every seat launches on its
//! own, so one failure only fails that seat and nothing rolls back; with
//! "Worktree per agent" each agent gets a worktree first (F-CD-15).

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tandem_core::ids::{TabId, WorkspaceId};
use tandem_core::{ErrorCode, IpcError};
use tandem_store::repo::panes::PaneRecord;
use tandem_store::repo::presets::{self, Isolation, Seat, SeatKind};
use tandem_store::repo::tabs;
use tandem_store::repo::workspaces::{self, Workspace};
use ts_rs::TS;

use crate::commands::terminal::{create_browser_pane, create_pane};
use crate::services::lanes;
use crate::state::AppState;

/// How long a launched agent may take to start and get ready before its
/// first prompt is dropped.
pub const FIRST_PROMPT_WAIT: Duration = Duration::from_secs(90);
/// How long a shell seat waits for its prompt before typing its command.
pub const SHELL_COMMAND_WAIT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LaunchedSeat {
    /// The seat's position in the lineup, each row's count expanded.
    pub seat_index: u32,
    pub pane: PaneRecord,
    /// Where the pane opened: the workspace, or the seat's worktree.
    pub workspace_id: WorkspaceId,
    pub tab_id: TabId,
    pub lane_branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FailedSeat {
    pub seat_index: u32,
    /// The seat as launched (count 1), for Retry.
    pub seat: Seat,
    pub error: IpcError,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LaunchReport {
    pub launched: Vec<LaunchedSeat>,
    pub failed: Vec<FailedSeat>,
    /// The tab the shared-checkout seats opened in, when any did.
    pub shared_tab_id: Option<TabId>,
}

/// The briefs the sheet's roles start with (UI_SPEC UI-12).
pub fn role_brief(role: &str) -> Option<&'static str> {
    match role.trim().to_ascii_lowercase().as_str() {
        "planner" => Some("Break the job into steps and write them to TASKS.md. Don't edit code."),
        "builder" => Some("Implement the assigned step. Keep changes small and run tests."),
        "reviewer" => {
            Some("Review the diff on your branch against the goal. List problems; fix only obvious ones.")
        }
        "tester" => Some("Write and run tests for the change. Report failures with repro steps."),
        "lead" => Some("Coordinate the others through the task board; don't write code yourself."),
        _ => None,
    }
}

/// An agent's first prompt: its worktree briefing, its role, then the task.
/// Nothing is typed without a task.
pub fn first_prompt(
    briefing: Option<&str>,
    role: Option<&str>,
    task: Option<&str>,
) -> Option<String> {
    let task = task.map(str::trim).filter(|t| !t.is_empty())?;
    let mut parts: Vec<String> = Vec::new();
    if let Some(b) = briefing {
        parts.push(b.to_string());
    }
    if let Some(role) = role.map(str::trim).filter(|r| !r.is_empty()) {
        parts.push(match role_brief(role) {
            Some(brief) => format!("Your role: {role}. {brief}"),
            None => format!("Your role: {role}."),
        });
    }
    parts.push(format!("Task: {task}"));
    Some(parts.join("\n\n"))
}

/// The lineup as launched: each row repeated `count` times, count 1 each.
pub fn expand(seats: &[Seat]) -> Vec<Seat> {
    seats
        .iter()
        .flat_map(|s| {
            std::iter::repeat_n(
                Seat {
                    count: 1,
                    ..s.clone()
                },
                s.count as usize,
            )
        })
        .collect()
}

/// The shared tab's name: the task's first words, or "Agents".
pub fn tab_name(task: Option<&str>) -> String {
    let mut name = String::new();
    for word in task.unwrap_or("").split_whitespace() {
        if name.chars().count() + word.chars().count() + 1 > 28 {
            break;
        }
        if !name.is_empty() {
            name.push(' ');
        }
        name.push_str(word);
    }
    if name.is_empty() {
        "Agents".into()
    } else {
        name
    }
}

/// What a worktree's slug comes from: the seat's task, the shared task, its
/// role, or "session".
fn slug_source(seat: &Seat, task: Option<&str>) -> String {
    [seat.task.as_deref(), task, seat.role.as_deref()]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or("")
        .to_string()
}

/// Debug builds: `TANDEM_FAKE_LANE_FAIL=<n>` (or a file holding it) fails
/// the nth worktree of a launch, 1-based, so E2E can check partial failure
/// (F-CD-14 AC3). Release builds never read it.
fn injected_lane_failure(nth: usize) -> Option<IpcError> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let raw = std::env::var("TANDEM_FAKE_LANE_FAIL").ok()?;
    let text = match std::fs::read_to_string(&raw) {
        Ok(contents) if Path::new(&raw).is_file() => contents,
        _ => raw,
    };
    (text.trim().parse::<usize>().ok() == Some(nth))
        .then(|| IpcError::new(ErrorCode::Io, "Couldn't create the worktree").with_stage("git"))
}

fn opening(seats: &[Seat]) -> u32 {
    seats.iter().filter(|s| s.kind != SeatKind::Browser).count() as u32
}

/// `launch_lineup`: the whole lineup, or nothing when it would go over the
/// plan's live-pane cap.
/// A folder on an SSH host gets its worktrees from the host's git (D-1602);
/// a WSL folder has none yet (its git runs inside the distro), so the sheet
/// offers Shared only there.
fn check_isolation(ws: &Workspace, isolation: Isolation) -> Result<(), IpcError> {
    if isolation == Isolation::Worktree && ws.kind == "wsl" {
        return Err(IpcError::new(
            ErrorCode::Unavailable,
            "Worktrees aren't available in WSL folders yet",
        ));
    }
    Ok(())
}

pub async fn launch(
    state: &AppState,
    workspace: &WorkspaceId,
    seats: &[Seat],
    task: Option<&str>,
    isolation: Isolation,
) -> Result<LaunchReport, IpcError> {
    presets::validate(seats)?;
    let seats = expand(seats);
    let ws = workspaces::get(&state.store, workspace).await?;
    check_isolation(&ws, isolation)?;
    state
        .terminals
        .check_room(state.entitlements.limits().panes, opening(&seats))?;

    // Worktrees first, so each briefing can name its siblings.
    let mut lanes: Vec<Option<Result<Workspace, IpcError>>> = vec![None; seats.len()];
    if isolation == Isolation::Worktree && seats.iter().any(|s| s.kind == SeatKind::Agent) {
        let login = state.terminals.login_env().await;
        let git = lanes::git_for(&state.data, &login);
        let agents = seats
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind == SeatKind::Agent);
        for (nth, (i, seat)) in agents.enumerate() {
            lanes[i] = Some(match (&git, injected_lane_failure(nth + 1)) {
                (_, Some(fault)) => Err(fault),
                (Ok(git), None) => {
                    lanes::create(
                        &state.store,
                        git,
                        &state.entitlements.limits(),
                        workspace,
                        &slug_source(seat, task),
                    )
                    .await
                }
                (Err(e), None) => Err(e.clone()),
            });
        }
    }
    let branches: Vec<String> = lanes
        .iter()
        .filter_map(|l| l.as_ref().and_then(|r| r.as_ref().ok()))
        .filter_map(|l| l.lane_branch.clone())
        .collect();

    let mut report = LaunchReport {
        launched: Vec::new(),
        failed: Vec::new(),
        shared_tab_id: None,
    };
    for (i, (seat, lane)) in seats.iter().zip(lanes).enumerate() {
        let index = i as u32;
        match launch_seat(
            state,
            &ws,
            seat,
            task,
            lane,
            &branches,
            &mut report.shared_tab_id,
        )
        .await
        {
            Ok(mut launched) => {
                launched.seat_index = index;
                report.launched.push(launched);
            }
            Err(error) => report.failed.push(FailedSeat {
                seat_index: index,
                seat: seat.clone(),
                error,
            }),
        }
    }
    tracing::info!(
        code = "launch.lineup",
        launched = report.launched.len(),
        failed = report.failed.len(),
        "lineup launched"
    );
    Ok(report)
}

/// `launch_retry_seat`: one failed seat again. Its pane joins `tab` (the
/// lineup's shared tab) when it stays in the checkout.
pub async fn retry(
    state: &AppState,
    workspace: &WorkspaceId,
    seat: &Seat,
    task: Option<&str>,
    isolation: Isolation,
    tab: Option<TabId>,
) -> Result<LaunchedSeat, IpcError> {
    presets::validate(std::slice::from_ref(seat))?;
    let seat = Seat {
        count: 1,
        ..seat.clone()
    };
    let ws = workspaces::get(&state.store, workspace).await?;
    check_isolation(&ws, isolation)?;
    state.terminals.check_room(
        state.entitlements.limits().panes,
        opening(std::slice::from_ref(&seat)),
    )?;
    let top = ws.parent_id.clone().unwrap_or_else(|| ws.id.clone());
    let siblings: Vec<String> = workspaces::lanes(&state.store, &top)
        .await?
        .into_iter()
        .filter_map(|l| l.lane_branch)
        .collect();
    let lane = if isolation == Isolation::Worktree && seat.kind == SeatKind::Agent {
        let login = state.terminals.login_env().await;
        Some(match lanes::git_for(&state.data, &login) {
            Ok(git) => {
                lanes::create(
                    &state.store,
                    &git,
                    &state.entitlements.limits(),
                    workspace,
                    &slug_source(&seat, task),
                )
                .await
            }
            Err(e) => Err(e),
        })
    } else {
        None
    };
    let mut shared = tab;
    launch_seat(state, &ws, &seat, task, lane, &siblings, &mut shared).await
}

async fn launch_seat(
    state: &AppState,
    ws: &Workspace,
    seat: &Seat,
    task: Option<&str>,
    lane: Option<Result<Workspace, IpcError>>,
    branches: &[String],
    shared_tab: &mut Option<TabId>,
) -> Result<LaunchedSeat, IpcError> {
    if seat.kind == SeatKind::Agent {
        let id = seat
            .engine_id
            .as_deref()
            .ok_or_else(|| IpcError::new(ErrorCode::Invalid, "Choose an engine"))?;
        let engine = state
            .terminals
            .engines
            .get(id)
            .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "No engine with that id"))?;
        // A host's engines are its own: the pane checks them there.
        if ws.kind == "local" && state.terminals.engines.installed(id).is_none() {
            return Err(IpcError::new(
                ErrorCode::EngineMissing,
                format!("{} isn't installed", engine.label),
            ));
        }
    }
    // Where the pane goes: the seat's worktree, or the lineup's shared tab.
    let (home, tab, briefing) = match lane {
        Some(Ok(lane)) => {
            let tab = tabs::list(&state.store, &lane.id)
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "The worktree has no tab"))?;
            let branch = lane.lane_branch.clone().unwrap_or_default();
            let others: Vec<String> = branches.iter().filter(|b| **b != branch).cloned().collect();
            let briefing = tandem_git::lanes::briefing(Path::new(&lane.root), &branch, &others);
            (lane, tab.id, Some(briefing))
        }
        Some(Err(e)) => return Err(e),
        None => {
            let tab = match shared_tab {
                Some(tab) => tab.clone(),
                None => {
                    let tab = tabs::create(&state.store, &ws.id, Some(tab_name(task))).await?;
                    *shared_tab = Some(tab.id.clone());
                    tab.id
                }
            };
            (ws.clone(), tab, None)
        }
    };
    let pane = match seat.kind {
        // A browser seat opens its page (`localhost:3000` reads as http).
        SeatKind::Browser => create_browser_pane(state, &tab, seat.url.clone()).await?,
        SeatKind::Agent | SeatKind::Shell => {
            let kind = if seat.kind == SeatKind::Agent {
                "agent"
            } else {
                "terminal"
            };
            create_pane(
                state,
                &tab,
                kind,
                Some(home.root.clone()),
                seat.engine_id.clone(),
                seat.account_id.clone(),
            )
            .await?
        }
    };

    let terminals = state.terminals.clone();
    let id = pane.id.clone();
    match seat.kind {
        SeatKind::Agent => {
            let task = seat
                .task
                .as_deref()
                .filter(|t| !t.trim().is_empty())
                .or(task);
            if let Some(prompt) = first_prompt(briefing.as_deref(), seat.role.as_deref(), task) {
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = terminals
                        .type_prompt_when_live(&id, &prompt, FIRST_PROMPT_WAIT)
                        .await
                    {
                        tracing::warn!(code = "launch.prompt", error = %e.message, "first prompt not sent");
                    }
                });
            }
        }
        SeatKind::Shell => {
            if let Some(command) = seat.command.clone().filter(|c| !c.trim().is_empty()) {
                tauri::async_runtime::spawn(async move {
                    if !terminals
                        .type_command_when_ready(&id, &command, SHELL_COMMAND_WAIT)
                        .await
                    {
                        tracing::warn!(code = "launch.command", "shell seat didn't become ready");
                    }
                });
            }
        }
        SeatKind::Browser => {}
    }
    Ok(LaunchedSeat {
        seat_index: 0,
        workspace_id: home.id.clone(),
        lane_branch: home.lane_branch.clone(),
        tab_id: tab,
        pane,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_rm_01_worktrees_launch_on_hosts_but_not_in_wsl_folders() {
        let ws = |kind: &str| Workspace {
            id: WorkspaceId::new(),
            kind: kind.into(),
            host_id: "h".into(),
            root: "/home/tandem/project".into(),
            wsl_distro: None,
            name: "project".into(),
            parent_id: None,
            lane_branch: None,
            lane_base: None,
            sort_key: "a".into(),
            pinned: false,
            collapsed: false,
            sessions_collapsed: false,
            created_at: 0,
        };
        for kind in ["local", "ssh"] {
            assert!(check_isolation(&ws(kind), Isolation::Worktree).is_ok());
        }
        assert!(check_isolation(&ws("wsl"), Isolation::Shared).is_ok());
        let e = check_isolation(&ws("wsl"), Isolation::Worktree).expect_err("refused");
        assert_eq!(e.code, ErrorCode::Unavailable);
    }

    #[test]
    fn f_cd_14_1_each_agent_gets_its_role_then_the_task() {
        let p = first_prompt(None, Some("Builder"), Some("fix flaky tests")).expect("prompt");
        assert_eq!(
            p,
            "Your role: Builder. Implement the assigned step. Keep changes small and run tests.\n\nTask: fix flaky tests"
        );
        assert_eq!(
            first_prompt(None, Some("Release captain"), Some("ship it")).as_deref(),
            Some("Your role: Release captain.\n\nTask: ship it")
        );
        // No task, nothing typed.
        assert_eq!(
            first_prompt(Some("brief"), Some("Builder"), Some("  ")),
            None
        );
    }

    #[test]
    fn f_cd_15_a_worktree_agent_reads_its_briefing_before_its_role_and_task() {
        let p = first_prompt(
            Some("You are working in the git worktree"),
            Some("Tester"),
            Some("fix auth"),
        )
        .expect("prompt");
        let order: Vec<usize> = ["You are working", "Your role: Tester", "Task: fix auth"]
            .iter()
            .map(|s| p.find(s).expect(s))
            .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{p}");
    }

    #[test]
    fn f_cd_14_rows_expand_to_one_seat_each() {
        let rows = vec![
            Seat {
                count: 3,
                engine_id: Some("claude".into()),
                ..Seat::agent("Builder")
            },
            Seat::shell("Tests"),
        ];
        let seats = expand(&rows);
        assert_eq!(seats.len(), 4);
        assert!(seats.iter().all(|s| s.count == 1));
        assert_eq!(seats[3].kind, SeatKind::Shell);
        assert_eq!(opening(&seats), 4);
    }

    #[test]
    fn f_cd_14_the_shared_tab_is_named_after_the_task() {
        assert_eq!(tab_name(Some("fix flaky tests")), "fix flaky tests");
        assert_eq!(
            tab_name(Some(
                "migrate the billing service to the new ledger and retry queue"
            )),
            "migrate the billing service"
        );
        assert_eq!(tab_name(None), "Agents");
        assert_eq!(slug_source(&Seat::agent("Reviewer"), None), "Reviewer");
        assert_eq!(
            slug_source(
                &Seat {
                    task: Some("seat job".into()),
                    ..Seat::agent("x")
                },
                Some("all")
            ),
            "seat job"
        );
    }
}
