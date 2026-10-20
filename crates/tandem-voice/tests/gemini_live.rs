//! F-CN-01 against Google's real Live API (HUMAN_TODO "Check against a real
//! session"): the app's own client, instructions and audio path, on a token
//! the API's own `GeminiLive.mint` made. Ignored, since each test needs a
//! fresh token, a key and the network: `scripts/agent/gemini-live-check.sh`
//! mints them from `GEMINI_API_KEY` and runs these, `--long` for the
//! 10-minute reconnect too.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;
use tandem_voice::conductor::persona::{instructions, PersonaBrief};
use tandem_voice::realtime::gemini::GeminiLive;
use tandem_voice::realtime::pcm::Resampler;
use tandem_voice::realtime::{ClientEvent, Connection, RealtimeProvider, ServerEvent, SAMPLE_RATE};
use tokio::time::{sleep, timeout, Instant};

/// A session the check script minted: `{ws_url, token}`, from the file `var` names.
async fn open(var: &str) -> Connection {
    let path = std::env::var(var).unwrap_or_else(|_| {
        panic!("{var} names no session file: run scripts/agent/gemini-live-check.sh")
    });
    let session: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("session file")).expect("json");
    let (ws_url, token) = (
        session["ws_url"].as_str().expect("ws_url"),
        session["token"].as_str().expect("token"),
    );
    let conn = GeminiLive.connect(ws_url, token).await.expect("connects");
    let summary = "Workspace shop: the panes \"auth tests\" (Claude Code, working) and \"billing UI\" (Codex, idle).";
    conn.tx
        .send(ClientEvent::Configure {
            instructions: instructions(&PersonaBrief::default(), summary),
            voice: None,
        })
        .await
        .expect("configure");
    conn
}

/// The next event within `wait`, or None.
async fn next(conn: &mut Connection, wait: Duration) -> Option<ServerEvent> {
    timeout(wait, conn.rx.recv()).await.ok().flatten()
}

async fn ready(conn: &mut Connection) {
    // A call gives its talker 10 s to say Ready (CallTiming::ready).
    match next(conn, Duration::from_secs(10)).await {
        Some(ServerEvent::Ready) => {}
        other => panic!("expected Ready first, got {other:?}"),
    }
}

/// `open_code_mode_16k.wav` ("Please open code mode.") at the talker's 24 kHz,
/// then a second of silence for the service to hear the end of the turn.
fn speech() -> Vec<i16> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/open_code_mode_16k.wav");
    let mut r = hound::WavReader::open(path).expect("fixture opens");
    let rate = r.spec().sample_rate;
    let input: Vec<f32> = r
        .samples::<i16>()
        .map(|s| s.expect("sample") as f32 / 32768.0)
        .collect();
    let mut out = Vec::new();
    Resampler::new(rate).process(&input, &mut out);
    out.extend(std::iter::repeat_n(0, SAMPLE_RATE as usize));
    out
}

/// Sends `samples` in 100 ms chunks at real time, as the mic would.
async fn say(conn: &Connection, samples: &[i16]) {
    for chunk in samples.chunks(SAMPLE_RATE as usize / 10) {
        conn.tx
            .send(ClientEvent::Audio(chunk.to_vec()))
            .await
            .expect("audio");
        sleep(Duration::from_millis(100)).await;
    }
}

#[derive(Debug, Default)]
struct Heard {
    user: Vec<String>,
    assistant: Vec<String>,
    audio_ms: u64,
    acts: Vec<Value>,
    done: usize,
}

/// Events until the talker finishes a turn that isn't waiting on an act,
/// answering each act the way the doer would.
async fn turn(conn: &mut Connection, heard: &mut Heard, within: Duration) {
    let end = Instant::now() + within;
    loop {
        let left = end.saturating_duration_since(Instant::now());
        let Some(event) = next(conn, left).await else {
            panic!("no finished turn within {within:?}; heard so far: {heard:?}");
        };
        match event {
            ServerEvent::TranscriptUser(t) => heard.user.push(t),
            ServerEvent::TranscriptAssistant(t) => heard.assistant.push(t),
            ServerEvent::AudioDelta { pcm, .. } => {
                heard.audio_ms += pcm.len() as u64 * 1000 / SAMPLE_RATE as u64
            }
            ServerEvent::FunctionCall {
                call_id,
                name,
                args,
            } => {
                assert_eq!(name, "act", "the talker's one tool");
                heard.acts.push(args);
                conn.tx
                    .send(ClientEvent::FunctionOutput {
                        call_id,
                        output: "Code mode is open.".into(),
                    })
                    .await
                    .expect("act answer");
            }
            ServerEvent::ResponseDone => {
                heard.done += 1;
                // After an act, the talker says its answer in another turn.
                if heard.acts.is_empty() || heard.done >= 2 {
                    return;
                }
            }
            ServerEvent::SpeechStarted | ServerEvent::Ready => {}
            ServerEvent::Error(e) => panic!("the service sent an error: {e}; heard: {heard:?}"),
            ServerEvent::Closed => panic!("the service closed the socket; heard: {heard:?}"),
        }
    }
}

#[tokio::test]
#[ignore = "needs a real Gemini token: scripts/agent/gemini-live-check.sh"]
async fn f_cn_01_a_real_gemini_session_hears_speech_answers_and_calls_act() {
    let mut conn = open("TANDEM_GEMINI_SESSION").await;
    ready(&mut conn).await;
    say(&conn, &speech()).await;
    let mut heard = Heard::default();
    turn(&mut conn, &mut heard, Duration::from_secs(30)).await;
    println!("heard: {heard:#?}");
    assert!(
        !heard.user.is_empty(),
        "the service transcribed what was said"
    );
    assert!(heard.audio_ms > 0, "the talker answered aloud");
    assert!(!heard.assistant.is_empty(), "and transcribed its answer");
    for args in &heard.acts {
        let request = args["request"].as_str().unwrap_or_default();
        assert!(
            !request.trim().is_empty(),
            "an act carries the request: {args}"
        );
    }
    if heard.acts.is_empty() {
        println!("note: the talker answered without calling act this time");
    }
}

#[tokio::test]
#[ignore = "needs a real Gemini token and 12 minutes: scripts/agent/gemini-live-check.sh --long"]
async fn f_cn_01_a_real_gemini_session_carries_on_past_its_10_minute_reconnect() {
    let mut conn = open("TANDEM_GEMINI_SESSION_LONG").await;
    ready(&mut conn).await;
    // Google ends a connection after about 10 minutes, with goAway first; the
    // client reopens it on the same token with the latest handle (D-1015).
    let quiet = vec![0_i16; SAMPLE_RATE as usize / 10];
    let until = Instant::now() + Duration::from_secs(11 * 60);
    while Instant::now() < until {
        conn.tx
            .send(ClientEvent::Audio(quiet.clone()))
            .await
            .expect("still open");
        match next(&mut conn, Duration::from_millis(100)).await {
            Some(ServerEvent::Error(e)) => panic!("error while quiet: {e}"),
            Some(ServerEvent::Closed) => panic!("the socket closed while quiet"),
            _ => {}
        }
    }
    say(&conn, &speech()).await;
    let mut heard = Heard::default();
    turn(&mut conn, &mut heard, Duration::from_secs(30)).await;
    println!("heard after 11 minutes: {heard:#?}");
    assert!(
        heard.audio_ms > 0,
        "the talker still answers after the reconnect"
    );
}
