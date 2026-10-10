// The phone companion's wire protocol v1 (F-RM-03, ARCHITECTURE §13.3,
// D-1905). The same bytes are produced by `tandem_core::remote` on the
// desktop; `crates/tandem-core/tests/fixtures/remote_v1.json` pins both.
import { xchacha20poly1305 } from "@noble/ciphers/chacha.js";
import { randomBytes } from "@noble/ciphers/utils.js";
import { x25519 } from "@noble/curves/ed25519.js";
import { hkdf } from "@noble/hashes/hkdf.js";
import { hmac } from "@noble/hashes/hmac.js";
import { sha256 } from "@noble/hashes/sha2.js";

export const VERSION = 1;
export const KIND_HELLO = 0x01;
export const KIND_SEALED = 0x02;
export const HEADER_LEN = 39;
/** The largest plaintext a sealed frame carries, so frames stay under 64 KB. */
export const MAX_PLAINTEXT = 61_440;
export const MAX_FRAME = 64 * 1024;

export type Dir = "d2p" | "p2d";
export type Role = "desktop" | "phone";
const DIR_BYTE: Record<Dir, number> = { d2p: 0, p2d: 1 };

export class ProtocolError extends Error {
  constructor(
    readonly reason: "replay" | "epoch" | "bad_frame" | "too_large" | "bad_tag" | "bad_key",
    message?: string,
  ) {
    super(message ?? reason);
  }
}

const enc = new TextEncoder();
const dec = new TextDecoder();

