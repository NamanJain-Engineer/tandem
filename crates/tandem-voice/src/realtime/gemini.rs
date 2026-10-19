//! Google's Gemini Live API (BidiGenerateContent, D-1015) behind
//! [`RealtimeProvider`]: the free primary of D-1013.
//!
//! The API mints an ephemeral token that locks the model, spoken answers in
//! the call's voice, the `act` tool, both transcripts and context compression
//! (D-1014). The app opens the constrained socket with the token as
//! `access_token` in the query, never a header, and its first frame is the
//! setup with the call's instructions. The model comes in `ws_url`'s
//! fragment, which never leaves the app.
//!
//! App → vendor: `setup {model, generationConfig, systemInstruction, tools,
//! inputAudioTranscription, outputAudioTranscription, sessionResumption,
//! contextWindowCompression}` from [`ClientEvent::Configure`];
//! `realtimeInput {audio: {data, mimeType: "audio/pcm;rate=24000"}}` (the
//! service resamples); `toolResponse {functionResponses: [{id, name: "act",
//! response: {output}}]}`; `clientContent {turns, turnComplete: true}` for
//! something to say. A speed change and a barge-in truncate send nothing:
//! the Live API has no speech-rate setting and tracks what was heard itself.
//! Frames wait while a socket hasn't said `setupComplete`.
//!
//! Vendor → app, in text or binary frames: `setupComplete` → Ready (once);
//! `serverContent.modelTurn` audio parts → AudioDelta;
//! `inputTranscription` and `outputTranscription` pieces → one
//! TranscriptUser and one TranscriptAssistant per turn (when `finished`, or
//! at `turnComplete`); `interrupted` → SpeechStarted (stop playback);
//! `turnComplete` → ResponseDone; `toolCall.functionCalls` → a FunctionCall
//! each. `sessionResumptionUpdate` keeps the latest resumable handle, and
//! `goAway` (a connection lasts about 10 minutes) reopens the socket on the
//! same token with the same setup and that handle, so a 30-minute call
//! carries on; an unexpected close tries the same once per handle. The names
//! and fields are pinned by `fixtures/gemini_server_events.json`, taken from
//! Google's SDK; checking them against a recorded session waits on a key
//! (`HUMAN_TODO.md`).

use std::collections::VecDeque;

use async_trait::async_trait;
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use super::{
    pcm, socket, ClientEvent, Connection, RealtimeError, RealtimeProvider, ServerEvent, ACT_TOOL,
    SAMPLE_RATE,
};

pub struct GeminiLive;

/// The Live API's prebuilt voices (the API's `GEMINI_LIVE_VOICES`). The
/// token sets the call's voice; a persona voice from another vendor is left
/// to it.
const VOICES: [&str; 8] = [
    "Puck", "Charon", "Kore", "Fenrir", "Aoede", "Leda", "Orus", "Zephyr",
];

/// Frames held while a socket waits for `setupComplete`, oldest dropped first.
const WAITING: usize = 512;

/// What a frame tells the relay itself rather than the call.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Control {
    pub setup_complete: bool,
    pub go_away: bool,
    pub handle: Option<String>,
}

/// The current turn's transcript pieces.
#[derive(Debug, Default)]
pub(crate) struct Turn {
    user: String,
    assistant: String,
}

impl Turn {
    fn flush_user(&mut self, out: &mut Vec<ServerEvent>) {
        let text = std::mem::take(&mut self.user);
        if !text.trim().is_empty() {
            out.push(ServerEvent::TranscriptUser(text.trim().to_string()));
        }
    }

    fn flush_assistant(&mut self, out: &mut Vec<ServerEvent>) {
        let text = std::mem::take(&mut self.assistant);
        if !text.trim().is_empty() {
            out.push(ServerEvent::TranscriptAssistant(text.trim().to_string()));
        }
    }
}

