// §5.4 contrast: WCAG 2.2 AA over the design tokens, in both themes. Text
// needs 4.5:1 against every surface it sits on; UI parts that carry meaning
// (status dots, the focus ring) need 3:1. Alpha colours are blended over the
// surface they're drawn on before measuring.
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const css = readFileSync(join(import.meta.dirname, "tokens.css"), "utf8");

type Rgba = [number, number, number, number];

function block(selector: string): Map<string, string> {
  const start = css.indexOf(selector);
  if (start < 0) throw new Error(`no ${selector} block`);
  const body = css.slice(css.indexOf("{", start) + 1, css.indexOf("}", start));
  const vars = new Map<string, string>();
  for (const m of body.matchAll(/(--[\w-]+):\s*([^;]+);/g)) vars.set(m[1] as string, (m[2] as string).trim());
  return vars;
}

const palette = block(":root {");
const themes = { dark: block(':root, [data-theme="dark"]'), light: block('[data-theme="light"]') };

function parse(value: string, theme: Map<string, string>): Rgba {
  const v = value.replace(/\/\*.*?\*\//g, "").trim();
  const ref = /^var\((--[\w-]+)\)$/.exec(v);
  if (ref) {
    const next = theme.get(ref[1] as string) ?? palette.get(ref[1] as string);
    if (!next) throw new Error(`unresolved ${v}`);
    return parse(next, theme);
  }
  const hex = /^#([0-9a-f]{6})$/i.exec(v);
  if (hex) {
    const n = parseInt(hex[1] as string, 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255, 1];
  }
  const rgb = /^rgb\((\d+) (\d+) (\d+)(?: \/ ([\d.]+))?\)$/.exec(v);
  if (rgb) return [Number(rgb[1]), Number(rgb[2]), Number(rgb[3]), rgb[4] ? Number(rgb[4]) : 1];
  throw new Error(`can't parse ${v}`);
}

const over = (top: Rgba, under: Rgba): Rgba => {
  const a = top[3];
  return [0, 1, 2].map((i) => (top[i] as number) * a + (under[i] as number) * (1 - a)).concat(1) as Rgba;
};

const luminance = ([r, g, b]: Rgba) =>
  [r, g, b]
    .map((c) => c / 255)
    .map((c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4))
    .reduce((sum, c, i) => sum + c * ([0.2126, 0.7152, 0.0722][i] as number), 0);

function ratio(theme: Map<string, string>, fg: string, bg: string, base = "--tn-bg"): number {
  const ground = over(parse(theme.get(bg) as string, theme), parse(theme.get(base) as string, theme));
  const ink = over(parse(theme.get(fg) as string, theme), ground);
  const [hi, lo] = [luminance(ink), luminance(ground)].sort((a, b) => b - a) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}

const SURFACES = ["--tn-bg", "--tn-surface-1", "--tn-surface-2", "--tn-surface-3", "--tn-surface-4"];
const TEXT = [
  "--tn-text",
  "--tn-text-muted",
  "--tn-text-subtle",
  "--tn-accent",
  "--tn-status-working-text",
  "--tn-status-waiting-text",
  "--tn-status-done-text",
  "--tn-warning-text",
  "--tn-status-error",
  "--tn-danger",
];
/** Text on its own coloured fill (buttons, highlighted rows). */
const ON_FILL: Array<[string, string]> = [
  ["--tn-accent-fg", "--tn-accent"],
  ["--tn-accent-fg", "--tn-accent-hover"],
  ["--tn-danger-fg", "--tn-danger"],
  ["--tn-text", "--tn-accent-soft"],
  ["--tn-diff-add-fg", "--tn-diff-add-bg"],
  ["--tn-diff-del-fg", "--tn-diff-del-bg"],
];
/** Non-text parts that carry meaning: 3:1 against the surfaces they sit on. */
const UI = [
  "--tn-status-working",
  "--tn-status-waiting",
  "--tn-status-done",
  "--tn-status-error",
  "--tn-accent-ring",
];

describe("§5.4 WCAG 2.2 AA contrast over the tokens", () => {
  for (const [name, theme] of Object.entries(themes)) {
    it(`${name}: text is at least 4.5:1 on every surface`, () => {
      const low = TEXT.flatMap((fg) =>
        SURFACES.map((bg) => [fg, bg, ratio(theme, fg, bg)] as const).filter(([, , r]) => r < 4.5),
      );
      expect(low.map(([fg, bg, r]) => `${fg} on ${bg}: ${r.toFixed(2)}`)).toEqual([]);
    });

    it(`${name}: text on coloured fills is at least 4.5:1`, () => {
      const low = ON_FILL.map(([fg, bg]) => [fg, bg, ratio(theme, fg, bg, "--tn-surface-2")] as const).filter(
        ([, , r]) => r < 4.5,
      );
      expect(low.map(([fg, bg, r]) => `${fg} on ${bg}: ${r.toFixed(2)}`)).toEqual([]);
    });

    it(`${name}: status colours and the focus ring are at least 3:1 on every surface`, () => {
      const low = UI.flatMap((fg) =>
        SURFACES.map((bg) => [fg, bg, ratio(theme, fg, bg)] as const).filter(([, , r]) => r < 3),
      );
      expect(low.map(([fg, bg, r]) => `${fg} on ${bg}: ${r.toFixed(2)}`)).toEqual([]);
    });
  }

  it("§1.2a dark: surfaces rise in lightness with elevation, all of them neutral", () => {
    const t = themes.dark;
    const lum = SURFACES.map((name) => luminance(parse(t.get(name) as string, t)));
    for (let i = 1; i < lum.length; i++) expect(lum[i]).toBeGreaterThan(lum[i - 1] as number);
    for (const name of SURFACES) {
      const [r, g, b] = parse(t.get(name) as string, t);
      // Neutral: no channel more than 4 apart (the old slate leaned blue by 12).
      expect(Math.max(r, g, b) - Math.min(r, g, b)).toBeLessThanOrEqual(4);
    }
  });

  it("measures known pairs correctly", () => {
    const t = new Map([
      ["--tn-bg", "#ffffff"],
      ["--a", "#000000"],
      ["--b", "#777777"],
    ]);
    expect(ratio(t, "--a", "--tn-bg")).toBeCloseTo(21, 1);
    expect(ratio(t, "--b", "--tn-bg")).toBeCloseTo(4.48, 1);
  });

  it("every *-text colour utility the code uses has a token behind it", () => {
    const theme = css.slice(css.indexOf("@theme inline"));
    const used = new Set<string>();
    const walk = (dir: string) => {
      for (const e of readdirSync(dir, { withFileTypes: true })) {
        const p = join(dir, e.name);
        if (e.isDirectory()) {
          if (e.name !== "node_modules") walk(p);
        } else if (/\.tsx?$/.test(e.name)) {
          for (const m of readFileSync(p, "utf8").matchAll(/\b(?:text|bg|border)-([a-z]+(?:-[a-z]+)*-text)\b/g)) used.add(m[1] as string);
        }
      }
    };
    walk(join(import.meta.dirname, ".."));
    walk(join(import.meta.dirname, "../../../../apps/desktop/src"));
    expect(used.has("warning-text")).toBe(true);
    expect([...used].filter((name) => !theme.includes(`--color-${name}:`))).toEqual([]);
  });
});
