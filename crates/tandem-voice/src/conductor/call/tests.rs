use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::json;

use super::*;
use crate::conductor::exec::tests::{quick, FakeHost};
use crate::conductor::exec::PaneFacts;
use crate::conductor::view::UiAction;
use crate::realtime::{RealtimeError, SAMPLE_RATE};
use tandem_core::origin::Origin;
use tandem_core::settings::SettingKey;

/// The provider's side of a fake socket.
struct Wire {
    from_app: mpsc::Receiver<ClientEvent>,
    to_app: mpsc::Sender<ServerEvent>,
}

struct FakeProvider {
    delay: Duration,
    wires: mpsc::UnboundedSender<Wire>,
}

#[async_trait]
impl RealtimeProvider for FakeProvider {
    async fn connect(&self, _url: &str, token: &str) -> Result<Connection, RealtimeError> {
        assert_eq!(token, "rt_mock_token");
        tokio::time::sleep(self.delay).await;
        let (tx, from_app) = mpsc::channel(4096);
        let (to_app, rx) = mpsc::channel(256);
        let _ = self.wires.send(Wire { from_app, to_app });
        Ok(Connection { tx, rx })
    }
}

#[derive(Default)]
struct FakeSpeaker {
    played: Mutex<usize>,
    stops: Mutex<usize>,
}

impl Speaker for FakeSpeaker {
    fn play(&self, pcm: &[i16]) {
        *self.played.lock() += pcm.len();
    }
    fn stop(&self) {
        *self.stops.lock() += 1;
    }
    fn speaking(&self) -> bool {
        false
    }
    fn level(&self) -> f32 {
        0.5
    }
}

struct Host {
    tools: Arc<FakeHost>,
    now: AtomicI64,
    mint: Mutex<Option<HostedError>>,
    mint_delay: Duration,
    provider: Arc<FakeProvider>,
    acts: Mutex<VecDeque<Result<ActResponse, HostedError>>>,
    act_requests: Mutex<Vec<ActRequest>>,
    settles: Mutex<Vec<(String, u32)>>,
    mic: Mutex<Option<mpsc::UnboundedSender<MicEvent>>>,
    mic_owned: Mutex<bool>,
    views: Mutex<Vec<CallView>>,
    records: Mutex<Vec<CallRecord>>,
    speaker: Arc<FakeSpeaker>,
}

impl Host {
    fn new(connect_delay: Duration) -> (Arc<Self>, mpsc::UnboundedReceiver<Wire>) {
        let (wires, rx) = mpsc::unbounded_channel();
        (
            Arc::new(Self {
                tools: FakeHost::with_panes(),
                now: AtomicI64::new(1_000_000),
                mint: Mutex::new(None),
                mint_delay: Duration::ZERO,
                provider: Arc::new(FakeProvider {
                    delay: connect_delay,
                    wires,
                }),
                acts: Mutex::new(VecDeque::new()),
                act_requests: Mutex::new(Vec::new()),
                settles: Mutex::new(Vec::new()),
                mic: Mutex::new(None),
                mic_owned: Mutex::new(false),
                views: Mutex::new(Vec::new()),
                records: Mutex::new(Vec::new()),
                speaker: Arc::new(FakeSpeaker::default()),
            }),
            rx,
        )
    }

    fn advance(&self, ms: i64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }

    fn speak(&self, ms: u32) {
        let n = (16 * ms) as usize;
        let tone: Vec<f32> = (0..n).map(|i| ((i as f32) * 0.2).sin() * 0.5).collect();
        if let Some(tx) = self.mic.lock().as_ref() {
            let _ = tx.send(MicEvent::Samples(tone, 16_000));
        }
    }

    fn script(&self, rounds: Vec<Result<ActResponse, HostedError>>) {
        self.acts.lock().extend(rounds);
    }

    fn last(&self) -> CallView {
        self.views.lock().last().cloned().unwrap_or_default()
    }
}