/// The socket's address with the token, and the model, from a session's
/// `ws_url` (`…BidiGenerateContentConstrained#model=models/…`). Errors never
/// carry the token.
pub(crate) fn endpoint(ws_url: &str, token: &str) -> Result<(String, String), RealtimeError> {
    let mut url =
        reqwest::Url::parse(ws_url).map_err(|_| RealtimeError::Connect("a bad address".into()))?;
    let model = url
        .fragment()
        .and_then(|f| f.strip_prefix("model="))
        .filter(|m| !m.is_empty())
        .map(String::from)
        .ok_or_else(|| RealtimeError::Connect("the session names no model".into()))?;
    url.set_fragment(None);
    url.query_pairs_mut().append_pair("access_token", token);
    Ok((url.to_string(), model))
}

/// The setup frame: the call's instructions and everything the token locks,
/// so a resumption can send it again with `handle`.
pub(crate) fn setup(
    model: &str,
    instructions: &str,
    voice: Option<&str>,
    handle: Option<&str>,
) -> Value {
    let mut generation = json!({"responseModalities": ["AUDIO"]});
    if let Some(name) = voice.and_then(|v| VOICES.iter().find(|n| n.eq_ignore_ascii_case(v))) {
        generation["speechConfig"] =
            json!({"voiceConfig": {"prebuiltVoiceConfig": {"voiceName": name}}});
    }
    let resumption = match handle {
        Some(h) => json!({"handle": h}),
        None => json!({}),
    };
    json!({"setup": {
        "model": model,
        "generationConfig": generation,
        "systemInstruction": {"parts": [{"text": instructions}]},
        "tools": [{"functionDeclarations": [{
            "name": ACT_TOOL,
            "description": "Hand what the user wants done in the app to the planner, in their words.",
            "parameters": {
                "type": "object",
                "properties": {"request": {"type": "string"}},
                "required": ["request"]
            }
        }]}],
        "inputAudioTranscription": {},
        "outputAudioTranscription": {},
        "sessionResumption": resumption,
        "contextWindowCompression": {"slidingWindow": {}}
    }})
}

/// One app event as frames. Configure is the relay's (see [`setup`]).
pub(crate) fn encode(event: &ClientEvent) -> Vec<Value> {
    match event {
        ClientEvent::Audio(samples) => vec![json!({"realtimeInput": {"audio": {
            "data": pcm::encode(samples),
            "mimeType": format!("audio/pcm;rate={SAMPLE_RATE}")
        }}})],
        ClientEvent::FunctionOutput { call_id, output } => {
            vec![json!({"toolResponse": {"functionResponses": [
                {"id": call_id, "name": ACT_TOOL, "response": {"output": output}}
            ]}})]
        }
        ClientEvent::Say(text) => vec![json!({"clientContent": {
            "turns": [{"role": "user", "parts": [{"text": text}]}],
            "turnComplete": true
        }})],
        ClientEvent::SetSpeed(_) | ClientEvent::Truncate { .. } | ClientEvent::Configure { .. } => {
            Vec::new()
        }
    }
}

/// One frame as the call's events, and what the relay does itself.
pub(crate) fn decode(text: &str, turn: &mut Turn) -> (Vec<ServerEvent>, Control) {
    let mut out = Vec::new();
    let mut control = Control::default();
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return (out, control);
    };
    control.setup_complete = v.get("setupComplete").is_some();
    control.go_away = v.get("goAway").is_some();
    if let Some(update) = v.get("sessionResumptionUpdate") {
        if update.get("resumable").and_then(Value::as_bool) == Some(true) {
            control.handle = update
                .get("newHandle")
                .and_then(Value::as_str)
                .filter(|h| !h.is_empty())
                .map(String::from);
        }
    }
    if let Some(content) = v.get("serverContent") {
        if let Some(piece) = content.get("inputTranscription") {
            turn.user.push_str(
                piece
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            );
            if piece.get("finished").and_then(Value::as_bool) == Some(true) {
                turn.flush_user(&mut out);
            }
        }
        for part in content
            .pointer("/modelTurn/parts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let audio = part.get("inlineData").filter(|d| {
                d.get("mimeType")
                    .and_then(Value::as_str)
                    .is_some_and(|m| m.starts_with("audio/pcm"))
            });
            if let Some(pcm) = audio
                .and_then(|d| d.get("data"))
                .and_then(Value::as_str)
                .and_then(pcm::decode)
            {
                out.push(ServerEvent::AudioDelta { item: None, pcm });
            }
        }
        if let Some(piece) = content.get("outputTranscription") {
            turn.assistant.push_str(
                piece
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            );
            if piece.get("finished").and_then(Value::as_bool) == Some(true) {
                turn.flush_assistant(&mut out);
            }
        }
        if content.get("interrupted").and_then(Value::as_bool) == Some(true) {
            out.push(ServerEvent::SpeechStarted);
        }
        if content.get("turnComplete").and_then(Value::as_bool) == Some(true) {
            turn.flush_user(&mut out);
            turn.flush_assistant(&mut out);
            out.push(ServerEvent::ResponseDone);
        }
    }
    for call in v
        .pointer("/toolCall/functionCalls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        out.push(ServerEvent::FunctionCall {
            call_id: call
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            name: call
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            args: call.get("args").cloned().unwrap_or_else(|| json!({})),
        });
    }
    (out, control)
}

