import type { ThreadEvent, ThreadEventEnvelope, TurnEndReason } from "@tandem/protocol";

/** One row of a turn: an event at its place (`seq`). `streaming` while its
 * text is still arriving as deltas. */
export type Item = { seq: number; event: ThreadEvent; streaming: boolean };

export type TurnView = {
  id: string;
  /** Ordered by seq. A tool's updates replace its row. */
  items: Item[];
  ended: TurnEndReason | null;
};

export type ThreadView = {
  turns: TurnView[];
  /** The turn live events may still change (F-TH-03 AC3). */
  current: string | null;
};

export const emptyThread: ThreadView = { turns: [], current: null };

function insert(items: Item[], item: Item): Item[] {
  const at = items.findIndex((i) => i.seq >= item.seq);
  if (at < 0) return [...items, item];
  if (items[at]?.seq === item.seq) return items.map((i, n) => (n === at ? item : i));
  return [...items.slice(0, at), item, ...items.slice(at)];
}

function append(turn: TurnView, env: ThreadEventEnvelope): TurnView {
  const { event, seq } = env;
  if (event.type === "assistant_delta" || event.type === "reasoning_delta") {
    const existing = turn.items.find((i) => i.seq === seq);
    const before =
      existing?.event.type === "assistant"
        ? existing.event.markdown
        : existing?.event.type === "reasoning"
          ? existing.event.text
          : "";
    const text = before + event.text;
    const next: ThreadEvent =
      event.type === "assistant_delta"
        ? { type: "assistant", markdown: text }
        : { type: "reasoning", text, durationMs: null };
    return { ...turn, items: insert(turn.items, { seq, event: next, streaming: true }) };
  }
  if (event.type === "tool") {
    // Updates of a tool (running → ok) change its row in place.
    const at = turn.items.findIndex((i) => i.event.type === "tool" && i.event.id === event.id);
    if (at >= 0) {
      return { ...turn, items: turn.items.map((i, n) => (n === at ? { ...i, event } : i)) };
    }
  }
  const items = insert(turn.items, { seq, event, streaming: false });
  if (event.type === "turn_end") {
    return {
      ...turn,
      ended: event.reason,
      items: items.map((i) => (i.streaming ? { ...i, streaming: false } : i)),
    };
  }
  return { ...turn, items };
}

/** Applies one envelope. Pure: the same input always gives the same view. */
export function reduce(state: ThreadView, env: ThreadEventEnvelope): ThreadView {
  const at = state.turns.findIndex((t) => t.id === env.turnId);
  if (env.turnId !== state.current) {
    // A turn starts with its message (seq 0), or a mail row of its own
    // (F-AG-12); anything else from another turn is a straggler and changes
    // nothing.
    const opens = env.event.type === "user" || env.event.type === "mail_row";
    if (at >= 0 || env.seq !== 0 || !opens) return state;
    return {
      turns: [
        ...state.turns,
        { id: env.turnId, items: [{ seq: 0, event: env.event, streaming: false }], ended: null },
      ],
      current: env.turnId,
    };
  }
  const turn = state.turns[at];
  if (!turn || turn.ended !== null) return state;
  const next = append(turn, env);
  return { ...state, turns: state.turns.map((t, n) => (n === at ? next : t)) };
}

/** Stored events (`thread_events`, oldest first) as a view; the last turn
 * stays current, so live events continue it. */
export function fromHistory(envs: ThreadEventEnvelope[]): ThreadView {
  const turns: TurnView[] = [];
  for (const env of envs) {
    let turn = turns.find((t) => t.id === env.turnId);
    if (!turn) {
      turn = { id: env.turnId, items: [], ended: null };
      turns.push(turn);
    }
    const next = append(turn, env);
    turns[turns.indexOf(turn)] = next;
  }
  // History rows are final text, even a partial row a crash left.
  const settled = turns.map((t) => ({ ...t, items: t.items.map((i) => ({ ...i, streaming: false })) }));
  return { turns: settled, current: settled[settled.length - 1]?.id ?? null };
}
