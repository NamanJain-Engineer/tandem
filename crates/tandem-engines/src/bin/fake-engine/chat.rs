//! The fake engine as Claude Code in Thread mode (`-p --input-format
//! stream-json`): it reads user messages as stream-json lines and answers as
//! Claude does, so the app's E2E can drive a whole thread (E2E-M7-*).
//!
//! What a message says decides the turn:
//! - `run <command>`: asks the app's gateway (from `--mcp-config`) through
//!   `tandem_approvals__request` whether Bash may run it, then reports the
//!   tool as run or denied.
//! - `/slow <n>`: streams `n` pieces 150 ms apart (Stop can land mid-turn).
//! - `/hang`: streams one piece, then reads nothing more and ignores SIGINT
//!   (the tree must be killed, E2E-M7-3).
//! - `/crash`: streams one piece and exits 3 mid-turn.
//! - `write <path>=<text>;<path>=<text>…`: writes each file (`\n` is a line
//!   break) in its folder as Write tool calls, then says how many (E2E-M7-9).
//! - anything else: "You said: <text>" in two pieces.
//!
//! An interrupt (`control_request`) is acknowledged and ends the turn.

use std::io::{BufRead, Read, Write};
use std::net::TcpStream;
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{json, Value};

fn emit(v: Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

struct Turn {
    session: String,
    model: String,
    started: bool,
}

impl Turn {
    fn init(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        emit(
            json!({"type": "system", "subtype": "init", "session_id": self.session,
            "model": self.model, "tools": ["Bash", "Read", "Edit"],
            "mcp_servers": [{"name": "tandem", "status": "connected"}]}),
        );
    }

    fn delta(&self, text: &str) {
        emit(
            json!({"type": "stream_event", "session_id": self.session, "parent_tool_use_id": null,
            "event": {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text}}}),
        );
    }

    fn assistant(&self, content: Value) {
        emit(
            json!({"type": "assistant", "session_id": self.session, "parent_tool_use_id": null,
            "message": {"role": "assistant", "model": self.model, "content": content,
                "usage": {"input_tokens": 12, "output_tokens": 8}}}),
        );
    }

    fn say(&self, pieces: &[&str]) {
        emit(
            json!({"type": "stream_event", "session_id": self.session, "parent_tool_use_id": null,
            "event": {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}}),
        );
        for p in pieces {
            self.delta(p);
        }
        emit(
            json!({"type": "stream_event", "session_id": self.session, "parent_tool_use_id": null,
            "event": {"type": "content_block_stop", "index": 0}}),
        );
        self.assistant(json!([{"type": "text", "text": pieces.concat()}]));
    }

    fn done(&self, text: &str) {
        emit(
            json!({"type": "result", "subtype": "success", "is_error": false, "result": text,
            "session_id": self.session, "total_cost_usd": 0.001,
            "usage": {"input_tokens": 12, "output_tokens": 8, "cache_read_input_tokens": 0}}),
        );
    }

    fn interrupted(&self, request_id: &str) {
        emit(
            json!({"type": "control_response", "response": {"subtype": "success", "request_id": request_id}}),
        );
        emit(
            json!({"type": "result", "subtype": "error_during_execution", "is_error": true,
            "session_id": self.session, "usage": {"input_tokens": 1, "output_tokens": 1}}),
        );
    }
}

