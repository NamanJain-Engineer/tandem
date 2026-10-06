import { describe, expect, it } from "vitest";
import type { EngineView, PaneRecord } from "@tandem/protocol";
import { PromptCapture, cleanTitle, paneTitles, promptTitle } from "./titles";

const pane = (id: string, at: number, extra: Partial<PaneRecord> = {}): PaneRecord => ({
  id,
  tabId: "tb",
  workspaceId: "ws",
  kind: "agent",
  cwd: null,
  engineId: "claude",
  accountId: null,
  humanName: null,
  title: null,
  createdAt: at,
  ...extra,
});

const engines = [
  { spec: { id: "claude", label: "Claude Code", titlePatterns: ["^[✳✶✻✽·*]\\s*(.+)$"] } },
] as unknown as EngineView[];

describe("pane titles (F-EN-06)", () => {
  it("F-EN-06 hook > OSC (spinner stripped) > first prompt (8 words) > Engine · n", () => {
    const panes = { a: pane("a", 1), b: pane("b", 2), c: pane("c", 3), d: pane("d", 4) };
    const titles = paneTitles(panes, engines, {
      a: { hook: "Fix the auth guard", osc: "✳ Something else", firstPrompt: "x" },
      b: { osc: "✳ Refactor billing", firstPrompt: "x" },
      c: { firstPrompt: "please write the e2e tests for the checkout flow and billing" },
      d: {},
    });
    expect(titles).toEqual({
      a: "Fix the auth guard",
      b: "Refactor billing",
      c: "please write the e2e tests for the checkout…",
      d: "Claude Code · 4",
    });
  });

  it("a name the user gave wins over everything", () => {
    expect(paneTitles({ a: pane("a", 1, { humanName: "Rhea" }) }, engines, { a: { hook: "x" } })).toEqual({ a: "Rhea" });
  });

  it("spinner glyphs are stripped without a registry pattern", () => {
    expect(cleanTitle("⠋ Working on tests")).toBe("Working on tests");
    expect(cleanTitle("✳ Claude Code", ["^[✳](.+)$"])).toBe("Claude Code");
    expect(promptTitle("  one two  ")).toBe("one two");
  });

  it("the first prompt is what was typed before the first Enter, edits applied", () => {
    const got: string[] = [];
    const c = new PromptCapture((t) => got.push(t));
    c.input("he");
    c.input("lp\x7f\x7fllo wor");
    c.input("\x1b[200~ld\x1b[201~");
    c.input("\r");
    c.input("second\r");
    expect(got).toEqual(["hello world"]);
  });
});
