//! The phone companion's wire protocol v1 (F-RM-03, ARCHITECTURE §13.3,
//! D-1005): the QR handshake, sealed frames with monotonic counters, and
//! rekeying. `apps/web/lib/remote/protocol.ts` produces the same bytes;
//! `tests/fixtures/remote_v1.json` pins both.

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::KeyInit;
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroize;

pub const VERSION: u8 = 1;
pub const KIND_HELLO: u8 = 0x01;
pub const KIND_SEALED: u8 = 0x02;
pub const HEADER_LEN: usize = 39;
/// The largest plaintext a sealed frame carries, so frames stay under 64 KB.
pub const MAX_PLAINTEXT: usize = 61_440;
pub const MAX_FRAME: usize = 64 * 1024;
const HELLO_LEN: usize = 66;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Desktop,
    Phone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    D2p,
    P2d,
}

impl Dir {
    fn byte(self) -> u8 {
        match self {
            Dir::D2p => 0,
            Dir::P2d => 1,
        }
    }
}

/// Why a frame was refused. `as_str` is the `reason` the desktop reports back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    #[error("the frame's counter isn't newer than the last one")]
    Replay,
    #[error("the frame's epoch is neither the current nor the next")]
    Epoch,
    #[error("the frame is malformed or doesn't authenticate")]
    BadFrame,
    #[error("the message is over the frame limit")]
    TooLarge,
    #[error("the pairing code didn't match")]
    BadTag,
    #[error("the key is unusable")]
    BadKey,
}

impl ProtocolError {
    pub fn as_str(self) -> &'static str {
        match self {
            ProtocolError::Replay => "replay",
            ProtocolError::Epoch => "epoch",
            ProtocolError::BadFrame => "bad_frame",
            ProtocolError::TooLarge => "too_large",
            ProtocolError::BadTag => "bad_tag",
            ProtocolError::BadKey => "bad_key",
        }
    }
}

/// 32 secret bytes that are wiped on drop and never printed.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret32(#[serde(with = "b64")] pub [u8; 32]);

impl std::fmt::Debug for Secret32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret32(…)")
    }
}

impl Drop for Secret32 {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl Secret32 {
    pub fn random() -> Self {
        let mut b = [0u8; 32];
        OsRng.fill_bytes(&mut b);
        Secret32(b)
    }
}

/// An X25519 key pair.
pub struct KeyPair {
    pub secret: Secret32,
    pub public: [u8; 32],
}

impl KeyPair {
    pub fn generate() -> Self {
        Self::from_secret(Secret32::random())
    }

