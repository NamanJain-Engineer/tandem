//! The Conductor's realtime voice client (F-CN-01, ARCHITECTURE §12.6).
//!
//! The talker is a realtime speech model the app reaches directly over a
//! WebSocket, with the ephemeral token `POST /v1/voice/session` minted. A
//! [`RealtimeProvider`] turns its wire protocol into the normalized
//! [`ClientEvent`] and [`ServerEvent`]s, so the call never sees a vendor's
//! event names. [`mock::MockRealtime`] speaks the dev mock's protocol
//! (`scripts/mock-realtime.ts`); the vendor's provider needs a real key
//! (`HUMAN_TODO.md`, REALTIME_API_KEY).
//!
//! While the socket is being set up, the mic fills a [`buffer::PreConnect`]
//! buffer of up to 3 s, sent once connected (F-CN-01 AC4).

pub mod buffer;
pub mod mock;
pub mod pcm;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::mpsc;

/// The talker's audio: PCM16, mono, 24 kHz, both ways.
pub const SAMPLE_RATE: u32 = 24_000;

/// What the app sends the talker.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientEvent {
    /// The session's instructions (persona, tone, verbosity, speed and the
    /// workspace summary, never file contents) and its one tool, `act`.
    Configure {
        instructions: String,
        voice: Option<String>,
    },
    /// Mic audio, PCM16 at [`SAMPLE_RATE`].
    Audio(Vec<i16>),
    /// The answer to an `act` call: what the doer said, for the talker to say.
    FunctionOutput { call_id: String, output: String },
    /// Something for the talker to say now (a notice, a question).
    Say(String),
}

/// What the talker sends the app, normalized across providers.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerEvent {
    /// The session is configured and listening.
    Ready,
    /// Speech, PCM16 at [`SAMPLE_RATE`].
    AudioDelta(Vec<i16>),
    /// What the user said (the provider's transcript).
    TranscriptUser(String),
    /// What the talker is saying.
    TranscriptAssistant(String),
    /// The talker hands an intent to the doer: `act {request}`.
    FunctionCall {
        call_id: String,
        name: String,
        args: Value,
    },
    /// The user started talking: stop playback at once (barge-in).
    SpeechStarted,
    /// The talker finished its turn.
    ResponseDone,
    Error(String),
    /// The socket closed.
    Closed,
}

/// The one tool the talker has (ARCHITECTURE §12.6).
pub const ACT_TOOL: &str = "act";

/// An open session: events to send, and events as they arrive. Dropping the
/// sender closes the socket.
pub struct Connection {
    pub tx: mpsc::Sender<ClientEvent>,
    pub rx: mpsc::Receiver<ServerEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RealtimeError {
    #[error("Couldn't connect to the voice service: {0}")]
    Connect(String),
    #[error("The voice service refused the session")]
    Refused,
}

#[async_trait]
pub trait RealtimeProvider: Send + Sync {
    /// Opens the socket with the ephemeral token. Returns once the
    /// handshake is done; events flow until either side closes.
    async fn connect(&self, ws_url: &str, token: &str) -> Result<Connection, RealtimeError>;
}

/// The provider for a session's `provider` name. Only the mock speaks a
/// protocol this build can check without a vendor key.
pub fn for_provider(name: &str) -> Option<Box<dyn RealtimeProvider>> {
    match name {
        "mock" => Some(Box::new(mock::MockRealtime)),
        _ => None,
    }
}
