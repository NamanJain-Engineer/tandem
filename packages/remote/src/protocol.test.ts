import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  acceptRekey,
  b64url,
  completeRekey,
  decodeHello,
  decodeJson,
  encodeJson,
  encodePairingLink,
  equalBytes,
  generateKeyPair,
  helloTag,
  MAX_FRAME,
  MAX_PLAINTEXT,
  newSession,
  openNext,
  parsePairingFragment,
  ProtocolError,
  rootSecret,
  sealNext,
  type SessionState,
} from "./protocol";
import { buildVectors } from "./vectors";

const fixture = JSON.parse(
  readFileSync(new URL("../../../crates/tandem-core/tests/fixtures/remote_v1.json", import.meta.url), "utf8"),
);

function paired(pid = "pr_testpair01") {
  const d = generateKeyPair();
  const p = generateKeyPair();
  const s = crypto.getRandomValues(new Uint8Array(32));
  const tag = helloTag(s, pid, p.public);
  const desktopRoot = rootSecret(s, pid, d.public, p.public, d.secret, p.public);
  const phoneRoot = rootSecret(s, pid, d.public, p.public, p.secret, d.public);
  return {
    s,
    tag,
    d,
    p,
    desktop: newSession(pid, "desktop", desktopRoot),
    phone: newSession(pid, "phone", phoneRoot),
  };
}

function expectReject(fn: () => unknown, reason: ProtocolError["reason"]) {
  try {
    fn();
  } catch (err) {
    expect(err).toBeInstanceOf(ProtocolError);
    expect((err as ProtocolError).reason).toBe(reason);
    return;
  }
  throw new Error(`expected ${reason}`);
}

function send(from: SessionState, to: SessionState, message: unknown) {
  const sealed = sealNext(from, encodeJson(message));
  const opened = openNext(to, sealed.frame);
  return { from: sealed.state, to: opened.state, frame: sealed.frame, message: decodeJson<unknown>(opened.plaintext) };
}

