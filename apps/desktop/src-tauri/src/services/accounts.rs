//! Engine accounts (F-AC-02, UI-35): sign-in and sign-out with the engine's
//! own login in a mini terminal under the account's provider home, then
//! rename, default, remove, and workspace overrides (F-AC-03). Tandem never
//! sees a token: success is the engine's status command or a file's
//! presence (§6, F-AC-01).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::ids::AccountId;
use tandem_core::{ErrorCode, IpcError};
use tandem_engines::accounts::{self as engine_accounts, Step};
use tandem_engines::homes::Seed;
use tandem_engines::launch::{self, HostKind, TerminalLaunchRequest};
use tandem_engines::registry::{EngineSpec, KeychainIsolation};
use tandem_pty::SpawnSpec;
use tandem_store::repo::accounts::{self, Account};
use tandem_store::Store;
use ts_rs::TS;

use crate::services::terminal::TerminalService;
use crate::state::AppState;

/// How long a sign-in may take (device codes, browser flows).
const LOGIN_WAIT: Duration = Duration::from_secs(15 * 60);
/// How long a TUI gets to show its prompt before the slash command is typed.
const READY_WAIT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(800);

pub const MORE_ACCOUNTS_ON_PRO: &str = "Pro lets you add more accounts per engine.";
pub const KEYCHAIN_SINGLE: &str =
    "This engine stores its sign-in in your Keychain; only one account is supported.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LoginAction {
    SignIn,
    SignOut,
}

/// A running sign-in or sign-out: its terminal and whether the engine can
/// say when it's done (`false`: the user clicks Done).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoginSession {
    pub op_id: String,
    /// The mini terminal's PTY (attach with `pty_attach`).
    pub pane_id: String,
    pub account_id: AccountId,
    pub action: LoginAction,
    pub reports: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LoginState {
    SignedIn,
    SignedOut,
    Failed,
    Cancelled,
}

/// `accounts:login-finished`: how a sign-in or sign-out ended.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoginEvent {
    pub op_id: String,
    pub account_id: AccountId,
    pub state: LoginState,
    /// The engine's last line when it failed (redacted).
    pub message: Option<String>,
}

/// Whether an account is signed in, as far as its engine can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SignInState {
    SignedIn,
    SignedOut,
    Unknown,
}

struct Op {
    pty: String,
    account: Account,
    /// The account isn't saved yet: a cancel or failure deletes its home.
    new_account: bool,
    action: LoginAction,
    engine: EngineSpec,
    program: PathBuf,
    env: Vec<(String, String)>,
    over: Arc<AtomicBool>,
}

/// The sign-ins and sign-outs in flight.
#[derive(Default)]
pub struct LoginOps {
    ops: Mutex<HashMap<String, Op>>,
}

impl LoginOps {
    fn take(&self, op: &str) -> Option<Op> {
        self.ops.lock().ok()?.remove(op)
    }
}

fn engine(state: &AppState, engine_id: &str) -> Result<EngineSpec, IpcError> {
    state
        .terminals
        .engines
        .get(engine_id)
        .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "No engine with that id"))
}

/// `accounts_begin_add`: a new account signs in under a new provider home.
pub async fn begin_add(
    state: &AppState,
    engine_id: &str,
    label: &str,
) -> Result<LoginSession, IpcError> {
    let label = label.trim();
    if label.is_empty() || label.chars().count() > 40 {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "Names are 1–40 characters",
        ));
    }
    let engine = engine(state, engine_id)?;
    let count = accounts::count_for(&state.store, engine_id).await?;
    if let Some(max) = state.entitlements.limits().accounts_per_engine {
        if count >= max {
            return Err(IpcError::new(ErrorCode::LimitReached, MORE_ACCOUNTS_ON_PRO));
        }
    }
    if cfg!(target_os = "macos")
        && engine.keychain_isolation == KeychainIsolation::UnsupportedMac
        && count >= 1
    {
        return Err(IpcError::new(ErrorCode::Unavailable, KEYCHAIN_SINGLE));
    }
    if accounts::label_taken(&state.store, engine_id, label, None).await? {
        return Err(IpcError::new(
            ErrorCode::Conflict,
            "Another account for this engine has that name",
        ));
    }
    let homes = &state.terminals.engines.homes;
    let id = AccountId::new();
    let seed = Seed {
        theme: state.terminals.engines.theme().await,
    };
    let home = homes.create(&engine, id.as_str(), &seed).map_err(|e| {
        IpcError::new(ErrorCode::Internal, e.to_string()).with_stage("provider_home")
    })?;
    let account = Account {
        id,
        engine_id: engine_id.to_string(),
        label: label.to_string(),
        home_path: home.to_string_lossy().into_owned(),
        // An engine's first account is its default.
        is_default: count == 0,
        plan_label: None,
        created_at: tandem_store::now_ms(),
    };
    let program = state.terminals.resolve_engine(&engine).await;
    start(state, engine, program, account, true, LoginAction::SignIn).await
}

