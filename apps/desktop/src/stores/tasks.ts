// The task tracker (F-TK-01, F-TK-03): the issues, the meta (statuses,
// labels, projects, actors) and the open issue's detail. Kept fresh from
// `tasks:changed`, which the core sends within 500 ms of any write, by the
// UI or an agent.
import { create } from "zustand";
import type {
  AttemptView,
  ExecuteInput,
  IpcError,
  Issue,
  IssueStatus,
  IssueSummary,
  SaveIssue,
  TaskComment,
  TaskDetail,
  TaskPage,
  TasksMeta,
} from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";

export const STATUS_ORDER: IssueStatus[] = [
  "backlog",
  "todo",
  "in_progress",
  "in_review",
  "done",
  "canceled",
];

type Status = "idle" | "loading" | "ready" | "error";

type TasksState = {
  meta: TasksMeta | null;
  issues: IssueSummary[];
  status: Status;
  error: string | null;
  showArchived: boolean;
  selectedId: string | null;
  detail: TaskDetail | null;
  detailError: string | null;
  /** Grows with every change an event announced (the detail rebases). */
  version: number;
  load: () => Promise<void>;
  setShowArchived: (on: boolean) => Promise<void>;
  select: (id: string | null) => Promise<void>;
  refreshDetail: () => Promise<void>;
  quickAdd: (title: string, status?: IssueStatus) => Promise<IpcError | null>;
  save: (input: SaveIssue) => Promise<Issue | IpcError>;
  archive: (issue: { id: string; revision: number }) => Promise<IpcError | null>;
  restore: (issue: { id: string; revision: number }) => Promise<IpcError | null>;
  comment: (body: string, parentId?: string | null) => Promise<TaskComment | IpcError>;
  /** Every run, in queue order (F-TK-04). */
  attempts: AttemptView[];
  /** Whether the runs were read: a plan shows loading and errors (F-TK-07). */
  attemptsStatus: "idle" | "loading" | "ready" | "error";
  attemptsError: string | null;
  loadAttempts: () => Promise<void>;
  execute: (input: ExecuteInput) => Promise<AttemptView | IpcError>;
  attemptAction: (action: "stop" | "resume" | "retry", attemptId: string) => Promise<AttemptView | IpcError>;
};

export function isIpcError(e: unknown): e is IpcError {
  return !!e && typeof e === "object" && "code" in e && "message" in e;
}

function asError(e: unknown): IpcError {
  return isIpcError(e)
    ? e
    : { code: "internal", message: String(e), stage: null, hint: null, retryable: false };
}

async function allIssues(includeArchived: boolean): Promise<IssueSummary[]> {
  const out: IssueSummary[] = [];
  let cursor: string | null = null;
  // Pages of 50 until the cursor runs out (a bound, in case of a loop).
  for (let i = 0; i < 1000; i++) {
    const page: TaskPage<IssueSummary> = await invoke<TaskPage<IssueSummary>>("tasks_list", {
      filter: { include_archived: includeArchived },
      cursor,
    });
    out.push(...page.items);
    if (!page.next_cursor) break;
    cursor = page.next_cursor;
  }
  return out;
}

let unlisten: (() => void) | null = null;
let listening: Promise<void> | null = null;
let unlistenAttempts: (() => void) | null = null;
let listeningAttempts: Promise<void> | null = null;
/** The latest `task_attempts_list` read: only it writes the runs (F-TK-07). */
let attemptsRead = 0;
/** Runs `tasks:attempt` moved since that read began: newer than its list. */
let movedMeanwhile = new Map<string, AttemptView>();

function upsertAttempt(list: AttemptView[], v: AttemptView): AttemptView[] {
  const at = list.findIndex((a) => a.attempt.id === v.attempt.id);
  if (at < 0) return [...list, v];
  const out = list.slice();
  out[at] = v;
  return out;
}