describe("F-RM-03 phone protocol v1", () => {
  it("F-RM-03 matches the interop vectors the Rust side checks too", () => {
    expect(buildVectors()).toEqual(fixture);
  });

  it("F-RM-03 both ends derive the same keys from the QR secret and X25519", () => {
    const { desktop, phone } = paired();
    expect(equalBytes(desktop.current.d2p, phone.current.d2p)).toBe(true);
    expect(equalBytes(desktop.current.p2d, phone.current.p2d)).toBe(true);
    expect(equalBytes(desktop.current.d2p, desktop.current.p2d)).toBe(false);
  });

  it("F-RM-03 a hello made without the QR secret fails the tag check", () => {
    const { s, p, tag } = paired();
    expect(equalBytes(helloTag(s, "pr_testpair01", p.public), tag)).toBe(true);
    const wrong = crypto.getRandomValues(new Uint8Array(32));
    expect(equalBytes(helloTag(wrong, "pr_testpair01", p.public), tag)).toBe(false);
    expect(equalBytes(helloTag(s, "pr_otherpair1", p.public), tag)).toBe(false);
    expectReject(() => decodeHello(new Uint8Array(10)), "bad_frame");
  });

  it("F-RM-03 messages round-trip both ways and the relay sees no plaintext", () => {
    let { desktop, phone } = paired();
    const sentence = "Ship the login fix before lunch";
    const a = send(phone, desktop, { t: "cmd", id: 1, cmd: "thread.send", args: { text: sentence } });
    [phone, desktop] = [a.from, a.to];
    expect(a.message).toMatchObject({ args: { text: sentence } });
    expect(new TextDecoder("latin1").decode(a.frame)).not.toContain(sentence);
    const b = send(desktop, phone, { t: "res", id: 1, ok: true });
    expect(b.message).toEqual({ t: "res", id: 1, ok: true });
  });

  it("F-RM-03.2 a replayed command frame is rejected", () => {
    const { desktop, phone } = paired();
    const first = sealNext(phone, encodeJson({ t: "cmd", id: 1, cmd: "view.snapshot", args: {} }));
    const opened = openNext(desktop, first.frame);
    expectReject(() => openNext(opened.state, first.frame), "replay");
  });

  it("F-RM-03.2 an out-of-order frame is rejected and doesn't move the counter", () => {
    const { desktop, phone } = paired();
    const one = sealNext(phone, encodeJson({ n: 1 }));
    const two = sealNext(one.state, encodeJson({ n: 2 }));
    const afterTwo = openNext(desktop, two.frame).state;
    expectReject(() => openNext(afterTwo, one.frame), "replay");
    expect(afterTwo.rxCtr).toBe(2n);
  });

  it("F-RM-03 a tampered frame, a frame for another pairing, or one in the wrong direction is refused", () => {
    const { desktop, phone } = paired();
    const sealed = sealNext(phone, encodeJson({ t: "hi", device: "Phone" }));
    const tampered = sealed.frame.slice();
    tampered[tampered.length - 1] = (tampered.at(-1) ?? 0) ^ 1;
    expectReject(() => openNext(desktop, tampered), "bad_frame");
    expectReject(() => openNext({ ...desktop, pid: "pr_testpair02" }, sealed.frame), "bad_frame");
    const fromDesktop = sealNext(desktop, encodeJson({ t: "changed" }));
    expectReject(() => openNext(desktop, fromDesktop.frame), "bad_frame");
    // A failed frame leaves the session as it was.
    expect(openNext(desktop, sealed.frame).state.rxCtr).toBe(1n);
  });

  it("F-RM-03 messages are limited so a frame stays within 64 KB", () => {
    const { phone } = paired();
    const max = sealNext(phone, new Uint8Array(MAX_PLAINTEXT));
    expect(max.frame.length).toBeLessThanOrEqual(MAX_FRAME);
    expectReject(() => sealNext(phone, new Uint8Array(MAX_PLAINTEXT + 1)), "too_large");
  });

  it("F-RM-03 rekeying moves both ends to the next epoch and counters keep increasing", () => {
    let { desktop, phone } = paired();
    for (let i = 0; i < 3; i++) {
      const r = send(phone, desktop, { n: i });
      [phone, desktop] = [r.from, r.to];
    }
    const eph = generateKeyPair();
    const rekey = send(desktop, phone, { t: "rekey", eph: b64url(eph.public) });
    [desktop, phone] = [rekey.from, rekey.to];
    const accepted = acceptRekey(phone, eph.public);
    phone = accepted.state;
    const ack = send(phone, desktop, { t: "rekey_ack", eph: b64url(accepted.eph) });
    [phone, desktop] = [ack.from, ack.to];
    desktop = completeRekey(desktop, eph.secret, accepted.eph);
    // The desktop now sends under epoch 1; the phone switches when it sees it.
    const next = send(desktop, phone, { t: "changed" });
    [desktop, phone] = [next.from, next.to];
    expect(phone.current.epoch).toBe(1);
    expect(phone.next).toBeUndefined();
    // The desktop still accepts the phone's epoch-1 frames, then drops epoch 0.
    const reply = send(phone, desktop, { n: 99 });
    [phone, desktop] = [reply.from, reply.to];
    expect(desktop.current.epoch).toBe(1);
    expect(desktop.rxCtr).toBe(5n);
    expect(phone.txCtr).toBe(5n);
  });

  it("F-RM-03.2 a frame replayed from before a rekey is still rejected", () => {
    let { desktop, phone } = paired();
    const old = sealNext(phone, encodeJson({ t: "cmd", id: 1, cmd: "thread.send", args: {} }));
    phone = old.state;
    desktop = openNext(desktop, old.frame).state;
    const eph = generateKeyPair();
    const accepted = acceptRekey(phone, eph.public);
    phone = accepted.state;
    desktop = completeRekey(desktop, eph.secret, accepted.eph);
    const next = send(desktop, phone, { t: "changed" });
    [desktop, phone] = [next.from, next.to];
    const reply = send(phone, desktop, { n: 2 });
    desktop = reply.to;
    expectReject(() => openNext(desktop, old.frame), "replay");
  });

  it("F-RM-03 an epoch that is neither current nor next is refused", () => {
    const { desktop, phone } = paired();
    const eph = generateKeyPair();
    const ahead = completeRekey(desktop, eph.secret, generateKeyPair().public);
    const frame = sealNext(ahead, encodeJson({ t: "changed" })).frame;
    expectReject(() => openNext(phone, frame), "epoch");
  });

  it("F-RM-03 the pairing link round-trips and junk is refused", () => {
    const link = {
      v: 1 as const,
      pid: "pr_testpair01",
      dpk: b64url(new Uint8Array(32).fill(1)),
      s: b64url(new Uint8Array(32).fill(2)),
      relay: "wss://api.example.test/v1/relay/pr_testpair01",
      ptok: "tok",
    };
    const url = encodePairingLink("https://app.example.test/remote", link);
    expect(url.startsWith("https://app.example.test/remote#p=")).toBe(true);
    expect(parsePairingFragment(new URL(url).hash)).toEqual(link);
    expect(parsePairingFragment("#p=bm90LWpzb24")).toBeNull();
    expect(parsePairingFragment("")).toBeNull();
    expect(parsePairingFragment(`#p=${b64url(encodeJson({ ...link, relay: "http://x" }))}`)).toBeNull();
  });
});
