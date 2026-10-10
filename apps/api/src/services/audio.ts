// Measures audio while it streams through the STT relay (F-BE-07), without
// buffering or storing it: WAV from its header and byte count, Ogg Opus from
// page granule positions (48 kHz, less the OpusHead pre-skip).
import { Transform, type TransformCallback } from "node:stream";

export type AudioKind = "wav" | "ogg";

export const MAX_BYTES = 25 * 1024 * 1024;

export function kindOf(mime: string, filename = ""): AudioKind | null {
  const m = mime.toLowerCase();
  if (m.includes("wav") || m === "audio/wave") return "wav";
  if (m.includes("ogg") || m.includes("opus")) return "ogg";
  const f = filename.toLowerCase();
  if (f.endsWith(".wav")) return "wav";
  if (f.endsWith(".ogg") || f.endsWith(".opus")) return "ogg";
  return null;
}

export type OverReason = "bytes" | "seconds" | "malformed";

export class AudioLimitError extends Error {
  constructor(public readonly reason: OverReason) {
    super(`audio over its limit: ${reason}`);
    this.name = "AudioLimitError";
  }
}

/**
 * A pass-through stream that knows the audio's duration so far and fails as
 * soon as it passes `maxSeconds` or `maxBytes` (a WAV whose header declares
 * too much fails on its first chunk).
 */
export class AudioMeter extends Transform {
  bytes = 0;
  private head: Buffer = Buffer.alloc(0);
  // WAV
  private byteRate = 0;
  private dataStart = -1;
  private declaredData = -1;
  // Ogg
  private granule = 0;
  private preSkip = 0;
  private pending: Buffer = Buffer.alloc(0);

  constructor(
    private readonly kind: AudioKind,
    private readonly maxSeconds: number,
    private readonly maxBytes = MAX_BYTES,
  ) {
    super();
  }

  /** Seconds of audio seen so far. */
  get seconds(): number {
    if (this.kind === "wav") {
      if (this.byteRate <= 0 || this.dataStart < 0) return 0;
      const data = Math.max(0, this.bytes - this.dataStart);
      const counted = this.declaredData >= 0 ? Math.min(data, this.declaredData) : data;
      return counted / this.byteRate;
    }
    return Math.max(0, this.granule - this.preSkip) / 48_000;
  }

  override _transform(chunk: Buffer, _enc: BufferEncoding, done: TransformCallback): void {
    this.bytes += chunk.length;
    if (this.bytes > this.maxBytes) return done(new AudioLimitError("bytes"));
    try {
      if (this.kind === "wav") this.readWav(chunk);
      else this.readOgg(chunk);
    } catch (err) {
      return done(err instanceof AudioLimitError ? err : new AudioLimitError("malformed"));
    }
    if (this.seconds > this.maxSeconds) return done(new AudioLimitError("seconds"));
    done(null, chunk);
  }

  override _flush(done: TransformCallback): void {
    if (this.kind === "wav" && this.dataStart < 0) return done(new AudioLimitError("malformed"));
    done();
  }

  private readWav(chunk: Buffer): void {
    if (this.dataStart >= 0) return;
    this.head = Buffer.concat([this.head, chunk]).subarray(0, 64 * 1024);
    const h = this.head;
    if (h.length < 12) return;
    if (h.toString("ascii", 0, 4) !== "RIFF" || h.toString("ascii", 8, 12) !== "WAVE") {
      throw new AudioLimitError("malformed");
    }
    let at = 12;
    while (at + 8 <= h.length) {
      const id = h.toString("ascii", at, at + 4);
      const size = h.readUInt32LE(at + 4);
      if (id === "fmt ") {
        if (at + 8 + 16 > h.length) return;
        this.byteRate = h.readUInt32LE(at + 8 + 8);
        if (this.byteRate <= 0) throw new AudioLimitError("malformed");
      } else if (id === "data") {
        this.dataStart = at + 8;
        // Streaming writers put 0 or 0xFFFFFFFF here; trust only a real size.
        if (size > 0 && size < 0xffffffff) this.declaredData = size;
        if (
          this.byteRate > 0 &&
          this.declaredData > 0 &&
          this.declaredData / this.byteRate > this.maxSeconds
        ) {
          throw new AudioLimitError("seconds");
        }
        return;
      }
      at += 8 + size + (size % 2);
    }
    if (h.length >= 64 * 1024) throw new AudioLimitError("malformed");
  }

  private readOgg(chunk: Buffer): void {
    this.pending = Buffer.concat([this.pending, chunk]);
    for (;;) {
      const p = this.pending;
      if (p.length < 27) return;
      if (p.toString("ascii", 0, 4) !== "OggS") throw new AudioLimitError("malformed");
      const segments = p[26] ?? 0;
      if (p.length < 27 + segments) return;
      let body = 0;
      for (let i = 0; i < segments; i++) body += p[27 + i] ?? 0;
      const size = 27 + segments + body;
      if (p.length < size) return;
      const granule = p.readBigInt64LE(6);
      const payload = p.subarray(27 + segments, size);
      if (payload.length >= 12 && payload.toString("ascii", 0, 8) === "OpusHead") {
        this.preSkip = payload.readUInt16LE(10);
      }
      if (granule >= 0n) this.granule = Math.max(this.granule, Number(granule));
      this.pending = p.subarray(size);
    }
  }
}

/** Credits for `seconds` of audio: `perBlock` for every started 15 s (§3.3). */
export function sttCost(seconds: number, perBlock: number): number {
  return Math.ceil(Math.max(0, seconds) / 15) * perBlock;
}
