import { describe, expect, it } from "vitest";
import type { ThreadEvent, ThreadEventEnvelope } from "@tandem/protocol";
import { emptyThread, reduce } from "./reducer";
import { groupStatus, pendingApprovals, reuseRows, toRows, usageOf, type ToolEvent } from "./rows";

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
