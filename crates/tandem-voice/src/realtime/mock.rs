//! The dev mock's protocol (`scripts/mock-realtime.ts`, D-1102): JSON text
//! frames, PCM16 at 24 kHz in base64.
//!
//! App → mock: `session.update {session: {instructions, voice, tools}}`,
//! `input_audio.append {audio}`, `function_call.output {call_id, output}`,
//! `response.create {instructions}`.
//!
//! Mock → app: `session.ready`, `speech.started`, `transcript.user {text}`,
//! `transcript.assistant {text}`, `audio.delta {audio}`,
//! `function_call {call_id, name, arguments}` (`arguments` a JSON string),
//! `response.done`, `error {message}`.
//!
//! The token goes in the handshake's `Authorization: Bearer` header; it is
//! never logged.

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

use super::{pcm, ClientEvent, Connection, RealtimeError, RealtimeProvider, ServerEvent, ACT_TOOL};

pub struct MockRealtime;

/// The talker's one tool, as the session declares it.
pub fn act_tool() -> Value {
    json!({
        "type": "function",
        "name": ACT_TOOL,
        "description": "Hand what the user wants done in the app to the planner, in their words.",
        "parameters": {
            "type": "object",
            "properties": {"request": {"type": "string"}},
            "required": ["request"],
            "additionalProperties": false
        }
    })
}

/// One app event as the mock's JSON.
pub fn encode(event: &ClientEvent) -> Value {
    match event {
        ClientEvent::Configure {
            instructions,
            voice,
        } => json!({
            "type": "session.update",
            "session": {"instructions": instructions, "voice": voice, "tools": [act_tool()]}
        }),
        ClientEvent::Audio(samples) => {
            json!({"type": "input_audio.append", "audio": pcm::encode(samples)})
        }
        ClientEvent::FunctionOutput { call_id, output } => {
            json!({"type": "function_call.output", "call_id": call_id, "output": output})
        }
        ClientEvent::Say(text) => json!({"type": "response.create", "instructions": text}),
    }
}

/// One mock frame as a normalized event; unknown types are skipped.
pub fn decode(frame: &str) -> Option<ServerEvent> {
    let v: Value = serde_json::from_str(frame).ok()?;
    let text = |k: &str| v[k].as_str().unwrap_or_default().to_string();
    Some(match v["type"].as_str()? {
        "session.ready" => ServerEvent::Ready,
        "speech.started" => ServerEvent::SpeechStarted,
        "transcript.user" => ServerEvent::TranscriptUser(text("text")),
        "transcript.assistant" => ServerEvent::TranscriptAssistant(text("text")),
        "audio.delta" => ServerEvent::AudioDelta(pcm::decode(v["audio"].as_str()?)?),
        "function_call" => ServerEvent::FunctionCall {
            call_id: text("call_id"),
            name: text("name"),
            args: serde_json::from_str(v["arguments"].as_str().unwrap_or("{}"))
                .unwrap_or(Value::Null),
        },
        "response.done" => ServerEvent::ResponseDone,
        "error" => ServerEvent::Error(text("message")),
        _ => return None,
    })
}

#[async_trait]
impl RealtimeProvider for MockRealtime {
    async fn connect(&self, ws_url: &str, token: &str) -> Result<Connection, RealtimeError> {
        let mut req = ws_url
            .into_client_request()
            .map_err(|e| RealtimeError::Connect(e.to_string()))?;
        let bearer = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| RealtimeError::Connect("the token isn't a header value".into()))?;
        req.headers_mut().insert("Authorization", bearer);
        let (socket, _) = match tokio_tungstenite::connect_async(req).await {
            Ok(ok) => ok,
            Err(tokio_tungstenite::tungstenite::Error::Http(r)) if r.status().as_u16() == 401 => {
                return Err(RealtimeError::Refused)
            }
            // The URL carries the token: say what failed, not where.
            Err(e) => return Err(RealtimeError::Connect(kind_of(&e))),
        };
        let (mut sink, mut stream) = socket.split();
        let (to_server, mut outbox) = mpsc::channel::<ClientEvent>(256);
        let (inbox, from_server) = mpsc::channel::<ServerEvent>(256);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    out = outbox.recv() => {
                        let Some(event) = out else {
                            let _ = sink.close().await;
                            break;
                        };
                        if sink.send(Message::text(encode(&event).to_string())).await.is_err() {
                            let _ = inbox.send(ServerEvent::Closed).await;
                            break;
                        }
                    }
                    frame = stream.next() => {
                        match frame {
                            Some(Ok(Message::Text(text))) => {
                                if let Some(event) = decode(text.as_str()) {
                                    if inbox.send(event).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                                let _ = inbox.send(ServerEvent::Closed).await;
                                break;
                            }
                            Some(Ok(_)) => {}
                        }
                    }
                }
            }
        });
        Ok(Connection {
            tx: to_server,
            rx: from_server,
        })
    }
}

