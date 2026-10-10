//! F-DI-04 AC2 with real models: the committed "hello world" fixture
//! transcribes to text containing "hello world". The models come from
//! `bash scripts/fetch-models.sh base.en` (SHA-pinned, cached in
//! ~/.cache/tandem-models); run with `--include-ignored f_di_04_2`.

use std::path::PathBuf;

use tandem_voice::audio::{read_wav, resample_to_16k};
use tandem_voice::guards::{self, Verdict};
#[cfg(any(feature = "whisper", feature = "parakeet"))]
use tandem_voice::models::{Catalog, ModelStore};
#[cfg(any(feature = "whisper", feature = "parakeet"))]
use tandem_voice::stt::{self, SttOptions};

#[cfg(any(feature = "whisper", feature = "parakeet"))]
fn cache() -> ModelStore {
    let root = std::env::var_os("TANDEM_MODEL_CACHE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/tandem-models")))
        .expect("a model cache");
    ModelStore::new(root)
}

fn fixture(name: &str) -> Vec<f32> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let (samples, rate) = read_wav(&path).expect("fixture");
    resample_to_16k(&samples, rate)
}

#[cfg(any(feature = "whisper", feature = "parakeet"))]
fn transcribe(id: &str, wav: &str) -> String {
    let catalog = Catalog::bundled();
    let model = catalog.get(id).expect("in catalog");
    let store = cache();
    assert!(
        store.is_installed(model),
        "{id} isn't in the model cache: run `bash scripts/fetch-models.sh {id}`"
    );
    let engine = stt::load(model, &store).expect("load");
    let pcm = fixture(wav);
    let Verdict::Speech { end } = guards::check(&pcm) else {
        panic!("the fixture has no speech");
    };
    engine
        .transcribe(&pcm[..end], &SttOptions::default())
        .expect("transcribe")
        .text
}

#[cfg(feature = "whisper")]
#[test]
#[ignore = "needs whisper-base.en in the model cache"]
fn f_di_04_2_hello_world_with_whisper_base_en() {
    let text = transcribe("whisper-base.en", "hello_world_16k.wav");
    let norm: String = text
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    assert!(norm.contains("hello world"), "got {text:?}");
}

#[cfg(feature = "parakeet")]
#[test]
#[ignore = "needs Parakeet in the model cache"]
fn f_di_04_2_hello_world_with_parakeet() {
    let text = transcribe("parakeet-tdt-0.6b-v3-int8", "hello_world_16k.wav");
    let norm: String = text
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    assert!(norm.contains("hello world"), "got {text:?}");
}

#[test]
fn f_di_04_3_the_silence_fixture_is_no_speech() {
    assert_eq!(guards::check(&fixture("silence_2s.wav")), Verdict::NoSpeech);
    assert!(matches!(
        guards::check(&fixture("hello_world_16k.wav")),
        Verdict::Speech { .. }
    ));
}