export function concat(...parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

function u32(n: number): Uint8Array {
  const b = new Uint8Array(4);
  new DataView(b.buffer).setUint32(0, n, false);
  return b;
}

function u64be(n: bigint): Uint8Array {
  const b = new Uint8Array(8);
  new DataView(b.buffer).setBigUint64(0, n, false);
  return b;
}

function u64le(n: bigint): Uint8Array {
  const b = new Uint8Array(8);
  new DataView(b.buffer).setBigUint64(0, n, true);
  return b;
}

export function b64url(bytes: Uint8Array): string {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function fromB64url(text: string): Uint8Array {
  const s = atob(text.replace(/-/g, "+").replace(/_/g, "/") + "===".slice((text.length + 3) % 4));
  return Uint8Array.from(s, (c) => c.charCodeAt(0));
}

export function equalBytes(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= (a[i] ?? 0) ^ (b[i] ?? 0);
  return diff === 0;
}

// ── keys ─────────────────────────────────────────────────────────────────

export type KeyPair = { secret: Uint8Array; public: Uint8Array };

export function generateKeyPair(): KeyPair {
  const secret = x25519.utils.randomSecretKey();
  return { secret, public: x25519.getPublicKey(secret) };
}

export function publicKeyOf(secret: Uint8Array): Uint8Array {
  return x25519.getPublicKey(secret);
}

function shared(secret: Uint8Array, peer: Uint8Array): Uint8Array {
  if (peer.length !== 32) throw new ProtocolError("bad_key");
  let ss: Uint8Array;
  try {
    ss = x25519.getSharedSecret(secret, peer);
  } catch {
    throw new ProtocolError("bad_key");
  }
  if (ss.every((b) => b === 0)) throw new ProtocolError("bad_key");
  return ss;
}

/** The QR secret's proof that the phone scanned the code (ARCHITECTURE §13.3). */
export function helloTag(s: Uint8Array, pid: string, phonePub: Uint8Array): Uint8Array {
  return hmac(sha256, s, concat(enc.encode("tandem-pair-v1"), enc.encode(pid), phonePub));
}

export function rootSecret(
  s: Uint8Array,
  pid: string,
  desktopPub: Uint8Array,
  phonePub: Uint8Array,
  mySecret: Uint8Array,
  peerPub: Uint8Array,
): Uint8Array {
  const ss = shared(mySecret, peerPub);
  return hkdf(sha256, ss, s, concat(enc.encode("tandem-root-v1"), enc.encode(pid), desktopPub, phonePub), 32);
}

export type EpochKeys = { epoch: number; secret: Uint8Array; d2p: Uint8Array; p2d: Uint8Array };

export function epochKeys(epoch: number, secret: Uint8Array): EpochKeys {
  return {
    epoch,
    secret,
    d2p: hkdf(sha256, secret, undefined, enc.encode("tandem-d2p-v1"), 32),
    p2d: hkdf(sha256, secret, undefined, enc.encode("tandem-p2d-v1"), 32),
  };
}

export function nextSecret(current: EpochKeys, myEph: Uint8Array, peerEph: Uint8Array): Uint8Array {
  const ss = shared(myEph, peerEph);
  return hkdf(sha256, ss, current.secret, concat(enc.encode("tandem-rekey-v1"), u32(current.epoch + 1)), 32);
}

// ── frames ───────────────────────────────────────────────────────────────

export function encodeHello(phonePub: Uint8Array, tag: Uint8Array): Uint8Array {
  return concat(new Uint8Array([KIND_HELLO, VERSION]), phonePub, tag);
}

export function decodeHello(frame: Uint8Array): { phonePub: Uint8Array; tag: Uint8Array } {
  if (frame.length !== 66 || frame[0] !== KIND_HELLO || frame[1] !== VERSION) throw new ProtocolError("bad_frame");
  return { phonePub: frame.slice(2, 34), tag: frame.slice(34, 66) };
}

export type SealedHeader = { epoch: number; dir: Dir; ctr: bigint; nonce: Uint8Array };

export function aad(pid: string, epoch: number, dir: Dir, ctr: bigint): Uint8Array {
  return concat(enc.encode(pid), u32(epoch), new Uint8Array([DIR_BYTE[dir]]), u64be(ctr));
}

export function seal(
  pid: string,
  key: Uint8Array,
  h: { epoch: number; dir: Dir; ctr: bigint },
  plaintext: Uint8Array,
  random16: Uint8Array = randomBytes(16),
): Uint8Array {
  if (plaintext.length > MAX_PLAINTEXT) throw new ProtocolError("too_large");
  const nonce = concat(u64le(h.ctr), random16);
  const ct = xchacha20poly1305(key, nonce, aad(pid, h.epoch, h.dir, h.ctr)).encrypt(plaintext);
  return concat(
    new Uint8Array([KIND_SEALED, VERSION]),
    u32(h.epoch),
    new Uint8Array([DIR_BYTE[h.dir]]),
    u64be(h.ctr),
    nonce,
    ct,
  );
}

export function readHeader(frame: Uint8Array): SealedHeader {
  if (frame.length < HEADER_LEN + 16 || frame.length > MAX_FRAME) throw new ProtocolError("bad_frame");
  if (frame[0] !== KIND_SEALED || frame[1] !== VERSION) throw new ProtocolError("bad_frame");
  const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
  const dirByte = frame[6];
  if (dirByte !== 0 && dirByte !== 1) throw new ProtocolError("bad_frame");
  const ctr = view.getBigUint64(7, false);
  const nonce = frame.slice(15, 39);
  // The nonce starts with the counter, so a frame can't reuse another's nonce.
  if (!equalBytes(nonce.slice(0, 8), u64le(ctr))) throw new ProtocolError("bad_frame");
  return { epoch: view.getUint32(2, false), dir: dirByte === 0 ? "d2p" : "p2d", ctr, nonce };
}

export function open(pid: string, key: Uint8Array, h: SealedHeader, frame: Uint8Array): Uint8Array {
  try {
    return xchacha20poly1305(key, h.nonce, aad(pid, h.epoch, h.dir, h.ctr)).decrypt(frame.slice(HEADER_LEN));
  } catch {
    throw new ProtocolError("bad_frame");
  }
}

// ── session ──────────────────────────────────────────────────────────────

/** Everything a side persists between connections. */
export type SessionState = {
  pid: string;
  role: Role;
  current: EpochKeys;
  next?: EpochKeys;
  /** The desktop sends under `next` once the phone acknowledged the rekey. */
  txOnNext: boolean;
  txCtr: bigint;
  rxCtr: bigint;
};

export type Opened = { state: SessionState; plaintext: Uint8Array };

function txDir(role: Role): Dir {
  return role === "desktop" ? "d2p" : "p2d";
}

function keyFor(keys: EpochKeys, dir: Dir): Uint8Array {
  return dir === "d2p" ? keys.d2p : keys.p2d;
}

export function newSession(pid: string, role: Role, root: Uint8Array): SessionState {
  return { pid, role, current: epochKeys(0, root), txOnNext: false, txCtr: 0n, rxCtr: 0n };
}

/** Seals one message; the returned state has the counter advanced. */
export function sealNext(
  state: SessionState,
  plaintext: Uint8Array,
  random16?: Uint8Array,
): { state: SessionState; frame: Uint8Array } {
  const keys = state.txOnNext && state.next ? state.next : state.current;
  const dir = txDir(state.role);
  const ctr = state.txCtr + 1n;
  const frame = seal(state.pid, keyFor(keys, dir), { epoch: keys.epoch, dir, ctr }, plaintext, random16);
  return { state: { ...state, txCtr: ctr }, frame };
}

/**
 * Opens a frame from the peer. A counter at or below the last one is a
 * replay; an epoch other than the current or next is refused. The state only
 * advances when the frame authenticates.
 */
export function openNext(state: SessionState, frame: Uint8Array): Opened {
  const h = readHeader(frame);
  const want: Dir = state.role === "desktop" ? "p2d" : "d2p";
  if (h.dir !== want) throw new ProtocolError("bad_frame");
  if (h.ctr <= state.rxCtr) throw new ProtocolError("replay");
  let keys: EpochKeys;
  let promote = false;
  if (h.epoch === state.current.epoch) keys = state.current;
  else if (state.next && h.epoch === state.next.epoch) {
    keys = state.next;
    promote = true;
  } else throw new ProtocolError("epoch");
  const plaintext = open(state.pid, keyFor(keys, h.dir), h, frame);
  const advanced: SessionState = promote
    ? { ...state, current: keys, next: undefined, txOnNext: false, rxCtr: h.ctr }
    : { ...state, rxCtr: h.ctr };
  return { state: advanced, plaintext };
}

/** The phone's answer to `{t:"rekey"}`: the next keys, held until the desktop uses them. */
export function acceptRekey(state: SessionState, desktopEph: Uint8Array): { state: SessionState; eph: Uint8Array } {
  const mine = generateKeyPair();
  const next = epochKeys(state.current.epoch + 1, nextSecret(state.current, mine.secret, desktopEph));
  return { state: { ...state, next, txOnNext: false }, eph: mine.public };
}

/** The desktop's side once the phone acknowledged: send under the next keys from now on. */
export function completeRekey(state: SessionState, myEph: Uint8Array, phoneEph: Uint8Array): SessionState {
  const next = epochKeys(state.current.epoch + 1, nextSecret(state.current, myEph, phoneEph));
  return { ...state, next, txOnNext: true };
}

export function encodeJson(value: unknown): Uint8Array {
  return enc.encode(JSON.stringify(value));
}

export function decodeJson<T>(bytes: Uint8Array): T {
  return JSON.parse(dec.decode(bytes)) as T;
}

// ── pairing link ─────────────────────────────────────────────────────────

/** What the desktop's QR code carries, in the URL fragment (never sent to a server). */
export type PairingLink = { v: 1; pid: string; dpk: string; s: string; relay: string; ptok: string };

export function encodePairingLink(base: string, link: PairingLink): string {
  return `${base}#p=${b64url(encodeJson(link))}`;
}

export function parsePairingFragment(hash: string): PairingLink | null {
  const m = /(?:^#|&)p=([A-Za-z0-9_-]+)/.exec(hash);
  if (!m?.[1]) return null;
  try {
    const link = decodeJson<PairingLink>(fromB64url(m[1]));
    const ok =
      link.v === 1 &&
      /^pr_[A-Za-z0-9_-]{4,64}$/.test(link.pid) &&
      fromB64url(link.dpk).length === 32 &&
      fromB64url(link.s).length === 32 &&
      /^wss?:\/\//.test(link.relay) &&
      typeof link.ptok === "string" &&
      link.ptok.length > 0;
    return ok ? link : null;
  } catch {
    return null;
  }
}
