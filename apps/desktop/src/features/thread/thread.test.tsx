import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { EngineView, Thread, ThreadDetail, ThreadEvent, ThreadEventEnvelope, WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useEngines } from "../../stores/engines";
import { useThreads } from "../../stores/threads";
import { useWorkspaces } from "../../stores/workspaces";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { dropOnThread } from "./drops";
import { restoreAfterFailure } from "./ThreadComposer";
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
  return { thread: th, turns: [], running: null, queued: null, draft: "{}", attachments: [], ...extra };
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
    expect(calls.find((c) => c.cmd === "thread_send")?.args).toMatchObject({ threadId: created.id, text: "hello" });
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
    const groups = within(rail).getAllByRole("region").map((g) => g.getAttribute("aria-label"));
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
      thread_get: () => detail(th, { running: "turn_1", turns: [{ id: "turn_1", threadId: th.id, n: 1, engineId: "claude", accountId: null, state: "running", blockedActions: 0, startedAt: Date.now(), endedAt: null }] }),
      thread_events: () => ({ items: [env(th.id, "turn_1", 0, { type: "user", text: "fix it", attachments: [] })], nextBeforeId: null }),
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
      channel?.onmessage(env(th.id, "turn_1", 3, { type: "approval_resolved", id: "apr_1", decision: { type: "allow", scope: "thread" } }));
      channel?.onmessage(env(th.id, "turn_1", 4, { type: "turn_end", reason: "cancelled", blockedActions: 0 }));
    });
    expect(await screen.findByText("Stopped by you")).toBeTruthy();
    expect(screen.getByText("Allowed for this thread")).toBeTruthy();
    expect(screen.queryByTestId("thread-pending")).toBeNull();
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
        throw { code: "engine_missing", message: "Claude Code isn't installed", stage: null, hint: null, retryable: false };
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
    expect(calls.find((c) => c.cmd === "thread_attach_paths")?.args).toEqual({ threadId: th.id, paths: ["/tmp/a/notes.txt"] });
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
