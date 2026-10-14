//! The vendor's Realtime API (GA event names), on the socket the API minted:
//! `wss://…/v1/realtime?model=…` with the session's ephemeral client secret
//! as the bearer token. The client secret already carries the session's
//! instructions, voice and `act` tool (`apps/api/src/providers/realtime/openai.ts`);
//! [`ClientEvent::Configure`] sets them again with the call's persona and the
//! audio formats.
//!
//! App → vendor: `session.update {session: {type: "realtime", instructions,
//! audio: {input: {format, transcription, turn_detection}, output: {format,
//! voice?, speed?}}, tools: [act], tool_choice}}`, `input_audio_buffer.append
//! {audio}`, `conversation.item.create {item: function_call_output {call_id,
//! output}}` then `response.create`, a system message then
//! `response.create` for something to say, and `conversation.item.truncate
//! {item_id, content_index: 0, audio_end_ms}` on barge-in (D-1122).
//!
//! Vendor → app: `session.created` → Ready; `response.output_audio.delta
//! {item_id, delta}` → AudioDelta; `conversation.item.input_audio_transcription.completed
//! {transcript}` → TranscriptUser; `response.output_audio_transcript.done
//! {transcript}` → TranscriptAssistant; `response.output_item.done` with a
//! completed `function_call` item → FunctionCall (an interrupted call's item is
//! `incomplete` and runs nothing, which `response.function_call_arguments.done`
//! couldn't tell); `input_audio_buffer.speech_started` → SpeechStarted
//! (barge-in: the server's VAD cancels the response, the app stops playback);
//! `response.done` → ResponseDone; `error {error: {message}}` → Error. Every
//! other event is skipped. The names and fields are pinned by
//! `fixtures/openai_server_events.json`; checking it against a recorded
//! session waits on a key (`HUMAN_TODO.md`).

use async_trait::async_trait;
use serde_json::{json, Value};

use super::mock::act_tool;
use super::{
    pcm, socket, ClientEvent, Connection, RealtimeError, RealtimeProvider, ServerEvent, SAMPLE_RATE,
};

pub struct OpenAiRealtime;

/// The vendor's voices (`OPENAI_REALTIME_VOICES` in the API); another name
/// keeps the voice the session was minted with.
pub const VOICES: [&str; 10] = [
    "marin", "cedar", "alloy", "ash", "ballad", "coral", "echo", "sage", "shimmer", "verse",
];

/// The model that transcribes the user's speech for the call log and HUD.
pub const TRANSCRIBE_MODEL: &str = "gpt-4o-mini-transcribe";

fn pcm_format() -> Value {
    json!({"type": "audio/pcm", "rate": SAMPLE_RATE})
}

/// One app event as the vendor's client events, in order.
pub fn encode(event: &ClientEvent) -> Vec<Value> {
    match event {
        ClientEvent::Configure {
            instructions,
            voice,
        } => {
            let mut output = json!({"format": pcm_format()});
            if let Some(v) = voice.as_deref().filter(|v| VOICES.contains(v)) {
                output["voice"] = json!(v);
            }
            vec![json!({
                "type": "session.update",
                "session": {
                    "type": "realtime",
                    "instructions": instructions,
                    "audio": {
                        "input": {
                            "format": pcm_format(),
                            "transcription": {"model": TRANSCRIBE_MODEL},
                            "turn_detection": {
                                "type": "server_vad",
                                "create_response": true,
                                "interrupt_response": true
                            }
                        },
                        "output": output
                    },
                    "tools": [act_tool()],
                    "tool_choice": "auto"
                }
            })]
        }
        ClientEvent::Audio(samples) => {
            vec![json!({"type": "input_audio_buffer.append", "audio": pcm::encode(samples)})]
        }
        ClientEvent::FunctionOutput { call_id, output } => vec![
            json!({
                "type": "conversation.item.create",
                "item": {"type": "function_call_output", "call_id": call_id, "output": output}
            }),
            json!({"type": "response.create"}),
        ],
        // A system message keeps the session's persona; `response.instructions`
        // would replace it for that turn.
        ClientEvent::Say(text) => vec![
            json!({
                "type": "conversation.item.create",
                "item": {
                    "type": "message",
                    "role": "system",
                    "content": [{"type": "input_text", "text": text}]
                }
            }),
            json!({"type": "response.create"}),
        ],
        // f32 → two decimals, so 1.1 goes out as 1.1, not 1.100000023841858.
        ClientEvent::Truncate {
            item_id,
            audio_end_ms,
        } => vec![json!({
            "type": "conversation.item.truncate",
            "item_id": item_id,
            "content_index": 0,
            "audio_end_ms": audio_end_ms
        })],
        ClientEvent::SetSpeed(speed) => vec![json!({
            "type": "session.update",
            "session": {
                "type": "realtime",
                "audio": {"output": {"speed": (f64::from(*speed) * 100.0).round() / 100.0}}
            }
        })],
    }
}

