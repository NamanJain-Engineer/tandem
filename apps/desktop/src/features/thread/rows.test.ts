import { describe, expect, it } from "vitest";
import type { ThreadEvent, ThreadEventEnvelope } from "@tandem/protocol";
import { emptyThread, reduce } from "./reducer";
import { groupStatus, pendingApprovals, reuseRows, toRows, usageOf, type ToolEvent } from "./rows";
import { toolView } from "./tool";

let seq = 0;
function env(turnId: string, event: ThreadEvent, s?: number): ThreadEventEnvelope {
  return { threadId: "thr_1", turnId, seq: s ?? seq++, ts: 0, event };
}

function tool(id: string, status: ToolEvent["status"] = "ok"): ThreadEvent {
  return {
    type: "tool",
    id,
    verb: "read",
    argSummary: `src/${id}.ts`,
    status,
    output: null,
    outputTruncated: false,
    outputFile: null,
    diff: null,
    detail: null,
  };
}

function play(...events: ThreadEventEnvelope[]) {
  return events.reduce(reduce, emptyThread);
}

describe("transcript rows (F-TH-12, UI-21)", () => {
  it("F-TH-12 three or more consecutive tool calls fold; two stay rows", () => {
    seq = 0;
    const view = play(
      env("t1", { type: "user", text: "go", attachments: [] }),
      env("t1", tool("a")),
      env("t1", tool("b")),
      env("t1", { type: "assistant", markdown: "between" }),
      env("t1", tool("c")),
      env("t1", tool("d")),
      env("t1", tool("e", "running")),
    );
    const rows = toRows(view);
    expect(rows.map((r) => r.kind)).toEqual(["user", "tool", "tool", "assistant", "tools", "working"]);
    const group = rows[4];
    expect(group?.kind === "tools" && group.tools.map((t) => t.id)).toEqual(["c", "d", "e"]);
    expect(group?.kind === "tools" && groupStatus(group.tools)).toBe("running");
  });

  it("F-TH-16.1 an engine switch's divider streams after its message and reads above it", () => {
    seq = 0;
    const view = play(
      env("t1", { type: "user", text: "one", attachments: [] }),
      env("t1", { type: "turn_end", reason: "completed", blockedActions: 0 }),
      env("t2", { type: "user", text: "two", attachments: [] }, 0),
      env("t2", { type: "divider", text: "Continued on Codex (Default) with a summary" }, 1),
      env("t2", { type: "assistant", markdown: "ok" }, 2),
    );
    const rows = toRows(view);
    expect(rows.map((r) => r.kind)).toEqual(["user", "divider", "user", "assistant", "working"]);
    const divider = rows[1];
    expect(divider?.kind === "divider" && divider.text).toBe("Continued on Codex (Default) with a summary");
  });

  it("F-TH-11 approvals carry their decision; pending ones are the running turn's", () => {
    seq = 0;
    const req = (id: string): ThreadEvent => ({
      type: "approval_request",
      id,
      kind: "command",
      summary: "Run `npm test`",
      detail: "npm test",
      options: ["once", "thread"],
      expiresAt: 0,
    });
    const view = play(
      env("t1", { type: "user", text: "go", attachments: [] }),
      env("t1", req("apr_1")),
      env("t1", { type: "approval_resolved", id: "apr_1", decision: { type: "allow", scope: "thread" } }),
      env("t1", req("apr_2")),
    );
    const rows = toRows(view);
    const approvals = rows.filter((r) => r.kind === "approval");
    expect(approvals.map((r) => r.kind === "approval" && r.decision)).toEqual([
      { type: "allow", scope: "thread" },
      null,
    ]);
    expect(pendingApprovals(rows, "t1").map((r) => r.request.id)).toEqual(["apr_2"]);
    expect(pendingApprovals(rows, null)).toEqual([]);
  });

  it("F-TH-11 a stop and blocked actions end with a row; a clean end doesn't", () => {
    seq = 0;
    const done = play(
      env("t1", { type: "user", text: "go", attachments: [] }),
      env("t1", { type: "assistant", markdown: "ok" }),
      env("t1", { type: "turn_end", reason: "completed", blockedActions: 0 }),
    );
    expect(toRows(done).map((r) => r.kind)).toEqual(["user", "assistant"]);
    seq = 0;
    const blocked = play(
      env("t2", { type: "user", text: "go", attachments: [] }),
      env("t2", tool("x", "denied")),
      env("t2", { type: "turn_end", reason: "completed", blockedActions: 1 }),
    );
    expect(toRows(blocked).at(-1)).toMatchObject({ kind: "end", reason: "completed", blocked: 1 });
    seq = 0;
    const stopped = play(
      env("t3", { type: "user", text: "go", attachments: [] }),
      env("t3", { type: "assistant_delta", text: "part" }),
      env("t3", { type: "turn_end", reason: "cancelled", blockedActions: 0 }),
    );
    const rows = toRows(stopped);
    expect(rows.map((r) => r.kind)).toEqual(["user", "assistant", "end"]);
    expect(rows[1]).toMatchObject({ markdown: "part", streaming: false });
  });

  it("F-TH-12 streaming text is one row that grows; usage sums across turns", () => {
    seq = 0;
    let view = play(env("t1", { type: "user", text: "go", attachments: [] }));
    view = reduce(view, env("t1", { type: "assistant_delta", text: "Hel" }, 1));
    view = reduce(view, env("t1", { type: "assistant_delta", text: "lo" }, 1));
    const rows = toRows(view);
    expect(rows.filter((r) => r.kind === "assistant")).toEqual([
      { kind: "assistant", key: "t1:1", markdown: "Hello", streaming: true },
    ]);
    view = reduce(
      view,
      env(
        "t1",
        {
          type: "usage",
          input: 100,
          output: 20,
          cacheRead: 50,
          cacheWrite: 0,
          contextFillPct: 4.2,
          costUsd: null,
        },
        2,
      ),
    );
    view = reduce(view, env("t1", { type: "turn_end", reason: "completed", blockedActions: 0 }, 3));
    view = reduce(view, env("t2", { type: "user", text: "more", attachments: [] }, 0));
    view = reduce(
      view,
      env(
        "t2",
        {
          type: "usage",
          input: 10,
          output: 5,
          cacheRead: 0,
          cacheWrite: 0,
          contextFillPct: null,
          costUsd: null,
        },
        1,
      ),
    );
    expect(usageOf(view)).toEqual({ input: 160, output: 25, contextFillPct: 4.2, byEngine: [] });
    // F-TH-16: each turn's engine gets its own total.
    const engines: Record<string, string> = { t1: "claude", t2: "codex" };
    expect(usageOf(view, (id) => engines[id]).byEngine).toEqual([
      { engineId: "claude", input: 150, output: 20 },
      { engineId: "codex", input: 10, output: 5 },
    ]);
  });

  it("UI-21 a streamed delta keeps the objects of rows that didn't change", () => {
    seq = 0;
    const before = play(
      env("t1", { type: "user", text: "go", attachments: [] }),
      env("t1", tool("a")),
      env("t1", tool("b")),
      env("t1", tool("c")),
      env("t1", { type: "assistant_delta", text: "Hel" }, 10),
    );
    const prev = toRows(before);
    const next = reuseRows(
      prev,
      toRows(reduce(before, env("t1", { type: "assistant_delta", text: "lo" }, 10))),
    );
    expect(next.map((r) => r.kind)).toEqual(["user", "tools", "assistant", "working"]);
    expect(next[0]).toBe(prev[0]);
    expect(next[1]).toBe(prev[1]);
    expect(next[2]).not.toBe(prev[2]);
    expect(next[2]).toMatchObject({ markdown: "Hello" });
    expect(next[3]).toBe(prev[3]);
    // Nothing changed: the same array comes back.
    expect(
      reuseRows(next, toRows(reduce(before, env("t1", { type: "assistant_delta", text: "lo" }, 10)))),
    ).toBe(next);
  });
});