type Ws = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
type Sink = SplitSink<Ws, Message>;
type Stream = SplitStream<Ws>;

/// Opens the socket. The address carries the token: errors say what failed.
async fn dial(url: &str) -> Result<(Sink, Stream), RealtimeError> {
    match tokio_tungstenite::connect_async(url).await {
        Ok((ws, _)) => Ok(ws.split()),
        Err(tokio_tungstenite::tungstenite::Error::Http(r))
            if matches!(r.status().as_u16(), 401 | 403) =>
        {
            Err(RealtimeError::Refused)
        }
        Err(e) => Err(RealtimeError::Connect(socket::kind_of(&e))),
    }
}

/// One call's link to the service, across reconnects.
struct Relay {
    url: String,
    model: String,
    /// The setup the call configured, sent again with a handle on resuming.
    setup: Option<Value>,
    /// This socket said `setupComplete`: frames go straight out.
    live: bool,
    ready: bool,
    waiting: VecDeque<Value>,
    handle: Option<String>,
    turn: Turn,
}

impl Relay {
    fn hold(&mut self, frame: Value) {
        if self.waiting.len() == WAITING {
            self.waiting.pop_front();
        }
        self.waiting.push_back(frame);
    }

    /// Reopens the socket on the same token with the same setup and the
    /// latest handle, which it spends: a socket that fails again waits for
    /// the service's next handle.
    async fn resume(&mut self) -> Option<(Sink, Stream)> {
        let handle = self.handle.take()?;
        let mut frame = self.setup.clone()?;
        frame["setup"]["sessionResumption"] = json!({"handle": handle});
        let (mut sink, stream) = dial(&self.url).await.ok()?;
        sink.send(Message::text(frame.to_string())).await.ok()?;
        self.live = false;
        Some((sink, stream))
    }

