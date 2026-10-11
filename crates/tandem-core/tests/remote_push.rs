//! F-RM-06.2: sealed push alerts match the TypeScript bytes
//! (`fixtures/remote_push_v1.json`, made by `packages/remote/src/push.ts`).

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::Value;
use tandem_core::remote::protocol::Secret32;
use tandem_core::remote::push::{open_push, seal_push, seal_push_with, PushAlert};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/remote_push_v1.json")).expect("fixture")
}

fn bytes(v: &Value) -> Vec<u8> {
    URL_SAFE_NO_PAD
        .decode(v.as_str().expect("str"))
        .expect("b64")
}

#[test]
fn f_rm_06_2_push_frames_match_the_typescript_vectors() {
    let f = fixture();
    let key = Secret32(bytes(&f["key"]).try_into().expect("32"));
    let nonce: [u8; 12] = bytes(&f["nonce"]).try_into().expect("12");
    let pid = f["pid"].as_str().expect("pid");
    let plain = f["plaintext"].as_str().expect("plaintext").as_bytes();
    let frame = seal_push_with(pid, &key, 7, plain, nonce).expect("seal");
    assert_eq!(
        URL_SAFE_NO_PAD.encode(&frame),
        f["frame"].as_str().expect("frame")
    );
    let (seq, opened) = open_push(pid, &key, &frame).expect("open");
    assert_eq!(seq, 7);
    let alert: PushAlert = serde_json::from_slice(&opened).expect("alert");
    assert_eq!(alert.title, "Approval needed");
}

#[test]
fn f_rm_06_2_only_the_key_and_pairing_open_a_push() {
    let key = Secret32([9; 32]);
    let frame = seal_push("pr_a", &key, 1, b"hello").expect("seal");
    assert!(open_push("pr_b", &key, &frame).is_err(), "another pairing");
    assert!(
        open_push("pr_a", &Secret32([8; 32]), &frame).is_err(),
        "another key"
    );
    let mut tampered = frame.clone();
    tampered[5] ^= 1; // the sequence number is authenticated
    assert!(open_push("pr_a", &key, &tampered).is_err());
    assert!(
        seal_push("pr_a", &key, 2, &[0u8; 4096]).is_err(),
        "too large"
    );
}
