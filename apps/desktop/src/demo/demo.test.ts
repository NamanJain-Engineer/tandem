import { afterEach, describe, expect, it } from "vitest";
import type {
  PlanLaunch,
  PlanView,
  AppBoot,
  AuthState,
  DashboardItem,
  FileDiff,
  EntitlementView,
  GitPanel,
  Issue,
  IssueSummary,
  PaneRecord,
  RepoReview,
  ReviewItem,
  ReviewQueueView,
  ScratchNote,
  TabLayout,
  TaskDetail,
  TaskPage,
  TasksMeta,
  Thread,
  ThreadChanges,
  ThreadDetail,
  ThreadEvent,
  ThreadEventEnvelope,
  WorkspaceSummary,
} from "@tandem/protocol";
import pkg from "../../package.json";
import { listen } from "../ipc/events";
import { setInvokeBackend } from "../ipc/invoke";
import { LIMITS } from "@tandem/protocol/plans";
import { FREE } from "../test/fakeBackend";
import { NOTIFY_HREF } from "../features/billing/ComesWithAccounts";
import { DEMO_MESSAGE_LIMIT, DemoCore, FIRST_MESSAGE } from "./backend";
import { DemoShell, PROMPT, run } from "./shell";

const tick = () => new Promise((r) => setTimeout(r, 5));

async function until(check: () => boolean): Promise<void> {
  for (let i = 0; i < 200 && !check(); i++) await tick();
  expect(check()).toBe(true);
}

let core: DemoCore | null = null;
afterEach(() => {
  core?.dispose();
  setInvokeBackend(null);
});

