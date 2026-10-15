//! `fake-engine` (ARCHITECTURE §6.12, test support): stands in for any engine
//! CLI, chosen by the name it runs as (`claude`, `codex`, …) or
//! `TANDEM_FAKE_ENGINE`. Built only with `--features fixtures`.
//!
//! - `--version` prints `<name> 1.2.3 (test engine)`.
//! - Otherwise it is a small TUI: bracketed paste on, a banner, the cwd; each
//!   read from the terminal is logged as hex (so separate writes stay
//!   separate) and every line is answered.
//! - As `claude`, it runs the hooks Tandem wrote into
//!   `$CLAUDE_CONFIG_DIR/settings.json`, feeding them hook JSON that also
//!   carries transcript text (which must never leave `tandem-hook`).
//!
//! `--resume <id>` (or `resume <id>`) prints "Resumed session <id>" and
//! reports that id from its hooks; a new run reports `fake-<pid>`.
//!
//! Lines: `/exit` quits; `/ask` asks for approval (Notification);
//! `/script` runs UserPromptSubmit → Notification → Stop two seconds apart;
//! `/title <t>` sets the OSC title; a handoff brief request (F-CD-16) is
//! answered with a brief between its markers; anything else is answered,
//! then Stop.
//!
//! As `codex`, `app-server` answers JSON-RPC lines: `initialize`,
//! `account/rateLimits/read` from `TANDEM_FAKE_USAGE` ("40,85": a 5-hour and
//! a weekly window; empty or `none`: an error), F-AC-04, and the V2 thread
//! protocol (`thread/start|resume`, `turn/start` answered "You said: …").
//!
//! `TANDEM_FAKE_VERSION` (a value or a file of `<name> <version>` lines) sets
//! the version an engine reports (`--version`, Codex's `userAgent`).
//!
//! `/context <tokens>` (as `claude`) ends a turn whose transcript, under
//! `$CLAUDE_CONFIG_DIR/projects/`, reports that many context tokens (F-AC-06).
//!
//! `/subagents <n>` reports n subagent starts and keeps working for a
//! minute without reading its input (F-AC-09).
//!
//! `/limit <delay>` prints a usage-limit message that resets after the delay
//! ("5s", "2h"), as engines do (F-AC-05).
//!
//! `/tasks <tool> <json>` calls a tool of the `tandem_tasks` stdio server
//! named in `--mcp-config`, which it starts once and keeps, as engines keep
//! their MCP servers; it prints `tasks[<server pid>]: <answer>` (D-1606).
//!
//! Sign-in (F-AC-02): `/login` asks for a code like a device flow and, once a
//! line is typed, writes `$CLAUDE_CONFIG_DIR/.credentials.json`; `/logout`
//! deletes it. As `codex`, `login` does the same with `$CODEX_HOME/auth.json`,
//! `login status` exits 0 when it exists and `logout` deletes it.
//!
//! As `claude -p --input-format stream-json` (Thread mode), it is a chat
//! engine: see `fake-engine/chat.rs`. As `<engine> acp` it is an Agent Client
//! Protocol agent: see `fake-engine/acp.rs`.
//!
//! Run headless (`claude -p`, `codex exec`), it reads its prompt from stdin,
//! logs its size and prints a brief. `TANDEM_FAKE_BRIEF` (a value, or a
//! file holding it) set to `empty` makes both briefs empty.

#[path = "fake-engine/acp.rs"]
mod acp;
#[path = "fake-engine/chat.rs"]
mod chat;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

fn label(name: &str) -> &str {
    match name {
        "claude" => "Claude Code",
        "codex" => "Codex",
        "cursor-agent" | "agent" => "Cursor Agent",
        "gemini" => "Gemini CLI",
        "copilot" => "Copilot CLI",
        "grok" => "Grok",
        "opencode" => "OpenCode",
        "droid" => "Droid",
        "amp" => "Amp",
        "aider" => "Aider",
        "kimi" => "Kimi",
        other => other,
    }
}

