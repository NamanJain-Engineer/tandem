//! The bundled sherpa-onnx keyword spotter (feature `kws`): a 3.3M-parameter
//! streaming zipformer transducer (int8) trained on GigaSpeech, Apache-2.0
//! (`models/wake/README.md`). "Hey Conductor" is spelled in the model's BPE
//! units; two near-homophones are decoys, so "hey, conduct yourself" and
//! "hey Connor" match them instead of the phrase.

use std::path::Path;

use sherpa_onnx::{KeywordSpotter, KeywordSpotterConfig, OnlineStream};

use super::{Sensitivity, WakeDetector, KEYWORD};

/// "HEY CONDUCTOR" in the model's BPE units.
const PHRASE: &str = "▁HE Y ▁CON D U C T OR";
const DECOYS: &[&str] = &["▁HE Y ▁CON D U C T ▁YOUR S EL F", "▁HE Y ▁CON N OR"];
const DECOY_TUNING: (f32, f32) = (2.0, 0.2);

/// The keywords buffer for a sensitivity.
pub fn keywords(sensitivity: Sensitivity) -> String {
    let (score, threshold) = sensitivity.tuning();
    let mut out = format!("{PHRASE} :{score} #{threshold} @{KEYWORD}\n");
    for d in DECOYS {
        out.push_str(&format!(
            "{d} :{} #{} @DECOY\n",
            DECOY_TUNING.0, DECOY_TUNING.1
        ));
    }
    out
}

pub struct SherpaKws {
    spotter: KeywordSpotter,
    stream: OnlineStream,
}

impl SherpaKws {
    pub fn load(dir: &Path, sensitivity: Sensitivity) -> Result<Self, String> {
        for f in super::MODEL_FILES {
            if !dir.join(f).is_file() {
                return Err(format!("wake model file missing: {f}"));
            }
        }
        let p = |f: &str| Some(dir.join(f).to_string_lossy().into_owned());
        let mut config = KeywordSpotterConfig::default();
        config.model_config.transducer.encoder = p("encoder.int8.onnx");
        config.model_config.transducer.decoder = p("decoder.int8.onnx");
        config.model_config.transducer.joiner = p("joiner.int8.onnx");
        config.model_config.tokens = p("tokens.txt");
        config.model_config.num_threads = 1;
        config.model_config.provider = Some("cpu".into());
        config.max_active_paths = 4;
        config.num_trailing_blanks = 1;
        config.keywords_buf = Some(keywords(sensitivity));
        let spotter = KeywordSpotter::create(&config).ok_or("the wake model didn't load")?;
        let stream = spotter.create_stream();
        Ok(Self { spotter, stream })
    }
}

impl WakeDetector for SherpaKws {
    fn accept(&mut self, samples: &[f32]) -> bool {
        self.stream.accept_waveform(16_000, samples);
        let mut hit = false;
        while self.spotter.is_ready(&self.stream) {
            self.spotter.decode(&self.stream);
            if let Some(r) = self.spotter.get_result(&self.stream) {
                if !r.keyword.is_empty() {
                    hit |= r.keyword == KEYWORD;
                    self.spotter.reset(&self.stream);
                }
            }
        }
        hit
    }

    fn reset(&mut self) {
        self.stream = self.spotter.create_stream();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_cn_07_keywords_carry_the_phrase_and_decoys() {
        let k = keywords(Sensitivity::Medium);
        assert!(
            k.starts_with("▁HE Y ▁CON D U C T OR :2 #0.25 @HEY_CONDUCTOR\n"),
            "{k}"
        );
        assert_eq!(k.matches("@DECOY").count(), 2);
    }
}
