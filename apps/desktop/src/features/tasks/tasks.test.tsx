import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { IpcError, Issue, IssueSummary, SaveIssue, TaskComment, TasksMeta } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { useTasks } from "../../stores/tasks";
import { FREE_VIEW, SIGNED_IN, fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { useEngines } from "../../stores/engines";
import { useSession } from "../../stores/session";
import { useWorkspaces } from "../../stores/workspaces";
import { draftOf, edit, rebase, shown } from "./merge";
import { TasksPanel } from "./TasksPanel";

function issue(n: number, over: Partial<Issue> = {}): Issue {
  return {
    id: `TAN-${n}`,
    number: n,
    revision: 1,
    title: `Task ${n}`,
    description: "",
    status: "todo",
    priority: 0,
    labels: [],
    project: null,
    assignee: null,
    created_at: n,
    updated_at: n,
    archived: false,
    result: null,
    attempts: [],
    ...over,
  };
}

const META: TasksMeta = {
  version: 1,
  next_number: 1,
  projects: [],
  labels: [],
  statuses: ["backlog", "todo", "in_progress", "in_review", "done", "canceled"],
  actors: [
    { id: "me", kind: "human", name: "You" },
    { id: "tm_lead", kind: "teammate", name: "Lead" },
  ],
  teams: [{ id: "local", name: "Local" }],
};

const summary = (i: Issue): IssueSummary => ({
  id: i.id,
  number: i.number,
  revision: i.revision,
  title: i.title,
  status: i.status,
  priority: i.priority,
  labels: i.labels,
  project: i.project,
  assignee: i.assignee,
  updated_at: i.updated_at,
  archived: i.archived,
});

const conflict = (rev: number): IpcError => ({
  code: "revision_conflict",
  message: `changed (now at revision ${rev})`,
  stage: null,
  hint: null,
  retryable: false,
});

/** A fake tracker: the commands, plus `agent()` writing like an MCP server. */
function tracker(initial: Issue[] = []) {
  const issues = new Map(initial.map((i) => [i.id, i]));
  const comments: TaskComment[] = [];
  let next = initial.length + 1;
  const write = (id: string, patch: Partial<Issue>) => {
    const cur = issues.get(id)!;
    const out = { ...cur, ...patch, revision: cur.revision + 1, updated_at: cur.updated_at + 100 };
    issues.set(id, out);
    return out;
  };
  const calls = fakeBackend({
    tasks_meta: () => META,
    task_attempts_list: () => [],
    teammate_list: () => [],
    tasks_list: (a) => {
      const all = (a?.filter as { include_archived?: boolean }).include_archived;
      return {
        items: [...issues.values()].filter((i) => all || !i.archived).map(summary),
        next_cursor: null,
      };
    },
    task_get: (a) => ({ issue: issues.get(String(a?.id)), comments: [...comments], history: [] }),
    task_save: (a) => {
      const input = a?.input as SaveIssue;
      if (!input.id) {
        const created = issue(next++, { title: input.title!, status: input.status ?? "todo" });
        issues.set(created.id, created);
        return created;
      }
      const cur = issues.get(input.id)!;
      if (input.expected_revision !== cur.revision) throw conflict(cur.revision);
      const { id, expected_revision: _e, idempotency_key: _k, ...patch } = input;
      return write(id, patch as Partial<Issue>);
    },
    task_archive: (a) => write(String(a?.id), { archived: true }),
    task_restore: (a) => write(String(a?.id), { archived: false }),
    task_comment: (a) => {
      const c: TaskComment = {
        id: `cmt_${comments.length + 1}`,
        issue_id: String(a?.id),
        ts: 1,
        actor: "me",
        body: String(a?.body),
        parent_id: (a?.parentId as string | null) ?? null,
      };
      comments.push(c);
      return c;
    },
  });
  /** Another writer (an agent through MCP): the file changes and the core
   * announces it. */
  const agent = (id: string, patch: Partial<Issue>) => {
    write(id, patch);
    act(() => emitLocal("tasks:changed", { ids: [id] }));
  };
  return { issues, calls, agent };
}

const saves = (calls: Call[]) =>
  calls.filter((c) => c.cmd === "task_save").map((c) => c.args?.input as SaveIssue);

function mount() {
  return render(
    <UiProvider>
      <div style={{ width: 400, height: 600 }}>
        <TasksPanel />
      </div>
    </UiProvider>,
  );
}

beforeEach(() => resetStores());
afterEach(() => resetStores());

describe("F-TK-03 merge rule", () => {
  it("F-TK-03.1 another writer's change to another field keeps the unsaved title", () => {
    const base = issue(1);
    let d = edit(draftOf(base), "title", "My better title");
    d = rebase(d, { ...base, revision: 2, status: "in_progress" });
    expect(shown(d, "title")).toBe("My better title");
    expect(d.base.status).toBe("in_progress");
    expect(d.conflicts).toEqual({});
  });

  it("F-TK-03.2 a change to the same field is a conflict", () => {
    const base = issue(1);
    let d = edit(draftOf(base), "title", "Mine");
    d = rebase(d, { ...base, revision: 2, title: "Theirs" });
    expect(d.conflicts).toEqual({ title: "Theirs" });
    expect(shown(d, "title")).toBe("Mine");
  });

  it("F-TK-03 an edit the other writer happened to match needs nothing", () => {
    const base = issue(1);
    let d = edit(draftOf(base), "title", "Same");
    d = rebase(d, { ...base, revision: 2, title: "Same" });
    expect(d.edits).toEqual({});
    expect(d.conflicts).toEqual({});
  });
});

describe("F-TK-03 Tasks panel", () => {
  it("F-TK-03 lists tasks grouped by status, quick-adds with Enter, and archives and restores", async () => {
    const t = tracker([issue(1, { status: "in_progress" }), issue(2)]);
    mount();
    await screen.findByTestId("task-group-in_progress");
    expect(within(screen.getByTestId("task-group-todo")).getByText("Task 2")).toBeTruthy();
    const add = screen.getByTestId("task-quick-add");
    fireEvent.change(add, { target: { value: "Write the release notes" } });
    fireEvent.keyDown(add, { key: "Enter" });
    await screen.findByText("Write the release notes");
    expect(saves(t.calls)[0]).toEqual({ title: "Write the release notes", status: "todo" });
    // Archive (row ×) hides it; Show archived offers Restore.
    const row = screen.getAllByTestId("task-row").find((r) => r.dataset.id === "TAN-2")!;
    fireEvent.click(within(row).getByTestId("task-archive"));
    await waitFor(() => expect(screen.queryByText("Task 2")).toBeNull());
    expect(t.issues.get("TAN-2")!.archived).toBe(true);
    fireEvent.click(screen.getByRole("checkbox", { name: "Show archived" }));
    const restore = await screen.findByTestId("task-restore");
    fireEvent.click(restore);
    await waitFor(() => expect(t.issues.get("TAN-2")!.archived).toBe(false));
  });

  it("F-TK-03 shows empty, loading and error states", async () => {
    fakeBackend({
      tasks_meta: () => {
        throw {
          code: "unavailable",
          message: "Couldn't read tasks from /x.",
          stage: null,
          hint: null,
          retryable: true,
        };
      },
      tasks_list: () => ({ items: [], next_cursor: null }),
    });
    mount();
    expect(screen.getByTestId("tasks-loading")).toBeTruthy();
    expect(await screen.findByText("Couldn't read tasks from /x.")).toBeTruthy();
    resetStores();
    tracker([]);
    act(() => void useTasks.getState().load());
    expect(await screen.findByText("No tasks yet")).toBeTruthy();
  });

  it("F-TK-03.1 an agent changing the status while the title is edited keeps both", async () => {
    const t = tracker([issue(1)]);
    mount();
    fireEvent.click(await screen.findByText("Task 1"));
    const title = (await screen.findByTestId("task-title")) as HTMLInputElement;
    fireEvent.change(title, { target: { value: "Fix the login loop" } });
    t.agent("TAN-1", { status: "in_progress" });
    await waitFor(() => expect(screen.getByTestId("task-detail").textContent).toContain("In progress"));
    expect((screen.getByTestId("task-title") as HTMLInputElement).value).toBe("Fix the login loop");
    expect(screen.queryByTestId("task-conflict")).toBeNull();
    // Saving the title now goes on top of the agent's revision.
    fireEvent.keyDown(screen.getByTestId("task-title"), { key: "Enter" });
    await waitFor(() => expect(t.issues.get("TAN-1")!.title).toBe("Fix the login loop"));
    expect(t.issues.get("TAN-1")!.status).toBe("in_progress");
    expect(saves(t.calls).at(-1)).toMatchObject({
      id: "TAN-1",
      title: "Fix the login loop",
      expected_revision: 2,
    });
  });

  it("F-TK-03.2 the same field changed meanwhile shows the conflict banner; Keep mine saves mine", async () => {
    const t = tracker([issue(1)]);
    mount();
    fireEvent.click(await screen.findByText("Task 1"));
    fireEvent.change(await screen.findByTestId("task-title"), { target: { value: "Mine" } });
    t.agent("TAN-1", { title: "Theirs" });
    const banner = await screen.findByTestId("task-conflict");
    expect(banner.textContent).toContain("Someone else changed the Title while you were editing.");
    expect((screen.getByTestId("task-title") as HTMLInputElement).value).toBe("Mine");
    fireEvent.click(screen.getByRole("button", { name: "Keep mine" }));
    await waitFor(() => expect(t.issues.get("TAN-1")!.title).toBe("Mine"));
    await waitFor(() => expect(screen.queryByTestId("task-conflict")).toBeNull());
  });

  it("F-TK-03.2 Take theirs drops the local edit", async () => {
    const t = tracker([issue(1)]);
    mount();
    fireEvent.click(await screen.findByText("Task 1"));
    fireEvent.change(await screen.findByTestId("task-title"), { target: { value: "Mine" } });
    t.agent("TAN-1", { title: "Theirs" });
    await screen.findByTestId("task-conflict");
    fireEvent.click(screen.getByRole("button", { name: "Take theirs" }));
    await waitFor(() => expect((screen.getByTestId("task-title") as HTMLInputElement).value).toBe("Theirs"));
    expect(saves(t.calls).filter((s) => s.id === "TAN-1")).toHaveLength(0);
  });

  it("F-TK-03 the description edits with explicit Save and Cancel, and comments thread", async () => {
    const t = tracker([issue(1)]);
    mount();
    fireEvent.click(await screen.findByText("Task 1"));
    fireEvent.click(await screen.findByTestId("task-description-edit"));
    fireEvent.change(screen.getByTestId("task-description-input"), { target: { value: "Draft" } });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(t.issues.get("TAN-1")!.description).toBe("");
    fireEvent.click(screen.getByTestId("task-description-edit"));
    fireEvent.change(screen.getByTestId("task-description-input"), {
      target: { value: "Steps to reproduce" },
    });
    fireEvent.click(screen.getByTestId("task-description-save"));
    await waitFor(() => expect(t.issues.get("TAN-1")!.description).toBe("Steps to reproduce"));
    fireEvent.change(screen.getByTestId("task-comment-input"), { target: { value: "Looking into it" } });
    fireEvent.click(screen.getByRole("button", { name: "Comment" }));
    await screen.findByText("Looking into it");
    fireEvent.click(screen.getByRole("button", { name: "Reply" }));
    fireEvent.change(screen.getByTestId("task-comment-input"), { target: { value: "Thanks" } });
    fireEvent.click(screen.getByRole("button", { name: "Comment" }));
    await screen.findByText("Thanks");
    const reply = t.calls.filter((c) => c.cmd === "task_comment").at(-1)!;
    expect(reply.args?.parentId).toBe("cmt_1");
  });
});

describe("F-TK-04 Execute and runs", () => {
  /** "on": the offline build with no account, where runs are free (D-2902);
   * "off": limits that turn runs off. */
  async function openTask(t: ReturnType<typeof tracker>, runs: "on" | "off") {
    useSession.setState({
      auth: { ...SIGNED_IN, signedIn: false, email: null, offline: true, hosted: false },
      entitlement: { ...FREE_VIEW, limits: { ...FREE_VIEW.limits, taskExecution: runs === "on" } },
    });
    useWorkspaces.setState({ list: [{ id: "ws_1", name: "shop", parentId: null, missing: false } as never] });
    useEngines.setState({
      list: [{ spec: { id: "claude", label: "Claude Code" }, install: {}, chatCapable: true } as never],
      loaded: true,
    });
    mount();
    fireEvent.click(await screen.findByText("Task 1"));
    fireEvent.click(await screen.findByTestId("task-execute"));
    return t;
  }

  it("F-TK-04 Execute offers only folders on this computer, and says why SSH and WSL ones aren't listed (D-843)", async () => {
    await openTask(tracker([issue(1)]), "on");
    act(() =>
      useWorkspaces.setState({
        list: [
          { id: "ws_1", name: "shop", kind: "local", parentId: null, missing: false },
          { id: "ws_2", name: "box", kind: "ssh", parentId: null, missing: false },
          { id: "ws_3", name: "ubuntu", kind: "wsl", parentId: null, missing: false },
        ] as never,
      }),
    );
    expect(screen.getByTestId("task-execute-local-only").textContent).toBe(
      "Runs start in folders on this computer, so workspaces on SSH hosts and in WSL aren't listed.",
    );
    fireEvent.click(screen.getByRole("combobox", { name: "Workspace" }));
    expect((await screen.findAllByRole("option")).map((o) => o.textContent)).toEqual(["shop"]);
  });

  it("F-TK-04 limits that turn runs off: Execute says so, sells nothing, and can't start", async () => {
    const t = await openTask(tracker([issue(1)]), "off");
    const blocked = await screen.findByTestId("task-execute-blocked");
    expect(blocked.textContent).toBe(
      "Running tasks is turned off for this account. You can still track tasks.",
    );
    expect((screen.getByTestId("task-execute-start") as HTMLButtonElement).disabled).toBe(true);
    expect(t.calls.some((c) => c.cmd === "task_execute")).toBe(false);
  });

  it("F-TK-04 offline with no account: a new worktree at Edits with review by default; the run shows its queue place and stops", async () => {
    const t = tracker([issue(1)]);
    let started: Record<string, unknown> | undefined;
    const runs: unknown[] = [];
    fakeBackendAdd(t, {
      task_attempts_list: () => runs,
      task_execute: (a) => {
        started = a?.input as Record<string, unknown>;
        const v = {
          attempt: {
            id: "atm_1",
            issueId: "TAN-1",
            engineId: "claude",
            state: "queued",
            threadId: null,
            error: null,
            reviewVerdict: null,
          },
          queuePosition: 1,
        };
        runs.push(v);
        return v;
      },
      task_attempt_stop: () => ({
        attempt: {
          id: "atm_1",
          issueId: "TAN-1",
          engineId: "claude",
          state: "stopped",
          threadId: null,
          error: "Stopped by you",
          reviewVerdict: null,
        },
        queuePosition: null,
      }),
    });
    await openTask(t, "on");
    expect(screen.queryByTestId("task-execute-blocked")).toBeNull();
    fireEvent.click(screen.getByTestId("task-execute-start"));
    await waitFor(() => expect(started).toBeTruthy());
    expect(started).toMatchObject({
      issueId: "TAN-1",
      workspaceId: "ws_1",
      engineId: "claude",
      isolation: "lane",
      access: "edits",
      fullConfirmed: false,
      review: true,
    });
    // The run shows queued with its place, and Stop stops it.
    expect((await screen.findByTestId("task-run")).textContent).toContain("Queued (position 1)");
    fireEvent.click(screen.getByTestId("task-run-stop"));
    await waitFor(() => expect(screen.getByTestId("task-run").dataset.state).toBe("stopped"));
  });
});

/** Adds handlers to the fake tracker's backend. */
function fakeBackendAdd(
  t: ReturnType<typeof tracker>,
  more: Record<string, (a?: Record<string, unknown>) => unknown>,
) {
  const base: Record<string, (a?: Record<string, unknown>) => unknown> = {
    tasks_meta: () => META,
    teammate_list: () => [],
    tasks_list: () => ({ items: [...t.issues.values()].map(summary), next_cursor: null }),
    task_get: (a) => ({ issue: t.issues.get(String(a?.id)), comments: [], history: [] }),
  };
  fakeBackend({ ...base, ...more });
}
