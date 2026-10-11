//! Sealed push alerts (F-RM-06, D-2021). When push is on, the phone gives
//! the desktop (inside the encrypted channel) its push token and a random
//! 32-byte push key. The desktop seals each alert with ChaCha20-Poly1305
//! (RFC 8439), which iOS's CryptoKit opens in the Notification Service
//! Extension, and the API forwards the bytes unread.
//!
//! `PUSH = 0x03 ‖ 0x01 ‖ seq u64 BE ‖ nonce (12) ‖ ct ‖ tag (16)`,
//! `AAD = "tandem-push-v1" ‖ pid ‖ seq u64 BE`. `apps/mobile`'s and
//! `packages/remote`'s `push.ts` produce the same bytes;
//! `tests/fixtures/remote_push_v1.json` pins both.

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};

use super::protocol::{ProtocolError, Secret32, VERSION};

pub const KIND_PUSH: u8 = 0x03;
pub const PUSH_HEADER_LEN: usize = 2 + 8 + 12;
/// Sealed alerts stay small: FCM data messages hold 4 KB and the API takes
/// at most 3,000 base64url characters.
pub const MAX_PUSH_PLAINTEXT: usize = 2048;
const LABEL: &[u8] = b"tandem-push-v1";

/// What a push alert says, inside the seal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushAlert {
    /// `approval` or `agent`.
    pub kind: String,
    /// The approval id or agent key (so the phone can de-duplicate).
    pub id: String,
    pub title: String,
    pub body: String,
    pub at: i64,
}

fn aad(pid: &str, seq: u64) -> Vec<u8> {
    let mut a = Vec::with_capacity(LABEL.len() + pid.len() + 8);
    a.extend_from_slice(LABEL);
    a.extend_from_slice(pid.as_bytes());
    a.extend_from_slice(&seq.to_be_bytes());
    a
}

/// Seals an alert. `nonce` is random in the app; tests pin it.
pub fn seal_push_with(
    pid: &str,
    key: &Secret32,
    seq: u64,
    plaintext: &[u8],
    nonce: [u8; 12],
) -> Result<Vec<u8>, ProtocolError> {
    if plaintext.len() > MAX_PUSH_PLAINTEXT {
        return Err(ProtocolError::TooLarge);
    }
    let ct = ChaCha20Poly1305::new((&key.0).into())
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &aad(pid, seq),
            },
        )
        .map_err(|_| ProtocolError::BadFrame)?;
    let mut out = Vec::with_capacity(PUSH_HEADER_LEN + ct.len());
    out.extend_from_slice(&[KIND_PUSH, VERSION]);
    out.extend_from_slice(&seq.to_be_bytes());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

pub fn seal_push(
    pid: &str,
    key: &Secret32,
    seq: u64,
    plaintext: &[u8],
) -> Result<Vec<u8>, ProtocolError> {
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    seal_push_with(pid, key, seq, plaintext, nonce)
}

/// Opens an alert; returns its sequence number and plaintext.
pub fn open_push(pid: &str, key: &Secret32, frame: &[u8]) -> Result<(u64, Vec<u8>), ProtocolError> {
    if frame.len() < PUSH_HEADER_LEN + 16 || frame[0] != KIND_PUSH || frame[1] != VERSION {
        return Err(ProtocolError::BadFrame);
    }
    let mut seq_bytes = [0u8; 8];
    seq_bytes.copy_from_slice(&frame[2..10]);
    let seq = u64::from_be_bytes(seq_bytes);
    let plain = ChaCha20Poly1305::new((&key.0).into())
        .decrypt(
            Nonce::from_slice(&frame[10..22]),
            Payload {
                msg: &frame[22..],
                aad: &aad(pid, seq),
            },
        )
        .map_err(|_| ProtocolError::BadFrame)?;
    Ok((seq, plain))
}
