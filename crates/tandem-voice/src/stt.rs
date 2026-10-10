//! Speech-to-text engines (F-DI-04, ARCHITECTURE §12.4) and the slot that
//! holds the active one. A model switch requested during a take waits until
//! the take ends; if the new model fails to load the old one stays.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::models::{Model, ModelStore, Runtime};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SttOptions {
    /// `None` = detect.
    pub language: Option<String>,
    /// Preferred spellings from the dictionary, as Whisper's initial prompt.
    pub initial_prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SttError {
    #[error("Couldn't load {0}")]
    Load(String),
    #[error("Transcription failed: {0}")]
    Failed(String),
    #[error("This build can't run {0} models")]
    Unsupported(String),
}

pub trait SttEngine: Send + Sync {
    fn model_id(&self) -> &str;
    /// 16 kHz mono in, text out. CPU-bound: call it off the async runtime.
    fn transcribe(&self, pcm16k: &[f32], opts: &SttOptions) -> Result<Transcript, SttError>;
}

/// Loads a downloaded model with its runtime.
pub fn load(model: &Model, store: &ModelStore) -> Result<Arc<dyn SttEngine>, SttError> {
    let path = store.load_path(model);
    match model.runtime {
        Runtime::Whisper => whisper::load(&model.id, &path),
        Runtime::Onnx => parakeet::load(&model.id, &path),
    }
}

#[cfg(feature = "whisper")]
mod whisper {
    use super::*;
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

    pub struct WhisperEngine {
        id: String,
        ctx: WhisperContext,
    }

    pub fn load(id: &str, path: &std::path::Path) -> Result<Arc<dyn SttEngine>, SttError> {
        let path_str = path.to_str().ok_or_else(|| SttError::Load(id.into()))?;
        let ctx = WhisperContext::new_with_params(path_str, WhisperContextParameters::default())
            .map_err(|e| SttError::Load(format!("{id}: {e}")))?;
        Ok(Arc::new(WhisperEngine { id: id.into(), ctx }))
    }

    impl SttEngine for WhisperEngine {
        fn model_id(&self) -> &str {
            &self.id
        }

        fn transcribe(&self, pcm: &[f32], opts: &SttOptions) -> Result<Transcript, SttError> {
            let mut state = self
                .ctx
                .create_state()
                .map_err(|e| SttError::Failed(e.to_string()))?;
            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
            let threads = std::thread::available_parallelism()
                .map(|n| n.get().min(8))
                .unwrap_or(4);
            params.set_n_threads(threads as i32);
            params.set_language(opts.language.as_deref().or(Some("auto")));
            if let Some(prompt) = &opts.initial_prompt {
                params.set_initial_prompt(prompt);
            }
            // Hallucination guards (ARCHITECTURE §12.4).
            params.set_suppress_nst(true);
            params.set_suppress_blank(true);
            params.set_entropy_thold(2.4);
            params.set_no_speech_thold(0.6);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_special(false);
            params.set_print_timestamps(false);
            params.set_no_context(true);
            // Whisper needs at least a second; pad short takes with silence.
            let mut audio = pcm.to_vec();
            if audio.len() < 16_000 + 1_600 {
                audio.resize(16_000 + 1_600, 0.0);
            }
            state
                .full(params, &audio)
                .map_err(|e| SttError::Failed(e.to_string()))?;
            let mut text = String::new();
            for seg in state.as_iter() {
                if let Ok(s) = seg.to_str_lossy() {
                    text.push_str(&s);
                }
            }
            Ok(Transcript {
                text: text.trim().to_string(),
            })
        }
    }
}

#[cfg(not(feature = "whisper"))]
mod whisper {
    use super::*;
    pub fn load(_id: &str, _path: &std::path::Path) -> Result<Arc<dyn SttEngine>, SttError> {
        Err(SttError::Unsupported("Whisper".into()))
    }
}

#[cfg(feature = "parakeet")]
mod parakeet {
    use super::*;
    use transcribe_rs::onnx::parakeet::ParakeetModel;
    use transcribe_rs::onnx::Quantization;
    use transcribe_rs::{SpeechModel, TranscribeOptions};

    pub struct ParakeetEngine {
        id: String,
        model: Mutex<ParakeetModel>,
    }

    pub fn load(id: &str, dir: &std::path::Path) -> Result<Arc<dyn SttEngine>, SttError> {
        let model = ParakeetModel::load(dir, &Quantization::Int8)
            .map_err(|e| SttError::Load(format!("{id}: {e}")))?;
        Ok(Arc::new(ParakeetEngine {
            id: id.into(),
            model: Mutex::new(model),
        }))
    }

    impl SttEngine for ParakeetEngine {
        fn model_id(&self) -> &str {
            &self.id
        }