/// `accounts_begin_session`: an existing account signs in again or out.
pub async fn begin_session(
    state: &AppState,
    account_id: &AccountId,
    action: LoginAction,
) -> Result<LoginSession, IpcError> {
    let account = accounts::get(&state.store, account_id).await?;
    let engine = engine(state, &account.engine_id)?;
    let program = state.terminals.resolve_engine(&engine).await;
    start(state, engine, program, account, false, action).await
}

/// The env a sign-in runs with: what a pane gets plus the account's home.
pub(crate) async fn login_env(
    terminals: &TerminalService,
    engine: &EngineSpec,
    program: &std::path::Path,
    home: &std::path::Path,
    pty: &str,
) -> Result<Vec<(String, String)>, IpcError> {
    let login = terminals.login_env().await;
    let req = TerminalLaunchRequest {
        engine,
        program: program.to_path_buf(),
        extra_args: Vec::new(),
        resume: None,
        host: HostKind::Local,
        home: Some(home.to_path_buf()),
        providers_root: terminals.engines.homes.root().to_path_buf(),
        status: None,
    };
    let spec = launch::terminal_launch(&req)
        .map_err(|e| IpcError::new(ErrorCode::Unavailable, e.to_string()))?;
    let mut env = tandem_pty::env::pane_env(&login, pty, env!("CARGO_PKG_VERSION"));
    env.extend(spec.env);
    Ok(env.into_iter().collect())
}

async fn start(
    state: &AppState,
    engine: EngineSpec,
    program: Option<PathBuf>,
    account: Account,
    new_account: bool,
    action: LoginAction,
) -> Result<LoginSession, IpcError> {
    let homes = &state.terminals.engines.homes;
    let home = PathBuf::from(&account.home_path);
    let abandon = |e: IpcError| {
        if new_account {
            let _ = homes.remove(&home);
        }
        e
    };
    let Some(program) = program else {
        return Err(abandon(IpcError::new(
            ErrorCode::EngineMissing,
            format!("{} isn't installed on this machine.", engine.label),
        )));
    };
    let op_id = format!(
        "login_{}",
        AccountId::new().as_str().trim_start_matches("acc_")
    );
    let pty = op_id.clone();
    let step = match action {
        LoginAction::SignIn => engine_accounts::login_step(&engine),
        LoginAction::SignOut => engine_accounts::logout_step(&engine),
    };
    let mut argv = vec![program.to_string_lossy().into_owned()];
    if let Step::Argv(args) = &step {
        argv.extend(args.iter().cloned());
    }
    let env = login_env(&state.terminals, &engine, &program, &home, &pty)
        .await
        .map_err(abandon)?;
    let login = state.terminals.login_env().await;
    let args = tandem_pty::env::login_exec(&login.shell, &argv).map_err(|_| {
        abandon(IpcError::new(
            ErrorCode::Invalid,
            "The engine's path can't be launched safely",
        ))
    })?;
    let spec = SpawnSpec {
        pane_id: pty.clone(),
        program: login.shell.clone(),
        args,
        cwd: home.clone(),
        env: env.iter().cloned().collect(),
        cols: 100,
        rows: 20,
        scrollback_lines: 2_000,
        mark_token: None,
    };
    state
        .terminals
        .pty
        .spawn(spec, None)
        .map_err(|e| abandon(e.into()))?;
    let reports = engine.signed_in_file.is_some() || engine.status_cmd.is_some();
    let session = LoginSession {
        op_id: op_id.clone(),
        pane_id: pty.clone(),
        account_id: account.id.clone(),
        action,
        reports,
    };
    let over = Arc::new(AtomicBool::new(false));
    if let Ok(mut ops) = state.logins.ops.lock() {
        ops.insert(
            op_id.clone(),
            Op {
                pty,
                account,
                new_account,
                action,
                engine,
                program,
                env,
                over: over.clone(),
            },
        );
    }
    let watcher = Watcher {
        terminals: state.terminals.clone(),
        logins: state.logins.clone(),
        store: state.store.clone(),
        bus: state.bus.clone(),
    };
    tauri::async_runtime::spawn(watcher.run(op_id, step, over));
    tracing::info!(code = "accounts.login_start", action = ?action, "sign-in terminal started");
    Ok(session)
}

