import type { ApprovalDecisionView, ThreadEvent, TurnEndReason } from "@tandem/protocol";
import type { ThreadView } from "./reducer";
import { editCounts, testResult, type TestResult } from "./tool";

type Ev<T extends ThreadEvent["type"]> = Extract<ThreadEvent, { type: T }>;
export type ToolEvent = Ev<"tool">;
export type ApprovalRequest = Ev<"approval_request">;

/** One transcript row (UI-21). `key` is stable: `<turn>:<seq>`. */
export type Row =
  | { kind: "user"; key: string; turnId: string; text: string; attachments: string[] }
  | { kind: "reasoning"; key: string; text: string; durationMs: number | null; streaming: boolean }
  | { kind: "tool"; key: string; tool: ToolEvent }
  | { kind: "tools"; key: string; tools: ToolEvent[] }
  | { kind: "assistant"; key: string; markdown: string; streaming: boolean }
  | {
      kind: "approval";
      key: string;
      turnId: string;
      request: ApprovalRequest;
      decision: ApprovalDecisionView | null;
    }
  | { kind: "note"; key: string; text: string }
  | { kind: "error"; key: string; turnId: string; error: Ev<"error">; last: boolean }
  | { kind: "limit"; key: string; limit: Ev<"limit"> }
  | { kind: "divider"; key: string; text: string }
  | { kind: "mail"; key: string; mailId: string; from: string; to: string; preview: string }
  | { kind: "plan"; key: string; version: number }
  | { kind: "end"; key: string; reason: TurnEndReason; blocked: number }
  /** A finished turn's closing line (D-2604): files it edited with their
   * lines added and removed, and its last test run. Only from events the
   * engine sent; `add` and `del` are null when an edit didn't say its
   * lines, rather than a partial total (D-2608). */
  | {
      kind: "summary";
      key: string;
      files: number;
      add: number | null;
      del: number | null;
      tests: { passed: number; failed: number } | null;
    }
  | { kind: "working"; key: string; turnId: string };

function mailRow(key: string, e: Ev<"mail_row">): Row {
  return { kind: "mail", key, mailId: e.mailId, from: e.from, to: e.to, preview: e.preview };
}

/** The files a unified diff touches, from its `+++ b/<path>` headers. */
export function diffPaths(diff: string): string[] {
  return [...diff.matchAll(/^\+\+\+ (?:b\/)?(.+)$/gm)]
    .map((m) => (m[1] as string).trim())
    .filter((p) => p !== "/dev/null");
}

/** Consecutive tool calls fold from this many (UI-21). */
export const FOLD_AT = 3;

/** The transcript rows of a thread view: approvals carry their decision,
 * 3+ consecutive tools fold, and the running turn ends with a "Working" row. */
export function toRows(view: ThreadView): Row[] {
  const rows: Row[] = [];
  const lastTurn = view.turns[view.turns.length - 1];
  for (const turn of view.turns) {
    const decisions = new Map<string, ApprovalDecisionView>();
    for (const item of turn.items) {
      if (item.event.type === "approval_resolved") decisions.set(item.event.id, item.event.decision);
    }
    const edited = new Set<string>();
    let add = 0;
    let del = 0;
    let uncounted = false;
    let tests: TestResult | null = null;
    for (const item of turn.items) {
      const e = item.event;
      if (e.type !== "tool") continue;
      // Only edits that happened: a declined or failed patch keeps its diff.
      // Any tool that names the files it edited counts them, a notebook
      // edit's and a subagent's Task's too (D-2608). A file is keyed by the
      // absolute path the event names, then by its diff's header, so it
      // counts once however its edits were reported.
      const named = e.paths ?? [];
      const edit = e.verb === "edit" || e.verb === "write" || e.verb === "multiedit";
      if (e.status === "ok" && (edit || named.length > 0)) {
        const paths = named.length > 0 ? named : e.diff ? diffPaths(e.diff) : [];
        for (const path of paths.length > 0 ? paths : [e.argSummary]) edited.add(path);
        const c = editCounts(e);
        if (c) {
          add += c.add;
          del += c.del;
        } else uncounted = true;
      }
      if (e.verb === "bash") tests = testResult(e.output) ?? tests;
    }
    let tools: Array<{ key: string; tool: ToolEvent }> = [];
    const flush = () => {
      if (tools.length >= FOLD_AT) {
        rows.push({ kind: "tools", key: `${tools[0]?.key}+`, tools: tools.map((t) => t.tool) });
      } else {
        for (const t of tools) rows.push({ kind: "tool", key: t.key, tool: t.tool });
      }
      tools = [];
    };
    for (const item of turn.items) {
      const key = `${turn.id}:${item.seq}`;
      const e = item.event;
      if (e.type === "tool") {
        tools.push({ key, tool: e });
        continue;
      }
      // Usage and resolutions change other rows; they aren't rows themselves.
      if (e.type === "usage" || e.type === "approval_resolved") continue;
      flush();
      switch (e.type) {
        case "user": {
          // An engine switch's divider follows the message it was sent
          // with (a turn starts with its message) but reads above it.
          const divider = turn.items.find((i) => i.event.type === "divider");
          if (divider?.event.type === "divider")
            rows.push({ kind: "divider", key: `${turn.id}:${divider.seq}`, text: divider.event.text });
          // Delivered mail (F-AG-12): its row stands in for the message.
          const mail = turn.items.find((i) => i.event.type === "mail_row" && i.event.inbound);
          if (mail?.event.type === "mail_row") {
            rows.push(mailRow(`${turn.id}:${mail.seq}`, mail.event));
            break;
          }
          rows.push({ kind: "user", key, turnId: turn.id, text: e.text, attachments: e.attachments });
          break;
        }
        case "mail_row":
          // A delivery's row is shown with its message (above).
          if (!e.inbound) rows.push(mailRow(key, e));
          break;
        case "reasoning":
          if (e.text.trim())
            rows.push({
              kind: "reasoning",
              key,
              text: e.text,
              durationMs: e.durationMs,
              streaming: item.streaming,
            });
          break;
        case "assistant":
          if (e.markdown.trim() || item.streaming)
            rows.push({ kind: "assistant", key, markdown: e.markdown, streaming: item.streaming });
          break;
        case "approval_request":
          rows.push({
            kind: "approval",
            key,
            turnId: turn.id,
            request: e,
            decision: decisions.get(e.id) ?? null,
          });
          break;
        case "note":
          rows.push({ kind: "note", key, text: e.text });
          break;
        case "error":
          rows.push({ kind: "error", key, turnId: turn.id, error: e, last: turn === lastTurn });
          break;
        case "limit":
          rows.push({ kind: "limit", key, limit: e });
          break;
        case "divider":
          // Shown with the turn's message (above).
          break;
        case "plan_revision":
          rows.push({ kind: "plan", key, version: e.version });
          break;
        case "turn_end":
          // A clean end needs no row; a stop or blocked actions do.
          if (e.reason === "cancelled" || e.blockedActions > 0) {
            rows.push({ kind: "end", key, reason: e.reason, blocked: e.blockedActions });
          }
          if (e.reason !== "cancelled" && (edited.size > 0 || tests))
            rows.push({
              kind: "summary",
              key: `${key}:summary`,
              files: edited.size,
              add: uncounted ? null : add,
              del: uncounted ? null : del,
              tests,
            });
          break;
        default:
          break;
      }
    }
    flush();
    if (turn.ended === null && turn.id === view.current) {
      rows.push({ kind: "working", key: `${turn.id}:working`, turnId: turn.id });
    }
  }
  return rows;
}