        fn transcribe(&self, pcm: &[f32], opts: &SttOptions) -> Result<Transcript, SttError> {
            let options = TranscribeOptions {
                language: opts.language.clone(),
                ..TranscribeOptions::default()
            };
            let result = self
                .model
                .lock()
                .transcribe(pcm, &options)
                .map_err(|e| SttError::Failed(e.to_string()))?;
            Ok(Transcript {
                text: result.text.trim().to_string(),
            })
        }
    }
}

#[cfg(not(feature = "parakeet"))]
mod parakeet {
    use super::*;
    pub fn load(_id: &str, _path: &std::path::Path) -> Result<Arc<dyn SttEngine>, SttError> {
        Err(SttError::Unsupported("Parakeet".into()))
    }
}

/// Whether this build can run a runtime at all.
pub fn runtime_built(runtime: Runtime) -> bool {
    match runtime {
        Runtime::Whisper => cfg!(feature = "whisper"),
        Runtime::Onnx => cfg!(feature = "parakeet"),
    }
}

/// A test engine: says `text` for any take, counting its calls.
pub struct FakeStt {
    pub id: String,
    pub text: String,
    calls: Mutex<usize>,
}

impl FakeStt {
    pub fn new(id: &str, text: &str) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            calls: Mutex::new(0),
        }
    }

    pub fn calls(&self) -> usize {
        *self.calls.lock()
    }
}

impl SttEngine for FakeStt {
    fn model_id(&self) -> &str {
        &self.id
    }

    fn transcribe(&self, _pcm: &[f32], _opts: &SttOptions) -> Result<Transcript, SttError> {
        *self.calls.lock() += 1;
        Ok(Transcript {
            text: self.text.clone(),
        })
    }
}

/// What a switch request did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Switch {
    /// No take is running: load it now.
    Now(String),
    /// "Switches after this take."
    Queued,
}

#[derive(Default)]
struct Slot {
    engine: Option<Arc<dyn SttEngine>>,
    pending: Option<String>,
    in_take: bool,
}

/// The active engine, and a switch held back while a take runs (AC4).
#[derive(Default)]
pub struct EngineSlot {
    inner: Mutex<Slot>,
}

impl EngineSlot {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current(&self) -> Option<Arc<dyn SttEngine>> {
        self.inner.lock().engine.clone()
    }

    pub fn current_id(&self) -> Option<String> {
        self.inner
            .lock()
            .engine
            .as_ref()
            .map(|e| e.model_id().to_string())
    }

    /// A take starts: it keeps the engine it started with.
    pub fn begin_take(&self) -> Option<Arc<dyn SttEngine>> {
        let mut s = self.inner.lock();
        s.in_take = true;
        s.engine.clone()
    }

    /// The take is over; returns a switch that was waiting for it.
    pub fn end_take(&self) -> Option<String> {
        let mut s = self.inner.lock();
        s.in_take = false;
        s.pending.take()
    }

    pub fn request(&self, id: &str) -> Switch {
        let mut s = self.inner.lock();
        if s.in_take {
            s.pending = Some(id.to_string());
            Switch::Queued
        } else {
            s.pending = None;
            Switch::Now(id.to_string())
        }
    }

    pub fn pending(&self) -> Option<String> {
        self.inner.lock().pending.clone()
    }

    /// Installs a loaded engine (never during a take: that would change the
    /// model under it, so it's queued as pending instead).
    pub fn install(&self, engine: Arc<dyn SttEngine>) -> bool {
        let mut s = self.inner.lock();
        if s.in_take {
            s.pending = Some(engine.model_id().to_string());
            return false;
        }
        s.engine = Some(engine);
        true
    }

    pub fn clear(&self) {
        self.inner.lock().engine = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_di_04_4_a_switch_mid_take_applies_after_the_take() {
        let slot = EngineSlot::new();
        assert!(slot.install(Arc::new(FakeStt::new("base", "a"))));
        let during = slot.begin_take().expect("engine");
        assert_eq!(slot.request("turbo"), Switch::Queued);
        // The running take still uses the old model.
        assert_eq!(during.model_id(), "base");
        assert!(!slot.install(Arc::new(FakeStt::new("turbo", "b"))));
        assert_eq!(slot.current_id().as_deref(), Some("base"));
        assert_eq!(slot.end_take().as_deref(), Some("turbo"));
        assert_eq!(slot.request("turbo"), Switch::Now("turbo".into()));
        assert!(slot.install(Arc::new(FakeStt::new("turbo", "b"))));
        assert_eq!(slot.current_id().as_deref(), Some("turbo"));
    }

    #[test]
    fn f_di_04_a_failed_load_keeps_the_old_engine() {
        let slot = EngineSlot::new();
        slot.install(Arc::new(FakeStt::new("base", "a")));
        let dir = tempfile::tempdir().expect("dir");
        let store = ModelStore::new(dir.path().to_path_buf());
        let catalog = crate::models::Catalog::bundled();
        let model = catalog.get("whisper-base.en").expect("m");
        // Nothing downloaded: loading fails, and the slot is untouched.
        assert!(load(model, &store).is_err());
        assert_eq!(slot.current_id().as_deref(), Some("base"));
    }
}