    pub fn from_secret(secret: Secret32) -> Self {
        let public = PublicKey::from(&StaticSecret::from(secret.0)).to_bytes();
        KeyPair { secret, public }
    }
}

fn shared(secret: &Secret32, peer: &[u8; 32]) -> Result<[u8; 32], ProtocolError> {
    let ss = StaticSecret::from(secret.0).diffie_hellman(&PublicKey::from(*peer));
    if !ss.was_contributory() {
        return Err(ProtocolError::BadKey);
    }
    Ok(ss.to_bytes())
}

fn hkdf32(salt: Option<&[u8]>, ikm: &[u8], info: &[&[u8]]) -> [u8; 32] {
    let info: Vec<u8> = info.concat();
    let mut out = [0u8; 32];
    // 32 bytes is always a valid HKDF-SHA256 output length.
    let _ = Hkdf::<Sha256>::new(salt, ikm).expand(&info, &mut out);
    out
}

fn tag_mac(s: &Secret32, pid: &str, phone_pub: &[u8; 32]) -> Hmac<Sha256> {
    // HMAC zero-pads a key shorter than its block, so this is HMAC(S, …).
    let mut block = [0u8; 64];
    block[..32].copy_from_slice(&s.0);
    let mut mac = <Hmac<Sha256> as KeyInit>::new(&block.into());
    block.zeroize();
    mac.update(b"tandem-pair-v1");
    mac.update(pid.as_bytes());
    mac.update(phone_pub);
    mac
}

/// The QR secret's proof that the phone scanned the code.
pub fn hello_tag(s: &Secret32, pid: &str, phone_pub: &[u8; 32]) -> [u8; 32] {
    tag_mac(s, pid, phone_pub).finalize().into_bytes().into()
}

/// Checks a phone's hello against the QR secret in constant time.
pub fn verify_hello(
    s: &Secret32,
    pid: &str,
    phone_pub: &[u8; 32],
    tag: &[u8; 32],
) -> Result<(), ProtocolError> {
    tag_mac(s, pid, phone_pub)
        .verify_slice(tag)
        .map_err(|_| ProtocolError::BadTag)
}

/// `root = HKDF(salt = S, ikm = X25519(mine, peer), info = "tandem-root-v1" ‖ pid ‖ D.pub ‖ P.pub)`.
pub fn root_secret(
    s: &Secret32,
    pid: &str,
    desktop_pub: &[u8; 32],
    phone_pub: &[u8; 32],
    mine: &Secret32,
    peer: &[u8; 32],
) -> Result<Secret32, ProtocolError> {
    let ss = shared(mine, peer)?;
    Ok(Secret32(hkdf32(
        Some(&s.0),
        &ss,
        &[b"tandem-root-v1", pid.as_bytes(), desktop_pub, phone_pub],
    )))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpochKeys {
    pub epoch: u32,
    pub secret: Secret32,
    pub d2p: Secret32,
    pub p2d: Secret32,
}

impl EpochKeys {
    pub fn derive(epoch: u32, secret: Secret32) -> Self {
        let d2p = Secret32(hkdf32(None, &secret.0, &[b"tandem-d2p-v1"]));
        let p2d = Secret32(hkdf32(None, &secret.0, &[b"tandem-p2d-v1"]));
        EpochKeys {
            epoch,
            secret,
            d2p,
            p2d,
        }
    }

    fn key(&self, dir: Dir) -> &Secret32 {
        match dir {
            Dir::D2p => &self.d2p,
            Dir::P2d => &self.p2d,
        }
    }

    /// `secretₑ₊₁ = HKDF(salt = secretₑ, ikm = X25519(mine, peer), info = "tandem-rekey-v1" ‖ (e+1))`.
    pub fn next(&self, mine: &Secret32, peer: &[u8; 32]) -> Result<EpochKeys, ProtocolError> {
        let ss = shared(mine, peer)?;
        let epoch = self.epoch.checked_add(1).ok_or(ProtocolError::Epoch)?;
        let secret = hkdf32(
            Some(&self.secret.0),
            &ss,
            &[b"tandem-rekey-v1", &epoch.to_be_bytes()],
        );
        Ok(EpochKeys::derive(epoch, Secret32(secret)))
    }
}

pub fn encode_hello(phone_pub: &[u8; 32], tag: &[u8; 32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HELLO_LEN);
    out.extend_from_slice(&[KIND_HELLO, VERSION]);
    out.extend_from_slice(phone_pub);
    out.extend_from_slice(tag);
    out
}

pub fn decode_hello(frame: &[u8]) -> Result<([u8; 32], [u8; 32]), ProtocolError> {
    if frame.len() != HELLO_LEN || frame[0] != KIND_HELLO || frame[1] != VERSION {
        return Err(ProtocolError::BadFrame);
    }
    let mut pk = [0u8; 32];
    let mut tag = [0u8; 32];
    pk.copy_from_slice(&frame[2..34]);
    tag.copy_from_slice(&frame[34..66]);
    Ok((pk, tag))
}

/// Which kind of frame this is, without opening it.
pub fn frame_kind(frame: &[u8]) -> Option<u8> {
    frame.first().copied()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub epoch: u32,
    pub dir: Dir,
    pub ctr: u64,
    pub nonce: [u8; 24],
}

fn aad(pid: &str, epoch: u32, dir: Dir, ctr: u64) -> Vec<u8> {
    let mut a = Vec::with_capacity(pid.len() + 13);
    a.extend_from_slice(pid.as_bytes());
    a.extend_from_slice(&epoch.to_be_bytes());
    a.push(dir.byte());
    a.extend_from_slice(&ctr.to_be_bytes());
    a
}

/// Seals one frame. `random16` is the nonce's random tail (tests pin it).
pub fn seal(
    pid: &str,
    key: &Secret32,
    epoch: u32,
    dir: Dir,
    ctr: u64,
    plaintext: &[u8],
    random16: [u8; 16],
) -> Result<Vec<u8>, ProtocolError> {
    if plaintext.len() > MAX_PLAINTEXT {
        return Err(ProtocolError::TooLarge);
    }
    let mut nonce = [0u8; 24];
    nonce[..8].copy_from_slice(&ctr.to_le_bytes());
    nonce[8..].copy_from_slice(&random16);
    let cipher = XChaCha20Poly1305::new((&key.0).into());
    let ct = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &aad(pid, epoch, dir, ctr),
            },
        )
        .map_err(|_| ProtocolError::BadFrame)?;
    let mut out = Vec::with_capacity(HEADER_LEN + ct.len());
    out.extend_from_slice(&[KIND_SEALED, VERSION]);
    out.extend_from_slice(&epoch.to_be_bytes());
    out.push(dir.byte());
    out.extend_from_slice(&ctr.to_be_bytes());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

pub fn read_header(frame: &[u8]) -> Result<Header, ProtocolError> {
    if frame.len() < HEADER_LEN + 16 || frame.len() > MAX_FRAME {
        return Err(ProtocolError::BadFrame);
    }
    if frame[0] != KIND_SEALED || frame[1] != VERSION {
        return Err(ProtocolError::BadFrame);
    }
    let dir = match frame[6] {
        0 => Dir::D2p,
        1 => Dir::P2d,
        _ => return Err(ProtocolError::BadFrame),
    };
    let mut e = [0u8; 4];
    e.copy_from_slice(&frame[2..6]);
    let mut c = [0u8; 8];
    c.copy_from_slice(&frame[7..15]);
    let ctr = u64::from_be_bytes(c);
    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&frame[15..39]);
    // The nonce starts with the counter, so a frame can't reuse another's nonce.
    if nonce[..8] != ctr.to_le_bytes() {
        return Err(ProtocolError::BadFrame);
    }
    Ok(Header {
        epoch: u32::from_be_bytes(e),
        dir,
        ctr,
        nonce,
    })
}