/// The text of a stream-json user message.
fn text_of(v: &Value) -> String {
    match v.pointer("/message/content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// The gateway from `--mcp-config <file>`: its URL and Authorization header.
fn gateway(args: &[String]) -> Option<(String, String)> {
    let at = args.iter().position(|a| a == "--mcp-config")?;
    let file = std::fs::read_to_string(args.get(at + 1)?).ok()?;
    let v: Value = serde_json::from_str(&file).ok()?;
    let server = v.pointer("/mcpServers/tandem")?;
    Some((
        server.get("url")?.as_str()?.to_string(),
        server
            .pointer("/headers/Authorization")?
            .as_str()?
            .to_string(),
    ))
}

/// An HTTP response: status, lowercase headers, body.
type Response = (u16, Vec<(String, String)>, String);

/// One HTTP/1.1 POST to a loopback URL.
fn post(url: &str, headers: &[(&str, &str)], body: &str) -> Option<Response> {
    let rest = url.strip_prefix("http://")?;
    let (authority, path) = rest.split_once('/').map(|(a, p)| (a, format!("/{p}")))?;
    let mut stream = TcpStream::connect(authority).ok()?;
    let mut req = format!(
        "POST {path} HTTP/1.1\r\nHost: {authority}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    stream.write_all(req.as_bytes()).ok()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).ok()?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n")?;
    let mut lines = head.lines();
    let status = lines.next()?.split_whitespace().nth(1)?.parse().ok()?;
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let chunked = headers
        .iter()
        .any(|(k, v)| k == "transfer-encoding" && v.contains("chunked"));
    let body = if chunked {
        dechunk(body)
    } else {
        body.to_string()
    };
    Some((status, headers, body))
}

fn dechunk(mut s: &str) -> String {
    let mut out = String::new();
    while let Some((size, rest)) = s.split_once("\r\n") {
        let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if n == 0 || rest.len() < n {
            break;
        }
        out.push_str(&rest[..n]);
        s = rest[n..].trim_start_matches("\r\n");
    }
    out
}

/// The JSON-RPC message with an id in a JSON body or an SSE stream.
fn message(body: &str) -> Option<Value> {
    serde_json::from_str::<Value>(body).ok().or_else(|| {
        body.lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .filter_map(|d| serde_json::from_str::<Value>(d.trim()).ok())
            .find(|m| m.get("id").is_some())
    })
}

/// Asks the gateway's approvals tool; true when it allows.
fn approve(gw: &(String, String), command: &str, tool_use_id: &str) -> bool {
    let (url, auth) = gw;
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-03-26", "capabilities": {},
        "clientInfo": {"name": "fake-claude", "version": "1"}}});
    let Some((200, headers, _)) = post(url, &[("Authorization", auth)], &init.to_string()) else {
        return false;
    };
    let Some(session) = headers
        .iter()
        .find(|(k, _)| k == "mcp-session-id")
        .map(|(_, v)| v.clone())
    else {
        return false;
    };
    let with_session = [
        ("Authorization", auth.as_str()),
        ("mcp-session-id", session.as_str()),
        ("mcp-protocol-version", "2025-03-26"),
    ];
    let _ = post(
        url,
        &with_session,
        &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string(),
    );
    let call = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
        "name": "tandem_approvals__request",
        "arguments": {"tool_name": "Bash", "input": {"command": command}, "tool_use_id": tool_use_id}}});
    let Some((_, _, body)) = post(url, &with_session, &call.to_string()) else {
        return false;
    };
    let reply = message(&body)
        .and_then(|m| {
            m.pointer("/result/content/0/text")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .and_then(|t| serde_json::from_str::<Value>(&t).ok());
    reply
        .and_then(|r| {
            r.get("behavior")
                .and_then(Value::as_str)
                .map(|b| b == "allow")
        })
        .unwrap_or(false)
}

/// Lines from stdin on a thread, so an interrupt can arrive mid-turn.
fn reader() -> mpsc::Receiver<Value> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            super::log(&format!("stdin: {} bytes", line.len()));
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if tx.send(v).is_err() {
                    break;
                }
            }
        }
    });
    rx
}