describe("browser demo (vibezeo.com/demo)", () => {
  it("F-WB-01 the demo shell answers its commands and echoes typing", () => {
    expect(run("ls")?.join(" ")).toContain("package.json");
    expect(run("git status")?.join("\n")).toContain("src/checkout/redirect.ts");
    expect(run("npm test")?.join("\n")).toContain("19 passed");
    expect(run("rm -rf /")?.[0]).toContain("command not found");
    expect(run("clear")).toBeNull();
    const shell = new DemoShell();
    expect(shell.greeting()).toContain(PROMPT);
    expect(shell.write("pw")).toBe("pw");
    expect(shell.write("x\x7f")).toBe("x\b \b");
    expect(shell.write("d\r")).toBe(`d\r\n/home/you/shop\r\n${PROMPT}`);
  });

  it("F-DB-04 the demo's review queue: Claude Code's fix with its diff; approve, snooze and run the check", async () => {
    core = new DemoCore(0);
    const item = (core.handle("review_queue", {}) as ReviewQueueView).items[0];
    expect(item).toMatchObject({
      key: "pane:pn_claude",
      title: "Claude Code",
      engineId: "claude",
      state: "review",
      files: 2,
    });
    expect(item?.check?.state).toBe("passed");
    const key = item?.key ?? "";
    const files = core.handle("review_item_diff", { key, scope: "all" }) as FileDiff[];
    expect(files.map((f) => f.path)).toEqual(["src/checkout/redirect.test.ts", "src/checkout/redirect.ts"]);
    const seen: ReviewQueueView[] = [];
    const stop = await listen<ReviewQueueView>("review:queue", (q) => void seen.push(q));
    // Approve needs the diff that was read; the queue updates as in the app.
    expect(() => core?.handle("review_approve", { key, diffHash: "stale" })).toThrow();
    expect((core.handle("review_approve", { key, diffHash: item?.diffHash }) as ReviewItem).state).toBe(
      "approved",
    );
    expect(seen.at(-1)?.items[0]?.state).toBe("approved");
    expect((core.handle("review_unapprove", { key }) as ReviewItem).state).toBe("review");
    expect((core.handle("review_snooze", { key }) as ReviewItem).state).toBe("snoozed");
    expect((core.handle("review_snooze", { key, minutes: 0 }) as ReviewItem).state).toBe("review");
    expect((core.handle("review_run_check", { key }) as ReviewItem).check?.state).toBe("running");
    await until(() => seen.at(-1)?.items[0]?.check?.state === "passed");
    expect((core.handle("review_repo_get", { workspaceId: item?.workspaceId }) as RepoReview).check).toBe(
      "npm test",
    );
    expect(() => core?.handle("review_approve", { key: "pane:nope", diffHash: "x" })).toThrow();
    stop();
  });

  it("F-WB-01 the first thread sends its draft, asks before the tests, and reports the fix with its changes", async () => {
    core = new DemoCore(0);
    const threads = core.handle("thread_list", {}) as Thread[];
    const first = threads.find((t) => t.id === core?.firstThread);
    expect(first?.title).toBe("Fix the redirect loop after login");
    const detail = core.handle("thread_get", { threadId: core.firstThread }) as ThreadDetail;
    expect(JSON.parse(detail.draft)).toEqual({ text: FIRST_MESSAGE, attachmentIds: [] });

    const seen: ThreadEvent[] = [];
    core.handle("thread_subscribe", {
      threadId: core.firstThread,
      channel: { onmessage: (e: ThreadEventEnvelope) => seen.push(e.event) },
    });
    core.handle("thread_send", { threadId: core.firstThread, text: FIRST_MESSAGE, attachments: [] });
    await until(() => seen.some((e) => e.type === "approval_request"));
    const ask = seen.find((e) => e.type === "approval_request");
    expect(ask?.type === "approval_request" && ask.detail).toBe("npm test -- src/checkout");
    // Nothing more happens until the visitor answers.
    await tick();
    expect(seen.at(-1)?.type).toBe("approval_request");

    core.handle("approval_resolve", {
      approvalId: ask?.type === "approval_request" ? ask.id : "",
      decision: { type: "allow", scope: "once" },
    });
    await until(() => seen.some((e) => e.type === "turn_end"));
    const reply = seen.find((e) => e.type === "assistant");
    expect(reply?.type === "assistant" && reply.markdown).toContain("Fixed the redirect loop.");
    const changes = core.handle("thread_changes", { threadId: core.firstThread }) as ThreadChanges;
    expect(changes.files.map((f) => f.path)).toEqual([
      "src/checkout/redirect.test.ts",
      "src/checkout/redirect.ts",
    ]);

    // Later messages get a short simulated answer.
    core.handle("thread_send", { threadId: core.firstThread, text: "make a plan", attachments: [] });
    await until(() => seen.filter((e) => e.type === "turn_end").length === 2);
    expect(JSON.stringify(seen.filter((e) => e.type === "assistant").at(-1))).toContain("Here's a plan");
  });

  it("F-WB-01 an engine switch in the demo shows the divider above the next message", async () => {
    core = new DemoCore(0);
    const id = core.firstThread;
    const seen: ThreadEvent[] = [];
    core.handle("thread_subscribe", {
      threadId: id,
      channel: { onmessage: (e: ThreadEventEnvelope) => seen.push(e.event) },
    });
    core.handle("thread_send", { threadId: id, text: "hi", attachments: [] });
    core.handle("thread_stop", { threadId: id });
    core.handle("thread_switch_engine", { threadId: id, engineId: "codex" });
    core.handle("thread_send", { threadId: id, text: "again", attachments: [] });
    const at = seen.findIndex((e) => e.type === "user" && e.text === "again");
    expect(seen[at + 1]).toEqual({ type: "divider", text: "Continued on Codex (Default) with a summary" });
    core.handle("thread_stop", { threadId: id });
  });

  it("F-WB-01 commands the demo doesn't simulate are quiet, and lists stay lists", () => {
    core = new DemoCore(0);
    expect(core.handle("presets_list", {})).toEqual([]);
    expect(core.handle("workspace_remove", {})).toBeNull();
    expect(() => core?.handle("thread_get", { threadId: "nope" })).toThrow();
  });
});

