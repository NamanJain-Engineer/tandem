// D-1895 / D-2602 (M18 · Developer-first UI): engines are told apart by our
// own letters on a quiet tint per engine, set only in brand.css, with the
// letters in `fg` at AA on every tile in both themes.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ENGINE_MARKS, EngineGlyph, engineTint } from "./EngineGlyph";

const dir = join(import.meta.dirname, "../styles");
const brand = readFileSync(join(dir, "brand.css"), "utf8");
const tokens = readFileSync(join(dir, "tokens.css"), "utf8");

function blockOf(css: string, selector: string): string {
  const start = css.indexOf(selector);
  if (start < 0) throw new Error(`no ${selector}`);
  return css.slice(css.indexOf("{", start) + 1, css.indexOf("}", start));
}
const value = (block: string, name: string) => {
  const m = new RegExp(`${name}:\\s*([^;]+);`).exec(block);
  if (!m?.[1]) throw new Error(`no ${name}`);
  return m[1].trim();
};

type Rgb = [number, number, number];
/** OKLCH → sRGB (0–255), clamped. */
function oklch(text: string): Rgb {
  const m = /^oklch\(([\d.]+) ([\d.]+) ([\d.]+)\)$/.exec(text);
  if (!m) throw new Error(text);
  const [L, C, h] = [Number(m[1]), Number(m[2]), (Number(m[3]) * Math.PI) / 180];
  const a = C * Math.cos(h);
  const b = C * Math.sin(h);
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const mm = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  const lin = [
    4.0767416621 * l - 3.3077115913 * mm + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * mm - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * mm + 1.707614701 * s,
  ];
  return lin.map((c) => {
    const v = Math.min(1, Math.max(0, c));
    return Math.round((v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055) * 255);
  }) as Rgb;
}
const hex = (h: string): Rgb => {
  const n = parseInt(h.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
};
const lum = (c: Rgb) =>
  c
    .map((v) => v / 255)
    .map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4))
    .reduce((sum, v, i) => sum + v * ([0.2126, 0.7152, 0.0722][i] as number), 0);
const ratio = (x: Rgb, y: Rgb) => {
  const [hi, lo] = [lum(x), lum(y)].sort((p, q) => q - p) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
};

const THEMES = {
  dark: { tints: blockOf(brand, ':root, [data-theme="dark"]'), fg: value(blockOf(tokens, ':root, [data-theme="dark"]'), "--tn-text") },
  light: { tints: blockOf(brand, '[data-theme="light"]'), fg: value(blockOf(tokens, '[data-theme="light"]'), "--tn-text") },
};

describe("D-1895 engine marks", () => {
  for (const [name, theme] of Object.entries(THEMES)) {
    it(`${name}: fg is at least 4.5:1 on every engine's tile`, () => {
      const pairs = ENGINE_MARKS.map((id) => [id, ratio(hex(theme.fg), oklch(value(theme.tints, `--tn-engine-${id}`)))] as const);
      expect(pairs).toHaveLength(13);
      expect(pairs.filter(([, r]) => r < 4.5).map(([id, r]) => `${id}: ${r.toFixed(2)}`)).toEqual([]);
    });
  }

  it("the tints stay quiet: chroma at most 0.05, hues at least 15° apart", () => {
    const parts = ENGINE_MARKS.map((id) => /^oklch\(([\d.]+) ([\d.]+) ([\d.]+)\)$/.exec(value(THEMES.dark.tints, `--tn-engine-${id}`)));
    for (const p of parts) expect(Number(p?.[2])).toBeLessThanOrEqual(0.05);
    const hues = parts.map((p) => Number(p?.[3])).sort((a, b) => a - b);
    const gaps = hues.map((h, i) => ((hues[(i + 1) % hues.length] as number) - h + 360) % 360);
    expect(Math.min(...gaps)).toBeGreaterThanOrEqual(15);
  });

  it("known engines use their tint; Shell and anything else sit on the neutral surface", () => {
    expect(engineTint("claude")).toBe("var(--tn-engine-claude)");
    expect(engineTint("shell")).toBe("var(--tn-surface-4)");
    expect(engineTint(undefined)).toBe("var(--tn-surface-4)");
  });

  it("letters are fg, at 80% at rest and 100% on the focused pane; decorative beside a name", () => {
    const { container } = render(<EngineGlyph letters="X" engine="codex" />);
    const tile = container.firstElementChild as HTMLElement;
    expect(tile.getAttribute("aria-hidden")).toBe("true");
    expect(tile.className).toMatch(/text-fg\b/);
    expect(tile.className).toMatch(/opacity-80/);
    render(<EngineGlyph letters="C" engine="claude" label="Claude Code" active />);
    const img = screen.getByRole("img", { name: "Claude Code" });
    expect(img.className).toMatch(/opacity-100/);
    expect(img.getAttribute("style")).toContain("var(--tn-engine-claude)");
  });
});
