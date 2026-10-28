import { act, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { Thread, ThreadDetail, ThreadEventEnvelope } from "@tandem/protocol";
import { fakeBackend, resetStores, type Call } from "../test/fakeBackend";
import { useThreads } from "./threads";

type Channel = { onmessage: (e: ThreadEventEnvelope) => void };

function thread(id: string): Thread {
  return {
    id,
    kind: "thread",
    workspaceId: null,
    cwd: "/tmp/scratch",
    engineId: "claude",
    accountId: null,
    model: null,
    effort: null,
    access: "read",
    mode: "act",
    title: id,
    state: "idle",
    attended: true,
    engineSessionId: null,
    pinned: false,
    archived: false,
    unread: false,
    sortKey: "a",
    createdAt: 1,
    updatedAt: 1,
    teammateId: null,
    conversationRef: null,
  };
}

describe("F-TH-01 AC2: a thread is read when it's on screen, not when it's read", () => {
  // The core's side: unread per thread, set at a turn's end, cleared only by
  // thread_mark_read (thread_get never clears it).
  let unread: Record<string, boolean>;
  let channels: Record<string, Channel>;
  let calls: Call[];
  beforeEach(() => {
    resetStores();
    unread = { thr_a: false, thr_b: false };
    channels = {};
    const detail = (id: string): ThreadDetail => ({
      thread: { ...thread(id), unread: unread[id] ?? false },
      turns: [],
      running: null,
      queued: null,
      draft: "{}",
      attachments: [],
      planRevisions: [],
    });
    calls = fakeBackend({
      thread_list: () => ["thr_a", "thr_b"].map((id) => ({ ...thread(id), unread: unread[id] })),
      thread_get: (a) => detail(String(a?.threadId)),
      thread_events: () => ({ items: [], nextBeforeId: null }),
      thread_subscribe: (a) => {
        channels[String(a?.threadId)] = a?.channel as Channel;
        return null;
      },
      thread_mark_read: (a) => {
        unread[String(a?.threadId)] = false;
        return null;
      },
    });
  });

  const endTurn = (id: string) => {
    // The core marks it unread, then says the turn ended.
    unread[id] = true;
    act(() =>
      channels[id]?.onmessage({
        threadId: id,
        turnId: "turn_1",
        seq: 1,
        ts: Date.now(),
        event: { type: "turn_end", reason: "completed", blockedActions: 0 },
      }),
    );
  };
  const listed = (id: string) => useThreads.getState().list.find((t) => t.id === id);

  it("M1: a thread opened earlier but off screen keeps its unread dot when its turn ends", async () => {
    await useThreads.getState().refresh();
    useThreads.getState().select("thr_a");
    await waitFor(() => expect(channels.thr_a).toBeTruthy());
    useThreads.getState().select("thr_b");
    await waitFor(() => expect(channels.thr_b).toBeTruthy());
    const before = calls.length;
    endTurn("thr_a");
    // The reload on turn_end runs, and reads it as unread.
    await waitFor(() => expect(calls.slice(before).some((c) => c.cmd === "thread_get")).toBe(true));
    await waitFor(() => expect(listed("thr_a")?.unread).toBe(true));
    expect(calls.slice(before).some((c) => c.cmd === "thread_mark_read")).toBe(false);
    expect(unread.thr_a).toBe(true);
  });

  it("M1: a turn that ends on the thread on screen leaves it read", async () => {
    await useThreads.getState().refresh();
    useThreads.getState().select("thr_b");
    await waitFor(() => expect(channels.thr_b).toBeTruthy());
    endTurn("thr_b");
    await waitFor(() => expect(unread.thr_b).toBe(false));
    await waitFor(() => expect(listed("thr_b")?.unread).toBe(false));
    // Marked before the reload, so the reload can't bring the dot back.
    const after = calls.filter((c) => c.args?.threadId === "thr_b").map((c) => c.cmd);
    expect(after.lastIndexOf("thread_mark_read")).toBeLessThan(after.lastIndexOf("thread_get"));
  });

  it("M1: a thread in the grid is on screen too", async () => {
    await useThreads.getState().refresh();
    useThreads.getState().select("thr_a");
    await waitFor(() => expect(channels.thr_a).toBeTruthy());
    useThreads.getState().addToGrid("thr_b");
    await waitFor(() => expect(channels.thr_b).toBeTruthy());
    expect(useThreads.getState().grid).toEqual(expect.arrayContaining(["thr_a", "thr_b"]));
    endTurn("thr_a");
    await waitFor(() => expect(unread.thr_a).toBe(false));
  });

  it("M1: opening an unread thread marks it read", async () => {
    unread.thr_a = true;
    await useThreads.getState().refresh();
    expect(listed("thr_a")?.unread).toBe(true);
    useThreads.getState().select("thr_a");
    await waitFor(() => expect(unread.thr_a).toBe(false));
    await waitFor(() => expect(listed("thr_a")?.unread).toBe(false));
  });
});