describe("UI-21 F-TH-12 scannable turns (D-2604)", () => {
  const edit = (id: string, path: string, diff: string): ThreadEvent => ({
    type: "tool",
    id,
    verb: "edit",
    argSummary: path,
    status: "ok",
    output: null,
    outputTruncated: false,
    outputFile: null,
    diff,
    detail: null,
  });
  const bash = (id: string, cmd: string, output: string, status: ToolEvent["status"] = "ok"): ThreadEvent => ({
    type: "tool",
    id,
    verb: "bash",
    argSummary: cmd,
    status,
    output,
    outputTruncated: false,
    outputFile: null,
    diff: null,
    detail: null,
  });

  it("a finished turn closes with what it changed and how its last test run went", () => {
    seq = 0;
    const view = play(
      env("t1", { type: "user", text: "fix it", attachments: [] }),
      env("t1", edit("a", "src/a.ts", "--- a/src/a.ts\n+++ b/src/a.ts\n@@ -1 +1,2 @@\n-x\n+y\n+z")),
      env("t1", edit("b", "src/b.ts", "@@ -1 +1 @@\n-old\n+new")),
      env("t1", edit("c", "src/a.ts", "@@ -3 +3,2 @@\n+w")),
      env("t1", bash("d", "pnpm test", " Tests  3 failed | 15 passed (18)", "error")),
      env("t1", bash("e", "pnpm test", " Test Files  4 passed (4)\n      Tests  18 passed (18)")),
      env("t1", { type: "assistant", markdown: "Fixed." }),
      env("t1", { type: "turn_end", reason: "completed", blockedActions: 0 }),
    );
    const rows = toRows(view);
    const summary = rows[rows.length - 1];
    expect(summary).toMatchObject({ kind: "summary", files: 2, add: 4, del: 2, tests: { passed: 18, failed: 0 } });
  });

  it("a declined or failed edit isn't counted; a multi-file diff counts each file", () => {
    seq = 0;
    const denied: ThreadEvent = { ...(edit("a", "src/a.ts", "@@ -1 +1 @@\n-x\n+y") as ToolEvent), status: "denied" };
    const failed: ThreadEvent = { ...(edit("b", "src/b.ts", "@@ -1 +1 @@\n-x\n+y") as ToolEvent), status: "error" };
    const multi = edit(
      "c",
      "src/c.ts, src/d.ts, src/e.ts",
      [
        "--- a/src/c.ts\n+++ b/src/c.ts\n@@ -1 +1 @@\n-1\n+1",
        "--- a/src/d.ts\n+++ b/src/d.ts\n@@ -1 +1 @@\n+2",
        "--- /dev/null\n+++ b/src/e.ts\n@@ -0,0 +1 @@\n+3",
      ].join("\n"),
    );
    const view = play(
      env("t1", { type: "user", text: "go", attachments: [] }),
      env("t1", denied),
      env("t1", failed),
      env("t1", { type: "turn_end", reason: "completed", blockedActions: 0 }),
      env("t2", { type: "user", text: "again", attachments: [] }, 0),
      env("t2", multi, 1),
      env("t2", { type: "turn_end", reason: "completed", blockedActions: 0 }, 2),
    );
    const summaries = toRows(view).filter((r) => r.kind === "summary");
    expect(summaries).toHaveLength(1);
    expect(summaries[0]).toMatchObject({ files: 3, add: 3, del: 1 });
  });

  it("a turn that only read and talked needs no summary", () => {
    seq = 0;
    const view = play(
      env("t1", { type: "user", text: "explain", attachments: [] }),
      env("t1", tool("a")),
      env("t1", { type: "assistant", markdown: "It does x." }),
      env("t1", { type: "turn_end", reason: "completed", blockedActions: 0 }),
    );
    expect(toRows(view).some((r) => r.kind === "summary")).toBe(false);
  });
});