    async fn run(
        mut self,
        mut sink: Sink,
        mut stream: Stream,
        mut outbox: mpsc::Receiver<ClientEvent>,
        inbox: mpsc::Sender<ServerEvent>,
    ) {
        loop {
            tokio::select! {
                out = outbox.recv() => {
                    let Some(event) = out else {
                        let _ = sink.close().await;
                        return;
                    };
                    if let ClientEvent::Configure { instructions, voice } = &event {
                        let frame = setup(&self.model, instructions, voice.as_deref(), None);
                        self.setup = Some(frame.clone());
                        self.live = false;
                        if sink.send(Message::text(frame.to_string())).await.is_err() {
                            let _ = inbox.send(ServerEvent::Closed).await;
                            return;
                        }
                        continue;
                    }
                    for frame in encode(&event) {
                        if !self.live {
                            self.hold(frame);
                        } else if sink.send(Message::text(frame.to_string())).await.is_err() {
                            // The next read sees the closed socket and resumes or ends.
                            self.hold(frame);
                            break;
                        }
                    }
                }
                frame = stream.next() => {
                    let text = match frame {
                        Some(Ok(Message::Text(t))) => t.as_str().to_string(),
                        Some(Ok(Message::Binary(b))) => String::from_utf8_lossy(&b).into_owned(),
                        Some(Ok(Message::Close(close))) => {
                            if let Some((s, st)) = self.resume().await {
                                (sink, stream) = (s, st);
                                continue;
                            }
                            if let Some(c) = close.filter(|c| u16::from(c.code) != 1000) {
                                let reason: String = c.reason.as_str().chars().take(200).collect();
                                let _ = inbox
                                    .send(ServerEvent::Error(format!(
                                        "the service closed the socket ({}): {reason}",
                                        u16::from(c.code)
                                    )))
                                    .await;
                            }
                            let _ = inbox.send(ServerEvent::Closed).await;
                            return;
                        }
                        None | Some(Err(_)) => {
                            if let Some((s, st)) = self.resume().await {
                                (sink, stream) = (s, st);
                                continue;
                            }
                            let _ = inbox.send(ServerEvent::Closed).await;
                            return;
                        }
                        Some(Ok(_)) => continue,
                    };
                    let (events, control) = decode(&text, &mut self.turn);
                    if control.setup_complete {
                        self.live = true;
                        if !self.ready {
                            self.ready = true;
                            if inbox.send(ServerEvent::Ready).await.is_err() {
                                return;
                            }
                        }
                        while let Some(frame) = self.waiting.pop_front() {
                            if sink.send(Message::text(frame.to_string())).await.is_err() {
                                break;
                            }
                        }
                    }
                    for event in events {
                        if inbox.send(event).await.is_err() {
                            return;
                        }
                    }
                    if let Some(handle) = control.handle {
                        self.handle = Some(handle);
                    }
                    if control.go_away {
                        if let Some((s, st)) = self.resume().await {
                            let _ = sink.close().await;
                            (sink, stream) = (s, st);
                        }
                    }
                }
            }
        }
    }
}

