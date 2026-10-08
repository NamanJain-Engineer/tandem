import type { ApprovalDecisionView, ThreadEvent, TurnEndReason } from "@tandem/protocol";
import type { ThreadView } from "./reducer";

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
  | { kind: "approval"; key: string; turnId: string; request: ApprovalRequest; decision: ApprovalDecisionView | null }
  | { kind: "note"; key: string; text: string }
  | { kind: "error"; key: string; turnId: string; error: Ev<"error">; last: boolean }
  | { kind: "limit"; key: string; limit: Ev<"limit"> }
  | { kind: "divider"; key: string; text: string }
  | { kind: "plan"; key: string; version: number }
  | { kind: "end"; key: string; reason: TurnEndReason; blocked: number }
  | { kind: "working"; key: string; turnId: string };

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
        case "user":
          rows.push({ kind: "user", key, turnId: turn.id, text: e.text, attachments: e.attachments });
          break;
        case "reasoning":
          if (e.text.trim()) rows.push({ kind: "reasoning", key, text: e.text, durationMs: e.durationMs, streaming: item.streaming });
          break;
        case "assistant":
          if (e.markdown.trim() || item.streaming) rows.push({ kind: "assistant", key, markdown: e.markdown, streaming: item.streaming });
          break;
        case "approval_request":
          rows.push({ kind: "approval", key, turnId: turn.id, request: e, decision: decisions.get(e.id) ?? null });
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
          rows.push({ kind: "divider", key, text: e.text });
          break;
        case "plan_revision":
          rows.push({ kind: "plan", key, version: e.version });
          break;
        case "turn_end":
          // A clean end needs no row; a stop or blocked actions do.
          if (e.reason === "cancelled" || e.blockedActions > 0) {
            rows.push({ kind: "end", key, reason: e.reason, blocked: e.blockedActions });
          }
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

/** Approval requests of the running turn still waiting for an answer,
 * oldest first (UI-23: pinned above the composer). */
export function pendingApprovals(rows: Row[], running: string | null): Array<Extract<Row, { kind: "approval" }>> {
  return rows.filter(
    (r): r is Extract<Row, { kind: "approval" }> => r.kind === "approval" && r.decision === null && r.turnId === running,
  );
}

/** Tokens this thread (input + output across turns) and the latest context
 * fill, for the header meter and the composer readout. */
export function usageOf(view: ThreadView): { input: number; output: number; contextFillPct: number | null } {
  let input = 0;
  let output = 0;
  let contextFillPct: number | null = null;
  for (const turn of view.turns) {
    for (const item of turn.items) {
      if (item.event.type !== "usage") continue;
      input += item.event.input + item.event.cacheRead;
      output += item.event.output;
      if (item.event.contextFillPct !== null) contextFillPct = item.event.contextFillPct;
    }
  }
  return { input, output, contextFillPct };
}

/** The worst status in a group: running beats error beats denied beats ok. */
export function groupStatus(tools: ToolEvent[]): ToolEvent["status"] {
  const order: ToolEvent["status"][] = ["running", "pending", "error", "denied", "ok"];
  for (const s of order) if (tools.some((t) => t.status === s)) return s;
  return "ok";
}
