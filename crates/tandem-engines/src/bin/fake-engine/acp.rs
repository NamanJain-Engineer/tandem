//! `<engine> acp` (Thread mode, F-TH-06): a stand-in Agent Client Protocol
//! agent over stdio for any ACP engine (`cursor-agent`, `grok`, `opencode`).
//!
//! - `TANDEM_FAKE_MODELS` (a value or a file of `<engine> <id>,<id>…` lines)
//!   lists the models this engine offers; none for it: no models reported.
//!   The same file with `<engine> signed-out` makes `session/new` ask for a
//!   sign-in.
//! - A prompt is answered "You said: <text>", and a task review with a
//!   verdict, as the other fakes answer it. `run <cmd>` asks permission
//!   first and answers "Ran <cmd>." or "Okay, I won't run it."; `/model`
//!   answers with the session's model; `/hang` streams "Working on it" and
//!   waits for `session/cancel`.

use std::io::{BufRead, Write};

use serde_json::{json, Value};

use super::log;

fn send(v: Value) {
    let mut o = std::io::stdout();
    let _ = writeln!(o, "{v}");
    let _ = o.flush();
}

/// This engine's line of `TANDEM_FAKE_MODELS`.
fn setting(name: &str) -> Option<String> {
    let v = std::env::var("TANDEM_FAKE_MODELS").ok()?;
    let text = std::fs::read_to_string(&v).unwrap_or(v);
    text.lines()
        .filter_map(|l| l.trim().split_once(' '))
        .find(|(n, _)| *n == name)
        .map(|(_, rest)| rest.trim().to_string())
}

fn models(name: &str) -> Option<Vec<String>> {
    setting(name).filter(|s| s != "signed-out").map(|s| {
        s.split(',')
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .collect()
    })
}

fn models_json(list: &[String]) -> Value {
    json!({
        "availableModels": list.iter().map(|m| json!({"modelId": m, "name": m})).collect::<Vec<_>>(),
        "currentModelId": list.first(),
    })
}

fn update(session: &str, update: Value) {
    send(
        json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": session, "update": update}}),
    );
}

fn say(session: &str, text: &str) {
    update(
        session,
        json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}),
    );
}

struct Agent {
    name: String,
    session: String,
    model: Option<String>,
    next_id: u64,
}

impl Agent {
    /// Reads until the answer to our request `id` (a permission outcome).
    fn wait_answer(&mut self, lines: &mut impl Iterator<Item = String>, id: u64) -> Option<Value> {
        for line in lines.by_ref() {
            let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if msg.get("method").is_none() && msg.get("id").and_then(Value::as_u64) == Some(id) {
                return msg.get("result").cloned();
            }
        }
        None
    }

    fn prompt(&mut self, lines: &mut impl Iterator<Item = String>, id: &Value, text: &str) {
        let session = self.session.clone();
        let done = |reason: &str| {
            send(json!({"jsonrpc": "2.0", "id": id, "result": {"stopReason": reason}}))
        };
        if text == "/hang" {
            say(&session, "Working on it");
            for line in lines.by_ref() {
                if line.contains("\"session/cancel\"") {
                    log(&format!("acp cancel: {}", self.name));
                    return done("cancelled");
                }
            }
            return;
        }
        if text == "/model" {
            say(
                &session,
                &format!("Model: {}", self.model.as_deref().unwrap_or("default")),
            );
            return done("end_turn");
        }
        if let Some(cmd) = text.strip_prefix("run ") {
            self.next_id += 1;
            let ask = self.next_id;
            let call = format!("call_{ask}");
            update(
                &session,
                json!({"sessionUpdate": "tool_call", "toolCallId": call, "title": cmd, "kind": "execute", "status": "pending", "rawInput": {"command": cmd}}),
            );
            send(
                json!({"jsonrpc": "2.0", "id": ask, "method": "session/request_permission", "params": {
                    "sessionId": session,
                    "toolCall": {"toolCallId": call},
                    "options": [
                        {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                        {"optionId": "always", "name": "Always allow", "kind": "allow_always"},
                        {"optionId": "reject", "name": "Reject", "kind": "reject_once"},
                    ],
                }}),
            );
            let answer = self.wait_answer(lines, ask);
            let chosen = answer
                .as_ref()
                .and_then(|a| a.pointer("/outcome/optionId"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if chosen == "allow" || chosen == "always" {
                update(
                    &session,
                    json!({"sessionUpdate": "tool_call_update", "toolCallId": call, "status": "completed", "content": [{"type": "content", "content": {"type": "text", "text": "ok"}}]}),
                );
                say(&session, &format!("Ran {cmd}."));
            } else {
                update(
                    &session,
                    json!({"sessionUpdate": "tool_call_update", "toolCallId": call, "status": "failed"}),
                );
                say(&session, "Okay, I won't run it.");
            }
            return done("end_turn");
        }
        if let Some(said) = crate::review_reply(text) {
            say(&session, said);
            return done("end_turn");
        }
        say(&session, &format!("You said: {text}"));
        done("end_turn");
    }
}

pub fn run(name: &str) {
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines().map_while(Result::ok);
    let mut agent = Agent {
        name: name.to_string(),
        session: format!("acp-{}", std::process::id()),
        model: None,
        next_id: 1000,
    };
    while let Some(line) = lines.next() {
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let p = msg.get("params").cloned().unwrap_or(Value::Null);
        let reply = |result: Value| send(json!({"jsonrpc": "2.0", "id": id, "result": result}));
        match method {
            "initialize" => reply(json!({
                "protocolVersion": 1,
                "agentCapabilities": {"loadSession": true, "promptCapabilities": {"image": false}, "mcpCapabilities": {"http": true}},
                "authMethods": [{"id": "login", "name": "Log in"}],
                "agentInfo": {"name": name, "title": super::label(name), "version": "1.0.0-fake"},
            })),
            "session/new" | "session/load" => {
                if setting(name).as_deref() == Some("signed-out") {
                    send(
                        json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": "Authentication required"}}),
                    );
                    continue;
                }
                if method == "session/load" {
                    if let Some(old) = p.get("sessionId").and_then(Value::as_str) {
                        agent.session = old.to_string();
                    }
                }
                log(&format!(
                    "acp {method}: {name} mcp={}",
                    p["mcpServers"].as_array().map_or(0, Vec::len)
                ));
                let offered = models(name);
                agent.model = offered.as_ref().and_then(|m| m.first().cloned());
                let mut result = json!({"sessionId": agent.session});
                if let Some(list) = &offered {
                    result["models"] = models_json(list);
                }
                reply(result);
            }
            "session/set_model" => {
                let wanted = p.get("modelId").and_then(Value::as_str).unwrap_or("");
                if models(name).is_some_and(|m| m.iter().any(|x| x == wanted)) {
                    agent.model = Some(wanted.to_string());
                    reply(json!({}));
                } else {
                    send(
                        json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32602, "message": format!("unknown model {wanted}")}}),
                    );
                }
            }
            "session/prompt" => {
                let text: String = p
                    .get("prompt")
                    .and_then(Value::as_array)
                    .map(|blocks| {
                        blocks
                            .iter()
                            .filter_map(|b| b.get("text").and_then(Value::as_str))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                agent.prompt(&mut lines, &id, text.trim());
            }
            _ => {}
        }
    }
}