/// Follows one sign-in or sign-out to its end.
struct Watcher {
    terminals: Arc<TerminalService>,
    logins: Arc<LoginOps>,
    store: Store,
    bus: EventBus,
}

impl Watcher {
    async fn run(self, op_id: String, step: Step, over: Arc<AtomicBool>) {
        let Some((pty, engine, program, home, env, action)) = self.snapshot(&op_id) else {
            return;
        };
        if let Step::Slash(cmd) = &step {
            self.type_when_ready(&pty, &engine, cmd).await;
        }
        let started = tokio::time::Instant::now();
        loop {
            tokio::time::sleep(POLL).await;
            if over.load(Ordering::SeqCst) {
                return;
            }
            let live = self.terminals.pty.is_live(&pty);
            let check = engine_accounts::signed_in(&engine, &program, &home, &env).await;
            let done = match (action, check) {
                (LoginAction::SignIn, Some(true)) => Some(LoginState::SignedIn),
                (LoginAction::SignOut, Some(false)) => Some(LoginState::SignedOut),
                // The engine can't tell: closing its terminal ends a sign-out.
                (LoginAction::SignOut, None) if !live => Some(LoginState::SignedOut),
                _ if !live || started.elapsed() > LOGIN_WAIT => Some(LoginState::Failed),
                _ => None,
            };
            if let Some(state) = done {
                self.finish(&op_id, state).await;
                return;
            }
        }
    }

    #[allow(clippy::type_complexity)]
    fn snapshot(
        &self,
        op_id: &str,
    ) -> Option<(
        String,
        EngineSpec,
        PathBuf,
        PathBuf,
        Vec<(String, String)>,
        LoginAction,
    )> {
        let ops = self.logins.ops.lock().ok()?;
        let op = ops.get(op_id)?;
        Some((
            op.pty.clone(),
            op.engine.clone(),
            op.program.clone(),
            PathBuf::from(&op.account.home_path),
            op.env.clone(),
            op.action,
        ))
    }

