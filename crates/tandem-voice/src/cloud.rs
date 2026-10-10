//! Cloud speech-to-text (F-DI-05): the controller's side of it. The app
//! implements `CloudStt` over the hosted relay (`POST /v1/stt/transcribe`);
//! here is what the controller needs to call it from a take's thread and to
//! say what went wrong. A cloud failure never falls back to local on its own:
//! the toast offers "Use local for this take" (`Dictate::use_local`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Which engine transcribes a take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EngineChoice {
    #[default]
    Local,
    Cloud,
}

/// One take for the cloud.
#[derive(Debug, Clone, PartialEq)]
pub struct CloudRequest<'a> {
    /// 16 kHz mono.
    pub pcm16k: &'a [f32],
    /// `None` detects.
    pub language: Option<String>,
    /// Preferred words, only with "Share dictionary hints with cloud".
    pub hints: Vec<String>,
}

/// Why a cloud take failed, and so which action its toast offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CloudFailureKind {
    NeedsPro,
    OutOfCredits,
    DailyLimit,
    Offline,
    SignedOut,
    TooLarge,
    Other,
}

/// A failed cloud take: `message` is the line the user reads (the server's
/// words for a limit), `detail` the diagnostic for the tooltip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudFailure {
    pub kind: CloudFailureKind,
    pub message: String,
    pub detail: String,
}

/// The hosted transcription service, called from a take's own thread.
pub trait CloudStt: Send + Sync {
    fn transcribe(&self, req: CloudRequest<'_>) -> Result<String, CloudFailure>;
}

/// What a Dictate toast offers besides its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ToastAction {
    /// Transcribe the failed cloud take on this device.
    UseLocal,
    /// Open the plans (cloud needs Pro).
    SeePlans,
}

impl CloudFailureKind {
    pub fn actions(self) -> Vec<ToastAction> {
        match self {
            Self::NeedsPro => vec![ToastAction::UseLocal, ToastAction::SeePlans],
            _ => vec![ToastAction::UseLocal],
        }
    }
}

/// 16-bit PCM WAV, mono, at 16 kHz: what the relay accepts besides Opus.
/// A 10-minute take is about 19 MB, under its 25 MB limit.
pub fn wav16k(pcm16k: &[f32]) -> Vec<u8> {
    let data_len = (pcm16k.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&16_000u32.to_le_bytes());
    out.extend_from_slice(&32_000u32.to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm16k {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_di_05_wav_header_and_samples() {
        let wav = wav16k(&[0.0, 1.0, -1.0]);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(
            u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
            16_000
        );
        assert_eq!(u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]), 6);
        assert_eq!(wav.len(), 44 + 6);
        assert_eq!(i16::from_le_bytes([wav[46], wav[47]]), i16::MAX);
        // A standard reader parses it back.
        let r = hound::WavReader::new(std::io::Cursor::new(wav)).expect("wav");
        assert_eq!(r.spec().sample_rate, 16_000);
        assert_eq!(r.spec().channels, 1);
        assert_eq!(r.len(), 3);
    }

    #[test]
    fn f_di_05_every_cloud_failure_offers_local_and_pro_offers_plans() {
        for k in [
            CloudFailureKind::OutOfCredits,
            CloudFailureKind::DailyLimit,
            CloudFailureKind::Offline,
            CloudFailureKind::Other,
        ] {
            assert_eq!(k.actions(), vec![ToastAction::UseLocal]);
        }
        assert_eq!(
            CloudFailureKind::NeedsPro.actions(),
            vec![ToastAction::UseLocal, ToastAction::SeePlans]
        );
    }
}
