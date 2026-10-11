//! F-RM-03: the phone protocol's bytes match the interop vectors the PWA's
//! `apps/web/lib/remote/protocol.test.ts` checks too (D-1905), and the
//! session rules hold: counters, replay, epochs and rekeying.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::Value;
use tandem_core::remote::protocol::{
    decode_hello, encode_hello, hello_tag, read_header, root_secret, seal, verify_hello, Dir,
    EpochKeys, KeyPair, ProtocolError, Role, Secret32, Session, MAX_FRAME, MAX_PLAINTEXT,
};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/remote_v1.json")).expect("fixture")
}

fn bytes(v: &Value) -> Vec<u8> {
    URL_SAFE_NO_PAD
        .decode(v.as_str().expect("string"))
        .expect("base64url")
}

fn key(v: &Value) -> [u8; 32] {
    bytes(v).try_into().expect("32 bytes")
}

fn b64(b: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(b)
}

#[test]
fn f_rm_03_matches_the_interop_vectors() {
    let f = fixture();
    let i = &f["inputs"];
    let pid = i["pid"].as_str().expect("pid");
    let d = KeyPair::from_secret(Secret32(key(&i["desktopSecret"])));
    let p = KeyPair::from_secret(Secret32(key(&i["phoneSecret"])));
    let s = Secret32(key(&i["s"]));
    assert_eq!(b64(&d.public), f["desktopPublic"]);
    assert_eq!(b64(&p.public), f["phonePublic"]);

    let tag = hello_tag(&s, pid, &p.public);
    assert_eq!(b64(&tag), f["helloTag"]);
    assert_eq!(b64(&encode_hello(&p.public, &tag)), f["hello"]);
    let (pk, t) = decode_hello(&bytes(&f["hello"])).expect("hello");
    verify_hello(&s, pid, &pk, &t).expect("tag verifies");

    let root = root_secret(&s, pid, &d.public, &p.public, &d.secret, &p.public).expect("root");
    assert_eq!(b64(&root.0), f["root"]);
    let phone_root =
        root_secret(&s, pid, &d.public, &p.public, &p.secret, &d.public).expect("root");
    assert_eq!(phone_root, root, "both ends derive the same root");

    let e0 = EpochKeys::derive(0, root);
    assert_eq!(b64(&e0.d2p.0), f["epoch0"]["d2p"]);
    assert_eq!(b64(&e0.p2d.0), f["epoch0"]["p2d"]);

    let r: [u8; 16] = bytes(&i["random16"]).try_into().expect("16 bytes");
    let d2p = seal(
        pid,
        &e0.d2p,
        0,
        Dir::D2p,
        1,
        i["plaintextD2p"].as_str().expect("text").as_bytes(),
        r,
    )
    .expect("seal");
    assert_eq!(b64(&d2p), f["sealedD2pCtr1"]);
    let p2d = seal(
        pid,
        &e0.p2d,
        0,
        Dir::P2d,
        1,
        i["plaintextP2d"].as_str().expect("text").as_bytes(),
        r,
    )
    .expect("seal");
    assert_eq!(b64(&p2d), f["sealedP2dCtr1"]);

    let de = KeyPair::from_secret(Secret32(key(&i["desktopEph"])));
    let pe = KeyPair::from_secret(Secret32(key(&i["phoneEph"])));
    assert_eq!(b64(&de.public), f["desktopEphPublic"]);
    assert_eq!(b64(&pe.public), f["phoneEphPublic"]);
    let e1 = e0.next(&de.secret, &pe.public).expect("rekey");
    assert_eq!(e0.next(&pe.secret, &de.public).expect("rekey"), e1);
    assert_eq!(b64(&e1.secret.0), f["epoch1"]["secret"]);
    assert_eq!(b64(&e1.d2p.0), f["epoch1"]["d2p"]);
    assert_eq!(b64(&e1.p2d.0), f["epoch1"]["p2d"]);
    let sealed = seal(
        pid,
        &e1.d2p,
        1,
        Dir::D2p,
        7,
        i["plaintextD2p"].as_str().expect("text").as_bytes(),
        r,
    )
    .expect("seal");
    assert_eq!(b64(&sealed), f["sealedD2pEpoch1Ctr7"]);

    // The TypeScript-sealed phone frame opens here.
    let mut desktop = Session::new(
        pid,
        Role::Desktop,
        root_secret(&s, pid, &d.public, &p.public, &d.secret, &p.public).expect("root"),
    );
    let opened = desktop
        .open_next(&bytes(&f["sealedP2dCtr1"]))
        .expect("opens");
    assert_eq!(opened, i["plaintextP2d"].as_str().expect("text").as_bytes());
}

fn pair() -> (Session, Session, Secret32, KeyPair, KeyPair) {
    let pid = "pr_testpair01";
    let d = KeyPair::generate();
    let p = KeyPair::generate();
    let s = Secret32::random();
    let dr = root_secret(&s, pid, &d.public, &p.public, &d.secret, &p.public).expect("root");
    let pr = root_secret(&s, pid, &d.public, &p.public, &p.secret, &d.public).expect("root");
    (
        Session::new(pid, Role::Desktop, dr),
        Session::new(pid, Role::Phone, pr),
        s,
        d,
        p,
    )
}