pub fn open(pid: &str, key: &Secret32, h: &Header, frame: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    let cipher = XChaCha20Poly1305::new((&key.0).into());
    cipher
        .decrypt(
            XNonce::from_slice(&h.nonce),
            Payload {
                msg: &frame[HEADER_LEN..],
                aad: &aad(pid, h.epoch, h.dir, h.ctr),
            },
        )
        .map_err(|_| ProtocolError::BadFrame)
}

/// Everything a side keeps between connections: the keys (keychain) and the
/// counters (`pairings.tx_counter` / `rx_counter`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub pid: String,
    pub role: Role,
    pub current: EpochKeys,
    pub next: Option<EpochKeys>,
    /// The desktop sends under `next` once the phone acknowledged the rekey.
    pub tx_on_next: bool,
    pub tx_ctr: u64,
    pub rx_ctr: u64,
}

impl Session {
    pub fn new(pid: &str, role: Role, root: Secret32) -> Self {
        Session {
            pid: pid.to_string(),
            role,
            current: EpochKeys::derive(0, root),
            next: None,
            tx_on_next: false,
            tx_ctr: 0,
            rx_ctr: 0,
        }
    }

    fn tx_dir(&self) -> Dir {
        match self.role {
            Role::Desktop => Dir::D2p,
            Role::Phone => Dir::P2d,
        }
    }

    fn rx_dir(&self) -> Dir {
        match self.role {
            Role::Desktop => Dir::P2d,
            Role::Phone => Dir::D2p,
        }
    }

    /// The epoch new frames go out under.
    pub fn tx_epoch(&self) -> u32 {
        match (&self.next, self.tx_on_next) {
            (Some(n), true) => n.epoch,
            _ => self.current.epoch,
        }
    }

    /// Seals the next message, advancing the send counter.
    pub fn seal_next(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, ProtocolError> {
        let mut r = [0u8; 16];
        OsRng.fill_bytes(&mut r);
        self.seal_with(plaintext, r)
    }

    pub fn seal_with(
        &mut self,
        plaintext: &[u8],
        random16: [u8; 16],
    ) -> Result<Vec<u8>, ProtocolError> {
        let keys = match (&self.next, self.tx_on_next) {
            (Some(n), true) => n,
            _ => &self.current,
        };
        let dir = self.tx_dir();
        let ctr = self.tx_ctr.checked_add(1).ok_or(ProtocolError::TooLarge)?;
        let frame = seal(
            &self.pid,
            keys.key(dir),
            keys.epoch,
            dir,
            ctr,
            plaintext,
            random16,
        )?;
        self.tx_ctr = ctr;
        Ok(frame)
    }

    /// Opens a frame from the peer. A counter at or below the last one is a
    /// replay (AC2); an epoch other than the current or next is refused. The
    /// session changes only when the frame authenticates.
    pub fn open_next(&mut self, frame: &[u8]) -> Result<Vec<u8>, ProtocolError> {
        let h = read_header(frame)?;
        if h.dir != self.rx_dir() {
            return Err(ProtocolError::BadFrame);
        }
        if h.ctr <= self.rx_ctr {
            return Err(ProtocolError::Replay);
        }
        let promote = if h.epoch == self.current.epoch {
            false
        } else if self.next.as_ref().is_some_and(|n| n.epoch == h.epoch) {
            true
        } else {
            return Err(ProtocolError::Epoch);
        };
        let keys = if promote {
            self.next.as_ref().ok_or(ProtocolError::Epoch)?
        } else {
            &self.current
        };
        let plaintext = open(&self.pid, keys.key(h.dir), &h, frame)?;
        if promote {
            if let Some(next) = self.next.take() {
                self.current = next;
            }
            self.tx_on_next = false;
        }
        self.rx_ctr = h.ctr;
        Ok(plaintext)
    }

    /// The phone's answer to `{t:"rekey"}`: the next keys, held until the
    /// desktop sends under them. Returns the phone's ephemeral public key.
    pub fn accept_rekey(&mut self, desktop_eph: &[u8; 32]) -> Result<[u8; 32], ProtocolError> {
        let mine = KeyPair::generate();
        self.next = Some(self.current.next(&mine.secret, desktop_eph)?);
        self.tx_on_next = false;
        Ok(mine.public)
    }

    /// The desktop's side once the phone acknowledged: send under the next
    /// keys from now on, and accept both epochs until the phone switches.
    pub fn complete_rekey(
        &mut self,
        my_eph: &Secret32,
        phone_eph: &[u8; 32],
    ) -> Result<(), ProtocolError> {
        self.next = Some(self.current.next(my_eph, phone_eph)?);
        self.tx_on_next = true;
        Ok(())
    }
}

mod b64 {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&URL_SAFE_NO_PAD.encode(v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let text = String::deserialize(d)?;
        let bytes = URL_SAFE_NO_PAD
            .decode(text)
            .map_err(serde::de::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("expected 32 bytes"))
    }
}