#[async_trait]
impl RealtimeProvider for GeminiLive {
    async fn connect(&self, ws_url: &str, token: &str) -> Result<Connection, RealtimeError> {
        let (url, model) = endpoint(ws_url, token)?;
        let (sink, stream) = dial(&url).await?;
        let (to_server, outbox) = mpsc::channel::<ClientEvent>(256);
        let (inbox, from_server) = mpsc::channel::<ServerEvent>(256);
        let relay = Relay {
            url,
            model,
            setup: None,
            live: false,
            ready: false,
            waiting: VecDeque::new(),
            handle: None,
            turn: Turn::default(),
        };
        tokio::spawn(relay.run(sink, stream, outbox, inbox));
        Ok(Connection {
            tx: to_server,
            rx: from_server,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{SinkExt, StreamExt};
    use std::time::Duration;
    use tokio::net::{TcpListener, TcpStream};
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::WebSocketStream;

    const FIXTURE: &str = include_str!("fixtures/gemini_server_events.json");

    fn expected(v: &Value) -> ServerEvent {
        if let Some(name) = v.as_str() {
            return match name {
                "SpeechStarted" => ServerEvent::SpeechStarted,
                "ResponseDone" => ServerEvent::ResponseDone,
                other => panic!("unknown expectation {other}"),
            };
        }
        let (k, x) = v
            .as_object()
            .and_then(|o| o.iter().next())
            .expect("one key");
        let s = || x.as_str().expect("text").to_string();
        match k.as_str() {
            "TranscriptUser" => ServerEvent::TranscriptUser(s()),
            "TranscriptAssistant" => ServerEvent::TranscriptAssistant(s()),
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
        }
    }

    #[test]
    fn f_cn_01_every_gemini_fixture_frame_decodes_as_pinned() {
        let v: Value = serde_json::from_str(FIXTURE).expect("fixture json");
        let frames = v["events"].as_array().expect("events");
        assert!(frames.len() >= 15);
        let mut turn = Turn::default();
        let mut seen = Vec::new();
        for e in frames {
            let (events, control) = decode(&e["frame"].to_string(), &mut turn);
            let want: Vec<ServerEvent> = e["expect"]
                .as_array()
                .expect("expect")
                .iter()
                .map(expected)
                .collect();
            assert_eq!(events, want, "{}", e["frame"]);
            let c = &e["control"];
            let want = Control {
                setup_complete: c["setupComplete"].as_bool().unwrap_or(false),
                go_away: c["goAway"].as_bool().unwrap_or(false),
                handle: c["handle"].as_str().map(String::from),
            };
            assert_eq!(control, want, "{}", e["frame"]);
            seen.extend(events);
        }
        // Every event the call uses comes from a vendor frame, but Ready (the
        // relay's, on setupComplete) and Error and Closed (the socket's).
        for e in [
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
        ] {
            assert!(
                seen.iter()
                    .any(|s| std::mem::discriminant(s) == std::mem::discriminant(&e)),
                "{e:?}"
            );
        }
        assert_eq!(decode("not json", &mut turn), (vec![], Control::default()));
    }

    #[test]
    fn f_cn_01_the_setup_carries_the_calls_instructions_and_the_act_tool() {
        let frame = setup("models/gemini-live", "You are Ada.", Some("kore"), None);
        let s = &frame["setup"];
        assert_eq!(s["model"], "models/gemini-live");
        assert_eq!(s["systemInstruction"]["parts"][0]["text"], "You are Ada.");
        assert_eq!(
            s["generationConfig"]["responseModalities"],
            json!(["AUDIO"])
        );
        assert_eq!(
            s["generationConfig"]["speechConfig"]["voiceConfig"]["prebuiltVoiceConfig"]
                ["voiceName"],
            "Kore"
        );
        let tools = s["tools"][0]["functionDeclarations"]
            .as_array()
            .expect("declarations");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "act");
        assert_eq!(tools[0]["parameters"]["required"], json!(["request"]));
        assert_eq!(s["inputAudioTranscription"], json!({}));
        assert_eq!(s["outputAudioTranscription"], json!({}));
        assert_eq!(s["sessionResumption"], json!({}));
        assert_eq!(s["contextWindowCompression"], json!({"slidingWindow": {}}));
        // Another vendor's voice is left to the token; a resumption names its handle.
        let again = setup("models/m", "x", Some("marin"), Some("h1"));
        assert!(again["setup"]["generationConfig"]
            .get("speechConfig")
            .is_none());
        assert_eq!(again["setup"]["sessionResumption"], json!({"handle": "h1"}));
    }

    #[test]
    fn f_cn_01_app_events_become_live_api_frames() {
        assert_eq!(
            encode(&ClientEvent::Audio(vec![100, 100, -100, -100])),
            vec![json!({"realtimeInput": {"audio": {
                "data": "ZABkAJz/nP8=",
                "mimeType": "audio/pcm;rate=24000"
            }}})]
        );
        assert_eq!(
            encode(&ClientEvent::FunctionOutput {
                call_id: "fc_1".into(),
                output: "Done.".into()
            }),
            vec![json!({"toolResponse": {"functionResponses": [
                {"id": "fc_1", "name": "act", "response": {"output": "Done."}}
            ]}})]
        );
        assert_eq!(
            encode(&ClientEvent::Say("Say you're listening.".into())),
            vec![json!({"clientContent": {
                "turns": [{"role": "user", "parts": [{"text": "Say you're listening."}]}],
                "turnComplete": true
            }})]
        );
        // No speech-rate setting, and the service tracks barge-in itself.
        assert!(encode(&ClientEvent::SetSpeed(1.2)).is_empty());
        assert!(encode(&ClientEvent::Truncate {
            item_id: "i".into(),
            audio_end_ms: 5
        })
        .is_empty());
    }

    #[test]
    fn f_cn_01_the_token_goes_in_the_query_and_the_model_comes_from_the_fragment() {
        let (url, model) = endpoint(
            "wss://live.example/ws/x.BidiGenerateContentConstrained#model=models/gemini-live",
            "auth_tokens/a b",
        )
        .expect("endpoint");
        assert_eq!(
            url,
            "wss://live.example/ws/x.BidiGenerateContentConstrained?access_token=auth_tokens%2Fa+b"
        );
        assert_eq!(model, "models/gemini-live");
        let err = endpoint("wss://live.example/ws", "secret-token").expect_err("no model");
        assert!(!err.to_string().contains("secret-token"));
        assert!(crate::realtime::for_provider("gemini").is_some());
    }

    /// One accepted socket: the request's URI and Authorization header.
    #[allow(clippy::result_large_err)] // tungstenite's handshake callback signature
    async fn accept(
        listener: &TcpListener,
    ) -> (String, Option<String>, WebSocketStream<TcpStream>) {
        let (tcp, _) = listener.accept().await.expect("accept");
        let mut uri = String::new();
        let mut auth = None;
        let ws = tokio_tungstenite::accept_hdr_async(tcp, |req: &Request, res: Response| {
            uri = req.uri().to_string();
            auth = req
                .headers()
                .get("Authorization")
                .and_then(|v| v.to_str().ok())
                .map(String::from);
            Ok(res)
        })
        .await
        .expect("handshake");
        (uri, auth, ws)
    }

    async fn next_json(ws: &mut WebSocketStream<TcpStream>) -> Value {
        let frame = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("a frame in time")
            .expect("frame")
            .expect("ok");
        serde_json::from_str(frame.into_text().expect("text").as_str()).expect("json")
    }

    async fn recv(conn: &mut Connection) -> Option<ServerEvent> {
        tokio::time::timeout(Duration::from_secs(5), conn.rx.recv())
            .await
            .expect("an event in time")
    }

    #[tokio::test]
    async fn f_cn_01_sets_up_first_with_the_token_in_the_query_and_holds_audio_until_ready() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            let (uri, auth, mut ws) = accept(&listener).await;
            let setup = next_json(&mut ws).await;
            // Nothing else goes out before setupComplete.
            let early = tokio::time::timeout(Duration::from_millis(300), ws.next()).await;
            assert!(early.is_err(), "a frame before setupComplete: {early:?}");
            // The service may answer in binary frames.
            ws.send(Message::binary(br#"{"setupComplete":{}}"#.to_vec()))
                .await
                .expect("send");
            let audio = next_json(&mut ws).await;
            ws.send(Message::text(
                r#"{"serverContent":{"inputTranscription":{"text":"hello","finished":true}}}"#,
            ))
            .await
            .expect("send");
            ws.send(Message::text(
                r#"{"toolCall":{"functionCalls":[{"id":"fc_9","name":"act","args":{"request":"open code mode"}}]}}"#,
            ))
            .await
            .expect("send");
            (uri, auth, setup, audio)
        });
        let mut conn = GeminiLive
            .connect(
                &format!("ws://{addr}/ws/live#model=models/gemini-test"),
                "auth_tokens/t1",
            )
            .await
            .expect("connect");
        conn.tx
            .send(ClientEvent::Configure {
                instructions: "You are Ada.".into(),
                voice: None,
            })
            .await
            .expect("send");
        conn.tx
            .send(ClientEvent::Audio(vec![1, 2]))
            .await
            .expect("send");
        assert_eq!(recv(&mut conn).await, Some(ServerEvent::Ready));
        assert_eq!(
            recv(&mut conn).await,
            Some(ServerEvent::TranscriptUser("hello".into()))
        );
        assert_eq!(
            recv(&mut conn).await,
            Some(ServerEvent::FunctionCall {
                call_id: "fc_9".into(),
                name: "act".into(),
                args: json!({"request": "open code mode"}),
            })
        );
        let (uri, auth, setup, audio) = server.await.expect("server");
        assert_eq!(uri, "/ws/live?access_token=auth_tokens%2Ft1");
        assert_eq!(auth, None);
        assert_eq!(setup["setup"]["model"], "models/gemini-test");
        assert_eq!(
            setup["setup"]["systemInstruction"]["parts"][0]["text"],
            "You are Ada."
        );
        assert_eq!(
            audio["realtimeInput"]["audio"]["mimeType"],
            "audio/pcm;rate=24000"
        );
        // The server hung up and there was no handle to resume with.
        assert_eq!(recv(&mut conn).await, Some(ServerEvent::Closed));
    }

    #[tokio::test]
    async fn f_cn_01_a_go_away_resumes_on_the_same_token_with_the_latest_handle() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            let (first_uri, _, mut first) = accept(&listener).await;
            let _ = next_json(&mut first).await;
            for frame in [
                r#"{"setupComplete":{}}"#,
                r#"{"sessionResumptionUpdate":{"newHandle":"h_old","resumable":true}}"#,
                r#"{"sessionResumptionUpdate":{"newHandle":"h_new","resumable":true}}"#,
                r#"{"goAway":{"timeLeft":"10s"}}"#,
            ] {
                first.send(Message::text(frame)).await.expect("send");
            }
            let (second_uri, _, mut second) = accept(&listener).await;
            let resumed = next_json(&mut second).await;
            second
                .send(Message::text(r#"{"setupComplete":{}}"#))
                .await
                .expect("send");
            second
                .send(Message::text(
                    r#"{"serverContent":{"modelTurn":{"parts":[{"inlineData":{"mimeType":"audio/pcm;rate=24000","data":"ZABkAJz/nP8="}}]}}}"#,
                ))
                .await
                .expect("send");
            let audio = next_json(&mut second).await;
            (first_uri, second_uri, resumed, audio)
        });
        let mut conn = GeminiLive
            .connect(
                &format!("ws://{addr}/ws/live#model=models/gemini-test"),
                "auth_tokens/t2",
            )
            .await
            .expect("connect");
        conn.tx
            .send(ClientEvent::Configure {
                instructions: "You are Ada.".into(),
                voice: None,
            })
            .await
            .expect("send");
        assert_eq!(recv(&mut conn).await, Some(ServerEvent::Ready));
        // The resumed socket's setupComplete isn't a second Ready.
        assert_eq!(
            recv(&mut conn).await,
            Some(ServerEvent::AudioDelta {
                item: None,
                pcm: vec![100, 100, -100, -100]
            })
        );
        conn.tx
            .send(ClientEvent::Audio(vec![7]))
            .await
            .expect("send");
        let (first_uri, second_uri, resumed, audio) = server.await.expect("server");
        assert_eq!(first_uri, second_uri);
        assert_eq!(
            resumed["setup"]["sessionResumption"],
            json!({"handle": "h_new"})
        );
        assert_eq!(
            resumed["setup"]["systemInstruction"]["parts"][0]["text"],
            "You are Ada."
        );
        assert!(audio.get("realtimeInput").is_some());
    }

