import { act, render } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it, vi } from "vitest";
import { ThemeProvider } from "./ThemeProvider";
import { tandemDay, tandemHighContrast, tandemNight, terminalThemeFor } from "./terminalThemes";

type Listener = () => void;

/** A controllable matchMedia: `set(query, matches)` fires change listeners. */
function fakeMedia(initial: Record<string, boolean>) {
  const state = { ...initial };
  const listeners = new Map<string, Set<Listener>>();
  window.matchMedia = vi.fn((query: string) => ({
    get matches() {
      return state[query] ?? false;
    },
    media: query,
    onchange: null,
    addEventListener: (_: string, l: Listener) => {
      if (!listeners.has(query)) listeners.set(query, new Set());
      listeners.get(query)?.add(l);
    },
    removeEventListener: (_: string, l: Listener) => listeners.get(query)?.delete(l),
    addListener: () => undefined,
    removeListener: () => undefined,
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
  return {
    set(query: string, matches: boolean) {
      state[query] = matches;
      act(() => listeners.get(query)?.forEach((l) => l()));
    },
  };
}

const root = () => document.documentElement.dataset;

describe("ThemeProvider (F-SH-05)", () => {
  it("F-SH-05 applies dark and light themes", () => {
    fakeMedia({});
    const { rerender } = render(<ThemeProvider theme="dark">x</ThemeProvider>);
    expect(root().theme).toBe("dark");
    rerender(<ThemeProvider theme="light">x</ThemeProvider>);
    expect(root().theme).toBe("light");
  });

  it("F-SH-05 a theme switch is instant: transitions are off until the new colours land", () => {
    fakeMedia({});
    const frames: FrameRequestCallback[] = [];
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => frames.push(cb));
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => undefined);
    const { rerender } = render(<ThemeProvider theme="dark">x</ThemeProvider>);
    const settle = () => {
      while (frames.length) frames.shift()?.(0);
    };
    settle();
    expect(root().themeSwitching).toBeUndefined();
    rerender(<ThemeProvider theme="light">x</ThemeProvider>);
    expect(root().theme).toBe("light");
    expect(root().themeSwitching).toBe("");
    settle();
    expect(root().themeSwitching).toBeUndefined();
    // Settings that don't change colours leave transitions alone.
    rerender(<ThemeProvider theme="light" density="comfortable">x</ThemeProvider>);
    expect(root().themeSwitching).toBeUndefined();
    const css = readFileSync(resolve(process.cwd(), "src/styles/base.css"), "utf8");
    const start = css.indexOf("[data-theme-switching] *,");
    expect(css.slice(start, css.indexOf("}", start))).toContain("transition: none !important");
    vi.restoreAllMocks();
  });

  it("F-SH-05 system follows the OS live", () => {
    const media = fakeMedia({ "(prefers-color-scheme: light)": false });
    render(<ThemeProvider theme="system">x</ThemeProvider>);
    expect(root().theme).toBe("dark");
    media.set("(prefers-color-scheme: light)", true);
    expect(root().theme).toBe("light");
  });

  it("F-SH-05.3 reduce motion from the setting or the OS", () => {
    const media = fakeMedia({ "(prefers-reduced-motion: reduce)": false });
    const { rerender } = render(<ThemeProvider motion="on">x</ThemeProvider>);
    expect(root().motion).toBe("reduced");
    rerender(<ThemeProvider motion="system">x</ThemeProvider>);
    expect(root().motion).toBe("full");
    media.set("(prefers-reduced-motion: reduce)", true);
    expect(root().motion).toBe("reduced");
    rerender(<ThemeProvider motion="off">x</ThemeProvider>);
    expect(root().motion).toBe("full");
  });

  it("F-SH-05.3 reduced motion collapses every animation to 0.01 ms", () => {
    const css = readFileSync(resolve(process.cwd(), "src/styles/base.css"), "utf8");
    const rule = css.slice(css.indexOf('[data-motion="reduced"] *,'), css.indexOf("}", css.indexOf('[data-motion="reduced"] *,')));
    expect(rule).toContain("animation-duration: 0.01ms !important");
    expect(rule).toContain("transition-duration: 0.01ms !important");
    expect(rule).toContain("*::before");
  });

  it("UI-12 menus, selects and tooltips open above the sheet or dialog they're in, and below toasts", () => {
    const css = readFileSync(resolve(process.cwd(), "src/styles/tokens.css"), "utf8");
    const z = (name: string) => Number(new RegExp(`--z-${name}:\\s*(\\d+)`).exec(css)?.[1]);
    expect(z("popover")).toBeGreaterThan(z("dialog"));
    expect(z("dialog")).toBeGreaterThan(z("sheet"));
    expect(z("popover")).toBeLessThan(z("toast"));
  });

  it("F-SH-05 derives high contrast from the OS", () => {
    const media = fakeMedia({ "(prefers-contrast: more)": false });
    render(<ThemeProvider>x</ThemeProvider>);
    expect(root().contrast).toBeUndefined();
    media.set("(prefers-contrast: more)", true);
    expect(root().contrast).toBe("more");
  });

  it("F-SH-05 terminal palettes follow the app theme", () => {
    expect(terminalThemeFor("follow_app", "dark")).toBe(tandemNight);
    expect(terminalThemeFor("follow_app", "light")).toBe(tandemDay);
    expect(terminalThemeFor("follow_app", "dark", true)).toBe(tandemHighContrast);
    expect(terminalThemeFor("day", "dark")).toBe(tandemDay);
    expect(tandemNight.background).toBe("#0b111c");
  });
});