fn kind_of(e: &tokio_tungstenite::tungstenite::Error) -> String {
    use tokio_tungstenite::tungstenite::Error as E;
    match e {
        E::Io(io) => format!("{:?}", io.kind()),
        E::Http(r) => format!("HTTP {}", r.status().as_u16()),
        E::Url(_) => "a bad address".into(),
        E::Tls(_) => "TLS".into(),
        other => format!("{other}")
            .split(':')
            .next()
            .unwrap_or("error")
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

    #[test]
    fn f_cn_01_frames_round_trip_and_unknown_ones_are_skipped() {
        let call = r#"{"type":"function_call","call_id":"c1","name":"act","arguments":"{\"request\":\"open code mode\"}"}"#;
        assert_eq!(
            decode(call),
            Some(ServerEvent::FunctionCall {
                call_id: "c1".into(),
                name: "act".into(),
                args: json!({"request": "open code mode"}),
            })
        );
        assert_eq!(decode(r#"{"type":"rate_limits.updated"}"#), None);
        assert_eq!(decode("not json"), None);
        let audio = encode(&ClientEvent::Audio(vec![1, -1]));
        assert_eq!(audio["type"], "input_audio.append");
        assert_eq!(
            decode(&json!({"type":"audio.delta","audio":audio["audio"]}).to_string()),
            Some(ServerEvent::AudioDelta(vec![1, -1]))
        );
        let session = encode(&ClientEvent::Configure {
            instructions: "be brief".into(),
            voice: None,
        });
        // Exactly one tool, `act`.
        assert_eq!(
            session["session"]["tools"].as_array().map(|t| t.len()),
            Some(1)
        );
        assert_eq!(session["session"]["tools"][0]["name"], "act");
    }

    #[tokio::test]
    #[allow(clippy::result_large_err)] // tungstenite's handshake callback signature
    async fn f_cn_01_connects_with_the_token_and_relays_both_ways() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept");
            let mut auth = String::new();
            let mut ws =
                tokio_tungstenite::accept_hdr_async(tcp, |req: &Request, res: Response| {
                    auth = req
                        .headers()
                        .get("Authorization")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    Ok(res)
                })
                .await
                .expect("handshake");
            ws.send(Message::text(r#"{"type":"session.ready"}"#))
                .await
                .expect("send");
            let first = ws.next().await.expect("frame").expect("ok");
            ws.send(Message::text(
                r#"{"type":"transcript.user","text":"hello"}"#,
            ))
            .await
            .expect("send");
            (auth, first.into_text().expect("text").to_string())
        });
        let mut conn = MockRealtime
            .connect(&format!("ws://{addr}/realtime"), "rt_mock_t")
            .await
            .expect("connect");
        assert_eq!(conn.rx.recv().await, Some(ServerEvent::Ready));
        conn.tx
            .send(ClientEvent::Audio(vec![5, 6]))
            .await
            .expect("send");
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), conn.rx.recv())
                .await
                .expect("in time"),
            Some(ServerEvent::TranscriptUser("hello".into()))
        );
        let (auth, first) = server.await.expect("server");
        assert_eq!(auth, "Bearer rt_mock_t");
        let first: Value = serde_json::from_str(&first).expect("json");
        assert_eq!(first["type"], "input_audio.append");
        // The server hung up.
        assert_eq!(conn.rx.recv().await, Some(ServerEvent::Closed));
    }

    #[tokio::test]
    async fn f_cn_01_a_refused_or_missing_server_is_an_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        drop(listener);
        let err = MockRealtime
            .connect(&format!("ws://{addr}/realtime?token=secret"), "secret")
            .await
            .err()
            .expect("no server");
        assert!(matches!(err, RealtimeError::Connect(_)));
        assert!(!err.to_string().contains("secret"));
    }
}
