// The runtime polyfills, kept free of React Native imports so tests can run
// them (F-RM-05.6).

type Random = <T extends ArrayBufferView | null>(array: T) => T;

type Global = {
  crypto?: { getRandomValues?: Random };
  TextDecoder?: unknown;
};

/** A UTF-8 decoder for runtimes without `TextDecoder` (fatal on bad input, like `{fatal: true}`). */
export class Utf8Decoder {
  readonly encoding = "utf-8";

  decode(input?: ArrayBufferView | ArrayBuffer): string {
    if (!input) return "";
    const bytes =
      input instanceof ArrayBuffer ? new Uint8Array(input) : new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
    let out = "";
    let i = 0;
    while (i < bytes.length) {
      const b0 = bytes[i]!;
      let cp: number;
      let n: number;
      if (b0 < 0x80) [cp, n] = [b0, 1];
      else if (b0 >= 0xc2 && b0 < 0xe0) [cp, n] = [b0 & 0x1f, 2];
      else if (b0 >= 0xe0 && b0 < 0xf0) [cp, n] = [b0 & 0x0f, 3];
      else if (b0 >= 0xf0 && b0 < 0xf5) [cp, n] = [b0 & 0x07, 4];
      else throw new TypeError("Invalid UTF-8");
      if (i + n > bytes.length) throw new TypeError("Invalid UTF-8");
      for (let k = 1; k < n; k++) {
        const b = bytes[i + k]!;
        if ((b & 0xc0) !== 0x80) throw new TypeError("Invalid UTF-8");
        cp = (cp << 6) | (b & 0x3f);
      }
      if ((n === 3 && (cp < 0x800 || (cp >= 0xd800 && cp <= 0xdfff))) || (n === 4 && (cp < 0x10000 || cp > 0x10ffff))) {
        throw new TypeError("Invalid UTF-8");
      }
      out += String.fromCodePoint(cp);
      i += n;
    }
    return out;
  }
}

export function installPolyfills(g: Global, random: Random): void {
  if (!g.crypto) g.crypto = {};
  if (typeof g.crypto.getRandomValues !== "function") g.crypto.getRandomValues = random;
  if (typeof g.TextDecoder !== "function") g.TextDecoder = Utf8Decoder;
}
