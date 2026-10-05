import { inTauri, usingFakeBackend } from "./invoke";

/** Binary frame kinds (ARCHITECTURE §3.4). */
export const FrameKind = { Data: 1, Exit: 2, Notice: 3, ReplayBegin: 4, ReplayEnd: 5 } as const;

export type Frame = { kind: number; seq: number; payload: Uint8Array };

/** `u8 kind | u8[3] reserved | u64 seq (LE) | payload`. */
export function decodeFrame(buf: ArrayBuffer | Uint8Array): Frame {
  const bytes = buf instanceof Uint8Array ? buf : new Uint8Array(buf);
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const kind = bytes[0] ?? 0;
  const seq = bytes.byteLength >= 12 ? Number(view.getBigUint64(4, true)) : 0;
  return { kind, seq, payload: bytes.subarray(12) };
}

export function encodeFrame(kind: number, seq: number, payload: Uint8Array): Uint8Array {
  const out = new Uint8Array(12 + payload.byteLength);
  out[0] = kind;
  new DataView(out.buffer).setBigUint64(4, BigInt(seq), true);
  out.set(payload, 12);
  return out;
}

/** What `pty_spawn` / `pty_attach` take as `channel`. */
export type FrameChannel = { onmessage: (m: ArrayBuffer | Uint8Array) => void };

/** A Tauri Channel in the app; a plain object in tests. */
export async function createFrameChannel(onFrame: (f: Frame) => void): Promise<FrameChannel> {
  if (inTauri() && !usingFakeBackend()) {
    const core = await import("@tauri-apps/api/core");
    const channel = new core.Channel<ArrayBuffer>();
    channel.onmessage = (m) => onFrame(decodeFrame(m));
    return channel as unknown as FrameChannel;
  }
  return { onmessage: (m) => onFrame(decodeFrame(m)) };
}