/// An interrupt waiting in `rx`, if any (other lines are put back in order).
fn interrupt(rx: &mpsc::Receiver<Value>, held: &mut Vec<Value>) -> Option<String> {
    while let Ok(v) = rx.try_recv() {
        if v.get("type").and_then(Value::as_str) == Some("control_request") {
            return Some(
                v.get("request_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            );
        }
        held.push(v);
    }
    None
}

pub fn run(args: &[String], session: String) {
    let model = args
        .iter()
        .position(|a| a == "--model")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "fake-model".into());
    let gw = gateway(args);
    let mut turn = Turn {
        session,
        model,
        started: false,
    };
    let rx = reader();
    let mut held: Vec<Value> = Vec::new();
    let mut n = 0;
    loop {
        let v = if held.is_empty() {
            match rx.recv() {
                Ok(v) => v,
                Err(_) => return,
            }
        } else {
            held.remove(0)
        };
        match v.get("type").and_then(Value::as_str) {
            Some("control_request") => {
                // Nothing running: acknowledge.
                let id = v.get("request_id").and_then(Value::as_str).unwrap_or("");
                emit(
                    json!({"type": "control_response", "response": {"subtype": "success", "request_id": id}}),
                );
                continue;
            }
            Some("user") => {}
            _ => continue,
        }
        n += 1;
        turn.init();
        let text = text_of(&v);
        let first = text.lines().next().unwrap_or("").trim().to_string();
        if let Some(command) = first.strip_prefix("run ") {
            let id = format!("toolu_{n}");
            turn.assistant(json!([{"type": "tool_use", "id": id, "name": "Bash", "input": {"command": command}}]));
            let allowed = gw.as_ref().is_some_and(|g| approve(g, command, &id));
            if allowed {
                emit(
                    json!({"type": "user", "session_id": turn.session, "parent_tool_use_id": null,
                    "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": id,
                        "content": format!("ok: {command}"), "is_error": false}]}}),
                );
                turn.say(&["Ran ", command, "."]);
            } else {
                emit(
                    json!({"type": "user", "session_id": turn.session, "parent_tool_use_id": null,
                    "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": id,
                        "content": "Permission to use Bash has been denied.", "is_error": true}]}}),
                );
                turn.say(&["Okay, I won't run it."]);
            }
            turn.done("done");
        } else if let Some(count) = first.strip_prefix("/slow") {
            let count: usize = count.trim().parse().unwrap_or(20);
            let mut stopped = None;
            for i in 0..count {
                turn.delta(&format!("piece {i} "));
                std::thread::sleep(Duration::from_millis(150));
                if let Some(id) = interrupt(&rx, &mut held) {
                    stopped = Some(id);
                    break;
                }
            }
            match stopped {
                Some(id) => turn.interrupted(&id),
                None => {
                    turn.assistant(json!([{"type": "text", "text": "slow reply done"}]));
                    turn.done("done");
                }
            }
        } else if first == "/hang" {
            turn.delta("Working on it");
            #[cfg(unix)]
            unsafe {
                // SAFETY: ignoring SIGINT for this test process only.
                libc::signal(libc::SIGINT, libc::SIG_IGN);
            }
            loop {
                std::thread::sleep(Duration::from_secs(60));
            }
        } else if let Some(spec) = first.strip_prefix("write ") {
            let mut count = 0;
            for (k, part) in spec.split(';').enumerate() {
                let Some((path, body)) = part.split_once('=') else {
                    continue;
                };
                let path = path.trim();
                let body = body.replace("\\n", "\n");
                let id = format!("toolu_{n}_{k}");
                turn.assistant(json!([{"type": "tool_use", "id": id, "name": "Write",
                    "input": {"file_path": path, "content": body}}]));
                if let Some(dir) = std::path::Path::new(path).parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let ok = std::fs::write(path, &body).is_ok();
                emit(
                    json!({"type": "user", "session_id": turn.session, "parent_tool_use_id": null,
                    "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": id,
                        "content": if ok { "written" } else { "failed" }, "is_error": !ok}]}}),
                );
                count += 1;
            }
            let said = format!("Wrote {count} files.");
            turn.say(&[&said]);
            turn.done("done");
        } else if first == "/crash" {
            turn.delta("About to fail");
            std::process::exit(3);
        } else {
            turn.say(&["You said: ", &text]);
            turn.done(&format!("You said: {text}"));
        }
    }
}
