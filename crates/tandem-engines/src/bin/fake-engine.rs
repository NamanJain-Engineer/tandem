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
//! Sign-in (F-AC-02): `/login` asks for a code like a device flow and, once a
//! line is typed, writes `$CLAUDE_CONFIG_DIR/.credentials.json`; `/logout`
//! deletes it. As `codex`, `login` does the same with `$CODEX_HOME/auth.json`,
//! `login status` exits 0 when it exists and `logout` deletes it.
//!
//! Run headless (`claude -p`, `codex exec`), it reads its prompt from stdin,
//! logs its size and prints a brief. `TANDEM_FAKE_BRIEF` (a value, or a
//! file holding it) set to `empty` makes both briefs empty.

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
    for line in brief_lines(&format!("Fake {}", label(name))) {
        println!("{line}");
    }
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
        println!("{name} 1.2.3 (test engine)");
        return;
    }
    let args = &argv[1.min(argv.len())..];
    if let Some(code) = account_command(&name, args) {
        log(&format!("start: {name} args={args:?}"));
        std::process::exit(code);
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