describe("browser demo Files tab (F-CD-19)", () => {
  it("lists the demo project the terminal shows, edits in memory and refuses stale saves", () => {
    core = new DemoCore(0);
    const top = core.handle("fs_list", { dir: "" }) as Array<{
      path: string;
      kind: string;
      gitStatus?: string;
    }>;
    expect(top.map((e) => e.path)).toEqual([
      "src",
      "tests",
      "package.json",
      "README.md",
      "tsconfig.json",
      "vite.config.ts",
    ]);
    expect(top.find((e) => e.path === "src")?.gitStatus).toBe("modified");
    const checkout = core.handle("fs_list", { dir: "src/checkout" }) as Array<{
      path: string;
      gitStatus?: string;
    }>;
    expect(checkout.find((e) => e.path === "src/checkout/redirect.ts")?.gitStatus).toBe("modified");
    const file = core.handle("fs_read", { path: "src/checkout/redirect.ts" }) as {
      text: string;
      hash: string;
    };
    expect(file.text).toContain("export function nextStep");
    core.handle("fs_write", { path: "src/checkout/redirect.ts", text: "edited\n", expectedHash: file.hash });
    expect((core.handle("fs_read", { path: "src/checkout/redirect.ts" }) as { text: string }).text).toBe(
      "edited\n",
    );
    expect(() =>
      core?.handle("fs_write", { path: "src/checkout/redirect.ts", text: "x", expectedHash: file.hash }),
    ).toThrow();
    expect(() => core?.handle("fs_delete", { path: "README.md" })).toThrow();
  });

  it("F-CD-20 its Changes view says what the terminal's git status says, plus page edits, and runs no git", () => {
    core = new DemoCore(0);
    const terminal = run("git status")?.join("\n") ?? "";
    const before = core.handle("git_status", {}) as GitPanel;
    expect(before.status.branch).toBe("main");
    expect(terminal).toContain("On branch main");
    for (const f of before.status.unstaged) expect(terminal).toContain(f.path);
    expect(before.status.unstaged.map((f) => f.path)).toEqual([
      "src/checkout/redirect.test.ts",
      "src/checkout/redirect.ts",
    ]);
    const diff = core.handle("git_diff", { path: "src/checkout/redirect.ts", staged: false }) as {
      patch: string;
    };
    expect(diff.patch).toContain("Don't send the login page back to itself.");
    const readme = core.handle("fs_read", { path: "README.md" }) as { hash: string };
    core.handle("fs_write", { path: "README.md", text: "# shop, edited\n", expectedHash: readme.hash });
    const after = core.handle("git_status", {}) as GitPanel;
    expect(after.status.unstaged.map((f) => f.path)).toContain("README.md");
    expect(
      (core.handle("git_diff", { path: "README.md", staged: false }) as { patch: string }).patch,
    ).toContain("+# shop, edited");
    for (const cmd of ["git_stage", "git_commit", "git_push", "git_switch"]) {
      expect(() => core?.handle(cmd, { paths: ["README.md"] })).toThrow();
    }
  });
});

describe("browser demo Scratchpad (F-SH-12)", () => {
  it("F-SH-12 keeps notes in the page with the app's rules: newest first, titled by the first line, no save after a delete", () => {
    core = new DemoCore(0);
    const list = () => (core?.handle("scratchpad_list", {}) as ScratchNote[]).map((n) => [n.id, n.title]);
    expect(list()).toEqual([]);
    const a = core.handle("scratchpad_write", { id: null, markdown: "" }) as ScratchNote;
    expect(a.title).toBe("");
    core.handle("scratchpad_write", { id: a.id, markdown: "\n# **Groceries** list\n- [ ] milk" });
    const b = core.handle("scratchpad_write", { id: null, markdown: "- [x] call <u>Ana</u>" }) as ScratchNote;
    expect(b.id).not.toBe(a.id);
    expect(list()).toEqual([
      [b.id, "call Ana"],
      [a.id, "Groceries list"],
    ]);
    expect(core.handle("scratchpad_read", { id: a.id })).toBe("\n# **Groceries** list\n- [ ] milk");
    // Saving a note moves it to the top.
    core.handle("scratchpad_write", { id: a.id, markdown: "1. eggs" });
    expect(list()).toEqual([
      [a.id, "eggs"],
      [b.id, "call Ana"],
    ]);
    core.handle("scratchpad_delete", { id: a.id });
    expect(() => core?.handle("scratchpad_write", { id: a.id, markdown: "late" })).toThrow();
    expect(() => core?.handle("scratchpad_read", { id: a.id })).toThrow();
    expect(list()).toEqual([[b.id, "call Ana"]]);
  });
});