    /// Types `cmd` once the TUI shows one of its ready patterns (or after
    /// `READY_WAIT`), as keystrokes and then Enter.
    async fn type_when_ready(&self, pty: &str, engine: &EngineSpec, cmd: &str) {
        let patterns: Vec<regex::Regex> = engine
            .ready_patterns
            .iter()
            .filter_map(|p| regex::RegexBuilder::new(p).multi_line(true).build().ok())
            .collect();
        let started = tokio::time::Instant::now();
        while started.elapsed() < READY_WAIT && self.terminals.pty.is_live(pty) {
            let screen = self.terminals.pty.screen_text(pty).unwrap_or_default();
            if patterns.iter().any(|re| re.is_match(&screen)) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        let _ = self.terminals.pty.write(pty, cmd.as_bytes());
        tokio::time::sleep(Duration::from_millis(120)).await;
        let _ = self.terminals.pty.write(pty, b"\r");
    }

    async fn finish(&self, op_id: &str, state: LoginState) {
        let Some(op) = self.logins.take(op_id) else {
            return;
        };
        op.over.store(true, Ordering::SeqCst);
        let message = (state == LoginState::Failed).then(|| last_line(&self.terminals, &op.pty));
        let _ = self.terminals.pty.kill(&op.pty);
        conclude(
            &self.store,
            &self.terminals,
            &self.bus,
            op_id,
            op,
            state,
            message,
        )
        .await;
    }
}

fn last_line(terminals: &TerminalService, pty: &str) -> String {
    let text = terminals.pty.plain_tail(pty, 20).unwrap_or_default();
    let line = text
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .chars()
        .take(200)
        .collect::<String>();
    tandem_core::redact::redact(&line)
}

/// Saves a new account that signed in, or drops its home when it didn't,
/// then tells the webview.
async fn conclude(
    store: &Store,
    terminals: &TerminalService,
    bus: &EventBus,
    op_id: &str,
    op: Op,
    state: LoginState,
    message: Option<String>,
) {
    let mut state = state;
    if op.new_account {
        let home = PathBuf::from(&op.account.home_path);
        if state == LoginState::SignedIn {
            if let Err(e) = accounts::insert(store, &op.account).await {
                tracing::warn!(code = "accounts.save_failed", error = %e, "couldn't save the account");
                let _ = terminals.engines.homes.remove(&home);
                state = LoginState::Failed;
            }
        } else {
            let _ = terminals.engines.homes.remove(&home);
        }
    }
    tracing::info!(code = "accounts.login_end", state = ?state, "sign-in terminal ended");
    bus.publish(BusEvent::new(
        Topic::Engines,
        "accounts:login-finished",
        LoginEvent {
            op_id: op_id.to_string(),
            account_id: op.account.id.clone(),
            state,
            message,
        },
    ));
}

/// `accounts_finish_login`: the user says the engine is done (engines that
/// can't report their sign-in).
pub async fn finish_manually(state: &AppState, op_id: &str) -> Result<(), IpcError> {
    let op = state
        .logins
        .take(op_id)
        .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "That sign-in already ended"))?;
    op.over.store(true, Ordering::SeqCst);
    let _ = state.terminals.pty.kill(&op.pty);
    let result = match op.action {
        LoginAction::SignIn => LoginState::SignedIn,
        LoginAction::SignOut => LoginState::SignedOut,
    };
    conclude(
        &state.store,
        &state.terminals,
        &state.bus,
        op_id,
        op,
        result,
        None,
    )
    .await;
    Ok(())
}

/// `accounts_cancel_login`: kills the login process; a new account's home
/// is deleted and no account is created (F-AC-02 AC1).
pub async fn cancel(state: &AppState, op_id: &str) -> Result<(), IpcError> {
    let Some(op) = state.logins.take(op_id) else {
        return Ok(());
    };
    op.over.store(true, Ordering::SeqCst);
    let _ = state.terminals.pty.kill(&op.pty);
    conclude(
        &state.store,
        &state.terminals,
        &state.bus,
        op_id,
        op,
        LoginState::Cancelled,
        None,
    )
    .await;
    Ok(())
}

/// `accounts_status`: whether the account is signed in.
pub async fn status(state: &AppState, account_id: &AccountId) -> Result<SignInState, IpcError> {
    let account = accounts::get(&state.store, account_id).await?;
    let engine = engine(state, &account.engine_id)?;
    let Some(program) = state.terminals.resolve_engine(&engine).await else {
        return Ok(SignInState::Unknown);
    };
    let home = PathBuf::from(&account.home_path);
    let env = login_env(&state.terminals, &engine, &program, &home, "status").await?;
    Ok(
        match engine_accounts::signed_in(&engine, &program, &home, &env).await {
            Some(true) => SignInState::SignedIn,
            Some(false) => SignInState::SignedOut,
            None => SignInState::Unknown,
        },
    )
}

/// `accounts_remove`: the account goes and its provider home is deleted.
/// Sessions that used it keep their records but can't continue on it.
pub async fn remove(state: &AppState, account_id: &AccountId) -> Result<(), IpcError> {
    let account = accounts::remove(&state.store, account_id).await?;
    if let Err(e) = state
        .terminals
        .engines
        .homes
        .remove(std::path::Path::new(&account.home_path))
    {
        tracing::warn!(code = "accounts.home_remove_failed", error = %e, "couldn't delete a provider home");
    }
    Ok(())
}