export const useTasks = create<TasksState>((set, get) => ({
  meta: null,
  issues: [],
  status: "idle",
  error: null,
  showArchived: false,
  selectedId: null,
  detail: null,
  detailError: null,
  version: 0,
  attempts: [],
  attemptsStatus: "idle",
  attemptsError: null,
  loadAttempts: async () => {
    if (!listeningAttempts) {
      const pending: Promise<void> = listen<AttemptView>("tasks:attempt", (v) => {
        movedMeanwhile.set(v.attempt.id, v);
        set((s) => ({ attempts: upsertAttempt(s.attempts, v) }));
      }).then(
        (u) => {
          unlistenAttempts = u;
        },
        (e: unknown) => {
          // A retry listens again.
          if (listeningAttempts === pending) listeningAttempts = null;
          throw e;
        },
      );
      listeningAttempts = pending;
    }
    // Several views read the runs; only the latest read writes them, so an
    // earlier one settling last can't bring back an error or an old list.
    const read = ++attemptsRead;
    movedMeanwhile = new Map();
    // A refresh keeps what's shown; a first read or a retry shows loading.
    set((s) => ({
      attemptsStatus: s.attemptsStatus === "ready" ? "ready" : "loading",
      attemptsError: null,
    }));
    try {
      // The read goes out once the listener is up: a run that moved in
      // between would be missed, and no event comes after a run's last state.
      await listeningAttempts;
      const listed = (await invoke<AttemptView[] | null>("task_attempts_list")) ?? [];
      if (read !== attemptsRead) return;
      // A run that moved while the list was read keeps its newer state.
      let attempts = listed;
      for (const v of movedMeanwhile.values()) attempts = upsertAttempt(attempts, v);
      set({ attempts, attemptsStatus: "ready" });
    } catch (e) {
      if (read !== attemptsRead) return;
      // Runs stay as they were; the tracker itself still works.
      set({ attemptsStatus: "error", attemptsError: asError(e).message });
    }
  },
  execute: async (input) => {
    try {
      const v = await invoke<AttemptView>("task_execute", { input });
      set((s) => ({ attempts: upsertAttempt(s.attempts, v) }));
      return v;
    } catch (e) {
      return asError(e);
    }
  },
  attemptAction: async (action, attemptId) => {
    try {
      const v = await invoke<AttemptView>(`task_attempt_${action}`, { attemptId });
      set((s) => ({ attempts: upsertAttempt(s.attempts, v) }));
      return v;
    } catch (e) {
      return asError(e);
    }
  },
  load: async () => {
    listening ??= listen<{ ids: string[] }>("tasks:changed", (e) => {
      void get().load();
      const sel = get().selectedId;
      if (sel && (e.ids.length === 0 || e.ids.includes(sel))) void get().refreshDetail();
    }).then((u) => {
      unlisten = u;
    });
    if (get().status !== "ready") set({ status: "loading" });
    try {
      const [meta, issues] = await Promise.all([
        invoke<TasksMeta>("tasks_meta"),
        allIssues(get().showArchived),
      ]);
      set((s) => ({ meta, issues, status: "ready", error: null, version: s.version + 1 }));
    } catch (e) {
      set({ status: "error", error: asError(e).message });
    }
  },
  setShowArchived: async (on) => {
    set({ showArchived: on });
    await get().load();
  },
  select: async (id) => {
    set({
      selectedId: id,
      detail: id && get().detail?.issue.id === id ? get().detail : null,
      detailError: null,
    });
    if (id) await get().refreshDetail();
  },
  refreshDetail: async () => {
    const id = get().selectedId;
    if (!id) return;
    try {
      const detail = await invoke<TaskDetail>("task_get", { id });
      if (get().selectedId === id) set({ detail, detailError: null });
    } catch (e) {
      if (get().selectedId === id) set({ detailError: asError(e).message });
    }
  },
  quickAdd: async (title, status = "todo") => {
    const trimmed = title.trim();
    if (!trimmed) return null;
    try {
      await invoke<Issue>("task_save", { input: { title: trimmed, status } });
      await get().load();
      return null;
    } catch (e) {
      return asError(e);
    }
  },
  save: async (input) => {
    try {
      const issue = await invoke<Issue>("task_save", { input });
      const detail = get().detail;
      if (detail && detail.issue.id === issue.id) set({ detail: { ...detail, issue } });
      void get().load();
      return issue;
    } catch (e) {
      return asError(e);
    }
  },
  archive: async ({ id, revision }) => {
    try {
      await invoke<Issue>("task_archive", { id, expectedRevision: revision });
      await get().load();
      return null;
    } catch (e) {
      return asError(e);
    }
  },
  restore: async ({ id, revision }) => {
    try {
      await invoke<Issue>("task_restore", { id, expectedRevision: revision });
      await get().load();
      return null;
    } catch (e) {
      return asError(e);
    }
  },
  comment: async (body, parentId) => {
    const id = get().selectedId;
    if (!id) return asError("No task is open");
    try {
      const c = await invoke<TaskComment>("task_comment", {
        id,
        body,
        parentId: parentId ?? null,
        idempotencyKey: crypto.randomUUID(),
      });
      await get().refreshDetail();
      return c;
    } catch (e) {
      return asError(e);
    }
  },
}));

/** Tests: a fresh tracker view. */
export function resetTasks(): void {
  unlisten?.();
  unlisten = null;
  listening = null;
  unlistenAttempts?.();
  unlistenAttempts = null;
  listeningAttempts = null;
  // A read still in flight from before goes stale and never writes.
  attemptsRead++;
  movedMeanwhile = new Map();
  useTasks.setState({
    meta: null,
    issues: [],
    status: "idle",
    error: null,
    showArchived: false,
    selectedId: null,
    detail: null,
    detailError: null,
    version: 0,
    attempts: [],
    attemptsStatus: "idle",
    attemptsError: null,
  });
}