#[async_trait]
impl ToolHost for Host {
    async fn pane(&self, pane: &str) -> Option<PaneFacts> {
        self.tools.pane(pane).await
    }
    async fn type_prompt(&self, pane: &str, text: &str, send: bool) -> Result<(), String> {
        self.tools.type_prompt(pane, text, send).await
    }
    async fn run_in_shell(&self, pane: &str, cmd: &str, r: LocalReview) -> Result<(), String> {
        self.tools.run_in_shell(pane, cmd, r).await
    }
    async fn read_pane(&self, pane: &str, lines: usize) -> Result<String, String> {
        self.tools.read_pane(pane, lines).await
    }
    async fn get_setting(&self, key: SettingKey) -> Result<Value, String> {
        self.tools.get_setting(key).await
    }
    async fn set_setting(&self, key: SettingKey, value: Value) -> Result<(), String> {
        self.tools.set_setting(key, value).await
    }
    async fn run_tool(&self, name: &str, args: &Value) -> Result<String, String> {
        self.tools.run_tool(name, args).await
    }
    fn ui_action(&self, action: UiAction) {
        self.tools.ui_action(action)
    }
}

#[async_trait]
impl CallHost for Host {
    async fn mint(&self, _voice: Option<&str>) -> Result<SessionGrant, HostedError> {
        tokio::time::sleep(self.mint_delay).await;
        if let Some(e) = self.mint.lock().clone() {
            return Err(e);
        }
        Ok(SessionGrant {
            session_id: "vs_1".into(),
            provider: "fake".into(),
            ephemeral_token: "rt_mock_token".into(),
            ws_url: "ws://mock/realtime".into(),
            expires_at: "2026-10-10T00:30:00Z".into(),
            max_seconds: 1800,
        })
    }
    async fn act(&self, req: &ActRequest) -> Result<ActResponse, HostedError> {
        self.act_requests.lock().push(req.clone());
        self.acts.lock().pop_front().unwrap_or(Ok(ActResponse {
            tool_calls: vec![],
            say: Some("Done.".into()),
        }))
    }
    async fn settle(&self, session_id: &str, seconds: u32) -> Result<Settlement, HostedError> {
        self.settles.lock().push((session_id.into(), seconds));
        Ok(Settlement {
            charged: credits_for(seconds),
            refunded: 1200u32.saturating_sub(credits_for(seconds)),
        })
    }
    async fn snapshot(&self) -> (Value, String) {
        (json!({"panes": ["agent", "shell"]}), "2 panes".into())
    }
    async fn persona(&self) -> PersonaBrief {
        PersonaBrief::default()
    }
    fn provider(&self, name: &str) -> Option<Arc<dyn RealtimeProvider>> {
        (name == "fake").then(|| self.provider.clone() as Arc<dyn RealtimeProvider>)
    }
    fn mic_start(&self, tx: mpsc::UnboundedSender<MicEvent>) -> Result<(), String> {
        *self.mic.lock() = Some(tx);
        *self.mic_owned.lock() = true;
        Ok(())
    }
    fn mic_stop(&self) {
        *self.mic.lock() = None;
        *self.mic_owned.lock() = false;
    }
    fn speaker(&self) -> Arc<dyn Speaker> {
        self.speaker.clone()
    }
    fn now_ms(&self) -> i64 {
        self.now.load(Ordering::SeqCst)
    }
    fn emit(&self, view: &CallView) {
        self.views.lock().push(view.clone());
    }
    fn emit_level(&self, _level: f32) {}
    async fn record(&self, record: CallRecord) {
        self.records.lock().push(record);
    }
}

fn timing() -> CallTiming {
    CallTiming {
        tick: Duration::from_millis(10),
        receipt: Duration::from_millis(50),
        warn_before: Duration::from_secs(120),
        quiet: Duration::from_secs(120),
        exec: quick(),
    }
}