function sameRow(a: Row, b: Row): boolean {
  if (a.kind !== b.kind || a.key !== b.key) return false;
  const x = a as Record<string, unknown>;
  const y = b as Record<string, unknown>;
  for (const k of Object.keys(x)) {
    const u = x[k];
    const v = y[k];
    if (u === v) continue;
    if (!Array.isArray(u) || !Array.isArray(v) || u.length !== v.length || u.some((e, i) => e !== v[i]))
      return false;
  }
  return true;
}

/** `next` with every row that didn't change replaced by its object in
 * `prev`, so memoized rows skip rendering while a turn streams. */
export function reuseRows(prev: Row[], next: Row[]): Row[] {
  if (prev.length === 0) return next;
  const old = new Map(prev.map((r) => [r.key, r]));
  let same = prev.length === next.length;
  const out = next.map((r, i) => {
    const o = old.get(r.key);
    const keep = o && sameRow(o, r) ? o : r;
    if (keep !== prev[i]) same = false;
    return keep;
  });
  return same ? prev : out;
}

/** Approval requests of the running turn still waiting for an answer,
 * oldest first (UI-23: pinned above the composer). */
export function pendingApprovals(
  rows: Row[],
  running: string | null,
): Array<Extract<Row, { kind: "approval" }>> {
  return rows.filter(
    (r): r is Extract<Row, { kind: "approval" }> =>
      r.kind === "approval" && r.decision === null && r.turnId === running,
  );
}

/** Tokens this thread (input + output across turns) and the latest context
 * fill, for the header meter and the composer readout. */
export type Usage = {
  input: number;
  output: number;
  contextFillPct: number | null;
  /** Per engine, in the order first used (F-TH-16). */
  byEngine: Array<{ engineId: string; input: number; output: number }>;
};

/** Tokens over the whole thread; `engineOf` names the engine of each turn. */
export function usageOf(
  view: ThreadView,
  engineOf: (turnId: string) => string | undefined = () => undefined,
): Usage {
  let input = 0;
  let output = 0;
  let contextFillPct: number | null = null;
  const byEngine: Usage["byEngine"] = [];
  for (const turn of view.turns) {
    const engineId = engineOf(turn.id);
    for (const item of turn.items) {
      if (item.event.type !== "usage") continue;
      const i = item.event.input + item.event.cacheRead;
      input += i;
      output += item.event.output;
      if (item.event.contextFillPct !== null) contextFillPct = item.event.contextFillPct;
      if (engineId === undefined) continue;
      let entry = byEngine.find((e) => e.engineId === engineId);
      if (!entry) {
        entry = { engineId, input: 0, output: 0 };
        byEngine.push(entry);
      }
      entry.input += i;
      entry.output += item.event.output;
    }
  }
  return { input, output, contextFillPct, byEngine };
}

/** The worst status in a group: running beats error beats denied beats ok. */
export function groupStatus(tools: ToolEvent[]): ToolEvent["status"] {
  const order: ToolEvent["status"][] = ["running", "pending", "error", "denied", "ok"];
  for (const s of order) if (tools.some((t) => t.status === s)) return s;
  return "ok";
}
