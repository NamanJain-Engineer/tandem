//! F-DI-01 AC1 end to end inside the crate: a simulated hold, the "hello
//! world" fixture through the fake microphone, release; the real Whisper
//! base.en model transcribes it and the fake insertion backend receives the
//! paste, then gets its previous clipboard back. Needs the model cache
//! (`bash scripts/fetch-models.sh base.en`); run with `--include-ignored`.
#![cfg(feature = "whisper")]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tandem_voice::audio::{read_wav, resample_to_16k};
use tandem_voice::capture::{AudioService, CaptureConfig, FakeAudioBackend};
use tandem_voice::chimes::SilentPlayer;
use tandem_voice::dictate::{
    Dictate, DictateConfig, DictateDeps, DictateSink, DictateStateEvent, DictateToast, FinishedTake,
};
use tandem_voice::dictionary::Rule;
use tandem_voice::hotkey::{Bindings, FakeHotkeys, Phase, Which};
use tandem_voice::insert::fake::FakePlatform;
use tandem_voice::insert::{Clip, Target};
use tandem_voice::models::{Catalog, ModelStore};
use tandem_voice::stt::{self, EngineSlot};

#[derive(Default)]
struct Sink(Mutex<Vec<DictateToast>>);
impl DictateSink for Sink {
    fn state(&self, _: DictateStateEvent) {}
    fn toast(&self, t: DictateToast) {
        self.0.lock().push(t);
    }
    fn composer_insert(&self, _: &str, _: &str) {}
    fn finished(&self, _: FinishedTake) {}
    fn rules(&self) -> Vec<Rule> {
        Vec::new()
    }
    fn composer_focus(&self) -> Option<String> {
        None
    }
    fn key(&self, _: Phase, _: bool) {}
    fn verified(&self) {}
    fn switch_due(&self, _: String) {}
}

#[test]
#[ignore = "needs whisper-base.en in the model cache"]
fn f_di_01_1_hold_wav_release_pastes_hello_world_with_real_whisper() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hello_world_16k.wav");
    let (wav, rate) = read_wav(&fixture).expect("fixture");
    let wav = resample_to_16k(&wav, rate);
    let secs = wav.len() as f32 / 16_000.0;
    let audio = Arc::new(AudioService::start(
        Arc::new(FakeAudioBackend::with_samples(wav, 1)),
        CaptureConfig::default(),
        Arc::new(|_| {}),
    ));
    let cache = std::env::var_os("TANDEM_MODEL_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var("HOME").expect("home")).join(".cache/tandem-models")
        });
    let store = ModelStore::new(cache);
    let catalog = Catalog::bundled();
    let model = catalog.get("whisper-base.en").expect("model");
    let engine = Arc::new(EngineSlot::new());
    engine.install(stt::load(model, &store).expect("run scripts/fetch-models.sh base.en"));
    let keys = Arc::new(FakeHotkeys::default());
    let platform = Arc::new(FakePlatform::default());
    platform.set_target(Some(Target {
        app_id: Some("org.gnome.gedit".into()),
        app_name: Some("Text Editor".into()),
        window: 1,
        element: 0,
        terminal: false,
        elevated: false,
        own: false,
    }));
    platform.set_clipboard(Clip::Text("before".into()));
    let sink = Arc::new(Sink::default());
    let d = Dictate::new(
        DictateDeps {
            audio,
            hotkeys: keys.clone(),
            platform: platform.clone(),
            engine,
            chimes: Arc::new(SilentPlayer::default()),
            media: None,
            sink: sink.clone(),
        },
        DictateConfig::default(),
    );
    d.start_hotkeys(&Bindings {
        hold: Some("Ctrl+Alt".parse().expect("binding")),
        toggle: None,
    });
    keys.send(Which::Hold, Phase::Down);
    std::thread::sleep(Duration::from_secs_f32(secs + 0.2));
    keys.send(Which::Hold, Phase::Up);
    let t = Instant::now();
    while platform.pasted().is_empty() && t.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(50));
    }
    let pasted = platform.pasted();
    assert_eq!(pasted.len(), 1, "toasts: {:?}", sink.0.lock());
    let norm: String = pasted[0]
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    assert!(norm.contains("hello world"), "pasted {:?}", pasted[0]);
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(platform.clipboard(), Clip::Text("before".into()));
}