async fn until(what: &str, mut ok: impl FnMut() -> bool) {
    for _ in 0..500 {
        if ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {what}");
}

async fn connected(
    host: &Arc<Host>,
    wires: &mut mpsc::UnboundedReceiver<Wire>,
    origin: CallOrigin,
) -> (Arc<CallController>, Wire) {
    let c = CallController::new(host.clone(), timing());
    c.start(origin);
    let wire = tokio::time::timeout(Duration::from_secs(5), wires.recv())
        .await
        .expect("connects")
        .expect("wire");
    until("listening", || host.last().phase == CallPhase::Listening).await;
    (c, wire)
}

/// Drains what the app sent, counting audio samples.
fn drain(wire: &mut Wire) -> (Vec<ClientEvent>, usize) {
    let mut events = Vec::new();
    let mut audio = 0;
    while let Ok(e) = wire.from_app.try_recv() {
        if let ClientEvent::Audio(s) = &e {
            audio += s.len();
        }
        events.push(e);
    }
    (events, audio)
}

fn act(call_id: &str, request: &str) -> ServerEvent {
    ServerEvent::FunctionCall {
        call_id: call_id.into(),
        name: "act".into(),
        args: json!({ "request": request }),
    }
}

fn calls(names: &[(&str, Value)]) -> Result<ActResponse, HostedError> {
    Ok(ActResponse {
        tool_calls: names
            .iter()
            .enumerate()
            .map(|(i, (n, a))| ToolCall {
                id: format!("vc.1.{i}.{n}"),
                name: (*n).into(),
                args: a.clone(),
            })
            .collect(),
        say: None,
    })
}

async fn output(wire: &mut Wire, call_id: &str) -> String {
    loop {
        let e = tokio::time::timeout(Duration::from_secs(5), wire.from_app.recv())
            .await
            .expect("an output")
            .expect("open");
        if let ClientEvent::FunctionOutput {
            call_id: id,
            output,
        } = e
        {
            if id == call_id {
                return output;
            }
        }
    }
}

#[tokio::test]
async fn f_cn_01_1_a_65_second_call_settles_140_credits() {
    let (host, mut wires) = Host::new(Duration::ZERO);
    let (c, mut wire) = connected(&host, &mut wires, CallOrigin::Button).await;
    assert!(*host.mic_owned.lock(), "the call holds the mic");
    host.advance(65_000);
    until("a tick sees 65 s", || host.last().elapsed_ms >= 65_000).await;
    c.end();
    until("ended", || host.last().phase == CallPhase::Ended).await;
    assert_eq!(host.settles.lock().clone(), vec![("vs_1".to_string(), 65)]);
    let receipt = host.last().receipt.expect("receipt");
    assert_eq!((receipt.seconds, receipt.credits), (65, 140));
    assert!(!*host.mic_owned.lock(), "the mic is given back");
    assert!(host.records.lock().iter().any(|r| matches!(
        r,
        CallRecord::Ended {
            seconds: 65,
            credits: Some(140),
            reason: EndReason::HungUp,
            ..
        }
    )));
    // The configure event went first, with exactly the act tool.
    let (events, _) = drain(&mut wire);
    assert!(matches!(
        events.first(),
        Some(ClientEvent::Configure { .. })
    ));
    // The receipt clears after its 3 s (shortened here).
    until("idle", || host.last().phase == CallPhase::Idle).await;
}

#[tokio::test]
async fn f_cn_01_2_hang_up_during_a_pending_prompt_pane_delivers_nothing() {
    let (host, mut wires) = Host::new(Duration::ZERO);
    host.tools.set("agent", false, true); // busy: the prompt waits for it
    host.script(vec![calls(&[(
        "prompt_pane",
        json!({"pane": "agent", "text": "run the tests", "send": true}),
    )])]);
    let (c, wire) = connected(&host, &mut wires, CallOrigin::Button).await;
    wire.to_app
        .send(act("fc1", "ask the auth pane to run the tests"))
        .await
        .expect("send");
    until("the prompt is pending", || {
        host.last()
            .chips
            .iter()
            .any(|ch| ch.tool == "prompt_pane" && ch.state == ChipState::Running)
    })
    .await;
    c.end();
    until("ended", || host.last().phase == CallPhase::Ended).await;
    // The agent comes back to its prompt after the call ended.
    host.tools.set("agent", false, false);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        host.tools.typed.lock().is_empty(),
        "nothing reached the pane"
    );
}

