import { describe, expect, it } from "vitest";
import { inlineAsset } from "./assets";

describe("the demo's build (vibezeo.com/demo/app/)", () => {
  it("F-WB-01 fonts are never inlined, because the site's CSP (font-src 'self') refuses data: fonts", () => {
    // JetBrains Mono's Cyrillic-extended subset is under Vite's 4 KB limit:
    // inlined, the live demo logged a CSP refusal on every load.
    expect(inlineAsset("/n/@fontsource-variable/jetbrains-mono/files/jetbrains-mono-cyrillic-ext-wght-normal.woff2")).toBe(false);
    for (const font of ["a.woff", "a.ttf", "a.otf", "A.WOFF2"]) expect(inlineAsset(font), font).toBe(false);
    // Anything else keeps Vite's default (img-src allows data:).
    expect(inlineAsset("/src/logo.svg")).toBeUndefined();
  });
});
