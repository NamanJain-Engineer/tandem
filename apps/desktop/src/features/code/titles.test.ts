import { describe, expect, it } from "vitest";
import type { EngineView, PaneRecord } from "@tandem/protocol";
import { PromptCapture, cleanTitle, paneTitles, promptTitle, tabLabel } from "./titles";

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
  restoreState: "none",
  browserUrl: null,
  createdAt: at,
  ...extra,
});

const engines = [
  { spec: { id: "claude", label: "Claude Code", titlePatterns: ["^[✳✶✻✽·*]\\s*(.+)$"] } },
] as unknown as EngineView[];

describe("pane titles (F-EN-06)", () => {
  it("F-CD-17 a browser pane is named after its page, else the host it was on, and takes no shell number", () => {
    const shell = { kind: "terminal", engineId: null };
    const panes = {
      a: pane("a", 1, shell),
      b: pane("b", 2, { kind: "browser", engineId: null, browserUrl: "http://localhost:5173/docs" }),
      c: pane("c", 3, shell),
      d: pane("d", 4, { kind: "browser", engineId: null }),
    };
    expect(paneTitles(panes, engines, { b: { page: "Fixtures" } })).toEqual({
      a: "Shell · 1",
      b: "Fixtures",
      c: "Shell · 2",
      d: "Browser",
    });
    expect(paneTitles(panes, engines).b).toBe("localhost:5173");
  });

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

  it("D-2603 without a title, panes are named for their engine or shell and their folder", () => {
    const shell = { kind: "terminal", engineId: null };
    const panes = {
      a: pane("a", 1, shell),
      b: pane("b", 2),
      c: pane("c", 3, { ...shell, cwd: "/code/api/" }),
      d: pane("d", 4, shell),
      e: pane("e", 5),
    };
    expect(paneTitles(panes, engines, {}, "shop")).toEqual({
      a: "Shell · shop",
      b: "Claude Code · shop",
      c: "Shell · api",
      d: "Shell · shop 2",
      e: "Claude Code · shop 2",
    });
  });

  it("F-CD-12 the first name sits beside the title and doesn't replace it", () => {
    expect(paneTitles({ a: pane("a", 1, { humanName: "Rhea" }) }, engines, { a: { hook: "x" } })).toEqual({
      a: "x",
    });
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

describe("D-2603 tab labels", () => {
  it("a default tab reads as its task, else its workspace; a name the user gave stays", () => {
    expect(tabLabel("Tab 1", "shop")).toBe("shop");
    expect(tabLabel("Tab 3", "shop")).toBe("shop 3");
    expect(tabLabel("Tab 2", "shop", "Fix the redirect loop after login on checkout")).toBe("Fix the redirect loop after…");
    expect(tabLabel("Build", "shop", "Fix it")).toBe("Build");
  });
});
