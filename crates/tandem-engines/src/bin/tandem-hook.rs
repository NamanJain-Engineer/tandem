//! `tandem-hook`: run by engine hooks inside Tandem panes (ARCHITECTURE §6.6).
//!
//! `tandem-hook claude <Event>` (hook JSON on stdin) or
//! `tandem-hook codex '<json>'`. Prints nothing, always exits 0, and does
//! nothing at all outside a Tandem pane.
//!
//! Called as `vibezeo` (a link on an agent pane's PATH), it's
//! `vibezeo run [--] <command>` instead (F-TL-12).

use std::time::{SystemTime, UNIX_EPOCH};

use tandem_engines::hook::{self, PANE_VAR, POST_TIMEOUT};
use tandem_engines::launch::{STATUS_SECRET_VAR, STATUS_URL_VAR};

/// How long `vibezeo run` waits: an approval (30 minutes) and the run (10).
const RUN_WAIT: std::time::Duration = std::time::Duration::from_secs(45 * 60);

/// What `vibezeo run` prints when it can't work.
const OUTSIDE: &str = "vibezeo run works in agent panes in Vibezeo";

/// Called as `vibezeo` (the link Vibezeo puts on an agent pane's PATH).
fn invoked_as_vibezeo() -> bool {
    std::env::args_os()
        .next()
        .and_then(|a| {
            std::path::Path::new(&a)
                .file_stem()
                .map(|s| s.eq_ignore_ascii_case("vibezeo"))
        })
        .unwrap_or(false)
}

/// `vibezeo run [--] <command…>`: the app runs it with the folder's
/// secrets and prints its masked output; exits with its code (F-TL-12).
/// The arguments go as they are: one is a command line for the shell,
/// several a program and its arguments, which the app quotes.
fn vibezeo() -> i32 {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("run") {
        eprintln!("usage: vibezeo run [--] <command>");
        return 2;
    }
    args.remove(0);
    if args.first().map(String::as_str) == Some("--") {
        args.remove(0);
    }
    if args.iter().all(|a| a.trim().is_empty()) {
        eprintln!("usage: vibezeo run [--] <command>");
        return 2;
    }
    let (Ok(pane), Ok(secret), Ok(url)) = (
        std::env::var(PANE_VAR),
        std::env::var(STATUS_SECRET_VAR),
        std::env::var(STATUS_URL_VAR),
    ) else {
        eprintln!("vibezeo: {OUTSIDE}");
        return 1;
    };
    let mut nonce = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut nonce);
    let req = tandem_core::run_request::RunRequest {
        pane_id: pane,
        cwd: std::env::current_dir()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default(),
        argv: args,
        ts: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or_default(),
        nonce: hex::encode(nonce),
    };
    let Some(bytes) = hook::run_request(&req, &secret) else {
        eprintln!("vibezeo: couldn't build the request");
        return 1;
    };
    let max = (tandem_core::run_request::OUTPUT_CAP * 2 + 64 * 1024) as u64;
    let (code, body) = match hook::exchange(&url, &bytes, RUN_WAIT, max) {
        Ok(r) => r,
        Err(_) => {
            eprintln!("vibezeo: Vibezeo isn't running, or this pane can't reach it");
            return 1;
        }
    };
    if code == 401 {
        eprintln!("vibezeo: this pane's link to Vibezeo has expired. Restart the agent");
        return 1;
    }
    let Ok(reply) = serde_json::from_slice::<tandem_core::run_request::RunReply>(&body) else {
        eprintln!("vibezeo: Vibezeo refused the request (HTTP {code})");
        return 1;
    };
    use std::io::Write;
    if reply.cut {
        println!("[… earlier output cut]");
    }
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(reply.output.as_bytes());
    let _ = out.flush();
    if let Some(why) = reply.error {
        eprintln!("vibezeo: {why}");
        return 1;
    }
    reply.exit_code.unwrap_or(1)
}

fn main() {
    if invoked_as_vibezeo() {
        std::process::exit(vibezeo());
    }
    let (Ok(pane), Ok(secret), Ok(url)) = (
        std::env::var(PANE_VAR),
        std::env::var(STATUS_SECRET_VAR),
        std::env::var(STATUS_URL_VAR),
    ) else {
        return;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default();
    let mut args = std::env::args().skip(1);
    let report = match (args.next().as_deref(), args.next()) {
        (Some("claude"), Some(event)) => {
            let stdin = hook::read_limited(std::io::stdin().lock());
            hook::claude_report(&event, &stdin, &pane, now)
        }
        (Some("codex"), Some(json)) => hook::codex_report(&json, &pane, now),
        _ => None,
    };
    let Some(report) = report else { return };
    // Debug builds can record what's posted, so tests can check it.
    if cfg!(debug_assertions) {
        if let (Some(path), Ok(body)) = (
            std::env::var_os("TANDEM_HOOK_LOG"),
            serde_json::to_string(&report),
        ) {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(f, "{body}");
            }
        }
    }
    if let Some(req) = hook::request(&report, &secret) {
        let _ = hook::post(&url, &req, POST_TIMEOUT);
    }
}
