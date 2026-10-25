// The public site's layer (site.css, D-1801): its OS light theme must be the
// app's light theme exactly, and every colour pair the site draws must pass
// WCAG 2.2 AA in both themes (§5.4, extended to the site's own surfaces).
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const tokens = readFileSync(join(import.meta.dirname, "tokens.css"), "utf8");
const site = readFileSync(join(import.meta.dirname, "site.css"), "utf8");
const brand = readFileSync(join(import.meta.dirname, "brand.css"), "utf8");

type Rgba = [number, number, number, number];

function vars(css: string, selector: string, from = 0): Map<string, string> {
  const start = css.indexOf(selector, from);
  if (start < 0) throw new Error(`no ${selector} block`);
  const body = css.slice(css.indexOf("{", start) + 1, css.indexOf("}", start));
  const out = new Map<string, string>();
  for (const m of body.replace(/\/\*[\s\S]*?\*\//g, "").matchAll(/(--[\w-]+):\s*([^;]+);/g))
    out.set(m[1] as string, (m[2] as string).trim());
  return out;
}

const palette = vars(tokens, ":root {");
const appDark = vars(tokens, ':root, [data-theme="dark"]');
const appLight = vars(tokens, '[data-theme="light"]');
const LIGHT_SELECTOR = ':root:not([data-theme="dark"]) {';
const siteLightTn = vars(site, LIGHT_SELECTOR);
const siteRoot = vars(site, "/* ---------- 2.");
const siteLightSemantic = vars(site, LIGHT_SELECTOR, site.indexOf("/* ---------- 2."));
const brandDark = vars(brand, ':root, [data-theme="dark"]');
const brandLight = vars(brand, '[data-theme="light"]');
const siteLightBrand = vars(site, LIGHT_SELECTOR, site.indexOf("/* ---------- 1b."));

// The interim dark ink (T8, D-1883) until app-ui's ink semantics land.
const inkDark = vars(site, ':root[data-theme="dark"], .site [data-theme="dark"] {');
const inkDarkOs = vars(site, ':root:not([data-theme="light"]) {');

const themes = {
  dark: new Map([...appDark, ...brandDark, ...siteRoot, ...inkDark]),
  light: new Map([...appLight, ...brandLight, ...siteRoot, ...siteLightSemantic]),
};

function parse(value: string, theme: Map<string, string>): Rgba {
  const v = value.trim();
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

const over = (top: Rgba, under: Rgba): Rgba =>
  [0, 1, 2].map((i) => (top[i] as number) * top[3] + (under[i] as number) * (1 - top[3])).concat(1) as Rgba;

const luminance = ([r, g, b]: Rgba) =>
  [r, g, b]
    .map((c) => c / 255)
    .map((c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4))
    .reduce((sum, c, i) => sum + c * ([0.2126, 0.7152, 0.0722][i] as number), 0);

function ratio(theme: Map<string, string>, fg: string, bg: string, base = "--site-page"): number {
  const ground = over(parse(theme.get(bg) ?? bg, theme), parse(theme.get(base) ?? base, theme));
  const ink = over(parse(theme.get(fg) ?? fg, theme), ground);
  const [hi, lo] = [luminance(ink), luminance(ground)].sort((a, b) => b - a) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}

const SURFACES = ["--site-page", "--site-raised", "--site-card"];
const TEXT = [
  "--tn-text",
  "--tn-text-muted",
  "--tn-text-subtle",
  "--tn-accent",
  "--site-ink-a",
  "--site-ink-b",
  "--tn-status-working-text",
  "--tn-status-waiting-text",
  "--tn-status-done-text",
];

describe("F-WB-01 site.css (D-1801)", () => {
  it("F-WB-01 site light theme mirrors the app's light tokens", () => {
    expect(siteLightTn.size).toBeGreaterThan(40);
    expect(Object.fromEntries(siteLightTn)).toEqual(
      Object.fromEntries([...appLight].map(([k, v]) => [k, v.replace(/\/\*[\s\S]*?\*\//g, "").trim()])),
    );
  });

  it("F-WB-01 AC6 a visitor's Light sets exactly the site tokens the OS's light does (D-1870)", () => {
    const forced = vars(site, ':root[data-theme="light"] {');
    expect(forced.size).toBeGreaterThan(12);
    expect(Object.fromEntries(forced)).toEqual(Object.fromEntries(siteLightSemantic));
    // The brand tokens the site aliases (D-1891) get a visitor's Light from
    // brand.css's own [data-theme="light"] block.
    const brand = readFileSync(join(import.meta.dirname, "brand.css"), "utf8");
    const brandLight = vars(brand, '[data-theme="light"] {');
    const aliases = [...siteRoot].filter(([, v]) => /^var\(--tn-/.test(v)).map(([, v]) => v.slice(4, -1));
    const brandAliases = aliases.filter((t) => /^--tn-(edge|fill|glow|ring|spot|ink|elev)/.test(t));
    expect(brandAliases.length).toBeGreaterThan(10);
    for (const t of brandAliases) expect(brandLight.has(t), t).toBe(true);
    // The app's own tokens come from tokens.css's [data-theme="light"],
    // which the OS block above mirrors.
    expect(appLight.get("--tn-bg")).toBe("var(--slate-50)");
  });

  it("F-WB-01 AC6 the OS's light never overrides a visitor's Dark (D-1870)", () => {
    for (const m of site.matchAll(/@media \(prefers-color-scheme: light\) \{\s*([^{]+)\{/g))
      expect(m[1]?.trim()).toMatch(/^:root:not\(\[data-theme="dark"\]\)( &)?$/);
  });

  for (const [name, theme] of Object.entries(themes)) {
    it(`F-WB-01 site colour pairs meet WCAG AA in ${name}: text on every site surface`, () => {
      const low = TEXT.flatMap((fg) =>
        SURFACES.map((bg) => [fg, bg, ratio(theme, fg, bg)] as const).filter(([, , r]) => r < 4.5),
      );
      expect(low.map(([fg, bg, r]) => `${fg} on ${bg}: ${r.toFixed(2)}`)).toEqual([]);
    });

    it(`F-WB-01 site colour pairs meet WCAG AA in ${name}: buttons, the stage and the focus ring`, () => {
      // The primary button, and captions on the always-dark product stage.
      expect(ratio(theme, "--tn-accent-fg", "--tn-accent")).toBeGreaterThanOrEqual(4.5);
      expect(ratio(theme, "--tn-accent-fg", "--tn-accent-hover")).toBeGreaterThanOrEqual(4.5);
      for (const fg of ["--site-stage-fg", "--site-stage-muted", "--site-stage-accent"])
        expect(ratio(theme, fg, "--site-stage"), fg).toBeGreaterThanOrEqual(4.5);
      // "Tap to explore": the accent chip on the stage.
      expect(ratio(themes.dark, "--tn-accent-fg", "--tn-accent")).toBeGreaterThanOrEqual(4.5);
      for (const bg of SURFACES) expect(ratio(theme, "--tn-accent-ring", bg), bg).toBeGreaterThanOrEqual(3);
    });
  }

  it("F-WB-01 the primary button's label passes AA on both stops of its gradient, in both themes (D-1830)", () => {
    for (const [name, theme] of Object.entries(themes))
      for (const stop of ["--site-btn-top", "--site-btn-bottom", "--site-btn-hover"])
        expect(ratio(theme, "--tn-accent-fg", stop), `${name} ${stop}`).toBeGreaterThanOrEqual(4.5);
  });

  it("F-WB-01 site light theme mirrors the brand layer's light tokens (D-1891)", () => {
    expect(brandLight.size).toBeGreaterThan(12);
    expect(Object.fromEntries(siteLightBrand)).toEqual(Object.fromEntries(brandLight));
  });

  it("F-WB-01 every light, depth and edge token the dark theme sets has a light value (D-1830, D-1891)", () => {
    const themed = [...siteRoot.keys()].filter((k) => /^--site-(edge|fill|glow|ring|elev|bezel|line|btn|spot)/.test(k));
    expect(themed.length).toBeGreaterThan(12);
    // Set in the site's light block, or an alias of a brand.css token that has a light value.
    const lit = (k: string) => {
      if (siteLightSemantic.has(k)) return true;
      const alias = /^var\((--tn-[\w-]+)\)$/.exec(siteRoot.get(k) ?? "");
      return alias !== null && brandDark.has(alias[1] as string) && brandLight.has(alias[1] as string);
    };
    expect(themed.filter((k) => !lit(k))).toEqual([]);
    // Every brand token the dark theme sets has a light value.
    expect([...brandDark.keys()].filter((k) => !brandLight.has(k))).toEqual([]);
  });

  it("F-WB-01 eyebrows, card text and the accent pass AA on the bezel and card fills (D-1830)", () => {
    for (const [name, theme] of Object.entries(themes))
      for (const fg of ["--tn-text", "--tn-text-muted", "--tn-accent"])
        expect(ratio(theme, fg, "--site-card"), `${name} ${fg}`).toBeGreaterThanOrEqual(4.5);
  });

  it("F-WB-01 motion runs only when the visitor hasn't asked for less (D-1831)", () => {
    // Cut every `@media (prefers-reduced-motion: no-preference) { … }` block
    // out, braces balanced: nothing that moves may remain.
    let rest = site.replace(/\/\*[\s\S]*?\*\//g, "");
    for (;;) {
      const at = rest.indexOf("@media (prefers-reduced-motion: no-preference)");
      if (at < 0) break;
      let depth = 0;
      let end = rest.length;
      for (let i = rest.indexOf("{", at); i < rest.length; i++) {
        if (rest[i] === "{") depth++;
        else if (rest[i] === "}" && --depth === 0) {
          end = i + 1;
          break;
        }
      }
      rest = rest.slice(0, at) + rest.slice(end);
    }
    expect(rest).not.toMatch(/animation(-name)?\s*:\s*(?!none)/);
    expect(rest).not.toMatch(/animation-timeline/);
    expect(rest).not.toMatch(/@view-transition/);
  });

  it("F-WB-01 AC8 motion has its tokens, and scroll reveals only where the browser can run them (D-1874)", () => {
    const root = vars(site, "/* ---------- 5. Motion");
    for (const t of ["--ease-io-site", "--ease-spring-site", "--dur-pop", "--dur-panel", "--dur-spring", "--dur-reveal", "--stagger"])
      expect(root.has(t), t).toBe(true);
    expect(root.get("--ease-spring-site")).toBe("cubic-bezier(0.3, 1.35, 0.5, 1)");
    // Every view()-timeline rule sits inside @supports, so where the browser
    // lacks scroll timelines the content is simply there.
    const css = site.replace(/\/\*[\s\S]*?\*\//g, "");
    const supports = [...css.matchAll(/@supports \(animation-timeline: view\(\)\) \{/g)].map((m) => m.index ?? 0);
    for (const m of css.matchAll(/animation-timeline: view\(\);/g)) {
      const at = m.index ?? 0;
      expect(supports.some((s) => s < at && css.slice(s, at).split("{").length - css.slice(s, at).split("}").length > 0), `view() at ${at}`).toBe(true);
    }
    // Stagger never runs past five steps (DESIGN §4.7).
    expect(css).toContain(".reveal.stagger-5");
    expect(css).not.toContain(".reveal.stagger-6");
  });

  it("F-WB-01 AC8 motion moves only transform and opacity: no filter in any keyframe (D-1874)", () => {
    const css = site.replace(/\/\*[\s\S]*?\*\//g, "");
    for (const m of css.matchAll(/@keyframes ([\w-]+) \{([\s\S]*?)\}\s*\}?/g)) expect(m[2], m[1]).not.toMatch(/filter|width|height|top|left|margin/);
  });

  it("F-WB-01 the site's own tokens are ink, never slate (T8, D-1894)", () => {
    // Sections 1 and 1b mirror tokens.css and brand.css and follow them; the
    // site's own semantics (section 2 on) and the scenes carry no slate.
    const own = site.slice(site.indexOf("/* ---------- 2.")).replace(/\/\*[\s\S]*?\*\//g, "");
    const scenes = readFileSync(join(import.meta.dirname, "../../../../apps/web/components/site/scenes/scenes.css"), "utf8");
    for (const [name, css] of [["site.css", own], ["scenes.css", scenes]] as const) {
      expect(css, name).not.toMatch(/--slate-/);
      expect(css, name).not.toContain("148 163 184");
      expect(css, name).not.toContain("15 23 42");
    }
    expect(siteRoot.get("--site-stage")).toBe("var(--ink-900)");
    expect(siteLightSemantic.get("--site-page")).toBe("var(--ink-25)");
  });

  it("F-WB-01 dark is ink everywhere the dark theme applies, until app-ui's semantics land (T8, D-1883)", () => {
    expect(Object.fromEntries(inkDarkOs)).toEqual(Object.fromEntries(inkDark));
    expect(inkDark.get("--site-page")).toBe("var(--ink-950)");
    expect(inkDark.get("--tn-text")).toBe("#ededef");
    for (const v of inkDark.values()) expect(v).not.toMatch(/slate|148 163 184/);
  });

  it("F-WB-01 AC4 the theme control's selected cell is told apart at 3:1 or more, in both themes (WCAG 1.4.11)", () => {
    expect(site).toContain("0 0 0 1px var(--tn-text-muted), 0 1px 2px rgb(var(--tn-shadow-color) / 0.18);");
    for (const [name, theme] of Object.entries(themes))
      for (const bg of ["--site-glass", "--site-card"]) expect(ratio(theme, "--tn-text-muted", bg), `${name} ${bg}`).toBeGreaterThanOrEqual(3);
  });
});