#[tokio::test]
async fn f_cn_01_3_the_time_cap_ends_the_call_with_a_notice() {
    let (host, mut wires) = Host::new(Duration::ZERO);
    let (_c, _wire) = connected(&host, &mut wires, CallOrigin::Button).await;
    host.advance(1_680_000); // 28 minutes
    until("the 2-minute warning", || {
        host.last().notice.as_deref() == Some("2 minutes left in this call")
    })
    .await;
    host.advance(120_000);
    until("the cap", || host.last().phase == CallPhase::Error).await;
    let v = host.last();
    assert_eq!(
        v.error.as_ref().map(|e| e.code),
        Some(CallErrorCode::TimeCap)
    );
    assert_eq!(
        v.error.map(|e| e.message).as_deref(),
        Some("This call reached its 30-minute limit.")
    );
    assert_eq!(
        host.settles.lock().clone(),
        vec![("vs_1".to_string(), 1800)]
    );
    assert!(!*host.mic_owned.lock());
}

#[tokio::test]
async fn f_cn_01_4_speech_before_connect_is_delivered_after_connect() {
    let (host, mut wires) = Host::new(Duration::from_millis(300));
    let c = CallController::new(host.clone(), timing());
    c.start(CallOrigin::Button);
    until("the mic is on", || host.mic.lock().is_some()).await;
    host.speak(2_000); // two seconds of speech while connecting
    let mut wire = tokio::time::timeout(Duration::from_secs(5), wires.recv())
        .await
        .expect("connects")
        .expect("wire");
    until("listening", || host.last().phase == CallPhase::Listening).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (events, audio) = drain(&mut wire);
    assert!(matches!(
        events.first(),
        Some(ClientEvent::Configure { .. })
    ));
    // All of it, resampled to 24 kHz.
    let expected = 2 * SAMPLE_RATE as usize;
    assert!(
        (audio as i64 - expected as i64).abs() <= 4,
        "{audio} of {expected}"
    );
}

#[tokio::test]
async fn f_cn_01_muting_while_connecting_drops_the_buffer() {
    let (host, mut wires) = Host::new(Duration::from_millis(300));
    let c = CallController::new(host.clone(), timing());
    c.start(CallOrigin::Button);
    until("the mic is on", || host.mic.lock().is_some()).await;
    host.speak(1_000);
    tokio::time::sleep(Duration::from_millis(30)).await;
    c.mute(true);
    tokio::time::sleep(Duration::from_millis(30)).await;
    host.speak(1_000);
    let mut wire = wires.recv().await.expect("wire");
    until("listening", || host.last().phase == CallPhase::Listening).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (_, audio) = drain(&mut wire);
    assert_eq!(audio, 0);
    assert!(host.last().muted);
    // Muted mid-call: still nothing sent; unmuted: sent.
    host.speak(500);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(drain(&mut wire).1, 0);
    c.mute(false);
    tokio::time::sleep(Duration::from_millis(30)).await;
    host.speak(500);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(drain(&mut wire).1 > 0);
}

#[tokio::test]
async fn f_cn_01_hanging_up_while_connecting_settles_at_zero_without_an_error() {
    let (host, mut wires) = Host::new(Duration::from_millis(200));
    let c = CallController::new(host.clone(), timing());
    c.start(CallOrigin::Button);
    until("the mic is on", || host.mic.lock().is_some()).await;
    c.end();
    until("idle", || {
        host.last().phase == CallPhase::Idle && !c.is_live()
    })
    .await;
    let _ = wires.recv().await;
    until("settled", || !host.settles.lock().is_empty()).await;
    assert_eq!(host.settles.lock().clone(), vec![("vs_1".to_string(), 0)]);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        host.views.lock().iter().all(|v| v.error.is_none()),
        "no late error"
    );
}

#[tokio::test]
async fn f_cn_01_startup_errors_are_shown() {
    let (host, _wires) = Host::new(Duration::ZERO);
    *host.mint.lock() = Some(HostedError::NeedsPro);
    let c = CallController::new(host.clone(), timing());
    c.start(CallOrigin::Palette);
    until("the error", || host.last().phase == CallPhase::Error).await;
    assert_eq!(
        host.last().error.map(|e| e.code),
        Some(CallErrorCode::NeedsPro)
    );
    assert!(!*host.mic_owned.lock());
    assert!(!c.is_live());
    // It stays until dismissed.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(host.last().phase, CallPhase::Error);
    c.end();
    assert_eq!(host.last().phase, CallPhase::Idle);
}