#[test]
fn f_rm_03_hello_without_the_qr_secret_is_refused() {
    let (_, _, s, _, p) = pair();
    let tag = hello_tag(&s, "pr_testpair01", &p.public);
    assert_eq!(verify_hello(&s, "pr_testpair01", &p.public, &tag), Ok(()));
    let other = Secret32::random();
    assert_eq!(
        verify_hello(&other, "pr_testpair01", &p.public, &tag),
        Err(ProtocolError::BadTag)
    );
    assert_eq!(
        verify_hello(&s, "pr_testpair02", &p.public, &tag),
        Err(ProtocolError::BadTag)
    );
    assert_eq!(decode_hello(&[1, 1, 0]), Err(ProtocolError::BadFrame));
}

#[test]
fn f_rm_03_an_all_zero_shared_secret_is_refused() {
    let s = Secret32::random();
    let d = KeyPair::generate();
    let low_order = [0u8; 32];
    assert_eq!(
        root_secret(
            &s,
            "pr_testpair01",
            &d.public,
            &low_order,
            &d.secret,
            &low_order
        ),
        Err(ProtocolError::BadKey)
    );
}

#[test]
fn f_rm_03_2_a_replayed_command_frame_is_rejected() {
    let (mut desktop, mut phone, ..) = pair();
    let frame = phone
        .seal_next(br#"{"t":"cmd","id":1,"cmd":"view.snapshot"}"#)
        .expect("seal");
    desktop.open_next(&frame).expect("first time");
    assert_eq!(desktop.open_next(&frame), Err(ProtocolError::Replay));
}

#[test]
fn f_rm_03_2_out_of_order_frames_are_rejected_and_failures_change_nothing() {
    let (mut desktop, mut phone, ..) = pair();
    let one = phone.seal_next(b"1").expect("seal");
    let two = phone.seal_next(b"2").expect("seal");
    desktop.open_next(&two).expect("newer first");
    assert_eq!(desktop.open_next(&one), Err(ProtocolError::Replay));
    let mut tampered = phone.seal_next(b"3").expect("seal");
    let last = tampered.len() - 1;
    tampered[last] ^= 1;
    let before = desktop.clone();
    assert_eq!(desktop.open_next(&tampered), Err(ProtocolError::BadFrame));
    assert_eq!(desktop, before);
    // A desktop frame sent back at the desktop is the wrong direction.
    let own = desktop.seal_next(b"echo").expect("seal");
    assert_eq!(desktop.open_next(&own), Err(ProtocolError::BadFrame));
}

#[test]
fn f_rm_03_frames_stay_within_64_kb() {
    let (_, mut phone, ..) = pair();
    let max = phone.seal_next(&vec![0u8; MAX_PLAINTEXT]).expect("seal");
    assert!(max.len() <= MAX_FRAME);
    assert_eq!(
        phone.seal_next(&vec![0u8; MAX_PLAINTEXT + 1]),
        Err(ProtocolError::TooLarge)
    );
    assert_eq!(
        read_header(&vec![2u8; MAX_FRAME + 1]),
        Err(ProtocolError::BadFrame)
    );
}

#[test]
fn f_rm_03_rekeying_keeps_counters_increasing_and_old_frames_stay_replays() {
    let (mut desktop, mut phone, ..) = pair();
    let old = phone.seal_next(b"before").expect("seal");
    desktop.open_next(&old).expect("open");
    // Desktop starts; the phone answers under the old keys and holds the next.
    let eph = KeyPair::generate();
    let rekey = desktop.seal_next(b"rekey").expect("seal");
    phone.open_next(&rekey).expect("open");
    let phone_eph = phone.accept_rekey(&eph.public).expect("accept");
    let ack = phone.seal_next(b"ack").expect("seal");
    assert_eq!(read_header(&ack).expect("header").epoch, 0);
    desktop.open_next(&ack).expect("open");
    desktop
        .complete_rekey(&eph.secret, &phone_eph)
        .expect("complete");
    assert_eq!(desktop.tx_epoch(), 1);
    // A phone frame still in epoch 0 is accepted until the phone switches.
    let late = phone.seal_next(b"late").expect("seal");
    desktop.open_next(&late).expect("old epoch still accepted");
    let first_new = desktop.seal_next(b"changed").expect("seal");
    assert_eq!(read_header(&first_new).expect("header").epoch, 1);
    phone.open_next(&first_new).expect("phone switches");
    assert_eq!(phone.current.epoch, 1);
    assert!(phone.next.is_none());
    let reply = phone.seal_next(b"after").expect("seal");
    desktop.open_next(&reply).expect("open");
    assert_eq!(desktop.current.epoch, 1);
    assert_eq!(desktop.rx_ctr, 4);
    assert_eq!(desktop.open_next(&old), Err(ProtocolError::Replay));
    // Epoch 0 is gone now: a fresh-counter frame under it is refused.
    let mut stale = phone.clone();
    stale.current = EpochKeys::derive(0, Secret32::random());
    stale.tx_ctr = 100;
    let frame = stale.seal_next(b"stale").expect("seal");
    assert_eq!(desktop.open_next(&frame), Err(ProtocolError::Epoch));
}

#[test]
fn f_rm_03_a_session_survives_the_keychain_round_trip() {
    let (desktop, ..) = pair();
    let json = serde_json::to_string(&desktop).expect("json");
    assert!(!json.contains("Secret32"));
    let back: Session = serde_json::from_str(&json).expect("back");
    assert_eq!(back, desktop);
    assert_eq!(format!("{:?}", desktop.current.d2p), "Secret32(…)");
}
