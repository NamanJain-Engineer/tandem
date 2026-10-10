import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type {
  EngineView,
  Thread,
  ThreadDetail,
  ThreadEvent,
  ThreadEventEnvelope,
  WorkspaceSummary,
} from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useEngines } from "../../stores/engines";
import { useThreads } from "../../stores/threads";
import { useWorkspaces } from "../../stores/workspaces";
import { useSession } from "../../stores/session";
import { useUi } from "../../stores/ui";
import { FREE, FREE_VIEW, fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { referencedFiles } from "./ChangesPanel";
import type { Row, ToolEvent } from "./rows";
import { dropOnThread, insertIntoComposer } from "./drops";
import { engineOptionsFor, restoreAfterFailure } from "./ThreadComposer";
import { age, groupThreads } from "./groups";
import { ThreadMain } from "./ThreadMain";
import { ThreadRail } from "./ThreadRail";

let n = 0;
function thread(extra: Partial<Thread> = {}): Thread {
  n += 1;
  return {
    id: `thr_${n}`,
    kind: "thread",
    workspaceId: null,
    cwd: "/tmp/scratch",
    engineId: "claude",
    accountId: null,
    model: null,
    effort: null,
    access: "read",
    mode: "act",
    title: `Thread ${n}`,
    state: "idle",
    attended: true,
    engineSessionId: null,
    pinned: false,
    archived: false,
    unread: false,
    sortKey: "a",
    createdAt: 1_000 + n,
    updatedAt: 1_000 + n,
    ...extra,
  };
}

const CLAUDE: EngineView = {
  spec: { id: "claude", label: "Claude Code", logo: "CC" } as EngineView["spec"],
  install: { path: "/usr/bin/claude", version: "2.0.0", source: "shell" } as EngineView["install"],
  chatCapable: true,
  unsupportedReason: null,
  extraArgs: [],
};

function detail(th: Thread, extra: Partial<ThreadDetail> = {}): ThreadDetail {
  return {
    thread: th,
    turns: [],
    running: null,
    queued: null,
    draft: "{}",
    attachments: [],
    planRevisions: [],
    ...extra,
  };
}

type Channel = { onmessage: (e: ThreadEventEnvelope) => void };

function env(threadId: string, turnId: string, seq: number, event: ThreadEvent): ThreadEventEnvelope {
  return { threadId, turnId, seq, ts: Date.now(), event };
}

function mount(ui: React.ReactNode) {
  return render(<UiProvider theme="dark">{ui}</UiProvider>);
}

const box = () => screen.getByRole("textbox", { name: "Message" });

describe("Thread mode (F-TH-01, F-TH-02, F-TH-07, F-TH-11, F-TH-12)", () => {
  beforeEach(() => {
    resetStores();
    n = 0;
    useEngines.setState({ list: [CLAUDE], loaded: true });
  });

  it("F-TH-02.1 with no thread, the composer starts one under No folder", async () => {
    const created = thread({ title: "" });
    const calls: Call[] = fakeBackend({
      thread_list: () => [],
      thread_create: () => created,
      thread_get: () => detail(created),
      thread_events: () => ({ items: [], nextBeforeId: null }),
      thread_subscribe: () => null,
      thread_send: () => ({ turnId: "turn_1", queued: false }),
    });
    mount(<ThreadMain />);
    expect(screen.getByText("Start without a folder")).toBeTruthy();
    fireEvent.change(box(), { target: { value: "hello" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(calls.some((c) => c.cmd === "thread_send")).toBe(true));
    const create = calls.find((c) => c.cmd === "thread_create");
    expect(create?.args).toEqual({ args: { kind: "thread", workspaceId: null } });
    expect(calls.find((c) => c.cmd === "thread_send")?.args).toMatchObject({
      threadId: created.id,
      text: "hello",
    });
    expect(useThreads.getState().selectedId).toBe(created.id);
  });

  it("F-TH-01 the rail groups Pinned, No folder and folders; unread shows; archived hides", async () => {
    const ws = { id: "ws_1", name: "shop", root: "/code/shop", branch: "main" } as WorkspaceSummary;
    useWorkspaces.setState({ list: [ws] });
    const list = [
      thread({ title: "Pinned one", pinned: true, workspaceId: "ws_1" }),
      thread({ title: "Loose", unread: true }),
      thread({ title: "In shop", workspaceId: "ws_1", state: "working" }),
      thread({ title: "Old", archived: true }),
    ];
    fakeBackend({ thread_list: () => list });
    await act(async () => {
      await useThreads.getState().refresh();
    });
    mount(<ThreadRail collapsed={false} />);
    const rail = screen.getByRole("navigation", { name: "Threads" });
    const groups = within(rail)
      .getAllByRole("region")
      .map((g) => g.getAttribute("aria-label"));
    expect(groups).toEqual(["Pinned", "No folder", "shop"]);
    expect(within(screen.getByRole("region", { name: "shop" })).getByText("Working")).toBeTruthy();
    expect(screen.getByRole("img", { name: "Finished while you were away" })).toBeTruthy();
    expect(screen.queryByText("Old")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Show archived" }));
    expect(screen.getByText("Old")).toBeTruthy();
  });

  it("F-TH-12 live events stream into the transcript; F-TH-11 the pending card answers inline", async () => {
    const th = thread({ title: "Fix redirect", state: "working" });
    let channel: Channel | null = null;
    const calls = fakeBackend({
      thread_list: () => [th],
      thread_get: () =>
        detail(th, {
          running: "turn_1",
          turns: [
            {
              id: "turn_1",
              threadId: th.id,
              n: 1,
              engineId: "claude",
              accountId: null,
              state: "running",
              blockedActions: 0,
              startedAt: Date.now(),
              endedAt: null,
            },
          ],
        }),
      thread_events: () => ({
        items: [env(th.id, "turn_1", 0, { type: "user", text: "fix it", attachments: [] })],
        nextBeforeId: null,
      }),
      thread_subscribe: (a) => {
        channel = a?.channel as Channel;
        return null;
      },
      approval_resolve: () => null,
      models_list: () => [],
    });
    await act(async () => {
      await useThreads.getState().refresh();
      useThreads.getState().select(th.id);
    });
    mount(<ThreadMain />);
    expect(await screen.findByText("fix it")).toBeTruthy();
    await waitFor(() => expect(channel).not.toBeNull());
    act(() => {
      channel?.onmessage(env(th.id, "turn_1", 1, { type: "assistant_delta", text: "The loop " }));
      channel?.onmessage(env(th.id, "turn_1", 1, { type: "assistant_delta", text: "comes from the guard." }));
      channel?.onmessage(
        env(th.id, "turn_1", 2, {
          type: "approval_request",
          id: "apr_1",
          kind: "command",
          summary: "Run `npm test`",
          detail: "npm test",
          options: ["once", "thread"],
          expiresAt: Date.now() + 60_000,
        }),
      );
    });
    expect(await screen.findByText("The loop comes from the guard.")).toBeTruthy();
    expect(screen.getByTestId("thread-working")).toBeTruthy();
    const pinned = await screen.findByTestId("thread-pending");
    fireEvent.click(within(pinned).getByRole("button", { name: "Allow for this thread" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "approval_resolve")).toBe(true));
    expect(calls.find((c) => c.cmd === "approval_resolve")?.args).toEqual({
      approvalId: "apr_1",
      decision: { type: "allow", scope: "thread" },
      via: "inline",
    });
    act(() => {
      channel?.onmessage(
        env(th.id, "turn_1", 3, {
          type: "approval_resolved",
          id: "apr_1",
          decision: { type: "allow", scope: "thread" },
        }),
      );
      channel?.onmessage(
        env(th.id, "turn_1", 4, { type: "turn_end", reason: "cancelled", blockedActions: 0 }),
      );
    });
    expect(await screen.findByText("Stopped by you")).toBeTruthy();
    expect(screen.getByText("Allowed for this thread")).toBeTruthy();
    expect(screen.queryByTestId("thread-pending")).toBeNull();
  });

  it("F-TH-01.6 a thread a crash interrupted says Interrupted — Resume? and Resume sends its turn again", async () => {
    const th = thread({ title: "Crashed", state: "interrupted" });
    const calls = fakeBackend({
      thread_list: () => [th],
      thread_get: () =>
        detail(th, {
          turns: [
            {
              id: "turn_1",
              threadId: th.id,
              n: 1,
              engineId: "claude",
              accountId: null,
              state: "interrupted",
              blockedActions: 0,
              startedAt: Date.now(),
              endedAt: Date.now(),
            },
          ],
        }),
      thread_events: () => ({
        items: [env(th.id, "turn_1", 0, { type: "user", text: "long job", attachments: [] })],
        nextBeforeId: null,
      }),
      thread_subscribe: () => null,
      thread_retry: () => ({ turnId: "turn_2", queued: false }),
      models_list: () => [],
    });
    await act(async () => {
      await useThreads.getState().refresh();
      useThreads.getState().select(th.id);
    });
    mount(<ThreadMain />);
    const banner = await screen.findByTestId("thread-interrupted");
    expect(banner.textContent).toContain("Interrupted — Resume?");
    // Never shown as done.
    expect(screen.getByTestId("thread-state").textContent).toBe("Interrupted");
    fireEvent.click(within(banner).getByRole("button", { name: "Resume" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "thread_retry")).toBe(true));
    expect(calls.find((c) => c.cmd === "thread_retry")?.args).toEqual({ threadId: th.id });
  });

  it("F-TH-12 a turn that ends before history loads shows once (subscribe first, then history)", async () => {
    const th = thread({ title: "Quick" });
    const turn: ThreadEventEnvelope[] = [
      env(th.id, "turn_1", 0, { type: "user", text: "hi", attachments: [] }),
      env(th.id, "turn_1", 1, { type: "assistant", markdown: "Quick reply." }),
      env(th.id, "turn_1", 2, { type: "turn_end", reason: "completed", blockedActions: 0 }),
    ];
    const order: string[] = [];
    fakeBackend({
      thread_list: () => [th],
      thread_get: () => detail(th),
      thread_events: () => {
        order.push("history");
        return { items: turn, nextBeforeId: null };
      },
      thread_subscribe: (a) => {
        order.push("subscribe");
        // The whole turn streams before the history read.
        for (const e of turn) (a?.channel as Channel).onmessage(e);
        return null;
      },
      models_list: () => [],
    });
    await act(async () => {
      await useThreads.getState().refresh();
      useThreads.getState().select(th.id);
    });
    mount(<ThreadMain />);
    expect(await screen.findByText("Quick reply.")).toBeTruthy();
    expect(screen.getAllByText("Quick reply.")).toHaveLength(1);
    expect(order).toEqual(["subscribe", "history"]);
  });

  it("F-TH-10.1 each plan card shows its own version; Implement sends that version and Revise pre-fills", async () => {
    const th = thread({ mode: "plan" });
    const plan = (v: number) => ({
      version: v,
      turnId: `turn_${v}`,
      text: `1. Step of plan ${v}`,
      createdAt: 1_000 * v,
    });
    const items: ThreadEventEnvelope[] = [1, 2].flatMap((v) => [
      env(th.id, `turn_${v}`, 0, { type: "user", text: `plan it ${v}`, attachments: [] }),
      env(th.id, `turn_${v}`, 1, { type: "assistant", markdown: `1. Step of plan ${v}` }),
      env(th.id, `turn_${v}`, 2, { type: "plan_revision", version: v }),
      env(th.id, `turn_${v}`, 3, { type: "turn_end", reason: "completed", blockedActions: 0 }),
    ]);
    const calls: Call[] = fakeBackend({
      thread_list: () => [th],
      thread_get: () => detail(th, { planRevisions: [plan(1), plan(2)] }),
      thread_events: () => ({ items, nextBeforeId: null }),
      thread_subscribe: () => null,
      thread_implement_plan: () => ({ turnId: "turn_3", queued: false }),
      models_list: () => [],
    });
    await act(async () => {
      await useThreads.getState().refresh();
      useThreads.getState().select(th.id);
    });
    mount(<ThreadMain />);
    const cards = await screen.findAllByTestId("thread-plan");
    expect(cards.map((c) => c.getAttribute("data-version"))).toEqual(["1", "2"]);
    expect(within(cards[0] as HTMLElement).getByText("Plan · version 1")).toBeTruthy();
    fireEvent.click(within(cards[0] as HTMLElement).getByRole("button", { name: "Implement this plan" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "thread_implement_plan")).toBe(true));
    expect(calls.find((c) => c.cmd === "thread_implement_plan")?.args).toMatchObject({
      threadId: th.id,
      version: 1,
    });
    fireEvent.click(within(cards[1] as HTMLElement).getByRole("button", { name: "Revise…" }));
    await waitFor(() =>
      expect((screen.getByRole("textbox", { name: /message/i }) as HTMLTextAreaElement).value).toBe(
        "Revise the plan: ",
      ),
    );
    // The header lists both versions.
    expect(screen.getByTestId("plan-versions").textContent).toBe("Plan versions (2)");
  });

  it("F-TH-13 the Changes panel lists files with their diff; Files opens what the thread read or edited", async () => {
    const th = thread({ title: "Edits" });
    const tool = (id: string, verb: "read" | "edit", path: string) =>
      ({
        type: "tool",
        id,
        verb,
        argSummary: path,
        status: "ok",
        output: null,
        outputTruncated: false,
        outputFile: null,
        diff: null,
        detail: null,
      }) as ThreadEvent;
    const items: ThreadEventEnvelope[] = [
      env(th.id, "turn_1", 0, { type: "user", text: "edit two files", attachments: [] }),
      env(th.id, "turn_1", 1, tool("t1", "read", "src/a.ts")),
      env(th.id, "turn_1", 2, tool("t2", "edit", "src/a.ts")),
      env(th.id, "turn_1", 3, tool("t3", "edit", "src/b.ts")),
      env(th.id, "turn_1", 4, { type: "turn_end", reason: "completed", blockedActions: 0 }),
    ];
    const patch =
      "diff --git a/src/a.ts b/src/a.ts\n--- a/src/a.ts\n+++ b/src/a.ts\n@@ -1 +1 @@\n-old line\n+new line\n";
    const calls: Call[] = fakeBackend({
      thread_list: () => [th],
      thread_get: () => detail(th),
      thread_events: () => ({ items, nextBeforeId: null }),
      thread_subscribe: () => null,
      thread_changes: () => ({
        source: "git",
        files: [
          {
            path: "src/a.ts",
            change: "modified",
            additions: 1,
            deletions: 1,
            binary: false,
            patch,
            truncated: false,
          },
          {
            path: "src/b.ts",
            change: "added",
            additions: 2,
            deletions: 0,
            binary: false,
            patch: "",
            truncated: false,
          },
        ],
      }),
      thread_file_read: (a) => ({
        path: a?.path,
        content: "export const b = 1;\n",
        truncated: false,
        binary: false,
      }),
      models_list: () => [],
    });
    await act(async () => {
      await useThreads.getState().refresh();
      useThreads.getState().select(th.id);
    });
    mount(<ThreadMain />);
    fireEvent.click(await screen.findByTestId("changes-toggle"));
    const rows = await screen.findAllByTestId("change-row");
    expect(rows.map((r) => r.getAttribute("data-path"))).toEqual(["src/a.ts", "src/b.ts"]);
    expect(screen.getByText("Changes (2)")).toBeTruthy();
    fireEvent.click(rows[0] as HTMLElement);
    expect(within(await screen.findByTestId("change-diff")).getByText("new line")).toBeTruthy();
    expect(calls.filter((c) => c.cmd === "thread_changes")).toHaveLength(1);

    fireEvent.mouseDown(screen.getByRole("tab", { name: /Files/ }));
    const files = await screen.findAllByTestId("file-row");
    expect(files.map((f) => f.textContent)).toEqual(["src/b.tsEdited", "src/a.tsEdited"]);
    fireEvent.click(files[0] as HTMLElement);
    expect(await screen.findByText("export const b = 1;")).toBeTruthy();
    expect(calls.find((c) => c.cmd === "thread_file_read")?.args).toMatchObject({
      threadId: th.id,
      path: "src/b.ts",
    });
    act(() => useThreads.getState().toggleChanges());
    expect(screen.queryByTestId("changes-panel")).toBeNull();
  });

  it("F-TH-13 files the thread touched: latest first, edited wins over read, commands left out", () => {
    const tool = (verb: ToolEvent["verb"], argSummary: string) =>
      ({
        type: "tool",
        id: argSummary,
        verb,
        argSummary,
        status: "ok",
        output: null,
        outputTruncated: false,
        outputFile: null,
        diff: null,
        detail: null,
      }) as ToolEvent;
    const rows: Row[] = [
      { kind: "tool", key: "1", tool: tool("edit", "a.ts") },
      {
        kind: "tools",
        key: "2",
        tools: [tool("read", "b.ts"), tool("bash", "npm test"), tool("read", "a.ts")],
      },
    ];
    expect(referencedFiles(rows)).toEqual([
      { path: "a.ts", wrote: true },
      { path: "b.ts", wrote: false },
    ]);
  });

  it("F-TH-14.1 threads dropped from the rail show side by side, up to 6; Free shows 1", async () => {
    const threads = Array.from({ length: 7 }, (_, i) => thread({ title: `Grid ${i}` }));
    const calls: Call[] = fakeBackend({
      thread_list: () => threads,
      thread_get: (a) => detail(threads.find((x) => x.id === a?.threadId) ?? (threads[0] as Thread)),
      thread_events: () => ({ items: [], nextBeforeId: null }),
      thread_subscribe: () => null,
      models_list: () => [],
      settings_set: () => null,
    });
    act(() => useSession.setState({ entitlement: { ...FREE_VIEW, limits: { ...FREE, threadGrid: 6 } } }));
    await act(async () => {
      await useThreads.getState().refresh();
      useThreads.getState().select(threads[0]?.id ?? null);
    });
    mount(<ThreadMain />);
    await screen.findByTestId("thread-main");
    const area = screen.getByTestId("thread-area");
    const drop = (id: string) => {
      const data = new Map([["application/x-vibezeo-thread", id]]);
      const dataTransfer = {
        types: [...data.keys()],
        getData: (k: string) => data.get(k) ?? "",
        dropEffect: "none",
      };
      fireEvent.dragOver(area, { dataTransfer });
      fireEvent.drop(area, { dataTransfer });
    };
    for (const th of threads.slice(1)) act(() => drop(th.id));
    const cells = await screen.findAllByTestId("grid-cell");
    expect(cells.map((c) => c.getAttribute("data-thread-id"))).toEqual(
      threads.slice(0, 6).map((th) => th.id),
    );
    expect(await screen.findByText("Grid is full (6)")).toBeTruthy();
    // The layout is saved for the next launch.
    const saved = calls.filter((c) => c.cmd === "settings_set" && c.args?.key === "thread_grid").at(-1);
    expect(saved?.args?.value).toEqual({ ids: threads.slice(0, 6).map((th) => th.id) });
    // Mod+Alt+Arrow moves between cells; closing one keeps the rest.
    // The thread dropped last is the open one.
    expect(useThreads.getState().selectedId).toBe(threads[5]?.id);
    act(() => {
      useThreads.getState().setGridColumns(3);
      useThreads.getState().select(threads[0]?.id ?? null);
    });
    act(() => useThreads.getState().focusCell("right"));
    expect(useThreads.getState().selectedId).toBe(threads[1]?.id);
    act(() => useThreads.getState().focusCell("down"));
    expect(useThreads.getState().selectedId).toBe(threads[4]?.id);
    fireEvent.click(within(cells[4] as HTMLElement).getByTestId("grid-close"));
    await waitFor(() => expect(screen.getAllByTestId("grid-cell")).toHaveLength(5));

    // Free: a second thread stays in the list.
    act(() => {
      useThreads.getState().removeFromGrid(threads[1]?.id ?? "");
      for (const th of threads.slice(2, 6)) useThreads.getState().removeFromGrid(th.id);
      useSession.setState({ entitlement: FREE_VIEW });
    });
    expect(screen.queryAllByTestId("grid-cell")).toHaveLength(0);
    act(() => drop(threads[6]?.id ?? ""));
    expect(await screen.findByText("Free shows 1 thread at a time")).toBeTruthy();
    expect(screen.queryAllByTestId("grid-cell")).toHaveLength(0);
  });

  it("F-TH-07.3 a failed send puts the text back ahead of anything typed since", async () => {
    expect(restoreAfterFailure("first message", "")).toBe("first message");
    expect(restoreAfterFailure("first message", "typed later")).toBe("first message\ntyped later");
    const th = thread();
    fakeBackend({
      thread_list: () => [th],
      thread_get: () => detail(th),
      thread_events: () => ({ items: [], nextBeforeId: null }),
      thread_subscribe: () => null,
      models_list: () => [],
      thread_send: () => {
        throw {
          code: "engine_missing",
          message: "Claude Code isn't installed",
          stage: null,
          hint: null,
          retryable: false,
        };
      },
    });
    await act(async () => {
      await useThreads.getState().refresh();
      useThreads.getState().select(th.id);
    });
    mount(<ThreadMain />);
    await screen.findByText("Start without a folder");
    fireEvent.change(box(), { target: { value: "please fix" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect((box() as HTMLTextAreaElement).value).toBe("please fix"));
  });

  it("F-TH-07 files dropped on the thread become composer chips", async () => {
    const th = thread();
    const calls = fakeBackend({
      thread_list: () => [th],
      thread_get: () => detail(th),
      thread_events: () => ({ items: [], nextBeforeId: null }),
      thread_subscribe: () => null,
      models_list: () => [],
      thread_attach_paths: (a) =>
        ((a?.paths as string[]) ?? []).map((p, i) => ({
          id: `att_${i}`,
          threadId: th.id,
          kind: "file",
          path: p,
          name: p.split("/").pop() ?? p,
          mime: null,
          bytes: 3,
          sent: false,
          error: null,
        })),
    });
    await act(async () => {
      await useThreads.getState().refresh();
      useThreads.getState().select(th.id);
    });
    mount(<ThreadMain />);
    await screen.findByRole("textbox", { name: "Message" });
    await act(async () => {
      expect(await dropOnThread(["/tmp/a/notes.txt"], () => undefined)).toBe(true);
    });
    expect(await screen.findByText("notes.txt")).toBeTruthy();
    expect(calls.find((c) => c.cmd === "thread_attach_paths")?.args).toEqual({
      threadId: th.id,
      paths: ["/tmp/a/notes.txt"],
    });
  });

  it("F-CD-18.2 a brief sent to the thread goes after its draft and what's typed, and isn't sent", async () => {
    const th = thread();
    let loaded: () => void = () => undefined;
    const draftIn = new Promise<void>((resolve) => (loaded = resolve));
    const calls = fakeBackend({
      thread_list: () => [th],
      thread_get: async () => {
        await draftIn;
        return detail(th, { draft: JSON.stringify({ text: "from before", attachmentIds: [] }) });
      },
      thread_events: () => ({ items: [], nextBeforeId: null }),
      thread_subscribe: () => null,
      models_list: () => [],
    });
    await act(async () => {
      await useThreads.getState().refresh();
      useThreads.getState().select(th.id);
    });
    mount(<ThreadMain />);
    await screen.findByRole("textbox", { name: "Message" });
    // Sent while the composer waits for its draft: the draft doesn't replace it.
    act(() => insertIntoComposer(th.id, "# Picked elements\n\nPage: http://localhost:5173/"));
    await act(async () => loaded());
    await waitFor(() =>
      expect((box() as HTMLTextAreaElement).value).toBe("from before\n\n# Picked elements\n\nPage: http://localhost:5173/"),
    );
    fireEvent.change(box(), { target: { value: "look at these  " } });
    act(() => insertIntoComposer(th.id, "# Picked elements"));
    await waitFor(() => expect((box() as HTMLTextAreaElement).value).toBe("look at these\n\n# Picked elements"));
    expect(calls.some((c) => c.cmd === "thread_send")).toBe(false);
  });

  it("groups and ages are pure", () => {
    const list = [thread({ title: "a b" }), thread({ title: "c", archived: true })];
    expect(groupThreads(list, [], { showArchived: false, query: "" }).map((g) => g.kind)).toEqual(["none"]);
    expect(groupThreads(list, [], { showArchived: false, query: "zzz" })).toEqual([]);
    const now = 10 * 24 * 3600 * 1000;
    expect(age(now - 5_000, now)).toBe("now");
    expect(age(now - 5 * 60_000, now)).toBe("5m");
    expect(age(now - 3 * 3600_000, now)).toBe("3h");
    expect(age(now - 30 * 3600_000, now)).toBe("yesterday");
  });
});

const chatEngine = (id: string, label: string, logo: string): EngineView => ({
  ...CLAUDE,
  spec: { id, label, logo } as EngineView["spec"],
});
const CODEX = chatEngine("codex", "Codex", "CX");
const OPENCODE = chatEngine("opencode", "OpenCode", "OC");
const GROK = chatEngine("grok", "Grok", "GK");
const PRO_VIEW = { ...FREE_VIEW, plan: "pro" as const, limits: { ...FREE, chatEngines: null } };

describe("Free plan chat engines (F-TH-06, PRODUCT_SPEC §3.2)", () => {
  beforeEach(() => {
    resetStores();
    n = 0;
    useEngines.setState({ list: [CLAUDE, CODEX, OPENCODE, GROK], loaded: true });
  });

  it("F-TH-06 on Free the engine picker enables Claude Code and Codex; the others show Pro", () => {
    const options = engineOptionsFor([CLAUDE, CODEX, OPENCODE, GROK], FREE.chatEngines);
    expect(options.map((o) => [o.value, Boolean(o.disabled), o.disabledReason ?? null])).toEqual([
      ["claude", false, null],
      ["codex", false, null],
      ["opencode", true, "Pro"],
      ["grok", true, "Pro"],
    ]);
  });

  it("F-TH-06 on Pro every chat engine is enabled", () => {
    const options = engineOptionsFor([CLAUDE, CODEX, OPENCODE, GROK], null);
    expect(options.every((o) => !o.disabled)).toBe(true);
  });

  it("F-TH-06 a thread on a Pro-only engine shows the Free gate; See plans opens the plans", async () => {
    const th = thread({ engineId: "opencode" });
    fakeBackend({
      thread_list: () => [th],
      thread_get: () => detail(th),
      thread_events: () => ({ items: [], nextBeforeId: null }),
      thread_subscribe: () => null,
      models_list: () => [],
    });
    await act(async () => {
      await useThreads.getState().refresh();
      useThreads.getState().select(th.id);
    });
    mount(<ThreadMain />);
    const gate = await screen.findByTestId("thread-engine-gate");
    expect(gate.textContent).toContain("Free threads run Claude Code and Codex");
    expect(gate.textContent).toContain("OpenCode needs Pro");
    fireEvent.click(within(gate).getByRole("button", { name: "See plans" }));
    expect(useUi.getState().upgrade).toMatchObject({ open: true, gate: { kind: "chat_engines" } });
  });

  it("F-TH-06 on Pro the same thread shows no gate", async () => {
    useSession.setState({ entitlement: PRO_VIEW });
    const th = thread({ engineId: "opencode" });
    fakeBackend({
      thread_list: () => [th],
      thread_get: () => detail(th),
      thread_events: () => ({ items: [], nextBeforeId: null }),
      thread_subscribe: () => null,
      models_list: () => [],
      entitlement_get: () => PRO_VIEW,
    });
    await act(async () => {
      await useThreads.getState().refresh();
      useThreads.getState().select(th.id);
    });
    mount(<ThreadMain />);
    await screen.findByRole("textbox", { name: "Message" });
    expect(screen.queryByTestId("thread-engine-gate")).toBeNull();
  });
});