#[tokio::test]
async fn f_cn_04_1_five_actions_make_five_chips() {
    let (host, mut wires) = Host::new(Duration::ZERO);
    host.script(vec![calls(&[
        ("switch_mode", json!({"mode": "code"})),
        ("list_panes", json!({})),
        ("read_pane", json!({"pane": "agent"})),
        ("get_setting", json!({"key": "theme"})),
        ("focus_pane", json!({"pane": "agent"})),
    ])]);
    let (_c, mut wire) = connected(&host, &mut wires, CallOrigin::Button).await;
    wire.to_app
        .send(act("fc1", "five things"))
        .await
        .expect("send");
    assert_eq!(output(&mut wire, "fc1").await, "Done.");
    let v = host.last();
    assert_eq!(v.chips.len(), 5);
    assert!(
        v.chips.iter().all(|c| c.state == ChipState::Done),
        "{:?}",
        v.chips
    );
    // The second round sent every result so far.
    let reqs = host.act_requests.lock().clone();
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[1].results.len(), 5);
    assert_eq!(reqs[0].session_id, "vs_1");
}

#[tokio::test]
async fn f_cn_03_2_a_spoken_allow_answers_only_the_request_it_names() {
    let (host, mut wires) = Host::new(Duration::ZERO);
    host.script(vec![
        calls(&[("close_pane", json!({"pane": "agent"}))]),
        calls(&[(
            "run_command_in_shell",
            json!({"pane": "shell", "cmd": "pnpm test"}),
        )]),
    ]);
    let (c, mut wire) = connected(&host, &mut wires, CallOrigin::Button).await;
    wire.to_app
        .send(act("fa", "close the auth pane"))
        .await
        .expect("send");
    until("question A", || host.last().question.is_some()).await;
    let a = host.last().question.expect("A");
    wire.to_app
        .send(act("fb", "run the tests in the shell"))
        .await
        .expect("send");
    until("B asked", || {
        host.last()
            .chips
            .iter()
            .any(|ch| ch.tool == "run_command_in_shell")
    })
    .await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    // The HUD still shows A; "allow" answers A only.
    assert_eq!(
        host.last().question.as_ref().map(|q| q.request_id.clone()),
        Some(a.request_id.clone())
    );
    wire.to_app
        .send(act("fy", &format!("answer {} allow", a.request_id)))
        .await
        .expect("send");
    assert_eq!(output(&mut wire, "fy").await, "Got it.");
    assert_eq!(
        host.tools.tools.lock().clone(),
        vec!["close_pane".to_string()]
    );
    until("B shown", || {
        host.last()
            .question
            .as_ref()
            .is_some_and(|q| q.request_id != a.request_id)
    })
    .await;
    let b = host.last().question.expect("B");
    assert!(host.tools.shell.lock().is_empty());
    // Saying "allow" again, for A or for B, runs nothing in the shell.
    wire.to_app
        .send(act("fz", &format!("answer {} allow", a.request_id)))
        .await
        .expect("send");
    assert!(output(&mut wire, "fz").await.contains("no question"));
    wire.to_app.send(act("fw", "allow")).await.expect("send");
    assert!(output(&mut wire, "fw").await.contains("click on screen"));
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(host.tools.shell.lock().is_empty());
    // Only the click runs it.
    let review = LocalReview::check(&Origin::main_window()).expect("main");
    c.confirm_on_screen(&b.request_id, Some(review))
        .expect("click");
    until("ran", || !host.tools.shell.lock().is_empty()).await;
    assert_eq!(host.tools.shell.lock()[0].1, "pnpm test");
    // The log says how each was confirmed.
    until("logged", || {
        host.records.lock().iter().any(
            |r| matches!(r, CallRecord::Event { text, .. } if text.contains("Confirmed on screen")),
        )
    })
    .await;
    assert!(host.records.lock().iter().any(
        |r| matches!(r, CallRecord::Event { text, .. } if text.contains("Confirmed by voice"))
    ));
}