fn log(line: &str) {
    if let Some(path) = std::env::var_os("TANDEM_FAKE_ENGINE_LOG") {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn out(s: &str) {
    let mut o = std::io::stdout();
    let _ = o.write_all(s.as_bytes());
    let _ = o.flush();
}

/// `TANDEM_FAKE_BRIEF`: the value, or the contents of the file it names.
fn brief_mode() -> String {
    let Ok(v) = std::env::var("TANDEM_FAKE_BRIEF") else {
        return String::new();
    };
    std::fs::read_to_string(&v).unwrap_or(v).trim().to_string()
}

/// A brief under the template's headings, one line each.
fn brief_lines(label: &str) -> Vec<String> {
    if brief_mode() == "empty" {
        return Vec::new();
    }
    [
        "## Goal".to_string(),
        format!("Finish the work {label} started."),
        "## What's done".into(),
        "The parser is fixed.".into(),
        "## Current state".into(),
        "Tests are running.".into(),
        "## Files touched".into(),
        "src/parse.rs".into(),
        "## Open problems".into(),
        "None known.".into(),
        "## Next steps".into(),
        "Run the tests.".into(),
    ]
    .to_vec()
}

/// Where this engine keeps its fake sign-in.
fn credentials(name: &str) -> Option<PathBuf> {
    let (var, file) = if name == "codex" {
        ("CODEX_HOME", "auth.json")
    } else {
        ("CLAUDE_CONFIG_DIR", ".credentials.json")
    };
    std::env::var_os(var).map(|home| PathBuf::from(home).join(file))
}

static AWAITING_CODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn sign_in(name: &str) {
    if let Some(path) = credentials(name) {
        let _ = std::fs::write(&path, "{}");
    }
    log(&format!("signed in: {name}"));
    out("Login successful\r\n");
}

fn usage_mode() -> String {
    let Ok(v) = std::env::var("TANDEM_FAKE_USAGE") else {
        return String::new();
    };
    std::fs::read_to_string(&v).unwrap_or(v).trim().to_string()
}

/// The version this engine reports: `TANDEM_FAKE_VERSION` (a value or a file
/// of `<name> <version>` lines), else 1.2.3.
fn version(name: &str) -> String {
    let Ok(v) = std::env::var("TANDEM_FAKE_VERSION") else {
        return "1.2.3".into();
    };
    let text = std::fs::read_to_string(&v).unwrap_or(v);
    text.lines()
        .filter_map(|l| l.trim().split_once(' '))
        .find(|(n, _)| *n == name)
        .map(|(_, ver)| ver.trim().to_string())
        .unwrap_or_else(|| "1.2.3".into())
}

fn send_line(v: serde_json::Value) {
    println!("{v}");
    let _ = std::io::stdout().flush();
}

/// A turn of the stand-in app-server: "You said: <text>" as an agent message,
/// token usage, then `turn/completed` (Thread mode, E2E-M7-5).
fn app_turn(thread: &str, turn: &str, text: &str) {
    let note = |method: &str, params: serde_json::Value| {
        send_line(serde_json::json!({"jsonrpc": "2.0", "method": method, "params": params}));
    };
    note(
        "turn/started",
        serde_json::json!({"threadId": thread, "turn": {"id": turn, "status": "inProgress", "items": []}}),
    );
    let item = format!("msg_{turn}");
    // A task review (F-TK-04): a verdict instead of the echo.
    let verdict = "Verdict: Approve\nThe change does what the task asks.";
    let (first, rest) =
        if text.contains("Post one comment on TAN-") && text.contains("Verdict: Approve") {
            ("", verdict)
        } else {
            ("You said: ", text)
        };
    note(
        "item/started",
        serde_json::json!({"threadId": thread, "turnId": turn, "item": {"type": "agentMessage", "id": item, "text": ""}}),
    );
    for piece in [first, rest] {
        note(
            "item/agentMessage/delta",
            serde_json::json!({"threadId": thread, "turnId": turn, "itemId": item, "delta": piece}),
        );
    }
    note(
        "item/completed",
        serde_json::json!({"threadId": thread, "turnId": turn, "item": {"type": "agentMessage", "id": item, "text": format!("{first}{rest}")}}),
    );
    note(
        "thread/tokenUsage/updated",
        serde_json::json!({"threadId": thread, "turnId": turn, "tokenUsage": {
        "total": {"totalTokens": 120, "inputTokens": 100, "cachedInputTokens": 0, "outputTokens": 20, "reasoningOutputTokens": 0},
        "last": {"totalTokens": 120, "inputTokens": 100, "cachedInputTokens": 0, "outputTokens": 20, "reasoningOutputTokens": 0},
        "modelContextWindow": 272000}}),
    );
    note(
        "turn/completed",
        serde_json::json!({"threadId": thread, "turn": {"id": turn, "status": "completed", "items": []}}),
    );
}

/// A stand-in `codex app-server`: one JSON-RPC message per line.
fn app_server() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let stdin = std::io::stdin();
    let mut line = String::new();
    let mut turns = 0;
    while stdin.read_line(&mut line).is_ok_and(|n| n > 0) {
        let msg: serde_json::Value = serde_json::from_str(line.trim()).unwrap_or_default();
        line.clear();
        let Some(id) = msg.get("id").cloned() else {
            continue;
        };
        let reply = match msg.get("method").and_then(serde_json::Value::as_str) {
            Some("account/rateLimits/read") => {
                let pcts: Vec<f64> = usage_mode()
                    .split(',')
                    .filter_map(|p| p.trim().parse().ok())
                    .collect();
                log(&format!("app-server: rateLimits {pcts:?}"));
                match pcts.as_slice() {
                    [primary, rest @ ..] => serde_json::json!({"id": id, "result": {"rateLimits": {
                        "primary": {"usedPercent": primary, "windowMinutes": 300, "resetsAt": now + 7_200},
                        "secondary": rest.first().map(|s| serde_json::json!({"usedPercent": s, "windowMinutes": 10_080, "resetsAt": now + 259_200})),
                    }}}),
                    [] => {
                        serde_json::json!({"id": id, "error": {"code": -32000, "message": "no usage data"}})
                    }
                }
            }
            Some("initialize") => {
                serde_json::json!({"id": id, "result": {"userAgent": format!("codex_cli_rs/{}", version("codex"))}})
            }
            Some("thread/start") => {
                let thread = format!("thr_fake_{}", std::process::id());
                log(&format!("app-server: thread/start {thread}"));
                serde_json::json!({"id": id, "result": {"thread": {"id": thread}}})
            }
            Some("thread/resume") => {
                let thread = msg
                    .pointer("/params/threadId")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                log(&format!("app-server: thread/resume {thread}"));
                serde_json::json!({"id": id, "result": {"thread": {"id": thread}}})
            }
            Some("turn/start") => {
                turns += 1;
                let thread = msg
                    .pointer("/params/threadId")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let text = msg
                    .pointer("/params/input/0/text")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let turn = format!("turn_{turns}");
                send_line(
                    serde_json::json!({"id": id, "result": {"turn": {"id": turn, "status": "inProgress", "items": []}}}),
                );
                app_turn(&thread, &turn, &text);
                continue;
            }
            _ => serde_json::json!({"id": id, "result": {}}),
        };
        println!("{reply}");
        let _ = std::io::stdout().flush();
    }
}

/// `codex login` / `login status` / `logout`; true when handled.
fn account_command(name: &str, args: &[String]) -> Option<i32> {
    if name != "codex" {
        return None;
    }
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["login", "status"] => Some(if credentials(name).is_some_and(|p| p.exists()) {
            0
        } else {
            1
        }),
        ["login"] => {
            out("Open https://example.invalid/device and enter ABCD-1234\r\nPaste the code to finish: ");
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            sign_in(name);
            Some(0)
        }
        ["app-server", ..] => {
            app_server();
            Some(0)
        }
        ["logout"] => {
            if let Some(p) = credentials(name) {
                let _ = std::fs::remove_file(p);
            }
            out("Logged out\r\n");
            Some(0)
        }
        _ => None,
    }
}

