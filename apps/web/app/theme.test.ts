// @vitest-environment jsdom
// The theme script (F-WB-01 AC4, AC6, AC7; D-1870, D-1871), run as the
// browser runs it: first thing, before the body exists.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { THEME_COLOR, THEME_KEY, THEME_SCRIPT } from "../lib/theme";

const root = document.documentElement;

/** A fresh page: no attributes on <html>, an empty body, then the script. */
function boot(stored?: string) {
  for (const a of [...root.attributes]) root.removeAttribute(a.name);
  document.head.innerHTML =
    '<meta name="theme-color" media="(prefers-color-scheme: dark)" content="#060a12">' +
    '<meta name="theme-color" media="(prefers-color-scheme: light)" content="#f8fafc">';
  document.body.innerHTML = "";
  localStorage.clear();
  if (stored !== undefined) localStorage.setItem(THEME_KEY, stored);
  new Function(THEME_SCRIPT)(); // the inline script, as the page runs it
}

/** The header's control, as ThemeSwitch renders it. */
function control() {
  document.body.innerHTML = ["system", "light", "dark"]
    .map((v) => `<label data-choice="${v}"><input type="radio" name="theme" value="${v}" data-theme-set=""></label>`)
    .join("");
  document.dispatchEvent(new Event("DOMContentLoaded"));
}

function pick(value: string) {
  const input = document.querySelector<HTMLInputElement>(`input[value="${value}"]`);
  if (!input) throw new Error(value);
  input.checked = true;
  input.dispatchEvent(new Event("change", { bubbles: true }));
}

const themeColors = () => [...document.querySelectorAll<HTMLMetaElement>('meta[name="theme-color"]')].map((m) => m.content);

beforeEach(() => {
  vi.stubGlobal("requestAnimationFrame", (f: FrameRequestCallback) => setTimeout(() => f(0), 0));
  Object.defineProperty(document, "readyState", { value: "loading", configurable: true });
});
afterEach(() => vi.unstubAllGlobals());

describe("F-WB-01 AC4 the theme script", () => {
  it("F-WB-01 AC4 F-WB-01 AC7 starts on System, sets nothing to force a theme, and stores nothing", () => {
    boot();
    expect(root.getAttribute("data-theme-choice")).toBe("system");
    expect(root.hasAttribute("data-theme")).toBe(false);
    expect(root.style.colorScheme).toBe("");
    expect(localStorage.length).toBe(0);
    expect(themeColors()).toEqual(["#060a12", "#f8fafc"]);
  });

  it("F-WB-01 AC4 F-WB-01 AC6 applies a stored Light or Dark before the body exists, color-scheme and the browser bar included", () => {
    for (const choice of ["light", "dark"] as const) {
      boot(choice);
      expect(root.getAttribute("data-theme-choice")).toBe(choice);
      expect(root.getAttribute("data-theme")).toBe(choice);
      expect(root.style.colorScheme).toBe(choice);
      control();
      expect(themeColors()).toEqual([THEME_COLOR[choice], THEME_COLOR[choice]]);
      expect(document.querySelector<HTMLInputElement>(`input[value="${choice}"]`)?.checked).toBe(true);
    }
  });

  it("F-WB-01 AC4 ignores a stored value it doesn't know, and a storage that throws", () => {
    boot("sepia");
    expect(root.getAttribute("data-theme-choice")).toBe("system");
    const getItem = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new DOMException("blocked", "SecurityError");
    });
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new DOMException("blocked", "SecurityError");
    });
    expect(() => boot()).not.toThrow();
    expect(root.getAttribute("data-theme-choice")).toBe("system");
    control();
    expect(() => pick("dark")).not.toThrow();
    expect(root.getAttribute("data-theme")).toBe("dark");
    getItem.mockRestore();
    setItem.mockRestore();
  });

  it("F-WB-01 AC4 F-WB-01 AC7 the control switches at once, keeps the choice under one key, and System forgets it", () => {
    boot();
    control();
    pick("light");
    expect(root.getAttribute("data-theme")).toBe("light");
    expect(root.getAttribute("data-theme-choice")).toBe("light");
    // Colours jump rather than fade, as in the app (D-815).
    expect(root.hasAttribute("data-theme-switching")).toBe(true);
    expect(Object.keys(localStorage)).toEqual([THEME_KEY]);
    expect(localStorage.getItem(THEME_KEY)).toBe("light");
    pick("dark");
    expect(localStorage.getItem(THEME_KEY)).toBe("dark");
    pick("system");
    expect(root.hasAttribute("data-theme")).toBe(false);
    expect(root.getAttribute("data-theme-choice")).toBe("system");
    expect(localStorage.length).toBe(0);
    expect(themeColors()).toEqual(["#060a12", "#f8fafc"]);
  });

  it("F-WB-01 AC4 follows a choice made in another tab", () => {
    boot();
    localStorage.setItem(THEME_KEY, "light");
    window.dispatchEvent(new StorageEvent("storage", { key: THEME_KEY, newValue: "light" }));
    expect(root.getAttribute("data-theme")).toBe("light");
  });

  it("F-WB-01 AC4 is plain ES5 with no requests, and touches only its own key", () => {
    expect(THEME_SCRIPT).not.toMatch(/=>|\b(let|const|class)\s|`/);
    expect(THEME_SCRIPT).not.toMatch(/https?:|fetch\(|XMLHttpRequest|sendBeacon|document\.cookie|sessionStorage|indexedDB|import\(/);
    expect(THEME_SCRIPT.match(/localStorage\.\w+/g)).toEqual([
      "localStorage.getItem",
      "localStorage.removeItem",
      "localStorage.setItem",
    ]);
    expect(THEME_SCRIPT.length).toBeLessThan(2000);
  });
});

describe("F-WB-01 AC6 both themes reach every token", () => {
  const css = (p: string) => readFileSync(join(import.meta.dirname, p), "utf8");
  const site = css("../../../packages/ui/src/styles/site.css");

  it("F-WB-01 AC6 code blocks in the docs and the blog follow a forced Light as well as the OS", () => {
    const docs = css("docs/docs.css");
    expect(docs).toContain(':root[data-theme="light"] .docs-prose :where(pre span) { color: var(--shiki-light); }');
    expect(docs).toContain(':root:not([data-theme="dark"]) .docs-prose :where(pre span) { color: var(--shiki-light); }');
    expect(docs).not.toMatch(/prefers-color-scheme: light\)\s*\{\s*\.docs/);
    const mdx = css("../components/blog/mdx.tsx");
    expect(mdx).toContain("light:[&_span]:text-(--shiki-light)");
    expect(site).toMatch(/@custom-variant light \{[\s\S]*:root:not\(\[data-theme="dark"\]\) & \{ @slot; \}[\s\S]*:root\[data-theme="light"\] & \{ @slot; \}/);
  });

  it("F-WB-01 AC6 nothing on the site answers the OS's light without also answering a forced Dark", () => {
    const sources = [site, css("docs/docs.css"), css("globals.css")];
    for (const s of sources)
      for (const m of s.matchAll(/@media \(prefers-color-scheme: light\) \{\s*([^{]+)\{/g))
        expect(m[1]?.trim(), m[0]).toMatch(/^:root:not\(\[data-theme="dark"\]\)/);
  });
});
