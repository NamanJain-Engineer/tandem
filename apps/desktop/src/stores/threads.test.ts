import { act, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Thread, ThreadDetail, ThreadEventEnvelope } from "@tandem/protocol";
import { setInvokeBackend } from "../ipc/invoke";
import { FREE_VIEW, fakeBackend, resetStores, type Call } from "../test/fakeBackend";
import { useThreads } from "./threads";
import { useUi } from "./ui";
import { useSession } from "./session";

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
    // Threads are showing unless a test says otherwise.
    useUi.setState({ mode: "thread", settingsOpen: false });
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

  it("N3: a thread opened for the first time is marked read only once subscribed", async () => {
    unread.thr_a = true;
    await useThreads.getState().refresh();
    useThreads.getState().select("thr_a");
    await waitFor(() => expect(unread.thr_a).toBe(false));
    const cmds = calls.filter((c) => c.args?.threadId === "thr_a").map((c) => c.cmd);
    expect(cmds.indexOf("thread_subscribe")).toBeLessThan(cmds.indexOf("thread_mark_read"));
  });

  it("N3: with the window hidden, a turn that ends on the selected thread leaves it unread until the window is shown", async () => {
    await useThreads.getState().refresh();
    useThreads.getState().select("thr_b");
    await waitFor(() => expect(channels.thr_b).toBeTruthy());
    const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    try {
      endTurn("thr_b");
      await waitFor(() => expect(listed("thr_b")?.unread).toBe(true));
      expect(unread.thr_b).toBe(true);
      hidden.mockReturnValue(false);
      act(() => {
        document.dispatchEvent(new Event("visibilitychange"));
      });
      await waitFor(() => expect(unread.thr_b).toBe(false));
      await waitFor(() => expect(listed("thr_b")?.unread).toBe(false));
    } finally {
      hidden.mockRestore();
    }
  });

  it("N3: a reload in flight when the thread is marked read can't bring the dot back", async () => {
    await useThreads.getState().refresh();
    useThreads.getState().select("thr_a");
    await waitFor(() => expect(channels.thr_a).toBeTruthy());
    useThreads.getState().select("thr_b");
    await waitFor(() => expect(channels.thr_b).toBeTruthy());
    // thr_a's next read is slow: it answers "unread" after the person has
    // opened it.
    let release: () => void = () => undefined;
    const gate = new Promise<void>((r) => (release = r));
    const slowUnread = { ...thread("thr_a"), unread: true };
    setInvokeBackend(async (cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "thread_get" && args?.threadId === "thr_a") {
        await gate;
        return { thread: slowUnread, turns: [], running: null, queued: null, draft: "{}", attachments: [], planRevisions: [] };
      }
      if (cmd === "thread_mark_read") {
        unread[String(args?.threadId)] = false;
        return null;
      }
      return null;
    });
    unread.thr_a = true;
    const reload = useThreads.getState().reloadDetail("thr_a");
    await useThreads.getState().markRead("thr_a");
    release();
    await reload;
    expect(listed("thr_a")?.unread).toBe(false);
  });

  it("N2: in Code mode, or with Settings open, the selected thread isn't on screen: a turn that ends there stays unread until threads show again", async () => {
    await useThreads.getState().refresh();
    useThreads.getState().select("thr_b");
    await waitFor(() => expect(channels.thr_b).toBeTruthy());
    act(() => useUi.setState({ mode: "code" }));
    endTurn("thr_b");
    await waitFor(() => expect(listed("thr_b")?.unread).toBe(true));
    expect(unread.thr_b).toBe(true);
    // Back in Thread mode, but Settings is open over it.
    act(() => useUi.setState({ mode: "thread", settingsOpen: true }));
    await act(() => new Promise((r) => setTimeout(r, 50)));
    expect(unread.thr_b).toBe(true);
    // Settings closed: the thread is on screen, so it's read.
    act(() => useUi.setState({ settingsOpen: false }));
    await waitFor(() => expect(unread.thr_b).toBe(false));
    await waitFor(() => expect(listed("thr_b")?.unread).toBe(false));
  });

  it("N3: a turn that ends after the mark, while a reload from before it is in flight, keeps its dot", async () => {
    await useThreads.getState().refresh();
    useThreads.getState().select("thr_a");
    await waitFor(() => expect(channels.thr_a).toBeTruthy());
    // A slow reload of thr_a, then a mark, then the person leaves and its
    // turn ends off screen (a fast reload says unread), then the slow one
    // lands.
    let release: () => void = () => undefined;
    const gate = new Promise<void>((r) => (release = r));
    let slow = true;
    setInvokeBackend(async (cmd, args) => {
      calls.push({ cmd, args });
      const id = String(args?.threadId);
      if (cmd === "thread_get") {
        const wasSlow = slow && id === "thr_a";
        const reading = unread[id] ?? false;
        if (wasSlow) {
          slow = false;
          await gate;
        }
        return { thread: { ...thread(id), unread: reading }, turns: [], running: null, queued: null, draft: "{}", attachments: [], planRevisions: [] };
      }
      if (cmd === "thread_mark_read") {
        unread[id] = false;
        return null;
      }
      return null;
    });
    unread.thr_a = true;
    const stale = useThreads.getState().reloadDetail("thr_a");
    await useThreads.getState().markRead("thr_a");
    act(() => useUi.setState({ mode: "code" }));
    endTurn("thr_a");
    await waitFor(() => expect(listed("thr_a")?.unread).toBe(true));
    release();
    await stale;
    expect(listed("thr_a")?.unread).toBe(true);
  });

  it("SF2: opening a thread while chats aren't showing (a saved grid restored at a launch in Code mode) leaves it unread until they show", async () => {
    unread.thr_a = true;
    unread.thr_b = true;
    act(() => useUi.setState({ mode: "code" }));
    await useThreads.getState().refresh();
    useThreads.getState().select("thr_a");
    useThreads.getState().addToGrid("thr_b");
    await waitFor(() => expect(channels.thr_b).toBeTruthy());
    await act(() => new Promise((r) => setTimeout(r, 50)));
    expect(calls.some((c) => c.cmd === "thread_mark_read")).toBe(false);
    expect(listed("thr_a")?.unread).toBe(true);
    // Thread mode shows them: both are read.
    act(() => useUi.setState({ mode: "thread" }));
    await waitFor(() => expect(unread.thr_a).toBe(false));
    await waitFor(() => expect(unread.thr_b).toBe(false));
  });

  it("SF3: behind another app (the window visible but not focused), a turn that ends stays unread until the window has focus", async () => {
    await useThreads.getState().refresh();
    useThreads.getState().select("thr_b");
    await waitFor(() => expect(channels.thr_b).toBeTruthy());
    act(() => {
      window.dispatchEvent(new Event("blur"));
    });
    expect(useThreads.getState().windowFocused).toBe(false);
    endTurn("thr_b");
    await waitFor(() => expect(listed("thr_b")?.unread).toBe(true));
    expect(unread.thr_b).toBe(true);
    act(() => {
      window.dispatchEvent(new Event("focus"));
    });
    await waitFor(() => expect(unread.thr_b).toBe(false));
  });

  it("nit 1: a saved grid that the limit doesn't show isn't on screen: a turn ending on its other cell stays unread", async () => {
    useSession.setState({ entitlement: { ...FREE_VIEW, limits: { ...FREE_VIEW.limits, threadGrid: 1 } } });
    await useThreads.getState().refresh();
    useThreads.getState().select("thr_a");
    await waitFor(() => expect(channels.thr_a).toBeTruthy());
    // Saved earlier, under a larger limit.
    act(() => useThreads.setState({ grid: ["thr_a", "thr_b"] }));
    void useThreads.getState().open("thr_b");
    await waitFor(() => expect(channels.thr_b).toBeTruthy());
    endTurn("thr_b");
    await waitFor(() => expect(listed("thr_b")?.unread).toBe(true));
    expect(unread.thr_b).toBe(true);
  });
});
