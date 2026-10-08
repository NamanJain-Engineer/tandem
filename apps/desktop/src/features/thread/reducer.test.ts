import { describe, expect, it } from "vitest";
import type { ThreadEvent, ThreadEventEnvelope } from "@tandem/protocol";
import { emptyThread, fromHistory, reduce, type ThreadView } from "./reducer";

const env = (turnId: string, seq: number, event: ThreadEvent): ThreadEventEnvelope => ({
  threadId: "th_1",
  turnId,
  seq,
  ts: 0,
  event,
});
const user = (text: string): ThreadEvent => ({ type: "user", text, attachments: [] });
const delta = (text: string): ThreadEvent => ({ type: "assistant_delta", text });
const assistant = (markdown: string): ThreadEvent => ({ type: "assistant", markdown });
const tool = (status: "running" | "ok"): ThreadEvent => ({
  type: "tool",
  id: "t1",
  verb: "bash",
  argSummary: "npm test",
  status,
  output: status === "ok" ? "42 passed" : null,
  outputTruncated: false,
  outputFile: null,
  diff: null,
  detail: null,
});
const end: ThreadEvent = { type: "turn_end", reason: "completed", blockedActions: 0 };

const run = (envs: ThreadEventEnvelope[], from: ThreadView = emptyThread) => envs.reduce(reduce, from);
const texts = (view: ThreadView, turn = 0) =>
  view.turns[turn]?.items.map((i) =>
    i.event.type === "assistant"
      ? `assistant:${i.event.markdown}`
      : i.event.type === "tool"
        ? `tool:${i.event.status}`
        : i.event.type,
  );

describe("thread reducer (F-TH-03)", () => {
  it("F-TH-03 deltas build the streaming block, and the final text replaces it", () => {
    let view = run([
      env("turn_1", 0, user("hi")),
      env("turn_1", 1, delta("Hel")),
      env("turn_1", 1, delta("lo")),
    ]);
    expect(texts(view)).toEqual(["user", "assistant:Hello"]);
    expect(view.turns[0]?.items[1]?.streaming).toBe(true);
    view = run([env("turn_1", 1, assistant("Hello!")), env("turn_1", 2, end)], view);
    expect(texts(view)).toEqual(["user", "assistant:Hello!", "turn_end"]);
    expect(view.turns[0]?.ended).toBe("completed");
    expect(view.turns[0]?.items.some((i) => i.streaming)).toBe(false);
  });

  it("F-TH-03.3 replaying turn 1's events after turn 2 started changes nothing in turn 2", () => {
    const turn1 = [
      env("turn_1", 0, user("one")),
      env("turn_1", 1, delta("first answer")),
      env("turn_1", 2, tool("running")),
      env("turn_1", 3, end),
    ];
    const view = run([...turn1, env("turn_2", 0, user("two")), env("turn_2", 1, delta("second"))]);
    const replayed = run(turn1, view);
    expect(replayed).toBe(view);
    expect(replayed.current).toBe("turn_2");
    expect(texts(replayed, 1)).toEqual(["user", "assistant:second"]);
    // A stray event of a turn nobody started is ignored too.
    expect(reduce(view, env("turn_9", 4, delta("?")))).toBe(view);
  });

  it("F-TH-03 a tool's updates change its row in place", () => {
    const view = run([
      env("turn_1", 0, user("test it")),
      env("turn_1", 1, tool("running")),
      env("turn_1", 2, delta("Running the tests.")),
      env("turn_1", 3, tool("ok")),
    ]);
    expect(texts(view)).toEqual(["user", "tool:ok", "assistant:Running the tests."]);
  });

  it("F-TH-03 a resync sends the running turn again without doubling its text", () => {
    let view = run([
      env("turn_1", 0, user("hi")),
      env("turn_1", 1, delta("Hel")),
      env("turn_1", 1, delta("lo")),
    ]);
    // The core resends the turn so far: stored rows and the open text.
    view = run(
      [env("turn_1", 0, user("hi")), env("turn_1", 1, assistant("Hello")), env("turn_1", 1, delta(" world"))],
      view,
    );
    expect(texts(view)).toEqual(["user", "assistant:Hello world"]);
  });

  it("F-TH-03 a view opened mid-turn: history, then the live text replaces the partial row", () => {
    const history = [
      env("turn_1", 0, user("old")),
      env("turn_1", 1, assistant("done before")),
      env("turn_1", 2, end),
      env("turn_2", 0, user("new")),
      // The 2 s partial row of the open block.
      env("turn_2", 1, assistant("Thinking")),
    ];
    let view = fromHistory(history);
    expect(view.current).toBe("turn_2");
    expect(view.turns[0]?.ended).toBe("completed");
    view = run(
      [
        env("turn_2", 0, user("new")),
        env("turn_2", 1, assistant("Thinking about it")),
        env("turn_2", 1, delta(".")),
      ],
      view,
    );
    expect(texts(view, 1)).toEqual(["user", "assistant:Thinking about it."]);
  });

  it("F-TH-03 an ended turn takes no more events", () => {
    const view = run([env("turn_1", 0, user("hi")), env("turn_1", 1, end)]);
    expect(reduce(view, env("turn_1", 2, delta("late")))).toBe(view);
  });
});