/// One-shot mode: the prompt on stdin, the answer on stdout.
fn headless(name: &str) {
    let mut input = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut input);
    log(&format!("headless: {name} read {} bytes", input.len()));
    // A teammate's learn follow-up (F-AG-08): JSON memory suggestions.
    let text = String::from_utf8_lossy(&input);
    if text.contains("<learn_request>") {
        println!("{}", learn_reply(&text));
        return;
    }
    for line in brief_lines(&format!("Fake {}", label(name))) {
        println!("{line}");
    }
}

/// `TANDEM_FAKE_LEARN` (a value or a file) is the reply; otherwise one note
/// from the last user message: `{"add": ["The user said: <text>"], …}`.
fn learn_reply(prompt: &str) -> String {
    if let Ok(v) = std::env::var("TANDEM_FAKE_LEARN") {
        return std::fs::read_to_string(&v).unwrap_or(v);
    }
    let said = prompt
        .split("<user>")
        .nth(1)
        .and_then(|r| r.split("</user>").next())
        .unwrap_or("")
        .trim()
        .replace('"', "'");
    format!(
        "{{\"add\": [\"The user said: {said}\"], \"remove\": [], \"rationale\": \"A preference worth keeping.\"}}"
    )
}

/// `--resume <id>` resumes that session; otherwise each run is a new one.
fn resumed() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.windows(2)
        .find(|w| w[0] == "--resume" || w[0] == "resume")
        .map(|w| w[1].clone())
}

