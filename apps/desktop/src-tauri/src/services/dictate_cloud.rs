//! Cloud speech-to-text over the hosted relay (F-DI-05, ARCHITECTURE §14.11):
//! the app's `CloudStt`. A take's audio goes up as 16 kHz WAV with its
//! duration, the language when one is chosen, and the dictionary hints only
//! when the user shares them. Free plans are refused here, before any audio
//! leaves the machine.

use std::sync::Arc;

use futures::future::BoxFuture;
use tandem_core::entitlement::Plan;
use tandem_voice::cloud::{wav16k, CloudFailure, CloudFailureKind, CloudRequest, CloudStt};

use crate::services::api::{ApiError, HostedFailure, SttUpload, Transcript};

/// One relay call, with a fresh token.
pub type SttCall =
    Arc<dyn Fn(SttUpload) -> BoxFuture<'static, Result<Transcript, ApiError>> + Send + Sync>;

pub const FEATURE: &str = "Cloud transcription";

pub struct HostedStt {
    plan: Arc<dyn Fn() -> Plan + Send + Sync>,
    call: SttCall,
    rt: tokio::runtime::Handle,
}

impl HostedStt {
    pub fn new(
        plan: Arc<dyn Fn() -> Plan + Send + Sync>,
        call: SttCall,
        rt: tokio::runtime::Handle,
    ) -> Self {
        Self { plan, call, rt }
    }
}

/// What a relay error means for a take, in the words the toast shows.
pub fn failure(err: &ApiError) -> CloudFailure {
    let hosted = HostedFailure::from_api(err, FEATURE);
    let kind = match &hosted {
        HostedFailure::NeedsPro { .. } => CloudFailureKind::NeedsPro,
        HostedFailure::OutOfCredits { .. } => CloudFailureKind::OutOfCredits,
        HostedFailure::DailyLimit { .. } => CloudFailureKind::DailyLimit,
        HostedFailure::TooLarge { .. } => CloudFailureKind::TooLarge,
        HostedFailure::SignedOut { .. } => CloudFailureKind::SignedOut,
        HostedFailure::Unavailable { .. } => CloudFailureKind::Offline,
        HostedFailure::RateLimited { .. } | HostedFailure::Other { .. } => CloudFailureKind::Other,
    };
    CloudFailure {
        kind,
        message: hosted.message().to_string(),
        // The status and code only: a detail never carries transcript text.
        detail: match err {
            ApiError::Http { status, code, .. } => format!("HTTP {status} {code}"),
            ApiError::Network(e) => format!("network: {e}"),
            ApiError::Decode(e) => format!("decode: {e}"),
        },
    }
}

impl CloudStt for HostedStt {
    fn transcribe(&self, req: CloudRequest<'_>) -> Result<String, CloudFailure> {
        if (self.plan)() == Plan::Free {
            return Err(CloudFailure {
                kind: CloudFailureKind::NeedsPro,
                message: format!("{FEATURE} needs Vibezeo Pro."),
                detail: "The plan is Free; no audio was sent.".into(),
            });
        }
        let upload = SttUpload {
            mime: "audio/wav".into(),
            audio: wav16k(req.pcm16k),
            duration_ms: Some(req.pcm16k.len() as u64 / 16),
            language: req.language,
            hints: req.hints,
        };
        self.rt
            .block_on((self.call)(upload))
            .map(|t| t.text)
            .map_err(|e| failure(&e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    fn rig(
        plan: Plan,
        reply: Result<Transcript, ApiError>,
    ) -> (
        HostedStt,
        Arc<Mutex<Vec<SttUpload>>>,
        tokio::runtime::Runtime,
    ) {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("rt");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let reply = Arc::new(reply);
        let call: SttCall = Arc::new(move |u: SttUpload| {
            log.lock().push(u);
            let reply = reply.clone();
            Box::pin(async move { (*reply).clone() })
        });
        let stt = HostedStt::new(Arc::new(move || plan), call, rt.handle().clone());
        (stt, seen, rt)
    }

    fn take() -> Vec<f32> {
        vec![0.1; 16_000]
    }

    #[test]
    fn f_di_05_1_a_pro_take_uploads_wav_with_duration_and_returns_the_text() {
        let (stt, seen, _rt) = rig(
            Plan::Pro,
            Ok(Transcript {
                text: "hello world".into(),
                language: Some("en".into()),
                duration: 1.0,
            }),
        );
        let pcm = take();
        let got = stt.transcribe(CloudRequest {
            pcm16k: &pcm,
            language: Some("en".into()),
            hints: vec!["Tandem".into()],
        });
        assert_eq!(got.as_deref(), Ok("hello world"));
        let up = seen.lock()[0].clone();
        assert_eq!(up.mime, "audio/wav");
        assert_eq!(up.duration_ms, Some(1_000));
        assert_eq!(up.audio.len(), 44 + 32_000);
        assert_eq!(up.language.as_deref(), Some("en"));
        assert_eq!(up.hints, vec!["Tandem".to_string()]);
    }

    #[test]
    fn f_di_05_free_is_refused_before_any_audio_leaves() {
        let (stt, seen, _rt) = rig(
            Plan::Free,
            Ok(Transcript {
                text: "x".into(),
                language: None,
                duration: 0.0,
            }),
        );
        let pcm = take();
        let err = stt
            .transcribe(CloudRequest {
                pcm16k: &pcm,
                language: None,
                hints: vec![],
            })
            .expect_err("free");
        assert_eq!(err.kind, CloudFailureKind::NeedsPro);
        assert_eq!(err.message, "Cloud transcription needs Vibezeo Pro.");
        assert!(seen.lock().is_empty(), "nothing uploaded");
    }

    #[test]
    fn f_di_05_2_relay_errors_name_the_limit() {
        let http = |status: u16, code: &str, detail: Option<&str>| ApiError::Http {
            status,
            code: code.into(),
            detail: detail.map(str::to_string),
        };
        let cases = [
            (
                http(
                    402,
                    "CREDITS_EXHAUSTED",
                    Some("Out of credits for cloud transcription"),
                ),
                CloudFailureKind::OutOfCredits,
                "Out of credits for cloud transcription",
            ),
            (
                http(
                    429,
                    "DAILY_LIMIT",
                    Some("You've reached today's cloud limit of 120 minutes. Switch to Local."),
                ),
                CloudFailureKind::DailyLimit,
                "You've reached today's cloud limit of 120 minutes. Switch to Local.",
            ),
            (
                http(403, "FORBIDDEN_PLAN", None),
                CloudFailureKind::NeedsPro,
                "Cloud transcription needs Vibezeo Pro.",
            ),
            (
                ApiError::Network("refused".into()),
                CloudFailureKind::Offline,
                "Couldn't reach Vibezeo. Check your connection and try again.",
            ),
        ];
        for (err, kind, message) in cases {
            let f = failure(&err);
            assert_eq!((f.kind, f.message.as_str()), (kind, message));
        }
        let (stt, _seen, _rt) = rig(
            Plan::Pro,
            Err(http(
                402,
                "CREDITS_EXHAUSTED",
                Some("Out of credits for cloud transcription"),
            )),
        );
        let pcm = take();
        let err = stt
            .transcribe(CloudRequest {
                pcm16k: &pcm,
                language: None,
                hints: vec![],
            })
            .expect_err("402");
        assert_eq!(err.kind, CloudFailureKind::OutOfCredits);
        assert_eq!(err.detail, "HTTP 402 CREDITS_EXHAUSTED");
    }
}
