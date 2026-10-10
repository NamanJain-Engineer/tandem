import { afterEach, describe, expect, it } from "vitest";
import type {
  AuthState,
  DashboardItem,
  EntitlementView,
  GitPanel,
  PaneRecord,
  ScratchNote,
  TabLayout,
  Thread,
  ThreadChanges,
  ThreadDetail,
  ThreadEvent,
  ThreadEventEnvelope,
  WorkspaceSummary,
} from "@tandem/protocol";
import { setInvokeBackend } from "../ipc/invoke";
import { FREE } from "../test/fakeBackend";
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
    core.handle("thread_subscribe", { threadId: id, channel: { onmessage: (e: ThreadEventEnvelope) => seen.push(e.event) } });
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
    const top = core.handle("fs_list", { dir: "" }) as Array<{ path: string; kind: string; gitStatus?: string }>;
    expect(top.map((e) => e.path)).toEqual(["src", "tests", "package.json", "README.md", "tsconfig.json", "vite.config.ts"]);
    expect(top.find((e) => e.path === "src")?.gitStatus).toBe("modified");
    const checkout = core.handle("fs_list", { dir: "src/checkout" }) as Array<{ path: string; gitStatus?: string }>;
    expect(checkout.find((e) => e.path === "src/checkout/redirect.ts")?.gitStatus).toBe("modified");
    const file = core.handle("fs_read", { path: "src/checkout/redirect.ts" }) as { text: string; hash: string };
    expect(file.text).toContain("export function nextStep");
    core.handle("fs_write", { path: "src/checkout/redirect.ts", text: "edited\n", expectedHash: file.hash });
    expect((core.handle("fs_read", { path: "src/checkout/redirect.ts" }) as { text: string }).text).toBe("edited\n");
    expect(() => core?.handle("fs_write", { path: "src/checkout/redirect.ts", text: "x", expectedHash: file.hash })).toThrow();
    expect(() => core?.handle("fs_delete", { path: "README.md" })).toThrow();
  });

  it("F-CD-20 its Changes view says what the terminal's git status says, plus page edits, and runs no git", () => {
    core = new DemoCore(0);
    const terminal = run("git status")?.join("\n") ?? "";
    const before = core.handle("git_status", {}) as GitPanel;
    expect(before.status.branch).toBe("main");
    expect(terminal).toContain("On branch main");
    for (const f of before.status.unstaged) expect(terminal).toContain(f.path);
    expect(before.status.unstaged.map((f) => f.path)).toEqual(["src/checkout/redirect.test.ts", "src/checkout/redirect.ts"]);
    const diff = core.handle("git_diff", { path: "src/checkout/redirect.ts", staged: false }) as { patch: string };
    expect(diff.patch).toContain("Don't send the login page back to itself.");
    const readme = core.handle("fs_read", { path: "README.md" }) as { hash: string };
    core.handle("fs_write", { path: "README.md", text: "# shop, edited\n", expectedHash: readme.hash });
    const after = core.handle("git_status", {}) as GitPanel;
    expect(after.status.unstaged.map((f) => f.path)).toContain("README.md");
    expect((core.handle("git_diff", { path: "README.md", staged: false }) as { patch: string }).patch).toContain("+# shop, edited");
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
  const send = (c: DemoCore, threadId: string, text: string) => c.handle("thread_send", { threadId, text, attachments: [] });

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

  it("demo the sample world runs the Free plan the 0.3.0 download runs", () => {
    core = new DemoCore(0);
    const view = core.handle("entitlement_get", {}) as EntitlementView;
    expect(view.plan).toBe("free");
    expect(view.limits).toEqual(FREE);
    expect((core.handle("auth_state", {}) as AuthState).hosted).toBe(false);
  });

  it("F-WB-01 demo runs early access as the 0.3.0 download does: no account, no credits", () => {
    core = new DemoCore(0);
    // What the core reports after "Start using Vibezeo" (AuthService::continue_offline, hosted off).
    const earlyAccess: AuthState = {
      signedIn: false,
      email: null,
      offline: true,
      offlineDaysLeft: null,
      hosted: false,
      pending: null,
      error: null,
    };
    expect(core.handle("auth_state", {})).toEqual(earlyAccess);
    const view = core.handle("entitlement_get", {}) as EntitlementView;
    expect(view.source).toBe("free");
    expect(view.credits).toBeNull();
  });

  it("demo the sample world: three workspaces, Claude Code, Codex and Gemini CLI panes, and the Dashboard's states", () => {
    core = new DemoCore(0);
    const workspaces = core.handle("workspace_list", {}) as WorkspaceSummary[];
    expect(workspaces.map((w) => w.name)).toEqual(["shop", "api", "docs-site"]);
    const panes = core.handle("pane_list", {}) as PaneRecord[];
    expect(panes.filter((p) => p.kind === "agent").map((p) => p.engineId).sort()).toEqual(["claude", "codex", "gemini"]);
    // The Free plan's 4 live panes.
    expect(panes.length).toBeLessThanOrEqual(FREE.panes ?? 0);
    for (const w of workspaces) {
      const layouts = core.handle("layout_get", { workspaceId: w.id }) as TabLayout[];
      expect(layouts.length, w.name).toBe(1);
      // Each tab lists only its own panes.
      const tabPanes = core.handle("pane_list", { tabId: layouts[0]?.tab.id }) as PaneRecord[];
      expect(tabPanes.every((p) => p.workspaceId === w.id), w.name).toBe(true);
    }
    expect((core.handle("pane_list", { tabId: "tb_demo" }) as PaneRecord[]).map((p) => p.id)).toEqual(["pn_claude", "pn_codex", "pn_shell"]);
    const states = new Set((core.handle("status_snapshot", {}) as DashboardItem[]).map((d) => d.status));
    for (const s of ["working", "waiting", "done"] as const) expect(states.has(s), s).toBe(true);
  });

  it("demo the shell answers help, ls, pwd, git status, git diff, git log, npm test and clear", () => {
    const help = run("help")?.join("\n") ?? "";
    for (const c of ["ls", "pwd", "git status", "git diff", "git log", "npm test", "clear"]) expect(help).toContain(c);
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
    c.handle("thread_subscribe", { threadId: id, channel: { onmessage: (e: ThreadEventEnvelope) => seen.push(e.event) } });
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