    #[tokio::test]
    async fn f_cn_01_a_socket_closed_at_its_setup_is_an_error_and_never_ready() {
        use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
        use tokio_tungstenite::tungstenite::protocol::CloseFrame;
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            let (_, _, mut ws) = accept(&listener).await;
            let _setup = next_json(&mut ws).await;
            // Over its limits, the service turns the session down: the call
            // falls back to the next provider (D-1017).
            ws.close(Some(CloseFrame {
                code: CloseCode::Error,
                reason: "quota exceeded".into(),
            }))
            .await
            .expect("close");
        });
        let mut conn = GeminiLive
            .connect(
                &format!("ws://{addr}/ws/live#model=models/gemini-test"),
                "auth_tokens/t3",
            )
            .await
            .expect("connect");
        conn.tx
            .send(ClientEvent::Configure {
                instructions: "You are Ada.".into(),
                voice: None,
            })
            .await
            .expect("send");
        assert_eq!(
            recv(&mut conn).await,
            Some(ServerEvent::Error(
                "the service closed the socket (1011): quota exceeded".into()
            ))
        );
        assert_eq!(recv(&mut conn).await, Some(ServerEvent::Closed));
        server.await.expect("server");
    }

    #[tokio::test]
    async fn f_cn_01_a_refused_or_missing_gemini_server_is_an_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        drop(listener);
        let err = GeminiLive
            .connect(&format!("ws://{addr}/ws#model=models/m"), "secret")
            .await
            .err()
            .expect("no server");
        assert!(matches!(err, RealtimeError::Connect(_)));
        assert!(!err.to_string().contains("secret"));
    }
}
