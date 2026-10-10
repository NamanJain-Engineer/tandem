// Synthetic audio for the STT relay tests: valid containers whose length the
// relay reads from headers (WAV) or granule positions (Ogg Opus).

/** 8 kHz mono 16-bit WAV (16,000 bytes a second). `declare` false writes a streaming header. */
export function wav(seconds: number, declare = true): Buffer {
  const rate = 8000;
  const data = Buffer.alloc(Math.round(seconds * rate * 2));
  const h = Buffer.alloc(44);
  h.write("RIFF", 0, "ascii");
  h.writeUInt32LE(declare ? 36 + data.length : 0xffffffff, 4);
  h.write("WAVE", 8, "ascii");
  h.write("fmt ", 12, "ascii");
  h.writeUInt32LE(16, 16);
  h.writeUInt16LE(1, 20);
  h.writeUInt16LE(1, 22);
  h.writeUInt32LE(rate, 24);
  h.writeUInt32LE(rate * 2, 28);
  h.writeUInt16LE(2, 32);
  h.writeUInt16LE(16, 34);
  h.write("data", 36, "ascii");
  h.writeUInt32LE(declare ? data.length : 0xffffffff, 40);
  return Buffer.concat([h, data]);
}

function page(granule: bigint, seq: number, payload: Buffer, type = 0): Buffer {
  const segments: number[] = [];
  let left = payload.length;
  while (left >= 255) {
    segments.push(255);
    left -= 255;
  }
  segments.push(left);
  const h = Buffer.alloc(27 + segments.length);
  h.write("OggS", 0, "ascii");
  h[4] = 0;
  h[5] = type;
  h.writeBigInt64LE(granule, 6);
  h.writeUInt32LE(0x1234, 14);
  h.writeUInt32LE(seq, 18);
  h[26] = segments.length;
  segments.forEach((s, i) => (h[27 + i] = s));
  return Buffer.concat([h, payload]);
}

/** An Ogg Opus stream of `seconds`, one page per second, with a 312-sample pre-skip. */
export function opus(seconds: number): Buffer {
  const head = Buffer.alloc(19);
  head.write("OpusHead", 0, "ascii");
  head[8] = 1;
  head[9] = 1;
  head.writeUInt16LE(312, 10);
  head.writeUInt32LE(16000, 12);
  const tags = Buffer.from("OpusTags\0\0\0\0\0\0\0\0", "binary");
  const pages = [page(0n, 0, head, 2), page(0n, 1, tags)];
  for (let s = 1; s <= Math.ceil(seconds); s++) {
    const at = Math.min(s, seconds);
    pages.push(page(BigInt(Math.round(at * 48_000) + 312), s + 1, Buffer.alloc(120, s % 251)));
  }
  return Buffer.concat(pages);
}

export function form(
  parts: Array<
    { name: string; value: string } | { name: string; file: Buffer; type: string; filename: string }
  >,
): { body: Buffer; type: string } {
  const boundary = "----tandemtest";
  const chunks: Buffer[] = [];
  for (const p of parts) {
    if ("file" in p) {
      chunks.push(
        Buffer.from(
          `--${boundary}\r\nContent-Disposition: form-data; name="${p.name}"; filename="${p.filename}"\r\nContent-Type: ${p.type}\r\n\r\n`,
        ),
        p.file,
        Buffer.from("\r\n"),
      );
    } else {
      chunks.push(
        Buffer.from(
          `--${boundary}\r\nContent-Disposition: form-data; name="${p.name}"\r\n\r\n${p.value}\r\n`,
        ),
      );
    }
  }
  chunks.push(Buffer.from(`--${boundary}--\r\n`));
  return { body: Buffer.concat(chunks), type: `multipart/form-data; boundary=${boundary}` };
}
