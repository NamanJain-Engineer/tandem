import { afterEach, describe, expect, it } from "vitest";
import type {
  TeammateSummary,
  TeammateTemplate,
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
import { decodeFrame } from "../ipc/pty";
import { LIMITS } from "@tandem/protocol/plans";
import { FREE } from "../test/fakeBackend";
import { NOTIFY_HREF } from "../features/billing/ComesWithAccounts";
import { DEMO_ACTIVE, DEMO_MESSAGE_LIMIT, DEMO_WAITING, DEMO_WORKTREES, DemoCore, FIRST_MESSAGE } from "./backend";
import { MAIN, changedIn, diffsOf, testRun } from "./repo";
import { STORY, branchOf, editsOf, engineLabel, totalsOf, worktreeIn } from "./story";

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
  it("F-WB-01 the demo shell answers its commands from its worktree's checkout and echoes typing", async () => {
    core = new DemoCore(0);
    const working = worktreeIn("working");
    const root = `/home/you/${STORY.repo}/.tandem/worktrees/${working.slug}`;
    const prompt = `\x1b[32myou@vibezeo\x1b[0m:\x1b[34m~/${STORY.repo}/.tandem/worktrees/${working.slug}\x1b[0m$ `;
    let screen = "";
    const decode = (bytes: Uint8Array) => new TextDecoder().decode(decodeFrame(bytes).payload);
    core.handle("pty_spawn", { paneId: "pn_shell", channel: { onmessage: (f: Uint8Array) => (screen += decode(f)) } });
    await until(() => screen.includes(prompt));
    const type = (text: string) => core?.handle("pty_write", { paneId: "pn_shell", data: text });
    const said = async (line: string): Promise<string> => {
      const from = screen.length;
      type(`${line}\r`);
      await until(() => screen.slice(from).endsWith(prompt));
      return screen.slice(from);
    };
    expect(await said("pwd")).toContain(root);
    expect(await said("ls")).toContain("package.json");
    // git status lists the working agent's uncommitted edits, on its branch.
    const status = await said("git status");
    expect(status).toContain(`On branch ${branchOf(working.slug)}`);
    for (const e of editsOf(working)) expect(status).toContain(e.edit);
    expect(status).toContain("Untracked files:");
    // git diff shows the tracked file's change; cat shows a file as the worktree has it.
    expect(await said("git diff")).toContain("+import { stack, type StackRule } from \"./rules\";");
    expect(await said("cat src/cart/rules.ts")).toContain("export function stack(");
    expect(await said("git log")).toContain(STORY.history[0]?.subject ?? "?");
    expect(await said("pnpm test coupon")).toContain("Tests  19 passed (19)");
    expect(await said("rm -rf /")).toContain("command not found");
    const from = screen.length;
    type("x\x7f");
    expect(screen.slice(from)).toBe("x\b \b");
  });

  it("UI-70 F-AG-01 the demo's Agent mode: Lead and three teammates, one working and one waiting, and the app's templates", () => {
    core = new DemoCore(0);
    const list = core.handle("teammate_list", {}) as TeammateSummary[];
    expect(list.map((s) => s.teammate.name)).toEqual(["Lead", "Reviewer", "Release notes writer", "Researcher"]);
    expect(list[0]?.teammate.isLead).toBe(true);
    expect(list.filter((s) => s.working > 0)).toHaveLength(1);
    expect(list.filter((s) => s.waiting)).toHaveLength(1);
    for (const s of list) expect(s.lastLine).toBeTruthy();
    const templates = core.handle("teammate_templates", {}) as TeammateTemplate[];
    expect(templates.map((t) => t.id)).toEqual(["reviewer", "release-notes", "researcher", "triage"]);
    expect(core.handle("teammate_suggestions", { id: "tm_lead" })).toHaveLength(3);
  });

  it("F-DB-04 the demo's review queue: the finished agent's work in its worktree with its diff; approve, snooze and run the check", async () => {
    core = new DemoCore(0);
    const done = worktreeIn("done");
    const item = (core.handle("review_queue", {}) as ReviewQueueView).items[0];
    const totals = totalsOf(done);
    expect(item).toMatchObject({
      key: "pane:pn_claude",
      title: engineLabel(done.engine),
      engineId: done.engine,
      workspace: done.slug,
      branch: branchOf(done.slug),
      closing: done.closing,
      state: "review",
      files: totals.files,
      additions: totals.adds,
      deletions: totals.dels,
    });
    expect(item?.laneId).toBe(item?.workspaceId);
    // The core's local rules: the session file touches auth paths, nothing else fires.
    expect(item?.risks).toEqual([{ rule: "auth", files: ["src/auth/session.ts"] }]);
    expect(item?.check?.state).toBe("passed");
    expect(item?.check?.output).toBe(testRun({ ...MAIN, ...changedIn(done.slug) }).join("\n"));
    const key = item?.key ?? "";
    const files = core.handle("review_item_diff", { key, scope: "all" }) as FileDiff[];
    expect(files).toEqual(diffsOf(done.slug));
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
      STORY.check,
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
    expect(ask?.type === "approval_request" && ask.detail).toBe("pnpm test auth");
    // Its edits are in shop's folder, uncommitted, by the time it asks.
    const shop = (core.handle("git_status", { context: { type: "thread", threadId: core.firstThread } }) as GitPanel).status;
    expect(shop.unstaged).toEqual([
      { path: "src/auth/session.test.ts", change: "untracked" },
      { path: "src/auth/session.ts", change: "modified" },
    ]);
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
    // The tests it ran are the ones the fix's files have.
    const run = seen.find((e) => e.type === "tool" && e.verb === "bash" && e.status === "ok");
    expect(run?.type === "tool" && run.output).toBe(testRun({ ...MAIN, ...changedIn("thread") }, "auth").join("\n"));
    expect(reply?.type === "assistant" && reply.markdown).toContain("all 6 tests pass");
    const changes = core.handle("thread_changes", { threadId: core.firstThread }) as ThreadChanges;
    expect(changes.files).toEqual(diffsOf("thread"));
    expect(changes.files.map((f) => [f.path, f.change])).toEqual([
      ["src/auth/session.test.ts", "added"],
      ["src/auth/session.ts", "modified"],
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
  const ws = (workspaceId: string) => ({ context: { type: "workspace", workspaceId } });
  const shopId = () => (core?.handle("workspace_list", {}) as WorkspaceSummary[]).find((w) => w.parentId === null)?.id ?? "";

  it("lists each worktree's checkout of the sample repository, edits in memory and refuses stale saves", () => {
    core = new DemoCore(0);
    const working = worktreeIn("working");
    const id = DEMO_WORKTREES[STORY.worktrees.indexOf(working)] ?? "";
    const top = core.handle("fs_list", { ...ws(id), dir: "" }) as Array<{ path: string; gitStatus?: string }>;
    expect(top.map((e) => e.path)).toEqual([
      "src",
      "tests",
      "package.json",
      "README.md",
      "tsconfig.json",
      "vite.config.ts",
    ]);
    expect(top.find((e) => e.path === "src")?.gitStatus).toBe("modified");
    const cart = core.handle("fs_list", { ...ws(id), dir: "src/cart" }) as Array<{ path: string; gitStatus?: string }>;
    expect(cart.find((e) => e.path === "src/cart/coupon.ts")?.gitStatus).toBe("modified");
    expect(cart.find((e) => e.path === "src/cart/rules.ts")?.gitStatus).toBe("untracked");
    // shop itself is on main, as committed: no rules.ts, and nothing changed.
    const main = core.handle("fs_list", { ...ws(shopId()), dir: "src/cart" }) as Array<{ path: string; gitStatus?: string }>;
    expect(main.map((e) => e.path)).not.toContain("src/cart/rules.ts");
    expect(main.every((e) => e.gitStatus === undefined)).toBe(true);
    const file = core.handle("fs_read", { ...ws(id), path: "src/cart/coupon.ts" }) as { text: string; hash: string };
    expect(file.text).toBe(changedIn(working.slug)["src/cart/coupon.ts"]);
    core.handle("fs_write", { ...ws(id), path: "src/cart/coupon.ts", text: "edited\n", expectedHash: file.hash });
    expect((core.handle("fs_read", { ...ws(id), path: "src/cart/coupon.ts" }) as { text: string }).text).toBe("edited\n");
    expect(() =>
      core?.handle("fs_write", { ...ws(id), path: "src/cart/coupon.ts", text: "x", expectedHash: file.hash }),
    ).toThrow();
    expect(() => core?.handle("fs_delete", { ...ws(id), path: "README.md" })).toThrow();
  });

  it("F-CD-20 each checkout's Changes view: its branch, its agent's uncommitted edits, page edits, and no git", () => {
    core = new DemoCore(0);
    for (const [i, w] of STORY.worktrees.entries()) {
      const panel = (core.handle("git_status", ws(DEMO_WORKTREES[i] ?? "")) as GitPanel).status;
      expect(panel.branch, w.slug).toBe(branchOf(w.slug));
      // A finished agent's work is committed; a working or waiting one's isn't yet.
      expect(panel.unstaged.map((f) => f.path).sort(), w.slug).toEqual(
        w.state === "done" ? [] : editsOf(w).map((e) => e.edit).sort(),
      );
    }
    const waiting = worktreeIn("waiting");
    const id = DEMO_WORKTREES[STORY.worktrees.indexOf(waiting)] ?? "";
    const diff = core.handle("git_diff", { ...ws(id), path: "src/search/results.tsx", staged: false }) as FileDiff;
    expect([diff.additions, diff.deletions]).toEqual([22, 6]);
    expect((core.handle("git_status", ws(shopId())) as GitPanel).status).toMatchObject({ branch: STORY.base, unstaged: [] });
    const readme = core.handle("fs_read", { ...ws(shopId()), path: "README.md" }) as { hash: string };
    core.handle("fs_write", { ...ws(shopId()), path: "README.md", text: "# shop, edited\n", expectedHash: readme.hash });
    const after = core.handle("git_status", ws(shopId())) as GitPanel;
    expect(after.status.unstaged).toEqual([{ path: "README.md", change: "modified" }]);
    expect((core.handle("git_diff", { ...ws(shopId()), path: "README.md", staged: false }) as FileDiff).patch).toContain(
      "+# shop, edited",
    );
    expect((core.handle("git_branches", ws(id)) as Array<{ name: string; current: boolean }>).filter((b) => b.current)).toEqual([
      expect.objectContaining({ name: branchOf(waiting.slug) }),
    ]);
    for (const cmd of ["git_stage", "git_commit", "git_push", "git_switch"]) {
      expect(() => core?.handle(cmd, { ...ws(shopId()), paths: ["README.md"] })).toThrow();
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

  it("demo the shell's help names what it answers, and clear clears", () => {
    core = new DemoCore(0);
    const lines: string[] = [];
    core.handle("pty_spawn", { paneId: "pn_shell", channel: { onmessage: () => undefined } });
    const said = (line: string) => core?.handle("pty_write", { paneId: "pn_shell", data: `${line}\r` });
    // pty_write answers through the channel; the shell itself is checked above.
    core.handle("pty_attach", {
      paneId: "pn_shell",
      channel: { onmessage: (f: Uint8Array) => lines.push(new TextDecoder().decode(decodeFrame(f).payload)) },
    });
    said("help");
    const help = lines.join("");
    for (const c of ["ls", "cat", "pwd", "git status", "git diff", "git log", "pnpm test", "clear"]) expect(help).toContain(c);
    lines.length = 0;
    said("clear");
    expect(lines.join("")).toContain("\x1b[2J");
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

describe("D-2609 the demo's sample world is the story the site's scenes tell (D-1899)", () => {
  const screenOf = async (c: DemoCore, paneId: string): Promise<string> => {
    c.handle("pty_spawn", { paneId, channel: { onmessage: () => undefined } });
    await tick();
    const snap = c.handle("pty_attach", { paneId, channel: { onmessage: () => undefined } }) as { replay: string };
    return new TextDecoder().decode(Uint8Array.from(atob(snap.replay), (ch) => ch.charCodeAt(0)));
  };

  it("D-2609 Code mode is shop and its three worktrees, each agent in its own worktree's tab, opening on the working one", () => {
    core = new DemoCore(0);
    const workspaces = core.handle("workspace_list", {}) as WorkspaceSummary[];
    const shop = workspaces.find((w) => w.name === STORY.repo);
    expect(shop?.parentId).toBeNull();
    // One repository and its worktrees, nothing else (D-1899).
    expect(workspaces).toHaveLength(1 + STORY.worktrees.length);
    const wts = workspaces.filter((w) => w.parentId === shop?.id);
    expect(wts.map((w) => w.name)).toEqual(STORY.worktrees.map((w) => w.slug));
    expect(wts.map((w) => w.laneBranch)).toEqual(STORY.worktrees.map((w) => branchOf(w.slug)));
    expect(wts.every((w) => w.laneBase === STORY.base && w.root.startsWith(`${shop?.root}/.tandem/worktrees/`))).toBe(true);
    const panes = core.handle("pane_list", {}) as PaneRecord[];
    for (const w of STORY.worktrees) {
      const ws = wts.find((x) => x.name === w.slug);
      const agents = panes.filter((p) => p.kind === "agent" && p.workspaceId === ws?.id);
      expect(agents.map((p) => p.engineId), w.slug).toEqual([w.engine]);
      // A worktree's tab holds only its own panes: its agent shares no grid.
      const layouts = core.handle("layout_get", { workspaceId: ws?.id }) as TabLayout[];
      expect(layouts, w.slug).toHaveLength(1);
      const tabPanes = core.handle("pane_list", { tabId: layouts[0]?.tab.id }) as PaneRecord[];
      expect(tabPanes.every((p) => p.workspaceId === ws?.id), w.slug).toBe(true);
    }
    // The Dashboard's states are the story's.
    const dash = core.handle("status_snapshot", {}) as DashboardItem[];
    for (const w of STORY.worktrees) {
      const pane = panes.find((p) => p.kind === "agent" && p.engineId === w.engine);
      expect(dash.find((d) => d.key === `pane:${pane?.id}`)?.status, w.slug).toBe(w.state);
    }
    // It opens on the working agent's worktree; the coach points at the waiting one.
    expect(DEMO_ACTIVE).toBe(wts.find((w) => w.name === worktreeIn("working").slug)?.id);
    expect(DEMO_WAITING.workspaceId).toBe(wts.find((w) => w.name === worktreeIn("waiting").slug)?.id);
    expect(DEMO_WAITING.branch).toBe(branchOf(worktreeIn("waiting").slug));
  });

  it("D-2609 each agent's terminal says what the story says it did; the waiting one asks to run its command", async () => {
    core = new DemoCore(0);
    const panes = core.handle("pane_list", {}) as PaneRecord[];
    for (const w of STORY.worktrees) {
      const pane = panes.find((p) => p.kind === "agent" && p.engineId === w.engine);
      const screen = await screenOf(core, pane?.id ?? "");
      expect(screen, w.slug).toContain(w.task);
      for (const step of w.steps) {
        const said = "read" in step ? step.read : "edit" in step ? step.edit : step.run;
        expect(screen, `${w.slug}: ${said}`).toContain(said);
        if ("edit" in step) expect(screen).toContain(`+${step.add} -${step.del}`);
      }
    }
    expect(await screenOf(core, DEMO_WAITING.paneId)).toContain(worktreeIn("waiting").ask ?? "");
  });

  it("D-2609 the worktrees answer as the app does with no pull request, no overlap and no landing yet", () => {
    core = new DemoCore(0);
    const wts = (core.handle("workspace_list", {}) as WorkspaceSummary[]).filter((w) => w.parentId !== null);
    for (const w of wts) {
      expect(core.handle("lane_checks", { laneId: w.id })).toMatchObject({ laneId: w.id, state: "no_pr", pr: null, checks: [] });
    }
    expect(core.handle("lane_radar", { laneIds: wts.map((w) => w.id) })).toEqual([]);
    expect(core.handle("merge_queue_get", { workspaceId: wts[0]?.parentId })).toBeNull();
  });
});