fn session_id() -> String {
    resumed().unwrap_or_else(|| format!("fake-{}", std::process::id()))
}

/// Runs every Tandem hook for `event` with hook JSON on stdin.
fn hook(event: &str, extra: serde_json::Value) {
    let Some(home) = std::env::var_os("CLAUDE_CONFIG_DIR") else {
        return;
    };
    let Ok(text) = std::fs::read_to_string(PathBuf::from(home).join("settings.json")) else {
        return;
    };
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(&text) else {
        return;
    };
    let mut input = serde_json::json!({
        "hook_event_name": event,
        "session_id": session_id(),
        "transcript_path": "/tmp/fake-transcript.jsonl",
        "cwd": std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default(),
        "transcript": [{"role": "user", "content": "SECRET TRANSCRIPT TEXT"}],
        "prompt": "SECRET PROMPT TEXT",
    });
    if let (Some(obj), Some(more)) = (input.as_object_mut(), extra.as_object()) {
        for (k, v) in more {
            obj.insert(k.clone(), v.clone());
        }
    }
    let groups = doc["hooks"][event].as_array().cloned().unwrap_or_default();
    for group in groups {
        for h in group["hooks"].as_array().cloned().unwrap_or_default() {
            let Some(cmd) = h["command"].as_str() else {
                continue;
            };
            log(&format!("hook: {event}"));
            let child = Command::new("sh")
                .args(["-c", cmd])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            if let Ok(mut child) = child {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(input.to_string().as_bytes());
                }
                let _ = child.wait();
            }
        }
    }
}

#[cfg(unix)]
fn raw_mode() -> Option<libc::termios> {
    // SAFETY: termios calls on stdin with a zeroed struct filled by tcgetattr.
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(0, &mut t) != 0 {
            return None;
        }
        let saved = t;
        t.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG | libc::IEXTEN);
        t.c_iflag &= !(libc::ICRNL | libc::IXON);
        t.c_cc[libc::VMIN] = 1;
        t.c_cc[libc::VTIME] = 0;
        libc::tcsetattr(0, libc::TCSANOW, &t);
        Some(saved)
    }
}