describe("F-TH-12 Claude Code's edits in the turn summary (D-2608)", () => {
  /** A Claude Code edit's result: no diff, its files absolute, and the
   * lines its input describes when the edit happened. */
  const claudeEdit = (
    id: string,
    verb: "edit" | "multiedit" | "write",
    path: string,
    lines: { added: number; removed: number } | undefined,
    status: ToolEvent["status"] = "ok",
  ): ThreadEvent => ({
    type: "tool",
    id,
    verb,
    argSummary: path.replace("/work/shop/", ""),
    status,
    output: "The file has been updated.",
    outputTruncated: false,
    outputFile: null,
    diff: null,
    detail: null,
    paths: [path],
    ...(lines ? { lines } : {}),
  });
  const turn = (...events: ThreadEvent[]) => {
    seq = 0;
    return toRows(
      play(
        env("t1", { type: "user", text: "fix the loop", attachments: [] }),
        ...events.map((e) => env("t1", e)),
        env("t1", { type: "turn_end", reason: "completed", blockedActions: 0 }),
      ),
    );
  };

  it("F-TH-12.4 a turn of Edit, MultiEdit and a new file's Write says Changed 3 files +7 −4", () => {
    const rows = turn(
      claudeEdit("a", "edit", "/work/shop/src/redirect.ts", { added: 2, removed: 2 }),
      claudeEdit("b", "multiedit", "/work/shop/src/next.ts", { added: 2, removed: 2 }),
      claudeEdit("c", "write", "/work/shop/src/redirect.test.ts", { added: 3, removed: 0 }),
    );
    expect(rows.find((r) => r.kind === "summary")).toMatchObject({ files: 3, add: 7, del: 4 });
  });

  it("F-TH-12.4 each of those rows shows its own counts", () => {
    const one = claudeEdit("a", "edit", "/work/shop/src/redirect.ts", { added: 2, removed: 2 }) as ToolEvent;
    expect(toolView(one).meta).toBe("+2 −2");
    const created = claudeEdit("c", "write", "/work/shop/src/new.ts", { added: 3, removed: 0 }) as ToolEvent;
    expect(toolView(created).meta).toBe("+3 −0");
  });

  it("F-TH-12.4 a file edited twice counts once, and its lines twice", () => {
    const rows = turn(
      claudeEdit("a", "edit", "/work/shop/src/next.ts", { added: 1, removed: 1 }),
      claudeEdit("b", "edit", "/work/shop/src/next.ts", { added: 2, removed: 0 }),
    );
    expect(rows.find((r) => r.kind === "summary")).toMatchObject({ files: 1, add: 3, del: 1 });
  });

  it("F-TH-12.4 a Write over an existing file counts the file but no lines; with nothing counted, no +0 −0", () => {
    const rewrite = claudeEdit("a", "write", "/work/shop/README.md", undefined);
    expect(toolView(rewrite as ToolEvent).meta).toBeUndefined();
    expect(turn(rewrite).find((r) => r.kind === "summary")).toMatchObject({ files: 1, add: null, del: null });
  });

  it("F-TH-12.4 when one edit can't say its lines, the summary names the files with no partial total", () => {
    const rewrite = claudeEdit("a", "write", "/work/shop/README.md", undefined);
    const mixed = turn(rewrite, claudeEdit("b", "edit", "/work/shop/src/a.ts", { added: 1, removed: 0 }));
    expect(mixed.find((r) => r.kind === "summary")).toMatchObject({ files: 2, add: null, del: null });
    // A Codex patch with its diff beside it changes nothing about that.
    const codex: ThreadEvent = {
      ...(claudeEdit("c", "edit", "/work/shop/src/b.ts", undefined) as ToolEvent),
      diff: "--- a/src/b.ts\n+++ b/src/b.ts\n@@ -1 +1,2 @@\n-a\n+b\n+c\n",
    };
    expect(turn(codex).find((r) => r.kind === "summary")).toMatchObject({ files: 1, add: 2, del: 1 });
    expect(turn(codex, rewrite).find((r) => r.kind === "summary")).toMatchObject({ files: 2, add: null, del: null });
  });

  it("F-TH-12.4 a notebook edit and a subagent's Task count the files they name (D-2608)", () => {
    const notebook: ThreadEvent = {
      ...(claudeEdit("n", "edit", "/work/shop/prices.ipynb", undefined) as ToolEvent),
      verb: "other",
      argSummary: "prices.ipynb",
    };
    const task: ThreadEvent = {
      ...(claudeEdit("t", "edit", "/work/shop/docs/a.md", { added: 4, removed: 1 }) as ToolEvent),
      verb: "task",
      argSummary: "Update the docs",
      paths: ["/work/shop/docs/a.md", "/work/shop/docs/faq.md"],
    };
    expect(turn(task).find((r) => r.kind === "summary")).toMatchObject({ files: 2, add: 4, del: 1 });
    expect(turn(task, notebook).find((r) => r.kind === "summary")).toMatchObject({ files: 3, add: null, del: null });
    // A tool that names no files isn't an edit.
    const read: ThreadEvent = { ...(notebook as ToolEvent), id: "r", verb: "read", paths: undefined };
    expect(turn(read).some((r) => r.kind === "summary")).toBe(false);
  });

  it("F-TH-12.4 a file reported by its diff and by its absolute path counts once", () => {
    const withDiff: ThreadEvent = {
      ...(claudeEdit("a", "edit", "/work/shop/src/b.ts", undefined) as ToolEvent),
      diff: "--- a/src/b.ts\n+++ b/src/b.ts\n@@ -1 +1 @@\n-a\n+b\n",
    };
    const again = claudeEdit("b", "edit", "/work/shop/src/b.ts", { added: 1, removed: 0 });
    expect(turn(withDiff, again).find((r) => r.kind === "summary")).toMatchObject({ files: 1, add: 2, del: 1 });
  });

  it("F-TH-12.4 a row's counts read as words", () => {
    const one = claudeEdit("a", "edit", "/work/shop/src/redirect.ts", { added: 7, removed: 1 }) as ToolEvent;
    expect(toolView(one)).toMatchObject({ meta: "+7 −1", metaLabel: "7 lines added, 1 removed" });
    const single = claudeEdit("b", "edit", "/work/shop/src/a.ts", { added: 1, removed: 0 }) as ToolEvent;
    expect(toolView(single).metaLabel).toBe("1 line added, 0 removed");
  });

  it("F-TH-12.5 a refused or failed edit adds no file and no lines", () => {
    const rows = turn(
      claudeEdit("a", "edit", "/work/shop/.env", { added: 1, removed: 1 }, "denied"),
      claudeEdit("b", "edit", "/work/shop/src/next.ts", { added: 1, removed: 1 }, "error"),
    );
    expect(rows.some((r) => r.kind === "summary")).toBe(false);
    // Beside an edit that happened, only that one counts, lines and all.
    const mixed = turn(
      claudeEdit("a", "edit", "/work/shop/.env", { added: 5, removed: 5 }, "denied"),
      claudeEdit("b", "edit", "/work/shop/src/next.ts", { added: 3, removed: 3 }, "error"),
      claudeEdit("c", "edit", "/work/shop/src/ok.ts", { added: 2, removed: 1 }),
    );
    expect(mixed.find((r) => r.kind === "summary")).toMatchObject({ files: 1, add: 2, del: 1 });
    const denied = claudeEdit("a", "edit", "/work/shop/.env", { added: 1, removed: 1 }, "denied") as ToolEvent;
    expect(toolView(denied).meta).toBe("Denied");
  });
});
