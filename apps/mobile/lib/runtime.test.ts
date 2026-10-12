import { describe, expect, it } from "vitest";
import { installPolyfills, Utf8Decoder } from "./runtime";

describe("F-RM-05.6 runtime polyfills", () => {
  it("installs a CSPRNG and a UTF-8 decoder only where missing", () => {
    const random = <T extends ArrayBufferView | null>(a: T) => a;
    const bare: { crypto?: { getRandomValues?: typeof random }; TextDecoder?: unknown } = {};
    installPolyfills(bare, random);
    expect(bare.crypto?.getRandomValues).toBe(random);
    expect(bare.TextDecoder).toBe(Utf8Decoder);
    const own = () => null;
    const full = { crypto: { getRandomValues: own as never }, TextDecoder: TextDecoder };
    installPolyfills(full, random);
    expect(full.crypto.getRandomValues).toBe(own);
    expect(full.TextDecoder).toBe(TextDecoder);
  });

  it("decodes UTF-8 like TextDecoder, and refuses malformed bytes", () => {
    const d = new Utf8Decoder();
    for (const text of ["", "plain", "Café ☕ — 日本語 🚀", '{"t":"welcome","desktop":"Ünïcode"}']) {
      expect(d.decode(new TextEncoder().encode(text))).toBe(text);
    }
    for (const bad of [[0xc0, 0x80], [0xe2, 0x82], [0xed, 0xa0, 0x80], [0xf8, 0x80, 0x80, 0x80], [0x80]]) {
      expect(() => d.decode(new Uint8Array(bad))).toThrow();
    }
  });
});
