//! The E2E test harness (ARCHITECTURE §3.2): debug builds only, and only
//! when `TANDEM_E2E=1`. Release builds don't compile this module.

use serde::{Deserialize, Serialize};
use tauri::State;
use ts_rs::TS;

use tandem_core::{ErrorCode, IpcError};

use crate::services::auth::AuthState;
use crate::services::terminal::SpawnRecord;
use crate::state::AppState;

fn guard(state: &AppState) -> Result<(), IpcError> {
    if state.e2e {
        Ok(())
    } else {
        Err(IpcError::new(
            ErrorCode::PermissionDenied,
            "The test harness is off",
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TestCounters {
    pub pty_resizes: u64,
    /// `pty_resize` calls per pane.
    pub pty_resizes_by_pane: std::collections::BTreeMap<String, u32>,
    pub webgl_contexts: u64,
    pub dom_renderers: u64,
    pub reflows: u64,
    /// The most agents ever spawning at once during restore (F-SH-07 AC3).
    pub restore_max_spawning: u64,
}

#[tauri::command]
pub fn test_counters(
    state: State<'_, AppState>,
    reset: Option<bool>,
) -> Result<TestCounters, IpcError> {
    guard(&state)?;
    let reset = reset.unwrap_or(false);
    Ok(TestCounters {
        pty_resizes_by_pane: crate::commands::app::resize_counts_by_pane(reset),
        pty_resizes: crate::commands::app::resize_count(reset),
        webgl_contexts: 0,
        dom_renderers: 0,
        reflows: 0,
        restore_max_spawning: state.restore.max_spawning(reset) as u64,
    })
}

#[tauri::command]
pub fn test_spawn_log(state: State<'_, AppState>) -> Result<Vec<SpawnRecord>, IpcError> {
    guard(&state)?;
    Ok(state.terminals.spawn_records())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ClockNow {
    pub now: i64,
}

#[tauri::command]
pub fn test_clock_advance(state: State<'_, AppState>, ms: u64) -> Result<ClockNow, IpcError> {
    guard(&state)?;
    let clock = state.mock_clock.as_ref().ok_or_else(|| {
        IpcError::new(
            ErrorCode::Unavailable,
            "Start with TANDEM_FAKE_CLOCK to move time",
        )
    })?;
    clock.advance(std::time::Duration::from_millis(ms));
    Ok(ClockNow {
        now: state.clock.now_ms(),
    })
}

/// Forces the refresh path, e.g. to observe a revoked device (E2E-M2-6).
#[tauri::command]
pub async fn test_auth_refresh(state: State<'_, AppState>) -> Result<AuthState, IpcError> {
    guard(&state)?;
    let _ = state.auth.force_refresh().await;
    state.entitlements.refresh().await;
    Ok(state.auth.state())
}

/// A second account for an engine, with its own home (E2E-M4-4). The real
/// sign-in flow is `accounts_begin_add` (F-AC-02).
#[tauri::command]
pub async fn test_account_create(
    state: State<'_, AppState>,
    engine_id: String,
    label: String,
) -> Result<tandem_store::repo::accounts::Account, IpcError> {
    guard(&state)?;
    let engine = state
        .terminals
        .engines
        .get(&engine_id)
        .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "No engine with that id"))?;
    state
        .terminals
        .engines
        .create_account(&engine, &label)
        .await
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FixtureAgent {
    pub id: String,
    pub title: String,
    pub status: tandem_core::status::AgentStatus,
    pub engine_id: Option<String>,
    pub workspace_id: Option<String>,
    pub preview: Option<String>,
    /// Asks for approval with this summary (the agent waits until answered).
    pub approval: Option<String>,
}

/// A scripted agent for the Dashboard and notification E2E (the "fixture
/// source"): it shows in the status bus like any agent; with `approval` it
/// waits on the broker, and once allowed it goes back to work.
#[tauri::command]
pub async fn test_fixture_agent(
    state: State<'_, AppState>,
    agent: FixtureAgent,
) -> Result<Option<tandem_core::approvals::ApprovalView>, IpcError> {
    use tandem_core::approvals::{ApprovalKind, ApprovalRequest, ApprovalScope};
    use tandem_core::source::SourceRef;
    use tandem_core::status::{AgentStatus, StatusUpdate, WaitingReason};
    guard(&state)?;
    let source = SourceRef::Fixture {
        fixture_id: agent.id.clone(),
    };
    let meta = crate::services::status::ExternalMeta {
        source: source.clone(),
        title: agent.title.clone(),
        engine_id: agent.engine_id.clone(),
        workspace_id: agent.workspace_id.clone(),
    };
    let status = state.terminals.status.clone();
    let update = |status: AgentStatus, reason: Option<WaitingReason>, preview: Option<String>| {
        StatusUpdate {
            status,
            reason,
            ts: 0,
            tool: None,
            title: Some(agent.title.clone()),
            preview,
        }
    };
    let Some(summary) = agent.approval.clone() else {
        status.set_external(meta, update(agent.status, None, agent.preview.clone()));
        return Ok(None);
    };
    status.set_external(
        meta.clone(),
        update(
            AgentStatus::Waiting,
            Some(WaitingReason::Approval),
            Some(format!("Wants to run: {summary}")),
        ),
    );
    let (view, ticket) = state.approvals.broker.request(ApprovalRequest {
        source,
        kind: ApprovalKind::Command,
        summary: format!("Run `{summary}`"),
        detail: summary.clone(),
        options: vec![ApprovalScope::Once],
    });
    let title = agent.title.clone();
    tauri::async_runtime::spawn(async move {
        let outcome = ticket.outcome().await;
        let (next, preview) = if outcome.allowed() {
            (AgentStatus::Working, format!("Running {summary}"))
        } else {
            (AgentStatus::Idle, "Denied".to_string())
        };
        status.set_external(
            meta,
            StatusUpdate {
                status: next,
                reason: None,
                ts: 0,
                tool: None,
                title: Some(title),
                preview: Some(preview),
            },
        );
    });
    Ok(Some(view))
}

/// What the recording notifier was asked to show (F-DB-03 AC1).
#[tauri::command]
pub fn test_notifier_calls(
    state: State<'_, AppState>,
    clear: Option<bool>,
) -> Result<Vec<crate::services::notifier::OsNotification>, IpcError> {
    guard(&state)?;
    let rec = state
        .notifications
        .recording
        .get()
        .ok_or_else(|| IpcError::new(ErrorCode::Unavailable, "No recording notifier"))?;
    let calls = rec.calls();
    if clear.unwrap_or(false) {
        rec.clear();
    }
    Ok(calls)
}

/// Clicks a recorded OS notification: focuses the app and its source.
#[tauri::command]
pub async fn test_notifier_click(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    index: usize,
) -> Result<(), IpcError> {
    guard(&state)?;
    let source = state
        .notifications
        .recording
        .get()
        .and_then(|r| r.calls().get(index).map(|c| c.source.clone()))
        .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "No such notification"))?;
    crate::services::notifications::NotificationService::click(&app, source).await;
    Ok(())
}

/// Pretends the window is focused or not (`None`: the real state).
#[tauri::command]
pub fn test_set_window_focused(
    state: State<'_, AppState>,
    focused: Option<bool>,
) -> Result<(), IpcError> {
    guard(&state)?;
    state.notifications.override_focus(focused);
    Ok(())
}

/// Seeds a thread row in `state` (E2E-M5-3: a thread a crash left running).
/// Threads arrive in M7; this only writes the row restore settles.
#[tauri::command]
pub async fn test_thread_seed(
    state: State<'_, AppState>,
    thread_state: String,
) -> Result<String, IpcError> {
    guard(&state)?;
    let id = format!("th_test{}", chrono::Utc::now().timestamp_millis());
    let (row, now) = (id.clone(), chrono::Utc::now().timestamp_millis());
    state
        .store
        .write(move |t| {
            t.execute(
                "INSERT INTO threads (id, kind, cwd, engine_id, access, mode, state, sort_key, created_at, updated_at)
                 VALUES (?1, 'thread', '/tmp', 'claude', 'edits', 'act', ?2, ?1, ?3, ?3)",
                rusqlite::params![row, thread_state, now],
            )?;
            Ok(())
        })
        .await?;
    Ok(id)
}

/// Writes `events` stored events into a thread as finished turns (threads
/// perf suite): a message, a reply, tool calls and the turn end, each turn
/// 6 events; `fence_bytes` adds a last reply holding a code fence that big.
#[tauri::command]
pub async fn test_thread_seed_events(
    state: State<'_, AppState>,
    thread_id: String,
    events: u32,
    fence_bytes: Option<u32>,
) -> Result<u32, IpcError> {
    use tandem_engines::events::{ThreadEvent, ToolStatus, ToolVerb, TurnEndReason};
    guard(&state)?;
    let now = chrono::Utc::now().timestamp_millis();
    let turns = events.div_ceil(6).max(1);
    let fence = fence_bytes.map(|n| {
        let line = "let value = compute(input, options);\n";
        let mut body = String::with_capacity(n as usize + 16);
        while body.len() < n as usize {
            body.push_str(line);
        }
        format!("A long listing:\n\n```rust\n{body}```\n")
    });
    let written = state
        .store
        .write(move |t| {
            let mut written = 0u32;
            let first: u32 = t.query_row(
                "SELECT COALESCE(MAX(n), 0) FROM thread_turns WHERE thread_id = ?1",
                [&thread_id],
                |r| r.get(0),
            )?;
            for i in 0..turns {
                let turn = format!("turn_seed{i}_{now}");
                let at = now - i64::from(turns - i) * 1000;
                t.execute(
                    "INSERT INTO thread_turns (id, thread_id, n, engine_id, state, blocked_actions, started_at, ended_at)
                     VALUES (?1, ?2, ?3, 'claude', 'completed', 0, ?4, ?4)",
                    rusqlite::params![turn, thread_id, first + i + 1, at],
                )?;
                let last = i + 1 == turns;
                let reply = match (&fence, last) {
                    (Some(f), true) => f.clone(),
                    _ => format!(
                        "## Step {i}\n\nThe change in `src/module_{i}.rs` keeps the **cache** warm. It touches:\n\n- the reader\n- the writer\n- [docs](https://vibezeo.com/docs)\n"
                    ),
                };
                let mut evs = vec![
                    ThreadEvent::User { text: format!("Message {i}: tidy the module and run the tests"), attachments: Vec::new() },
                    ThreadEvent::Assistant { markdown: reply },
                ];
                for k in 0..3 {
                    evs.push(ThreadEvent::Tool {
                        id: format!("tool_{i}_{k}"),
                        verb: if k == 0 { ToolVerb::Read } else { ToolVerb::Bash },
                        arg_summary: format!("src/module_{i}_{k}.rs"),
                        status: ToolStatus::Ok,
                        output: Some("ok".into()),
                        output_truncated: false,
                        output_file: None,
                        diff: None,
                        detail: None,
                    });
                }
                evs.push(ThreadEvent::TurnEnd { reason: TurnEndReason::Completed, blocked_actions: 0 });
                for (seq, ev) in evs.iter().enumerate() {
                    let payload = serde_json::to_string(ev).map_err(|e| tandem_store::StoreError::Invalid(e.to_string()))?;
                    let kind = serde_json::to_value(ev)
                        .ok()
                        .and_then(|v| v.get("type").and_then(|k| k.as_str()).map(str::to_string))
                        .unwrap_or_default();
                    t.execute(
                        "INSERT INTO thread_messages (thread_id, turn_id, seq, type, payload_json, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        rusqlite::params![thread_id, turn, seq as i64, kind, payload, at],
                    )?;
                    written += 1;
                }
            }
            Ok(written)
        })
        .await?;
    Ok(written)
}

/// A thread's stored state (E2E-M5-3).
#[tauri::command]
pub async fn test_thread_state(state: State<'_, AppState>, id: String) -> Result<String, IpcError> {
    guard(&state)?;
    Ok(state
        .store
        .read(move |c| {
            Ok(
                c.query_row("SELECT state FROM threads WHERE id = ?1", [id], |r| {
                    r.get::<_, String>(0)
                })?,
            )
        })
        .await?)
}

/// The version the harness's updater "installed" (E2E-M5-7), if any.
#[tauri::command]
pub fn test_update_installed(state: State<'_, AppState>) -> Result<Option<String>, IpcError> {
    guard(&state)?;
    Ok(std::fs::read_to_string(state.data.root().join("test-update-installed")).ok())
}

/// Moves the minimum-version notice `ms` into the past (E2E-M5-7's grace).
#[tauri::command]
pub fn test_min_version_shift(
    state: State<'_, AppState>,
    ms: i64,
) -> Result<crate::services::updates::UpdateState, IpcError> {
    guard(&state)?;
    state.updates.shift_min_version_seen(ms);
    Ok(state.updates.state())
}

/// Connects a test vendor (`test_echo`, `test_upper`, `test_down`) without a
/// sign-in: the debug build's stand-in for Settings › Plugins (M12).
#[tauri::command]
pub async fn test_plugin_connect(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<tandem_store::repo::plugins::PluginConnection, IpcError> {
    guard(&state)?;
    if !tandem_gateway::vendors::registry()
        .iter()
        .any(|m| m.manifest().id == plugin_id)
    {
        return Err(IpcError::new(
            ErrorCode::NotFound,
            format!("This build has no {plugin_id} module"),
        ));
    }
    let conn =
        tandem_store::repo::plugins::insert_connection(&state.store, &plugin_id, None, "none")
            .await?;
    state.gateway.refresh_connections().await;
    // `plugins:changed` (F-PL-03): the new connection's card.
    if let Some(card) = state
        .plugins
        .catalog()
        .await?
        .into_iter()
        .find(|c| c.connection_id.as_ref() == Some(&conn.id))
    {
        state.bus.publish(tandem_core::bus::BusEvent::new(
            tandem_core::bus::Topic::Plugins,
            "plugins:changed",
            card,
        ));
    }
    Ok(conn)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TestGateway {
    pub metrics: tandem_gateway::GatewayMetrics,
    /// Gateways started by this process (one, F-PL-01 AC2).
    pub starts: u32,
}

#[tauri::command]
pub async fn test_gateway_metrics(state: State<'_, AppState>) -> Result<TestGateway, IpcError> {
    guard(&state)?;
    state.gateway.handle().await?;
    let (metrics, starts) = state
        .gateway
        .metrics()
        .ok_or_else(|| IpcError::new(ErrorCode::Unavailable, "The gateway isn't running"))?;
    Ok(TestGateway { metrics, starts })
}

/// Each task run pauses `ms` before making its lane (F-TK-04 AC3's E2E).
#[tauri::command]
pub async fn test_task_lane_delay(state: State<'_, AppState>, ms: u64) -> Result<(), IpcError> {
    guard(&state)?;
    state.task_exec.set_test_lane_delay(ms);
    Ok(())
}

/// One scheduler pass as if the clock said `at` (ms), without moving the
/// app's clock: E2E-M11-3/4 "advance the clock to 09:00".
#[tauri::command]
pub async fn test_routines_tick(
    state: State<'_, AppState>,
    at: i64,
) -> Result<Vec<String>, IpcError> {
    guard(&state)?;
    Ok(state
        .routines
        .tick_at(at)
        .await
        .into_iter()
        .map(|r| r.as_str().to_string())
        .collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TestKeyWhich {
    Hold,
    Toggle,
    Esc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TestKeyPhase {
    Down,
    Up,
}

fn dictate_fakes(state: &AppState) -> Result<&crate::services::dictate::Fakes, IpcError> {
    guard(state)?;
    state
        .dictate
        .fakes
        .as_ref()
        .ok_or_else(|| IpcError::new(ErrorCode::Unavailable, "no fakes"))
}

/// Presses or releases the bound shortcut through the fake hotkey source.
#[tauri::command]
pub fn test_dictate_key(
    state: State<'_, AppState>,
    which: TestKeyWhich,
    phase: TestKeyPhase,
) -> Result<bool, IpcError> {
    use tandem_voice::hotkey::{Phase, Which};
    let fakes = dictate_fakes(&state)?;
    let which = match which {
        TestKeyWhich::Hold => Which::Hold,
        TestKeyWhich::Toggle => Which::Toggle,
        TestKeyWhich::Esc => Which::Esc,
    };
    let phase = match phase {
        TestKeyPhase::Down => Phase::Down,
        TestKeyPhase::Up => Phase::Up,
    };
    Ok(fakes.hotkeys.send(which, phase))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "source", rename_all = "snake_case")]
#[ts(export)]
pub enum TestMicSource {
    Wav { path: String },
    Silence,
    Zeros,
}

/// What the fake microphone plays from the next take on.
#[tauri::command]
pub fn test_dictate_mic(state: State<'_, AppState>, source: TestMicSource) -> Result<(), IpcError> {
    use tandem_voice::capture::FakeSource;
    guard(&state)?;
    let source = match source {
        TestMicSource::Wav { path } => {
            let (samples, rate) = tandem_voice::audio::read_wav(std::path::Path::new(&path))
                .map_err(|e| IpcError::new(ErrorCode::NotFound, e.to_string()))?;
            FakeSource::Samples {
                samples: std::sync::Arc::new(samples),
                rate,
                speed: 1,
            }
        }
        TestMicSource::Silence => FakeSource::Samples {
            samples: std::sync::Arc::new(crate::services::dictate::dither(16_000 * 30)),
            rate: 16_000,
            speed: 1,
        },
        TestMicSource::Zeros => FakeSource::Zeros,
    };
    state.dictate.test_mic(source)
}

/// The recording insertion backend: its calls, what each paste typed and
/// the clipboard; optionally reconfigured first.
#[tauri::command]
pub fn test_dictate_platform(
    state: State<'_, AppState>,
    clear: Option<bool>,
    target: Option<tandem_voice::insert::Target>,
    no_target: Option<bool>,
    fail_focus: Option<bool>,
    fail_paste: Option<bool>,
    clipboard: Option<String>,
) -> Result<crate::services::dictate::FakePlatformView, IpcError> {
    use tandem_voice::insert::fake::Call;
    use tandem_voice::insert::Clip;
    let p = &dictate_fakes(&state)?.platform;
    if clear.unwrap_or(false) {
        p.clear_calls();
    }
    if let Some(t) = target {
        p.set_target(Some(t));
    }
    if no_target.unwrap_or(false) {
        p.set_target(None);
    }
    if let Some(f) = fail_focus {
        p.fail_focus(f);
    }
    if let Some(f) = fail_paste {
        p.fail_paste(f);
    }
    if let Some(c) = clipboard {
        p.set_clipboard(Clip::Text(c));
    }
    Ok(crate::services::dictate::FakePlatformView {
        calls: p
            .calls()
            .iter()
            .map(|c| match c {
                Call::CaptureTarget => "capture_target".to_string(),
                Call::RestoreFocus(w) => format!("restore_focus:{w}"),
                Call::Paste { terminal } => {
                    format!("paste:{}", if *terminal { "terminal" } else { "text" })
                }
                Call::ReadClipboard => "read_clipboard".into(),
                Call::WriteClipboard(_) => "write_clipboard".into(),
                Call::RestoreClipboard => "restore_clipboard".into(),
            })
            .collect(),
        pasted: p.pasted(),
        clipboard: match p.clipboard() {
            Clip::Text(t) => Some(t),
            _ => None,
        },
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TestWidget {
    pub visible: bool,
    pub x: i32,
    pub y: i32,
}

/// The widget window as the OS sees it.
#[tauri::command]
pub fn test_dictate_widget(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<TestWidget, IpcError> {
    use tauri::Manager;
    guard(&state)?;
    let w = app
        .get_webview_window(crate::services::widget::LABEL)
        .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "no widget window"))?;
    let pos = w.outer_position().unwrap_or_default();
    Ok(TestWidget {
        visible: w.is_visible().unwrap_or(false),
        x: pos.x,
        y: pos.y,
    })
}

/// What `test_echo__charge` costs now (F-PL-06 AC2: the amount changes
/// between approval and execution).
#[tauri::command]
pub fn test_plugin_price(state: State<'_, AppState>, cents: i64) -> Result<(), IpcError> {
    guard(&state)?;
    #[cfg(feature = "test-vendors")]
    tandem_gateway::vendors::test_echo::set_price(cents);
    #[cfg(not(feature = "test-vendors"))]
    let _ = cents;
    Ok(())
}

/// A Team allowlist fixture (F-PL-05 AC2), as if the org's policy listed
/// these plugin ids; `null` clears it.
#[tauri::command]
pub fn test_plugin_allowlist(
    state: State<'_, AppState>,
    plugin_ids: Option<Vec<String>>,
) -> Result<(), IpcError> {
    guard(&state)?;
    state.plugins.set_allowlist_fixture(plugin_ids);
    Ok(())
}

/// An unattended run's thread, as a routine run would start one (M11's
/// runner calls the same `set_unattended_origin`): Edits, no one there.
#[tauri::command]
pub async fn test_unattended_run(
    state: State<'_, AppState>,
    workspace_id: tandem_core::ids::WorkspaceId,
    routine_id: String,
    run_id: String,
) -> Result<tandem_store::repo::threads::ThreadRecord, IpcError> {
    guard(&state)?;
    let ws = tandem_store::repo::workspaces::get(&state.store, &workspace_id)
        .await
        .map_err(|e| IpcError::new(ErrorCode::NotFound, e.to_string()))?;
    let thread = state
        .threads
        .create(tandem_store::repo::threads::NewThread {
            id: tandem_core::ids::ThreadId::new(),
            kind: tandem_store::repo::threads::ThreadKind::Thread,
            workspace_id: Some(workspace_id),
            cwd: ws.root,
            engine_id: "claude".into(),
            account_id: None,
            model: None,
            effort: None,
            access: tandem_core::access::Access::Edits,
            mode: tandem_core::access::ThreadMode::Act,
            attended: false,
        })
        .await?;
    state
        .gateway
        .set_unattended_origin(&thread.id, &format!("routine:{routine_id}:{run_id}"));
    Ok(thread)
}