#[tokio::test]
async fn f_cn_01_out_of_credits_ends_the_call() {
    let (host, mut wires) = Host::new(Duration::ZERO);
    host.script(vec![Err(HostedError::OutOfCredits)]);
    let (_c, wire) = connected(&host, &mut wires, CallOrigin::Button).await;
    wire.to_app
        .send(act("fc1", "anything"))
        .await
        .expect("send");
    until("ended", || host.last().phase == CallPhase::Error).await;
    assert_eq!(
        host.last().error.map(|e| e.code),
        Some(CallErrorCode::OutOfCredits)
    );
    assert_eq!(host.settles.lock().len(), 1);
}

#[tokio::test]
async fn f_cn_01_barge_in_stops_playback_and_the_provider_hanging_up_ends_the_call() {
    let (host, mut wires) = Host::new(Duration::ZERO);
    let (_c, wire) = connected(&host, &mut wires, CallOrigin::Button).await;
    wire.to_app
        .send(ServerEvent::AudioDelta(vec![100; 2400]))
        .await
        .expect("send");
    until("speaking", || {
        host.views
            .lock()
            .iter()
            .any(|v| v.phase == CallPhase::Speaking)
    })
    .await;
    wire.to_app
        .send(ServerEvent::SpeechStarted)
        .await
        .expect("send");
    until("playback stopped", || *host.speaker.stops.lock() >= 1).await;
    until("listening", || host.last().phase == CallPhase::Listening).await;
    assert_eq!(*host.speaker.played.lock(), 2400);
    wire.to_app
        .send(ServerEvent::TranscriptUser("open code mode".into()))
        .await
        .expect("send");
    until("utterance", || {
        host.last().utterance.as_deref() == Some("open code mode")
    })
    .await;
    drop(wire);
    until("failed", || host.last().phase == CallPhase::Error).await;
    assert_eq!(
        host.last().error.map(|e| e.code),
        Some(CallErrorCode::ConnectFailed)
    );
    assert_eq!(host.settles.lock().len(), 1);
}

#[tokio::test]
async fn f_cn_01_one_call_at_a_time() {
    let (host, mut wires) = Host::new(Duration::ZERO);
    let (c, _wire) = connected(&host, &mut wires, CallOrigin::Button).await;
    let again = c.start(CallOrigin::Shortcut);
    assert_eq!(again.call_id.as_deref(), Some("vs_1"));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(wires.try_recv().is_err(), "no second socket");
}

#[test]
fn f_cn_03_2_answers_parse_to_one_request() {
    let q = QuestionView {
        request_id: "q-1-2".into(),
        tool: "close_pane".into(),
        summary: "Close it?".into(),
        verb: "close it".into(),
        review: tandem_core::voice_tools::Review::VoiceOrScreen,
    };
    assert_eq!(
        parse_answer("answer q-1-7 yes", None),
        Some(("q-1-7".into(), true))
    );
    assert_eq!(
        parse_answer("Answer q-1-7, no.", None),
        Some(("q-1-7".into(), false))
    );
    assert_eq!(
        parse_answer("Allow.", Some(&q)),
        Some(("q-1-2".into(), true))
    );
    assert_eq!(parse_answer("allow", None), None);
    assert_eq!(
        parse_answer("yes please open the auth pane", Some(&q)),
        None
    );
    assert_eq!(parse_answer("open code mode", Some(&q)), None);
}

#[tokio::test]
async fn f_cn_02_injected_calls_run_through_the_same_rules() {
    let (host, mut wires) = Host::new(Duration::ZERO);
    let (c, _wire) = connected(&host, &mut wires, CallOrigin::Button).await;
    assert!(c.inject(vec![ToolCall {
        id: "t1".into(),
        name: "set_setting".into(),
        args: json!({"key": "telemetry_usage", "value": true}),
    }]));
    until("the chip fails", || {
        host.last()
            .chips
            .iter()
            .any(|ch| ch.id == "t1" && ch.state == ChipState::Failed)
    })
    .await;
    assert!(host.tools.settings.lock().is_empty());
    c.end();
    until("ended", || !c.is_live()).await;
    assert!(!c.inject(vec![]));
}