/// One vendor server event as a normalized event; the rest are skipped.
pub fn decode(frame: &str) -> Option<ServerEvent> {
    let v: Value = serde_json::from_str(frame).ok()?;
    let text = |k: &str| v[k].as_str().unwrap_or_default().to_string();
    Some(match v["type"].as_str()? {
        "session.created" => ServerEvent::Ready,
        "input_audio_buffer.speech_started" => ServerEvent::SpeechStarted,
        "conversation.item.input_audio_transcription.completed" => {
            ServerEvent::TranscriptUser(text("transcript"))
        }
        "response.output_audio_transcript.done" => {
            ServerEvent::TranscriptAssistant(text("transcript"))
        }
        "response.output_audio.delta" => ServerEvent::AudioDelta {
            item: v["item_id"].as_str().map(String::from),
            pcm: pcm::decode(v["delta"].as_str()?)?,
        },
        "response.output_item.done" => {
            let item = &v["item"];
            if item["type"] != "function_call" || item["status"] != "completed" {
                return None;
            }
            ServerEvent::FunctionCall {
                call_id: item["call_id"].as_str()?.to_string(),
                name: item["name"].as_str().unwrap_or_default().to_string(),
                args: serde_json::from_str(item["arguments"].as_str().unwrap_or("{}"))
                    .unwrap_or(Value::Null),
            }
        }
        "response.done" => ServerEvent::ResponseDone,
        "error" => ServerEvent::Error(
            v["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        ),
        _ => return None,
    })
}

#[async_trait]
impl RealtimeProvider for OpenAiRealtime {
    async fn connect(&self, ws_url: &str, token: &str) -> Result<Connection, RealtimeError> {
        socket::open(ws_url, token, socket::Codec { encode, decode }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{SinkExt, StreamExt};
    use std::time::Duration;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
    use tokio_tungstenite::tungstenite::Message;

    const FIXTURE: &str = include_str!("fixtures/openai_server_events.json");

    /// The fixture's frames, by their `type`, with what each should become.
    fn fixture() -> Vec<(Value, Option<ServerEvent>)> {
        let v: Value = serde_json::from_str(FIXTURE).expect("fixture json");
        v["events"]
            .as_array()
            .expect("events")
            .iter()
            .map(|e| (e["frame"].clone(), expected(&e["expect"])))
            .collect()
    }

    fn expected(v: &Value) -> Option<ServerEvent> {
        if v.is_null() {
            return None;
        }
        if let Some(name) = v.as_str() {
            return Some(match name {
                "Ready" => ServerEvent::Ready,
                "SpeechStarted" => ServerEvent::SpeechStarted,
                "ResponseDone" => ServerEvent::ResponseDone,
                other => panic!("unknown expectation {other}"),
            });
        }
        let (k, x) = v
            .as_object()
            .and_then(|o| o.iter().next())
            .expect("one key");
        let s = || x.as_str().expect("text").to_string();
        Some(match k.as_str() {
            "TranscriptUser" => ServerEvent::TranscriptUser(s()),
            "TranscriptAssistant" => ServerEvent::TranscriptAssistant(s()),
            "Error" => ServerEvent::Error(s()),
            "AudioDelta" => ServerEvent::AudioDelta {
                item: x["item"].as_str().map(String::from),
                pcm: x["pcm"]
                    .as_array()
                    .expect("samples")
                    .iter()
                    .map(|n| n.as_i64().expect("sample") as i16)
                    .collect(),
            },
            "FunctionCall" => ServerEvent::FunctionCall {
                call_id: x["call_id"].as_str().expect("call_id").into(),
                name: x["name"].as_str().expect("name").into(),
                args: x["args"].clone(),
            },
            other => panic!("unknown expectation {other}"),
        })
    }

    fn frame(of_type: &str) -> String {
        fixture()
            .into_iter()
            .find(|(f, _)| f["type"] == of_type)
            .map(|(f, _)| f.to_string())
            .unwrap_or_else(|| panic!("no {of_type} in the fixture"))
    }

    #[test]
    fn f_cn_01_every_fixture_event_decodes_as_pinned() {
        let events = fixture();
        assert!(events.len() >= 20);
        for (f, want) in events {
            assert_eq!(decode(&f.to_string()), want, "{}", f["type"]);
        }
        // Every normalized event the call uses has a vendor event.
        let got: Vec<_> = fixture()
            .into_iter()
            .filter_map(|(f, _)| decode(&f.to_string()))
            .map(|e| std::mem::discriminant(&e))
            .collect();
        for e in [
            ServerEvent::Ready,
            ServerEvent::AudioDelta {
                item: None,
                pcm: vec![],
            },
            ServerEvent::TranscriptUser(String::new()),
            ServerEvent::TranscriptAssistant(String::new()),
            ServerEvent::FunctionCall {
                call_id: String::new(),
                name: String::new(),
                args: Value::Null,
            },
            ServerEvent::SpeechStarted,
            ServerEvent::ResponseDone,
            ServerEvent::Error(String::new()),
        ] {
            assert!(got.contains(&std::mem::discriminant(&e)), "{e:?}");
        }
        assert_eq!(decode("not json"), None);
        assert_eq!(decode(r#"{"no":"type"}"#), None);
    }

    #[test]
    fn f_cn_01_an_interrupted_function_call_runs_nothing() {
        let incomplete = fixture()
            .into_iter()
            .find(|(f, _)| f["item"]["status"] == "incomplete")
            .map(|(f, _)| f)
            .expect("an incomplete call");
        assert_eq!(decode(&incomplete.to_string()), None);
        // The streaming `…arguments.done` comes for interrupted calls too: it's
        // skipped, and the completed item is what runs.
        assert_eq!(
            decode(&frame("response.function_call_arguments.done")),
            None
        );
    }

    #[test]
    fn f_cn_01_the_session_has_the_persona_pcm16_24k_and_exactly_one_tool() {
        let frames = encode(&ClientEvent::Configure {
            instructions: "You are Ada. Be brief.".into(),
            voice: Some("sage".into()),
        });
        assert_eq!(frames.len(), 1);
        let s = &frames[0];
        assert_eq!(s["type"], "session.update");
        assert_eq!(s["session"]["type"], "realtime");
        assert_eq!(s["session"]["instructions"], "You are Ada. Be brief.");
        let pcm = json!({"type": "audio/pcm", "rate": 24000});
        assert_eq!(s["session"]["audio"]["input"]["format"], pcm);
        assert_eq!(s["session"]["audio"]["output"]["format"], pcm);
        assert_eq!(s["session"]["audio"]["output"]["voice"], "sage");
        assert_eq!(
            s["session"]["audio"]["input"]["transcription"]["model"],
            TRANSCRIBE_MODEL
        );
        let vad = &s["session"]["audio"]["input"]["turn_detection"];
        assert_eq!(vad["type"], "server_vad");
        assert_eq!(vad["interrupt_response"], true);
        let tools = s["session"]["tools"].as_array().expect("tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["name"], "act");
        assert_eq!(tools[0]["parameters"]["required"], json!(["request"]));
        // "default", or a name the vendor lacks, keeps the minted voice.
        for voice in [
            None,
            Some("default".to_string()),
            Some("nova-x".to_string()),
        ] {
            let s = &encode(&ClientEvent::Configure {
                instructions: "x".into(),
                voice,
            })[0];
            assert!(s["session"]["audio"]["output"].get("voice").is_none());
        }
    }

    #[test]
    fn f_cn_01_audio_outputs_speech_and_speed_use_the_vendors_events() {
        let audio = encode(&ClientEvent::Audio(vec![1, -1]));
        assert_eq!(
            audio,
            vec![json!({"type": "input_audio_buffer.append", "audio": "AQD//w=="})]
        );
        let out = encode(&ClientEvent::FunctionOutput {
            call_id: "call_1".into(),
            output: "Code mode is open.".into(),
        });
        assert_eq!(
            out,
            vec![
                json!({
                    "type": "conversation.item.create",
                    "item": {"type": "function_call_output", "call_id": "call_1", "output": "Code mode is open."}
                }),
                json!({"type": "response.create"}),
            ]
        );
        let say = encode(&ClientEvent::Say("Two minutes left.".into()));
        assert_eq!(say.len(), 2);
        assert_eq!(say[0]["item"]["role"], "system");
        assert_eq!(say[0]["item"]["content"][0]["type"], "input_text");
        assert_eq!(say[0]["item"]["content"][0]["text"], "Two minutes left.");
        assert_eq!(say[1], json!({"type": "response.create"}));
        // The session's instructions stay; a response's own would replace them.
        assert!(say[1].get("response").is_none());
        let speed = &encode(&ClientEvent::SetSpeed(1.1))[0];
        assert_eq!(speed["session"]["type"], "realtime");
        assert_eq!(
            speed["session"]["audio"]["output"]["speed"].to_string(),
            "1.1"
        );
    }

    #[test]
    fn f_cn_01_barge_in_truncates_the_item_to_what_was_heard() {
        let v: Value = serde_json::from_str(FIXTURE).expect("fixture json");
        let pinned = v["client_events"]
            .as_array()
            .expect("client events")
            .iter()
            .find(|e| e["type"] == "conversation.item.truncate")
            .cloned()
            .expect("a pinned truncate");
        let sent = encode(&ClientEvent::Truncate {
            item_id: "item_a1".into(),
            audio_end_ms: 1_480,
        });
        assert_eq!(sent, vec![pinned]);
        // The vendor's acknowledgement changes nothing for the call.
        assert_eq!(decode(&frame("conversation.item.truncated")), None);
    }

    #[test]
    fn f_cn_01_for_provider_returns_it() {
        assert!(crate::realtime::for_provider("openai").is_some());
        assert!(crate::realtime::for_provider("mock").is_some());
        assert!(crate::realtime::for_provider("other").is_none());
    }

    async fn next_json(
        ws: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    ) -> Value {
        loop {
            let m = tokio::time::timeout(Duration::from_secs(5), ws.next())
                .await
                .expect("a frame in time")
                .expect("open")
                .expect("ok");
            if let Message::Text(t) = m {
                return serde_json::from_str(t.as_str()).expect("json");
            }
        }
    }

    async fn next(rx: &mut tokio::sync::mpsc::Receiver<ServerEvent>) -> Option<ServerEvent> {
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("an event in time")
    }

    /// A local server replaying the fixture: connect with the client secret,
    /// audio both ways, an `act` round trip, barge-in, an error and a close.
    #[tokio::test]
    #[allow(clippy::result_large_err)] // tungstenite's handshake callback signature
    async fn f_cn_01_a_session_against_a_local_vendor_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept");
            let mut seen = (String::new(), String::new());
            let mut ws =
                tokio_tungstenite::accept_hdr_async(tcp, |req: &Request, res: Response| {
                    seen.0 = req
                        .headers()
                        .get("Authorization")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    seen.1 = req.uri().to_string();
                    Ok(res)
                })
                .await
                .expect("handshake");
            let mut got = Vec::new();
            ws.send(Message::text(frame("session.created")))
                .await
                .expect("send");
            got.push(next_json(&mut ws).await); // session.update
            got.push(next_json(&mut ws).await); // input_audio_buffer.append
            for t in [
                "input_audio_buffer.speech_started",
                "conversation.item.input_audio_transcription.completed",
                "response.function_call_arguments.done",
                "response.output_item.done",
                "response.done",
            ] {
                ws.send(Message::text(frame(t))).await.expect("send");
            }
            got.push(next_json(&mut ws).await); // conversation.item.create
            got.push(next_json(&mut ws).await); // response.create
            for t in [
                "response.output_audio.delta",
                "response.output_audio_transcript.done",
                "input_audio_buffer.speech_started",
                "error",
            ] {
                ws.send(Message::text(frame(t))).await.expect("send");
            }
            ws.close(None).await.expect("close");
            (seen, got)
        });

        let mut conn = OpenAiRealtime
            .connect(
                &format!("ws://{addr}/v1/realtime?model=gpt-realtime"),
                "ek_test_123",
            )
            .await
            .expect("connect");
        assert_eq!(next(&mut conn.rx).await, Some(ServerEvent::Ready));
        conn.tx
            .send(ClientEvent::Configure {
                instructions: "Be brief.".into(),
                voice: Some("cedar".into()),
            })
            .await
            .expect("send");
        conn.tx
            .send(ClientEvent::Audio(vec![1, -1]))
            .await
            .expect("send");
        assert_eq!(next(&mut conn.rx).await, Some(ServerEvent::SpeechStarted));
        assert_eq!(
            next(&mut conn.rx).await,
            Some(ServerEvent::TranscriptUser("Open code mode.".into()))
        );
        assert_eq!(
            next(&mut conn.rx).await,
            Some(ServerEvent::FunctionCall {
                call_id: "call_1".into(),
                name: "act".into(),
                args: json!({"request": "open code mode"}),
            })
        );
        assert_eq!(next(&mut conn.rx).await, Some(ServerEvent::ResponseDone));
        conn.tx
            .send(ClientEvent::FunctionOutput {
                call_id: "call_1".into(),
                output: "Code mode is open.".into(),
            })
            .await
            .expect("send");
        assert_eq!(
            next(&mut conn.rx).await,
            Some(ServerEvent::AudioDelta {
                item: Some("item_a1".into()),
                pcm: vec![100, 100, -100, -100]
            })
        );
        assert_eq!(
            next(&mut conn.rx).await,
            Some(ServerEvent::TranscriptAssistant(
                "Code mode is open.".into()
            ))
        );
        assert_eq!(next(&mut conn.rx).await, Some(ServerEvent::SpeechStarted));
        assert_eq!(
            next(&mut conn.rx).await,
            Some(ServerEvent::Error(
                "Invalid value for 'audio.output.speed'.".into()
            ))
        );
        assert_eq!(next(&mut conn.rx).await, Some(ServerEvent::Closed));

        let ((auth, uri), got) = server.await.expect("server");
        assert_eq!(auth, "Bearer ek_test_123");
        assert_eq!(uri, "/v1/realtime?model=gpt-realtime");
        let types: Vec<_> = got
            .iter()
            .map(|f| f["type"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(
            types,
            [
                "session.update",
                "input_audio_buffer.append",
                "conversation.item.create",
                "response.create"
            ]
        );
        assert_eq!(got[0]["session"]["audio"]["output"]["voice"], "cedar");
        assert_eq!(got[1]["audio"], "AQD//w==");
        assert_eq!(got[2]["item"]["type"], "function_call_output");
        assert_eq!(got[2]["item"]["call_id"], "call_1");
        assert_eq!(got[2]["item"]["output"], "Code mode is open.");
    }

    #[tokio::test]
    async fn f_cn_01_a_refused_client_secret_is_refused() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let (mut tcp, _) = listener.accept().await.expect("accept");
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buf = [0u8; 2048];
            let _ = tcp.read(&mut buf).await;
            let _ = tcp
                .write_all(b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\n\r\n")
                .await;
        });
        let err = OpenAiRealtime
            .connect(&format!("ws://{addr}/v1/realtime"), "ek_expired")
            .await
            .err()
            .expect("refused");
        assert_eq!(err, RealtimeError::Refused);
        assert!(!err.to_string().contains("ek_expired"));
    }
}
