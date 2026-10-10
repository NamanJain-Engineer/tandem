import type { BrowserState, EngineView, PaneRecord } from "@tandem/protocol";
import { create } from "zustand";
import { t } from "../../i18n";
import { pageTitle } from "../../stores/browser";

/** What an agent pane's title can come from, besides its record. */
export type TitleSources = {
  /** From the engine's hooks (the status record). */
  hook?: string | null;
  /** OSC 0/2 from the TUI. */
  osc?: string | null;
  /** The first prompt typed into the pane. */
  firstPrompt?: string | null;
  /** A browser pane's page title (F-CD-17). */
  page?: string | null;
};

/** Browser panes' page titles, from the pages' states. */
export function browserSources(
  panes: Record<string, PaneRecord>,
  pages: Record<string, BrowserState>,
): Record<string, TitleSources> {
  const out: Record<string, TitleSources> = {};
  for (const p of Object.values(panes)) {
    const page = pages[p.id];
    if (p.kind === "browser" && page) out[p.id] = { page: pageTitle(page) };
  }
  return out;
}

/** A browser pane's name: its page's title, else the host it was on. */
function browserTitle(p: PaneRecord, page: string | null | undefined): string {
  if (page) return page;
  try {
    if (p.browserUrl) return new URL(p.browserUrl).host || t("browser.pane");
  } catch {
    // Not an address: fall through.
  }
  return t("browser.pane");
}

/** Spinner and status glyphs TUIs put before their titles. */
const SPINNER = /^[\s⠀-⣿✳✶✻✽✢·*•◐◓◑◒○●◌⏺|/\\-]+/u;

/** An OSC title without its spinner: the registry's `title_patterns` first
 * (capture group 1 is the work title), then the common spinner glyphs. */
export function cleanTitle(title: string, patterns: string[] = []): string {
  for (const p of patterns) {
    try {
      const m = new RegExp(p, "u").exec(title);
      if (m?.[1]) return m[1].trim();
    } catch {
      // A registry pattern JS can't read: skip it.
    }
  }
  return title.replace(SPINNER, "").trim();
}

/** The first eight words of a prompt. */
export function promptTitle(prompt: string): string {
  const words = prompt.trim().split(/\s+/).filter(Boolean);
  const head = words.slice(0, 8).join(" ");
  return words.length > 8 ? `${head}…` : head;
}

/** Pane titles (F-EN-06, UI-11): agent panes use the hook title, the
 * cleaned OSC title, the first prompt, and finally "{Engine} · n";
 * terminals are "Shell · n" — numbered per engine in creation order. The
 * pane's first name (F-CD-12) is shown beside the title, not instead. */
export function paneTitles(
  panes: Record<string, PaneRecord>,
  engines: EngineView[],
  sources: Record<string, TitleSources> = {},
): Record<string, string> {
  const spec = (id: string | null) => engines.find((e) => e.spec.id === id)?.spec;
  const sorted = Object.values(panes).sort((a, b) => a.createdAt - b.createdAt || a.id.localeCompare(b.id));
  const counts = new Map<string, number>();
  return Object.fromEntries(
    sorted.map((p) => {
      if (p.kind === "browser") return [p.id, browserTitle(p, sources[p.id]?.page)];
      const key = p.kind === "agent" ? (p.engineId ?? "agent") : "shell";
      const n = (counts.get(key) ?? 0) + 1;
      counts.set(key, n);
      const s = p.kind === "agent" ? spec(p.engineId) : undefined;
      const engine = p.kind === "agent" ? (s?.label ?? p.engineId ?? "Agent") : null;
      const fallback = engine ? t("pane.engine", { engine, n }) : t("pane.shell", { n });
      if (p.kind !== "agent") return [p.id, p.title ?? fallback];
      const src = sources[p.id] ?? {};
      const osc = src.osc ? cleanTitle(src.osc, s?.titlePatterns ?? []) : "";
      const title =
        (src.hook?.trim() || null) ??
        (osc || null) ??
        (src.firstPrompt ? promptTitle(src.firstPrompt) : null) ??
        p.title ??
        fallback;
      return [p.id, title];
    }),
  );
}

type FirstPrompts = { prompts: Record<string, string>; set: (paneId: string, text: string) => void };

/** The first prompt sent to each agent pane (typed, pasted or via
 * type_prompt), for its title until the engine names the work. */
export const useFirstPrompts = create<FirstPrompts>((set, get) => ({
  prompts: {},
  set: (paneId, text) => {
    const clean = text.trim();
    if (!clean || get().prompts[paneId]) return;
    set({ prompts: { ...get().prompts, [paneId]: clean } });
  },
}));

const PASTE_START = "\u001b[200~";
const PASTE_END = "\u001b[201~";

/** Collects what the user types into an agent pane until the first Enter. */
export class PromptCapture {
  private buf = "";
  private done = false;

  constructor(private onPrompt: (text: string) => void) {}

  /** Input as xterm sends it to the PTY. */
  input(data: string): void {
    if (this.done) return;
    const text = data.split(PASTE_START).join("").split(PASTE_END).join("");
    for (const ch of text) {
      if (ch === "\r") {
        if (this.buf.trim()) {
          this.done = true;
          this.onPrompt(this.buf);
          return;
        }
      } else if (ch === "\x7f" || ch === "\b") {
        this.buf = this.buf.slice(0, -1);
      } else if (ch === "\x1b") {
        // Escape sequences (arrows, Alt+…) aren't prompt text.
        return;
      } else if (ch >= " " || ch === "\n" || ch === "\t") {
        this.buf += ch;
      }
    }
  }
}