#[cfg(unix)]
fn restore(saved: Option<libc::termios>) {
    if let Some(t) = saved {
        // SAFETY: restores the attributes read in `raw_mode`.
        unsafe {
            libc::tcsetattr(0, libc::TCSANOW, &t);
        }
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let name = std::env::var("TANDEM_FAKE_ENGINE").ok().unwrap_or_else(|| {
        argv.first()
            .map(|a| {
                std::path::Path::new(a)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    });
    if argv.iter().skip(1).any(|a| a == "--version" || a == "-v") {
        println!("{name} {} (test engine)", version(&name));
        return;
    }
    let args = &argv[1.min(argv.len())..];
    if let Some(code) = account_command(&name, args) {
        log(&format!("start: {name} args={args:?}"));
        std::process::exit(code);
    }
    if name == "claude"
        && args
            .windows(2)
            .any(|w| w[0] == "--input-format" && w[1] == "stream-json")
    {
        log(&format!("start: {name} args={args:?}"));
        chat::run(args, session_id());
        return;
    }
    if args.first().is_some_and(|a| a == "acp") {
        log(&format!("start: {name} args={args:?}"));
        acp::run(&name);
        return;
    }
    if (name == "claude" && args.iter().any(|a| a == "-p"))
        || (name == "codex" && args.first().is_some_and(|a| a == "exec"))
    {
        log(&format!("start: {name} args={args:?}"));
        headless(&name);
        return;
    }
    let label = label(&name).to_string();
    log(&format!("start: {name} args={args:?}"));
    #[cfg(unix)]
    let saved = raw_mode();
    out("\x1b[?2004h");
    out(&format!("Fake {label} (test engine)\r\n"));
    out(&format!(
        "cwd: {}\r\n",
        std::env::current_dir()
            .map(|d| d.display().to_string())
            .unwrap_or_default()
    ));
    if let Some(id) = resumed() {
        out(&format!("Resumed session {id}\r\n"));
    }
    if name == "claude" {
        hook("SessionStart", serde_json::json!({}));
    }
    out("> ");
    let mut stdin = std::io::stdin();
    let mut buf = [0u8; 4096];
    let mut line: Vec<u8> = Vec::new();
    loop {
        let n = match stdin.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let chunk = &buf[..n];
        log(&format!(
            "raw: {}",
            chunk.iter().map(|b| format!("{b:02x}")).collect::<String>()
        ));
        for &b in chunk {
            if b == b'\r' || b == b'\n' {
                let text = String::from_utf8_lossy(&line)
                    .replace("\x1b[200~", "")
                    .replace("\x1b[201~", "");
                line.clear();
                out("\r\n");
                log(&format!("line: {text}"));
                if !answer(&name, &text) {
                    out("\x1b[?2004l");
                    #[cfg(unix)]
                    restore(saved);
                    return;
                }
                out("> ");
            } else {
                line.push(b);
                if b >= 0x20 && b != 0x7f {
                    let _ = std::io::stdout().write_all(&[b]);
                    let _ = std::io::stdout().flush();
                }
            }
        }
    }
    out("\x1b[?2004l");
    #[cfg(unix)]
    restore(saved);
}

/// Answers one line; false to quit.
/// The `tandem_tasks` server, once started: kept for the run.
struct TasksServer {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    lines: std::io::Lines<std::io::BufReader<std::process::ChildStdout>>,
    next: u64,
}

static TASKS: std::sync::Mutex<Option<TasksServer>> = std::sync::Mutex::new(None);

impl TasksServer {
    fn start() -> Result<Self, String> {
        let argv: Vec<String> = std::env::args().skip(1).collect();
        let (command, args) = chat::stdio_server(&argv, "tandem_tasks").ok_or("no tasks server")?;
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("the tasks server didn't start: {e}"))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err("no pipes".into());
        };
        use std::io::BufRead;
        let mut server = Self {
            child,
            stdin,
            lines: std::io::BufReader::new(stdout).lines(),
            next: 1,
        };
        server.request(
            "initialize",
            serde_json::json!({"protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "fake-tui", "version": "1"}}),
        )?;
        server
            .send(&serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))?;
        Ok(server)
    }

    fn send(&mut self, m: &serde_json::Value) -> Result<(), String> {
        writeln!(self.stdin, "{m}")
            .and_then(|_| self.stdin.flush())
            .map_err(|_| "the tasks server went".to_string())
    }

    fn request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let id = self.next;
        self.next += 1;
        self.send(
            &serde_json::json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        )?;
        for line in self.lines.by_ref() {
            let Ok(line) = line else { break };
            let Ok(m) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if m.get("id") == Some(&serde_json::json!(id)) {
                return match m.get("error") {
                    Some(e) => Err(e.to_string()),
                    None => Ok(m.get("result").cloned().unwrap_or_default()),
                };
            }
        }
        Err("the tasks server went".into())
    }
}

/// `/tasks <tool> <json>`: one call on the kept server.
fn tasks_call(tool: &str, args: serde_json::Value) -> String {
    let mut slot = TASKS.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_none() {
        match TasksServer::start() {
            Ok(s) => *slot = Some(s),
            Err(e) => return format!("tasks error: {e}"),
        }
    }
    let Some(server) = slot.as_mut() else {
        return "tasks error: no tasks server".into();
    };
    let pid = server.child.id();
    match server.request(
        "tools/call",
        serde_json::json!({"name": tool, "arguments": args}),
    ) {
        Ok(r) => format!(
            "tasks[{pid}]: {}",
            r.pointer("/content/0/text")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
        ),
        Err(e) => {
            *slot = None;
            format!("tasks error: {e}")
        }
    }
}

fn answer(name: &str, text: &str) -> bool {
    let claude = name == "claude";
    let t = text.trim();
    if t.contains("/exit") {
        out("Goodbye from the test engine\r\n");
        return false;
    }
    if t.is_empty() {
        return true;
    }
    if AWAITING_CODE.swap(false, std::sync::atomic::Ordering::SeqCst) {
        sign_in(name);
        return true;
    }
    if t == "/login" {
        AWAITING_CODE.store(true, std::sync::atomic::Ordering::SeqCst);
        out("Open https://example.invalid/device and enter ABCD-1234\r\nPaste the code to finish:\r\n");
        return true;
    }
    if t == "/logout" {
        if let Some(p) = credentials(name) {
            let _ = std::fs::remove_file(p);
        }
        out("Logged out\r\n");
        return true;
    }
    if claude {
        hook("UserPromptSubmit", serde_json::json!({}));
    }
    if let Some(rest) = t.split("--- handoff brief ").nth(1) {
        let nonce = rest.split_whitespace().next().unwrap_or("");
        out(&format!("⏺ --- handoff brief {nonce} ---\r\n"));
        for line in brief_lines(&format!("Fake {}", label(name))) {
            out(&format!("  {line}\r\n"));
        }
        out(&format!("  --- end of brief {nonce} ---\r\n"));
        if claude {
            hook("Stop", serde_json::json!({ "stop_hook_active": false }));
        }
        return true;
    }
    if let Some(tokens) = t.strip_prefix("/context ") {
        let tokens: u64 = tokens.trim().parse().unwrap_or(0);
        let Some(home) = std::env::var_os("CLAUDE_CONFIG_DIR") else {
            return true;
        };
        let dir = PathBuf::from(home).join("projects").join("fake");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("{}.jsonl", session_id()));
        let line = serde_json::json!({
            "type": "assistant",
            "message": {
                "model": "claude-sonnet-4-5",
                "content": [{"type": "text", "text": "SECRET ANSWER TEXT"}],
                "usage": {"input_tokens": tokens.saturating_sub(2_000), "cache_read_input_tokens": 2_000, "cache_creation_input_tokens": 0, "output_tokens": 40}
            }
        });
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(f, "{line}");
        }
        out(&format!("context: {tokens} tokens\r\n"));
        if claude {
            hook(
                "Stop",
                serde_json::json!({ "stop_hook_active": false, "transcript_path": path.to_string_lossy() }),
            );
        }
        return true;
    }
    if let Some(delay) = t.strip_prefix("/limit") {
        let delay = delay.trim();
        let delay = if delay.is_empty() { "2h" } else { delay };
        out(&format!(
            "{} usage limit reached. Try again in {delay}.\r\n",
            label(name)
        ));
        if claude {
            hook("Stop", serde_json::json!({ "stop_hook_active": false }));
        }
        return true;
    }
    // `/write a.txt=text`: edits a file in the cwd the moment the line
    // arrives, as an agent would (F-EX-01: checkpoints come first).
    if let Some(spec) = t.strip_prefix("/write ") {
        if let Some((path, text)) = spec.split_once('=') {
            let _ = std::fs::write(path.trim(), text);
            out(&format!("wrote {}\r\n", path.trim()));
        }
        return true;
    }
    if let Some(rest) = t.strip_prefix("/tasks ") {
        let (tool, args) = rest.split_once(' ').unwrap_or((rest, "{}"));
        let args = serde_json::from_str(args).unwrap_or_else(|_| serde_json::json!({}));
        out(&format!("{}\r\n", tasks_call(tool.trim(), args)));
        return true;
    }
    // `/subagents <n>` (F-AC-09): reports n subagent starts (PreToolUse of
    // the `Task` tool), then keeps working for a minute without reading.
    if let Some(n) = t.strip_prefix("/subagents ") {
        let n: u32 = n.trim().parse().unwrap_or(1);
        out("spawning subagents…\r\n");
        if claude {
            for _ in 0..n {
                hook("PreToolUse", serde_json::json!({ "tool_name": "Task" }));
            }
        }
        std::thread::sleep(Duration::from_secs(60));
        return true;
    }
    if let Some(title) = t.strip_prefix("/title ") {
        out(&format!("\x1b]2;{title}\x07"));
        return true;
    }
    match t {
        "/ask" => {
            out("Edit main.rs\r\nDo you want to proceed?\r\n❯ 1. Yes\r\n");
            if claude {
                hook(
                    "Notification",
                    serde_json::json!({ "notification_type": "permission_prompt" }),
                );
            }
        }
        "/script" => {
            out("working…\r\n");
            std::thread::sleep(Duration::from_secs(2));
            out("Do you want to proceed?\r\n");
            if claude {
                hook(
                    "Notification",
                    serde_json::json!({ "notification_type": "permission_prompt" }),
                );
            }
            std::thread::sleep(Duration::from_secs(2));
            out("done\r\n");
            if claude {
                hook("Stop", serde_json::json!({ "stop_hook_active": false }));
            }
        }
        _ => {
            out(&format!("you said: {t}\r\n"));
            if claude {
                hook("Stop", serde_json::json!({ "stop_hook_active": false }));
            }
        }
    }
    true
}
