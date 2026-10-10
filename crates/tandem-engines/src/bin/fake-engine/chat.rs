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
//! - `mcp <tool> <json>`: calls a gateway tool (a `tandem__*` built-in, a
//!   plugin tool, `tandem__call`) as a tool row and says what it answered.
//! - `/tool-output <word>`: a Bash tool whose output holds the word
//!   reversed, and the reply "Read the notes." (session search, E2E-M10-4).
//! - `tools`: says the names the gateway's `tools/list` gives this session.
//! - `tasks <tool> <json>`: calls a tool of the `tandem_tasks` stdio server
//!   named in `--mcp-config` (F-TK-02, E2E-M11-1) and says what it answered.
//! - a task run's briefing ("Report progress with save_comment on TAN-<n>"):
//!   a progress comment through the tasks server, a pause of N seconds when
//!   the task says `[hold N]`, then a one-line summary (F-TK-04, E2E-M11-2).
//! - a task review ("Post one comment on TAN-<n>… Verdict"): "Verdict:
//!   Approve" and a reason.
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

/// The `tandem_tasks` stdio server from `--mcp-config`: command and args.
fn tasks_server(args: &[String]) -> Option<(String, Vec<String>)> {
    let at = args.iter().position(|a| a == "--mcp-config")?;
    let file = std::fs::read_to_string(args.get(at + 1)?).ok()?;
    let v: Value = serde_json::from_str(&file).ok()?;
    let server = v.pointer("/mcpServers/tandem_tasks")?;
    let argv = server
        .get("args")?
        .as_array()?
        .iter()
        .filter_map(|a| a.as_str().map(str::to_string))
        .collect();
    Some((server.get("command")?.as_str()?.to_string(), argv))
}

/// One tool call on a fresh stdio server: (is_error, text).
fn call_stdio(server: &(String, Vec<String>), name: &str, args: Value) -> (bool, String) {
    use std::process::{Command, Stdio};
    let Ok(mut child) = Command::new(&server.0)
        .args(&server.1)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return (true, "the tasks server didn't start".into());
    };
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return (true, "no pipes".into());
    };
    let msgs = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "fake-claude", "version": "1"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": name, "arguments": args}}),
    ];
    for m in msgs {
        let _ = writeln!(stdin, "{m}");
    }
    let _ = stdin.flush();
    let mut out = (true, "the tasks server didn't answer".to_string());
    for line in std::io::BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let Ok(m) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if m.get("id") != Some(&json!(2)) {
            continue;
        }
        out = match m.get("error") {
            Some(e) => (true, e.to_string()),
            None => (
                m.pointer("/result/isError")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                m.pointer("/result/content/0/text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            ),
        };
        break;
    }
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    out
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

/// Opens an MCP session on the gateway; the headers for its requests.
fn open_session(gw: &(String, String)) -> Option<Vec<(&'static str, String)>> {
    let (url, auth) = gw;
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-03-26", "capabilities": {},
        "clientInfo": {"name": "fake-claude", "version": "1"}}});
    let Some((200, headers, _)) = post(url, &[("Authorization", auth)], &init.to_string()) else {
        return None;
    };
    let session = headers
        .iter()
        .find(|(k, _)| k == "mcp-session-id")
        .map(|(_, v)| v.clone())?;
    let with_session = vec![
        ("Authorization", auth.clone()),
        ("mcp-session-id", session),
        ("mcp-protocol-version", "2025-03-26".to_string()),
    ];
    let refs: Vec<(&str, &str)> = with_session.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let _ = post(
        url,
        &refs,
        &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string(),
    );
    Some(with_session)
}

/// One JSON-RPC request on a fresh MCP session: the message with its id.
fn rpc(gw: &(String, String), method: &str, params: Value) -> Option<Value> {
    let headers = open_session(gw)?;
    let refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let req = json!({"jsonrpc": "2.0", "id": 2, "method": method, "params": params});
    let (_, _, body) = post(&gw.0, &refs, &req.to_string())?;
    message(&body)
}

/// Calls a gateway tool: (is_error, text).
fn call_tool(gw: &(String, String), name: &str, args: Value) -> (bool, String) {
    match rpc(gw, "tools/call", json!({"name": name, "arguments": args})) {
        Some(m) => {
            let result = m.get("result").cloned().unwrap_or_default();
            let text = result
                .pointer("/content/0/text")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    m.pointer("/error/message")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default();
            (
                result
                    .get("isError")
                    .and_then(Value::as_bool)
                    .unwrap_or(m.get("error").is_some()),
                text,
            )
        }
        None => (true, "the gateway didn't answer".into()),
    }
}