describe("the interactive demo on the website (docs/FEATURES_TO_BUILD.md)", () => {
  const send = (c: DemoCore, threadId: string, text: string) =>
    c.handle("thread_send", { threadId, text, attachments: [] });

  it("demo AC5 the 13th message says Demo limit reached, and a message is up to 500 characters", async () => {
    core = new DemoCore(0);
    const c = core;
    const id = (c.handle("thread_create", { args: { workspaceId: null } }) as Thread).id;
    expect(() => send(c, id, "x".repeat(501))).toThrow(/500 characters/);
    for (let i = 1; i <= DEMO_MESSAGE_LIMIT; i++) {
      send(c, id, `message ${i}`);
      c.handle("thread_stop", { threadId: id });
    }
    expect(DEMO_MESSAGE_LIMIT).toBe(12);
    let error: { message?: string } = {};
    try {
      send(c, id, "one more");
    } catch (e) {
      error = e as { message?: string };
    }
    expect(error.message).toContain("Demo limit reached");
    expect(c.limitReached()).toBe(true);
  });

  it("demo the sample world runs the limits the download runs, at the download's version", () => {
    core = new DemoCore(0);
    const view = core.handle("entitlement_get", {}) as EntitlementView;
    expect(view.plan).toBe("free");
    expect(view.limits).toEqual(FREE);
    expect((core.handle("auth_state", {}) as AuthState).hosted).toBe(false);
    expect((core.handle("app_ready", {}) as AppBoot).version).toBe(pkg.version);
  });

  it("F-CD-03 demo runs the download's free limits: every local feature unlimited, hosted ones off (D-869, D-2900)", () => {
    core = new DemoCore(0);
    const { limits } = core.handle("entitlement_get", {}) as EntitlementView;
    expect(limits).toEqual(LIMITS.free);
    expect(limits).toMatchObject({
      panes: null,
      workspaces: null,
      threadGrid: 6,
      chatEngines: null,
      routines: true,
      taskExecution: true,
      sessionSearch: true,
      conductor: false,
      cloudStt: false,
    });
  });

  it("D-2914 demo opens Tell me when accounts open's email, and still no other scheme", () => {
    core = new DemoCore(0);
    const opened: string[] = [];
    const open = window.open;
    window.open = ((url?: string | URL) => {
      opened.push(String(url));
      return null;
    }) as typeof window.open;
    try {
      core.handle("app_open_external", { url: NOTIFY_HREF });
      core.handle("app_open_external", { url: "file:///etc/passwd" });
    } finally {
      window.open = open;
    }
    expect(opened).toEqual([NOTIFY_HREF]);
  });

  it("D-2905 demo says routines and task runs work in the app, instead of pretending to", () => {
    core = new DemoCore(0);
    const c = core;
    for (const cmd of ["routine_save", "routine_run_now", "task_execute", "night_schedule"]) {
      let error: { code?: string; message?: string } = {};
      try {
        c.handle(cmd, {});
      } catch (e) {
        error = e as typeof error;
      }
      expect(error.code, cmd).toBe("unavailable");
      expect(error.message, cmd).toContain("in the app, not in this demo");
    }
    expect(c.handle("night_accounts", {})).toEqual([]);
  });

  it("F-WB-01 demo runs early access as the download does: no account, no credits", () => {
    core = new DemoCore(0);
    // What the core reports after "Start using Vibezeo" (AuthService::continue_offline, hosted off).
    const earlyAccess: AuthState = {
      signedIn: false,
      email: null,
      offline: true,
      hosted: false,
      pending: null,
      error: null,
    };
    expect(core.handle("auth_state", {})).toEqual(earlyAccess);
    const view = core.handle("entitlement_get", {}) as EntitlementView;
    expect(view.source).toBe("free");
    expect(view.credits).toBeNull();
  });

  it("F-TL-05 the demo says it keeps nothing to export or delete, instead of pretending to", () => {
    core = new DemoCore(0);
    const c = core;
    for (const cmd of ["data_export", "data_delete_local"]) {
      let error: { code?: string; message?: string } = {};
      try {
        c.handle(cmd, { removeEngineSignIns: false });
      } catch (e) {
        error = e as typeof error;
      }
      expect(error.code, cmd).toBe("unavailable");
      expect(error.message, cmd).toContain("stays in this page");
    }
  });

  it("F-CN-05 the demo says Find work searches the user's own computer, instead of pretending to", () => {
    core = new DemoCore(0);
    let error: { code?: string; message?: string } = {};
    try {
      core.handle("find_work", { query: "login tests" });
    } catch (e) {
      error = e as typeof error;
    }
    expect(error.code).toBe("unavailable");
    expect(error.message).toContain("running on your computer");
  });

  it("F-TK-06 the demo simulates Lead's plan, creates its tasks, shows their runs, and can't run them", async () => {
    core = new DemoCore(0);
    const started = core.handle("plan_start", { workspaceId: "ws_demo", goal: "Add dark mode" }) as PlanView;
    expect([started.state, started.threadId]).toEqual(["planning", ""]);
    await new Promise((r) => setTimeout(r, 10));
    const ready = (core.handle("plan_list", {}) as PlanView[])[0]!;
    expect(ready.state).toBe("ready");
    expect(ready.tasks.map((t) => t.engineId)).toEqual(["claude", "codex", "claude"]);
    expect(ready.tasks[0]!.why).toContain("Simulated in the demo");
    const created = core.handle("plan_create_tasks", { id: ready.id, tasks: ready.tasks.slice(0, 2) }) as PlanView;
    expect(created.issueIds).toEqual(["TAN-1", "TAN-2"]);
    expect((core.handle("task_get", { id: "TAN-2" }) as { issue: Issue }).issue.suggested_engine).toBe("codex");
    // Execute all's check lists the runs and says they start in the app.
    const launch = core.handle("plan_prepare", { id: ready.id }) as PlanLaunch;
    expect(launch.blocked).toContain("it works in the app, not in this demo");
    expect(launch.runs.map((r) => [r.issueId, r.engineLabel, r.reviewEngineLabel])).toEqual([
      ["TAN-1", "Claude Code", "Codex"],
      ["TAN-2", "Codex", "Claude Code"],
    ]);
    expect(() => core?.handle("plan_execute", { id: ready.id, supervise: true })).toThrow();
    // Lead's answers come with runs (F-TK-08): not in the demo either.
    expect(created.questions).toEqual([]);
    expect(() => core?.handle("plan_supervise", { id: ready.id, on: false })).toThrow();
    expect(() => core?.handle("plan_answer", { id: ready.id, questionId: "pq_1", text: "Yes" })).toThrow();
  });

  it("F-TK-03 the demo keeps tasks in the page: quick add, edits with revisions and history, comments, archive", () => {
    core = new DemoCore(0);
    const page = () =>
      core?.handle("tasks_list", {
        filter: { include_archived: false },
        cursor: null,
      }) as TaskPage<IssueSummary>;
    expect(page()).toEqual({ items: [], next_cursor: null });
    expect((core.handle("tasks_meta", {}) as TasksMeta).statuses).toContain("in_review");
    const a = core.handle("task_save", {
      input: { title: "  Fix the login redirect ", status: "todo" },
    }) as Issue;
    expect([a.id, a.title, a.revision, a.status]).toEqual(["TAN-1", "Fix the login redirect", 1, "todo"]);
    const b = core.handle("task_save", {
      input: { id: a.id, status: "in_progress", expected_revision: 1 },
    }) as Issue;
    expect(b.revision).toBe(2);
    // A save on an old revision is refused, as the app refuses it.
    expect(() =>
      core?.handle("task_save", { input: { id: a.id, title: "x", expected_revision: 1 } }),
    ).toThrow();
    core.handle("task_comment", { id: a.id, body: "Repro in staging", parentId: null });
    const detail = core.handle("task_get", { id: a.id }) as TaskDetail;
    expect(detail.history.map((h) => [h.field, h.to])).toEqual([["status", "in_progress"]]);
    expect(detail.comments.map((c) => c.body)).toEqual(["Repro in staging"]);
    core.handle("task_archive", { id: a.id, expectedRevision: 2 });
    expect(page().items).toEqual([]);
    core.handle("task_restore", { id: a.id, expectedRevision: 3 });
    expect(page().items.map((i) => i.id)).toEqual(["TAN-1"]);
  });

  it("F-DI-01 F-PL-03 the demo says Dictate and plugins need the app, instead of pretending to", () => {
    core = new DemoCore(0);
    for (const [cmd, words] of [
      ["dictate_status", "microphone"],
      ["plugins_catalog", "your own accounts"],
    ] as const) {
      let error: { code?: string; message?: string } = {};
      try {
        core.handle(cmd, {});
      } catch (e) {
        error = e as typeof error;
      }
      expect(error.code, cmd).toBe("unavailable");
      expect(error.message, cmd).toContain(words);
    }
  });

  it("F-EX-01 F-EX-05 F-RM-03 the demo's new screens get their own shapes or say they need the app (D-833)", () => {
    core = new DemoCore(0);
    for (const [cmd, words] of [
      ["checkpoints_list", "folders on your computer"],
      ["checkpoint_create", "folders on your computer"],
      ["prompts_create", "files in your folders"],
      ["bestof_start", "git worktrees"],
      ["phone_pair_begin", "over your own network"],
    ] as const) {
      let error: { code?: string; message?: string } = {};
      try {
        core.handle(cmd, {});
      } catch (e) {
        error = e as typeof error;
      }
      expect(error.code, cmd).toBe("unavailable");
      expect(error.message, cmd).toContain(words);
    }
    // An object where the screen reads an object, never the [] or null that
    // blanked the page.
    expect(core.handle("phone_arm_state", {})).toEqual({ armedUntil: null });
    expect(core.handle("prompts_list", {})).toEqual({ items: [], skipped: [], excludeHidesRepo: false });
    expect(core.handle("checkpoints_usage", {})).toEqual({ bytes: 0, folders: 0 });
    expect(core.handle("history_search", {})).toEqual([]);
  });

  it("F-RM-04 the demo's Settings › Remote shows local-network pairing off, and pairing says it needs the app (D-833)", () => {
    core = new DemoCore(0);
    // The shape the Local network section reads; null left it loading for good.
    expect(core.handle("phone_lan_state", {})).toEqual({
      enabled: false,
      running: false,
      port: 47800,
      addresses: [],
      error: null,
    });
    for (const cmd of ["phone_lan_set", "phone_pair_begin"] as const) {
      let error: { code?: string; message?: string } = {};
      try {
        core.handle(cmd, { enabled: true });
      } catch (e) {
        error = e as typeof error;
      }
      expect(error.code, cmd).toBe("unavailable");
      expect(error.message, cmd).toContain("your own network");
    }
  });

  it("demo the sample world: three workspaces, Claude Code, Codex and Gemini CLI panes, and the Dashboard's states", () => {
    core = new DemoCore(0);
    const workspaces = core.handle("workspace_list", {}) as WorkspaceSummary[];
    expect(workspaces.map((w) => w.name)).toEqual(["shop", "api", "docs-site"]);
    const panes = core.handle("pane_list", {}) as PaneRecord[];
    expect(
      panes
        .filter((p) => p.kind === "agent")
        .map((p) => p.engineId)
        .sort(),
    ).toEqual(["claude", "codex", "gemini"]);
    // No live-pane limit with no account (D-2900); the sample world starts four.
    expect(FREE.panes).toBeNull();
    expect(panes.length).toBe(4);
    for (const w of workspaces) {
      const layouts = core.handle("layout_get", { workspaceId: w.id }) as TabLayout[];
      expect(layouts.length, w.name).toBe(1);
      // Each tab lists only its own panes.
      const tabPanes = core.handle("pane_list", { tabId: layouts[0]?.tab.id }) as PaneRecord[];
      expect(
        tabPanes.every((p) => p.workspaceId === w.id),
        w.name,
      ).toBe(true);
    }
    expect((core.handle("pane_list", { tabId: "tb_demo" }) as PaneRecord[]).map((p) => p.id)).toEqual([
      "pn_claude",
      "pn_codex",
      "pn_shell",
    ]);
    const states = new Set((core.handle("status_snapshot", {}) as DashboardItem[]).map((d) => d.status));
    for (const s of ["working", "waiting", "done"] as const) expect(states.has(s), s).toBe(true);
  });

  it("demo the shell answers help, ls, pwd, git status, git diff, git log, npm test and clear", () => {
    const help = run("help")?.join("\n") ?? "";
    for (const c of ["ls", "pwd", "git status", "git diff", "git log", "npm test", "clear"])
      expect(help).toContain(c);
    expect(run("git diff")?.join("\n")).toContain("Don't send the login page back to itself.");
  });

  it("demo threads to read: a finished one, one switched to another engine, and a Plan-mode turn makes a plan version", async () => {
    core = new DemoCore(0);
    const c = core;
    const threads = c.handle("thread_list", {}) as Thread[];
    const switched = threads.find((t) => t.title === "Speed up the product search");
    expect(switched).toBeTruthy();
    const page = c.handle("thread_events", { threadId: switched?.id }) as { items: ThreadEventEnvelope[] };
    expect(page.items.some((e) => e.event.type === "divider")).toBe(true);

    const id = (c.handle("thread_create", { args: { workspaceId: "ws_demo" } }) as Thread).id;
    const seen: ThreadEvent[] = [];
    c.handle("thread_subscribe", {
      threadId: id,
      channel: { onmessage: (e: ThreadEventEnvelope) => seen.push(e.event) },
    });
    c.handle("thread_update", { threadId: id, mode: "plan" });
    send(c, id, "add a coupon field to checkout");
    await until(() => seen.some((e) => e.type === "turn_end"));
    expect(seen.some((e) => e.type === "plan_revision" && e.version === 1)).toBe(true);
    const detail = c.handle("thread_get", { threadId: id }) as ThreadDetail;
    expect(detail.planRevisions.map((p) => p.version)).toEqual([1]);
    const turn = c.handle("thread_implement_plan", { threadId: id, version: 1 }) as { turnId: string };
    expect(turn.turnId).toBeTruthy();
    c.handle("thread_stop", { threadId: id });
  });
});
