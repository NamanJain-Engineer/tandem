// Sealed push alerts (F-RM-06, D-2021): the same bytes as
// `tandem_core::remote::push`, pinned by
// crates/tandem-core/tests/fixtures/remote_push_v1.json. ChaCha20-Poly1305
// (RFC 8439) so iOS's CryptoKit can open them in the Notification Service
// Extension.
import { chacha20poly1305 } from "@noble/ciphers/chacha.js";
import { randomBytes } from "@noble/ciphers/utils.js";
import { b64url, concat, decodeJson, encodeJson, fromB64url, ProtocolError, VERSION } from "./protocol";

export const KIND_PUSH = 0x03;
export const PUSH_HEADER_LEN = 22;
export const MAX_PUSH_PLAINTEXT = 2048;
const LABEL = new TextEncoder().encode("tandem-push-v1");

export type PushAlert = { kind: "approval" | "agent"; id: string; title: string; body: string; at: number };

function u64(n: bigint): Uint8Array {
  const b = new Uint8Array(8);
  new DataView(b.buffer).setBigUint64(0, n);
  return b;
}

function aad(pid: string, seq: bigint): Uint8Array {
  return concat(LABEL, new TextEncoder().encode(pid), u64(seq));
}

export function sealPush(pid: string, key: Uint8Array, seq: bigint, plaintext: Uint8Array, nonce = randomBytes(12)): Uint8Array {
  if (plaintext.length > MAX_PUSH_PLAINTEXT) throw new ProtocolError("too_large");
  const ct = chacha20poly1305(key, nonce, aad(pid, seq)).encrypt(plaintext);
  return concat(new Uint8Array([KIND_PUSH, VERSION]), u64(seq), nonce, ct);
}

export function openPush(pid: string, key: Uint8Array, frame: Uint8Array): { seq: bigint; plaintext: Uint8Array } {
  if (frame.length < PUSH_HEADER_LEN + 16 || frame[0] !== KIND_PUSH || frame[1] !== VERSION) {
    throw new ProtocolError("bad_frame");
  }
  const seq = new DataView(frame.buffer, frame.byteOffset + 2, 8).getBigUint64(0);
  try {
    const plaintext = chacha20poly1305(key, frame.slice(10, 22), aad(pid, seq)).decrypt(frame.slice(22));
    return { seq, plaintext };
  } catch {
    throw new ProtocolError("bad_frame");
  }
}

/** Opens a payload as the API forwards it (base64url) into the alert. */
export function openPushPayload(pid: string, key: Uint8Array, payload: string): { seq: bigint; alert: PushAlert } {
  const { seq, plaintext } = openPush(pid, key, fromB64url(payload));
  return { seq, alert: decodeJson<PushAlert>(plaintext) };
}

/** Fixed inputs for the interop vectors. */
export function pushVectors() {
  const key = new Uint8Array(32).fill(0x42);
  const nonce = new Uint8Array(12).fill(0x24);
  const pid = "pr_01JTESTVECTOR0000000000000";
  const plaintext = '{"kind":"approval","id":"ap_1","title":"Approval needed","body":"Run pnpm test","at":1700000000000}';
  const frame = sealPush(pid, key, 7n, encodeJson(JSON.parse(plaintext)), nonce);
  return { pid, key: b64url(key), nonce: b64url(nonce), seq: 7, plaintext, frame: b64url(frame) };
}