/// The names `tools/list` gives this session.
fn list_tools(gw: &(String, String)) -> Vec<String> {
    rpc(gw, "tools/list", json!({}))
        .and_then(|m| m.pointer("/result/tools").cloned())
        .and_then(|t| t.as_array().cloned())
        .map(|t| {
            t.iter()
                .filter_map(|t| t.get("name").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Asks the gateway's approvals tool; true when it allows.
fn approve(gw: &(String, String), command: &str, tool_use_id: &str) -> bool {
    let (_, text) = call_tool(
        gw,
        "tandem_approvals__request",
        json!({"tool_name": "Bash", "input": {"command": command}, "tool_use_id": tool_use_id}),
    );
    serde_json::from_str::<Value>(&text)
        .ok()
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
        } else if let Some(rest) = first.strip_prefix("mcp ") {
            // `mcp <tool> <json args>`: one gateway tool call, shown as a
            // tool row (teammate built-ins and plugins, E2E-M10-*).
            let (tool, args) = rest.split_once(' ').unwrap_or((rest, "{}"));
            let args: Value = serde_json::from_str(args).unwrap_or_else(|_| json!({}));
            let id = format!("toolu_{n}");
            turn.assistant(json!([{"type": "tool_use", "id": id,
                "name": format!("mcp__tandem__{tool}"), "input": args}]));
            let (is_error, text) = match gw.as_ref() {
                Some(g) => call_tool(g, tool, args),
                None => (true, "no gateway".into()),
            };
            emit(
                json!({"type": "user", "session_id": turn.session, "parent_tool_use_id": null,
                "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": id,
                    "content": text, "is_error": is_error}]}}),
            );
            let said = if is_error {
                format!("The tool failed: {text}")
            } else {
                format!("The tool said: {text}")
            };
            turn.say(&[&said]);
            turn.done(&said);
        } else if let Some(word) = first.strip_prefix("/tool-output ") {
            // A word that ends up only in a tool's output (reversed), never
            // in the user's or the assistant's prose (E2E-M10-4).
            let id = format!("toolu_{n}");
            turn.assistant(json!([{"type": "tool_use", "id": id, "name": "Bash",
                "input": {"command": "cat notes.txt"}}]));
            let hidden: String = word.trim().chars().rev().collect();
            emit(
                json!({"type": "user", "session_id": turn.session, "parent_tool_use_id": null,
                "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": id,
                    "content": format!("notes: {hidden}"), "is_error": false}]}}),
            );
            turn.say(&["Read the notes."]);
            turn.done("Read the notes.");
        } else if let Some(rest) = first.strip_prefix("tasks ") {
            // `tasks <tool> <json args>`: the tasks server, as a tool row.
            let (tool, targs) = rest.split_once(' ').unwrap_or((rest, "{}"));
            let targs: Value = serde_json::from_str(targs).unwrap_or_else(|_| json!({}));
            let id = format!("toolu_{n}");
            turn.assistant(json!([{"type": "tool_use", "id": id,
                "name": format!("mcp__tandem_tasks__{tool}"), "input": targs}]));
            let (is_error, text) = match tasks_server(args) {
                Some(s) => call_stdio(&s, tool, targs),
                None => (true, "no tasks server".into()),
            };
            emit(
                json!({"type": "user", "session_id": turn.session, "parent_tool_use_id": null,
                "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": id,
                    "content": text, "is_error": is_error}]}}),
            );
            let said = if is_error {
                "The tasks tool failed."
            } else {
                "Saved the task."
            };
            turn.say(&[said]);
            turn.done(said);
        } else if first == "tools" {
            // The gateway tools this session sees, sorted.
            let mut names = gw.as_ref().map(list_tools).unwrap_or_default();
            names.sort();
            let said = format!("Tools: {}", names.join(", "));
            turn.say(&[&said]);
            turn.done(&said);
        } else if let Some(issue) = text
            .split("Report progress with save_comment on ")
            .nth(1)
            .and_then(|r| r.split(['.', ' ']).next())
            .filter(|i| i.starts_with("TAN-"))
            .map(str::to_string)
        {
            // A task run: progress through the tasks server, then the summary.
            let comment = json!({"id": issue, "body": "Progress: reproduced the problem and working on a fix."});
            let id = format!("toolu_{n}");
            turn.assistant(json!([{"type": "tool_use", "id": id,
                "name": "mcp__tandem_tasks__save_comment", "input": comment}]));
            let (is_error, out) = match tasks_server(args) {
                Some(s) => call_stdio(&s, "save_comment", comment),
                None => (true, "no tasks server".into()),
            };
            emit(
                json!({"type": "user", "session_id": turn.session, "parent_tool_use_id": null,
                "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": id,
                    "content": out, "is_error": is_error}]}}),
            );
            let hold: u64 = text
                .split("[hold ")
                .nth(1)
                .and_then(|r| r.split(']').next())
                .and_then(|n| n.trim().parse().ok())
                .unwrap_or(0);
            std::thread::sleep(Duration::from_secs(hold.min(120)));
            let said = format!("Fixed {issue}: the change is in the worktree.");
            turn.say(&[&said]);
            turn.done(&said);
        } else if text.contains("Post one comment on TAN-") && text.contains("Verdict: Approve") {
            let said = "Verdict: Approve\nThe change does what the task asks.";
            turn.say(&[said]);
            turn.done(said);
        } else if first == "/crash" {
            turn.delta("About to fail");
            std::process::exit(3);
        } else {
            turn.say(&["You said: ", &text]);
            turn.done(&format!("You said: {text}"));
        }
    }
}
