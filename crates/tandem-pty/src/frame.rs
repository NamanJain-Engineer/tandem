//! Binary frames over the per-pane channel (ARCHITECTURE §3.4):
//! `u8 kind | u8[3] reserved | u64 seq (LE) | payload`.

use crate::ExitInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameKind {
    Data = 1,
    Exit = 2,
    Notice = 3,
    ReplayBegin = 4,
    ReplayEnd = 5,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyFrame {
    pub kind: FrameKind,
    pub seq: u64,
    pub payload: Vec<u8>,
}

impl PtyFrame {
    pub fn data(seq: u64, payload: Vec<u8>) -> Self {
        Self {
            kind: FrameKind::Data,
            seq,
            payload,
        }
    }

    pub fn exit(seq: u64, info: &ExitInfo) -> Self {
        Self {
            kind: FrameKind::Exit,
            seq,
            payload: serde_json::to_vec(info).unwrap_or_default(),
        }
    }

    pub fn notice(seq: u64, message: &str) -> Self {
        Self {
            kind: FrameKind::Notice,
            seq,
            payload: serde_json::to_vec(&serde_json::json!({ "message": message }))
                .unwrap_or_default(),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(12 + self.payload.len());
        out.push(self.kind as u8);
        out.extend_from_slice(&[0, 0, 0]);
        out.extend_from_slice(&self.seq.to_le_bytes());
        out.extend_from_slice(&self.payload);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_encode_kind_seq_and_payload() {
        let bytes = PtyFrame::data(258, b"hi".to_vec()).encode();
        assert_eq!(bytes[0], 1);
        assert_eq!(&bytes[1..4], &[0, 0, 0]);
        assert_eq!(
            u64::from_le_bytes(bytes[4..12].try_into().expect("8 bytes")),
            258
        );
        assert_eq!(&bytes[12..], b"hi");
    }
}
