/** The browser demo's stand-in for Vibezeo's core (vibezeo.com/demo): the
 * real interface runs against this simulation, in memory. Nothing runs and
 * nothing leaves the browser. */
import type {
  EditorTabs,
  AppBoot,
  AttachSnapshot,
  AuthState,
  DashboardItem,
  EngineSpec,
  EngineView,
  EntitlementView,
  FileText,
  LayoutNode,
  ModelEntry,
  PaneRecord,
  PlanRevision,
  SendResult,
  StatusRecord,
  Tab,
  TabLayout,
  Thread,
  ThreadChanges,
  ThreadDetail,
  ThreadEvent,
  ThreadEventEnvelope,
  ThreadEventPage,
  ThreadsChanged,
  Turn,
  WorkspaceSummary,
} from "@tandem/protocol";
import { emitLocal } from "../ipc/events";
import { setInvokeBackend } from "../ipc/invoke";
import { FrameKind, encodeFrame, type FrameChannel } from "../ipc/pty";
import { DEMO_CHANGES, afterAllow, afterDeny, chatTurn, fixTurn, planText, planTurn, type Step } from "./agent";
import { DemoFiles, NO_GIT } from "./files";
import { DemoNotes } from "./notes";
import { DemoShell, agentScreen } from "./shell";

type Args = Record<string, unknown> | undefined;

const WS = "ws_demo";
const TAB = "tb_demo";
const WS_API = "ws_api";
const WS_DOCS = "ws_docs";

/** Messages a visit can send, and their length (the page's limits). */
export const DEMO_MESSAGE_LIMIT = 12;
export const DEMO_MESSAGE_MAX = 500;
const LIMIT_REACHED = "Demo limit reached. Download Vibezeo for Linux to keep going, or reset the demo.";
const START = Date.now() - 42 * 60_000;

const BOOT: AppBoot = {
  version: "0.3.0",
  build: "demo",
  platform: "linux",
  dev: false,
  firstRun: false,
  lastMode: "thread",
  flags: [],
  dataDirDisplay: "~/.vibezeo",
};

/** Early access, as the released download runs it after "Start using
 * Vibezeo": no account, so no avatar email and no credits (D-810). */
const AUTH: AuthState = {
  signedIn: false,
  email: null,
  offline: true,
  offlineDaysLeft: null,
  hosted: false,
  pending: null,
  error: null,
};

/** The Free plan, as the released download runs it (D-128). */
const ENTITLEMENT: EntitlementView = {
  plan: "free",
  status: "none",
  limits: {
    panes: 4,
    workspaces: 3,
    threadGrid: 1,
    teammates: 1,
    routines: false,
    backgroundRoutines: false,
    taskExecution: false,
    pluginConnections: 1,
    accountsPerEngine: 1,
    cloudStt: false,
    enhance: false,
    conductor: false,
    auto: false,
    phone: false,
    admin: false,
    chatEngines: ["claude", "codex"],
    creditPacks: false,
  },
  source: "free",
  credits: null,
  policy: null,
  org: null,
  paidUntil: null,
  cancelAtPeriodEnd: false,
  online: true,
  confirmedUntil: null,
  monthlyGrant: 10_000,
};

function spec(id: string, label: string, vendor: string, logo: string, chatAdapter: string | null): EngineSpec {
  return {
    id,
    label,
    vendor,
    logo,
    commands: [id],
    versionCmd: [id, "--version"],
    versionRegex: "",
    install: { mac: null, lin: null, win: null },
    loginCmd: null,
    loginMode: "argv",
    logoutCmd: null,
    statusCmd: null,
    signedInFile: null,
    chatAdapter,
    acpCmd: null,
    adapterVersions: null,
    providerHomeEnv: null,
    homeStrategy: "none",
    keychainIsolation: "n/a",
    resumeCmdTemplate: null,
    supportsResume: true,
    newlineSequence: "\x1b\r",
    hooksSupport: "none",
    usageLogGlobs: [],
    skillsDir: null,
    cheapModel: null,
    headlessCmd: null,
    usageProbe: null,
    titlePatterns: [],
    readyPatterns: [],
    waitingPatterns: [],
    limitPatterns: [],
    onboardingSeed: null,
    mcpNative: null,
  };
}

const ENGINES: EngineView[] = [
  ["claude", "Claude Code", "Anthropic", "CC", "claude", "2.1.0"],
  ["codex", "Codex", "OpenAI", "CX", "codex", "0.46.0"],
  ["gemini", "Gemini CLI", "Google", "GE", null, "0.9.0"],
].map(([id, label, vendor, logo, adapter, version]) => ({
  spec: spec(id ?? "", label ?? "", vendor ?? "", logo ?? "", adapter ?? null),
  install: { path: `/usr/local/bin/${id}`, version: version ?? null, source: "shell" },
  chatCapable: adapter !== null,
  unsupportedReason: null,
  extraArgs: [],
}));

const MODELS: Record<string, ModelEntry[]> = {
  claude: [
    { id: "default", label: "Default", description: "The engine's own choice", isDefault: true, effortLevels: [], contextWindow: 200_000, source: "bundled" },
    { id: "sonnet", label: "Sonnet", description: "Fast and capable", isDefault: false, effortLevels: [], contextWindow: 200_000, source: "bundled" },
    { id: "opus", label: "Opus", description: "The most capable", isDefault: false, effortLevels: [], contextWindow: 200_000, source: "bundled" },
  ],
  codex: [
    { id: "default", label: "Default", description: "The engine's own choice", isDefault: true, effortLevels: ["low", "medium", "high"], contextWindow: 272_000, source: "bundled" },
  ],
};

const TAB_RECORD: Tab = { id: TAB, workspaceId: WS, name: "checkout", sortKey: "a0", createdAt: START };
const TABS: Tab[] = [
  TAB_RECORD,
  { id: "tb_api", workspaceId: WS_API, name: "docs", sortKey: "a0", createdAt: START },
  { id: "tb_docs", workspaceId: WS_DOCS, name: "main", sortKey: "a0", createdAt: START },
];

function workspace(id: string, name: string, branch: string, liveCount: number, sortKey: string): WorkspaceSummary {
  return {
    id,
    kind: "local",
    hostId: "local",
    root: `/home/you/${name}`,
    wslDistro: null,
    name,
    parentId: null,
    laneBranch: null,
    laneBase: null,
    sortKey,
    pinned: false,
    collapsed: false,
    sessionsCollapsed: false,
    createdAt: START,
    missing: false,
    branch,
    liveCount,
    tabs: TABS.filter((t) => t.workspaceId === id),
  };
}

const WORKSPACE = workspace(WS, "shop", "main", 3, "a0");
const WORKSPACES: WorkspaceSummary[] = [WORKSPACE, workspace(WS_API, "api", "main", 1, "a1"), workspace(WS_DOCS, "docs-site", "main", 0, "a2")];

function pane(id: string, kind: string, engineId: string | null, humanName: string | null, ws = WORKSPACE): PaneRecord {
  return {
    id,
    tabId: ws.tabs[0]?.id ?? TAB,
    workspaceId: ws.id,
    kind,
    cwd: ws.root,
    engineId,
    accountId: null,
    humanName,
    title: null,
    restoreState: "live",
    browserUrl: null,
    createdAt: START,
  };
}

const API_WS = WORKSPACES[1] ?? WORKSPACE;

/** Four live panes: the Free plan's limit. */
const PANES: PaneRecord[] = [
  pane("pn_claude", "agent", "claude", "Fixer"),
  pane("pn_codex", "agent", "codex", "Reviewer"),
  pane("pn_shell", "terminal", null, null),
  pane("pn_gemini", "agent", "gemini", "Docs", API_WS),
];

const TREE: LayoutNode = {
  type: "split",
  dir: "row",
  ratio: 0.55,
  a: { type: "leaf", paneId: "pn_claude" },
  b: {
    type: "split",
    dir: "col",
    ratio: 0.5,
    a: { type: "leaf", paneId: "pn_codex" },
    b: { type: "leaf", paneId: "pn_shell" },
  },
};

function layoutOf(workspaceId: unknown): TabLayout[] {
  const tab = TABS.find((t) => t.workspaceId === workspaceId);
  if (!tab) return [];
  if (tab.id === TAB) return [{ tab, tree: TREE, activePaneId: "pn_claude", maximizedPaneId: null }];
  if (tab.id === "tb_api") return [{ tab, tree: { type: "leaf", paneId: "pn_gemini" }, activePaneId: "pn_gemini", maximizedPaneId: null }];
  return [{ tab, tree: null, activePaneId: null, maximizedPaneId: null }];
}

const AGENT_SCREENS: Record<string, string> = {
  pn_claude: agentScreen("Claude Code", "Fix the redirect loop after login", [
    "● Read src/checkout/redirect.ts",
    "● Update src/checkout/redirect.ts (+3 -1)",
    "● Bash npm test -- src/checkout",
    "  ✓ 13 tests passed",
    "",
    "The loop came from sending /login back to itself. Fixed, with a test.",
  ]),
  pn_codex: agentScreen("Codex", "Review the open diff for edge cases", [
    "• Reading the diff in src/checkout…",
    "• Checking callers of nextStep()",
    "",
    "Working…",
  ]),
  pn_gemini: agentScreen("Gemini CLI", "Update the API docs for the new rate limit", [
    "✦ Read docs/orders.md",
    "✦ Drafted the \"Rate limits\" section (20 requests a minute per user)",
    "",
    "Allow running npm run docs:build? (y/n)",
  ]),
};

const STATUS: Record<string, { status: StatusRecord["status"]; title: string; preview: string }> = {
  pn_claude: { status: "done", title: "Fix the redirect loop after login", preview: "Fixed, with a test. 13 tests pass." },
  pn_codex: { status: "working", title: "Review the open diff", preview: "Checking callers of nextStep()" },
  pn_gemini: { status: "waiting", title: "Update the API docs", preview: "Allow running npm run docs:build?" },
};

type Pty = { channel: FrameChannel; seq: number; screen: string; shell: DemoShell | null };

type DemoThread = {
  thread: Thread;
  turns: Turn[];
  events: ThreadEventEnvelope[];
  channels: Set<{ onmessage: (e: ThreadEventEnvelope) => void }>;
  running: string | null;
  /** Steps waiting on the visitor's answer. */
  awaiting: { approvalId: string; turnId: string } | null;
  fixed: boolean;
  divider: string | null;
  /** The composer's draft (`{text, attachmentIds}` JSON). */
  draft: string;
  /** Plan versions from Plan-mode turns (F-TH-10). */
  plans: PlanRevision[];
};

/** What the first thread's composer starts with. */
export const FIRST_MESSAGE =
  "After signing in, checkout sends some people back to /login forever. Find out why and fix it.";

const encoder = new TextEncoder();

function base64(text: string): string {
  let bin = "";
  for (const b of encoder.encode(text)) bin += String.fromCharCode(b);
  return btoa(bin);
}

/** The simulated core. One per page. */
export class DemoCore {
  private ptys = new Map<string, Pty>();
  private threads = new Map<string, DemoThread>();
  private settings: Record<string, unknown> = { theme: "dark" };
  /** The Files tab's made-up project (in memory). */
  readonly files = new DemoFiles();
  readonly notes = new DemoNotes();
  private ids = 0;
  private timers = new Set<ReturnType<typeof setTimeout>>();

  constructor(private readonly speed = 1) {
    this.seed();
  }

  private id(prefix: string): string {
    this.ids += 1;
    return `${prefix}_demo${this.ids}`;
  }

  private later(ms: number, fn: () => void): void {
    const t = setTimeout(() => {
      this.timers.delete(t);
      fn();
    }, ms * this.speed);
    this.timers.add(t);
  }

  /** Stops every pending step (tests). */
  dispose(): void {
    for (const t of this.timers) clearTimeout(t);
    this.timers.clear();
  }

  private newThread(title: string, engineId: string, workspaceId: string | null, at: number): DemoThread {
    const thread: Thread = {
      id: this.id("thr"),
      kind: "thread",
      workspaceId,
      cwd: workspaceId ? WORKSPACE.root : "/home/you/.vibezeo/scratch",
      engineId,
      accountId: null,
      model: null,
      effort: null,
      access: workspaceId ? "edits" : "read",
      mode: "act",
      title,
      state: "idle",
      attended: true,
      engineSessionId: null,
      pinned: false,
      archived: false,
      unread: false,
      sortKey: String(at),
      createdAt: at,
      updatedAt: at,
    };
    const t: DemoThread = { thread, turns: [], events: [], channels: new Set(), running: null, awaiting: null, fixed: false, divider: null, draft: "{}", plans: [] };
    this.threads.set(thread.id, t);
    return t;
  }

  /** A finished thread to look at, and an empty one to try. */
  private seed(): void {
    const done = this.newThread("Add rate limiting to the API", "codex", WS, START);
    const turn = this.startTurn(done, "Add rate limiting to POST /api/orders: 20 requests a minute per user.");
    const steps: ThreadEvent[] = [
      { type: "reasoning", text: "There's already a token bucket in src/api/limits.ts; reuse it rather than adding a dependency.", durationMs: 2400 },
      { type: "tool", id: "s1", verb: "read", argSummary: "src/api/limits.ts", status: "ok", output: "(48 lines)", outputTruncated: false, outputFile: null, diff: null, detail: null },
      { type: "tool", id: "s2", verb: "edit", argSummary: "src/api/orders.ts", status: "ok", output: null, outputTruncated: false, outputFile: null, diff: null, detail: null },
      { type: "tool", id: "s3", verb: "write", argSummary: "src/api/rate-limit.test.ts", status: "ok", output: null, outputTruncated: false, outputFile: null, diff: null, detail: null },
      { type: "tool", id: "s4", verb: "bash", argSummary: "npm test -- src/api", status: "ok", output: "Tests  6 passed (6)", outputTruncated: false, outputFile: null, diff: null, detail: null },
      {
        type: "assistant",
        markdown:
          "Added a per-user limit of **20 requests a minute** to `POST /api/orders`, reusing the token bucket in `src/api/limits.ts`. Over the limit, the API answers `429` with a `Retry-After` header.\n\nSix new tests cover the limit, the reset and the header.",
      },
      { type: "usage", input: 22_310, output: 1_840, cacheRead: 0, cacheWrite: 0, contextFillPct: 11.4, costUsd: null },
    ];
    for (const e of steps) this.push(done, turn, e);
    this.endTurn(done, turn, "completed");
    done.thread.state = "done";
    done.thread.updatedAt = START + 20 * 60_000;

    // A thread moved from Claude Code to Codex (F-TH-16).
    const moved = this.newThread("Speed up the product search", "claude", WS, START + 30_000);
    const first = this.startTurn(moved, "Product search takes two seconds. Find the slow part.");
    this.push(moved, first, { type: "tool", id: "m1", verb: "read", argSummary: "src/search/query.ts", status: "ok", output: "(61 lines)", outputTruncated: false, outputFile: null, diff: null, detail: null });
    this.push(moved, first, {
      type: "assistant",
      markdown: "The search loads every product and filters in memory (`src/search/query.ts`, line 18). A `LIKE` query with an index on `name` would let the database do it.",
    });
    this.endTurn(moved, first, "completed");
    moved.thread.engineId = "codex";
    moved.divider = "Continued on Codex (Default) with a summary";
    const second = this.startTurn(moved, "Make that change and measure it.");
    this.push(moved, second, { type: "tool", id: "m2", verb: "edit", argSummary: "src/search/query.ts", status: "ok", output: null, outputTruncated: false, outputFile: null, diff: null, detail: null });
    this.push(moved, second, {
      type: "assistant",
      markdown: "Search now filters in the database with an index on `name`. The same query takes **40 ms** instead of two seconds on the sample data.",
    });
    this.endTurn(moved, second, "completed");
    moved.thread.updatedAt = START + 35 * 60_000;

    const fresh = this.newThread("Fix the redirect loop after login", "claude", WS, START + 60_000);
    fresh.draft = JSON.stringify({ text: FIRST_MESSAGE, attachmentIds: [] });
    this.firstThread = fresh.thread.id;
  }

  /** The thread the demo opens on. */
  firstThread = "";

  private startTurn(t: DemoThread, text: string): string {
    const id = this.id("turn");
    const turn: Turn = {
      id,
      threadId: t.thread.id,
      n: t.turns.length + 1,
      engineId: t.thread.engineId,
      accountId: null,
      state: "running",
      blockedActions: 0,
      startedAt: Date.now(),
      endedAt: null,
    };
    t.turns.push(turn);
    t.running = id;
    t.thread.state = "working";
    this.push(t, id, { type: "user", text, attachments: [] });
    if (t.divider) {
      this.push(t, id, { type: "divider", text: t.divider });
      t.divider = null;
    }
    return id;
  }

  private push(t: DemoThread, turnId: string, event: ThreadEvent): void {
    const seq = t.events.filter((e) => e.turnId === turnId).length;
    const env: ThreadEventEnvelope = { threadId: t.thread.id, turnId, seq, ts: Date.now(), event };
    t.events.push(env);
    t.thread.updatedAt = env.ts;
    for (const ch of t.channels) ch.onmessage(env);
  }

  private endTurn(t: DemoThread, turnId: string, reason: "completed" | "cancelled"): void {
    this.push(t, turnId, { type: "turn_end", reason, blockedActions: 0 });
    const turn = t.turns.find((x) => x.id === turnId);
    if (turn) {
      turn.state = reason === "completed" ? "completed" : "cancelled";
      turn.endedAt = Date.now();
    }
    t.running = null;
    t.thread.state = reason === "completed" ? "done" : "idle";
    this.changed(t);
  }

  private changed(t: DemoThread): void {
    const threads: ThreadsChanged = { threads: [...this.threads.values()].map((x) => ({ ...x.thread })) };
    emitLocal("threads:changed", threads);
    const status = this.dashboard().find((d) => d.key === `thread:${t.thread.id}`);
    if (status)
      emitLocal("status:changed", {
        records: [
          {
            key: status.key,
            status: status.status,
            reason: status.reason,
            since: status.since,
            ts: Date.now(),
            tool: null,
            title: status.title,
            preview: null,
          } satisfies StatusRecord,
        ],
        removed: [],
      });
  }

  /** Plays steps in order; an approval stops until it's answered. */
  private play(t: DemoThread, turnId: string, steps: Step[], atEnd?: () => void): void {
    const [step, ...rest] = steps;
    if (!step) {
      atEnd?.();
      return this.endTurn(t, turnId, "completed");
    }
    if ("approval" in step) {
      t.awaiting = { approvalId: step.approval.id, turnId };
      t.thread.state = "waiting";
      this.push(t, turnId, step.approval);
      this.changed(t);
      return;
    }
    this.later(step.wait, () => {
      if (t.running !== turnId) return;
      this.push(t, turnId, step.event);
      this.play(t, turnId, rest, atEnd);
    });
  }

  private thread(args: Args): DemoThread {
    const t = this.threads.get(String(args?.threadId ?? ""));
    if (!t) throw { code: "not_found", message: "No such thread" };
    return t;
  }

  private detail(t: DemoThread): ThreadDetail {
    return {
      thread: t.thread,
      turns: t.turns,
      running: t.running,
      queued: null,
      draft: t.draft,
      attachments: [],
      planRevisions: t.plans,
    };
  }

  private send(t: DemoThread, text: string): SendResult {
    if (t.running) throw { code: "conflict", message: "Working — send when it's done, or stop it" };
    if (text.length > DEMO_MESSAGE_MAX) throw { code: "invalid", message: `Demo messages are up to ${DEMO_MESSAGE_MAX} characters` };
    if (this.sent >= DEMO_MESSAGE_LIMIT) {
      this.notify();
      throw { code: "limit_reached", message: LIMIT_REACHED };
    }
    this.sent += 1;
    this.notify();
    const turnId = this.startTurn(t, text);
    this.changed(t);
    const first = !t.fixed && t.thread.workspaceId !== null && t.turns.length === 1 && t.thread.mode === "act";
    if (t.thread.mode === "plan") {
      const plan = planText(text);
      this.play(t, turnId, planTurn(plan), () => {
        const version = t.plans.length + 1;
        t.plans.push({ version, turnId, text: plan, createdAt: Date.now() });
        this.push(t, turnId, { type: "plan_revision", version });
      });
    } else {
      this.play(t, turnId, first ? fixTurn(this.id("apr"), Date.now() + 10 * 60_000) : chatTurn(text));
    }
    return { turnId, queued: false };
  }

  /** Messages sent this visit, and who wants to hear about it (DemoBar). */
  private sent = 0;
  private listeners = new Set<() => void>();

  messagesLeft(): number {
    return Math.max(0, DEMO_MESSAGE_LIMIT - this.sent);
  }

  limitReached(): boolean {
    return this.sent >= DEMO_MESSAGE_LIMIT;
  }

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  private notify(): void {
    for (const fn of this.listeners) fn();
  }

  private frame(pty: Pty, text: string): void {
    pty.seq += 1;
    pty.screen += text;
    pty.channel.onmessage(encodeFrame(FrameKind.Data, pty.seq, encoder.encode(text)));
  }

  private dashboard(): DashboardItem[] {
    const panes = PANES.filter((p) => STATUS[p.id]).map((p): DashboardItem => {
      const s = STATUS[p.id] ?? { status: "idle", title: "", preview: "" };
      const engine = ENGINES.find((e) => e.spec.id === p.engineId);
      return {
        key: `pane:${p.id}`,
        source: { type: "pane", paneId: p.id },
        status: s.status,
        reason: null,
        since: START,
        title: s.title,
        humanName: p.humanName,
        engineId: p.engineId,
        engineLabel: engine?.spec.label ?? null,
        engineLogo: engine?.spec.logo ?? null,
        accountLabel: null,
        workspaceId: p.workspaceId,
        workspace: WORKSPACES.find((w) => w.id === p.workspaceId)?.name ?? null,
        branch: "main",
        preview: s.preview,
        pendingApproval: null,
      };
    });
    const threads = [...this.threads.values()].map((t): DashboardItem => {
      const engine = ENGINES.find((e) => e.spec.id === t.thread.engineId);
      const status = t.thread.state === "working" ? "working" : t.thread.state === "waiting" ? "waiting" : t.thread.state === "done" ? "done" : "idle";
      return {
        key: `thread:${t.thread.id}`,
        source: { type: "thread", threadId: t.thread.id },
        status,
        reason: status === "waiting" ? "approval" : null,
        since: t.thread.updatedAt,
        title: t.thread.title,
        humanName: null,
        engineId: t.thread.engineId,
        engineLabel: engine?.spec.label ?? null,
        engineLogo: engine?.spec.logo ?? null,
        accountLabel: null,
        workspaceId: t.thread.workspaceId,
        workspace: t.thread.workspaceId ? WORKSPACE.name : null,
        branch: null,
        preview: null,
        pendingApproval: null,
      };
    });
    return [...panes, ...threads];
  }

  /** Answers one command, as the core would. */
  handle(cmd: string, args: Args): unknown {
    switch (cmd) {
      case "app_ready":
        return BOOT;
      case "auth_state":
        return AUTH;
      case "entitlement_get":
      case "entitlement_refresh":
        return ENTITLEMENT;
      case "settings_get_all":
        return this.settings;
      case "settings_set":
        this.settings[String(args?.key)] = args?.value;
        return null;
      case "onboarding_get":
        return { step: null, done: true, steps: [] };
      case "restore_get_snapshot":
        return { panes: [], interruptedThreads: 0, autoResume: false };
      case "updates_state":
        return { stage: "idle", current: BOOT.version, channel: "stable", version: null, notes: null, percent: null, errorStage: null, error: null, lastCheck: null, upToDate: true, minVersion: null };
      case "usage_report":
        // The demo reads no engine logs: an honest empty page (F-AC-07).
        return demoUsage();
      case "notifications_list":
        return { items: [], nextCursor: null, unread: 0 };
      case "sidebar_get":
        return { open: false, tabs: [], active: null, width: 360, fullscreen: false, pinned: false };
      case "accounts_status":
        return "signed_in";
      case "workspace_list":
        return WORKSPACES;
      case "layout_get":
        return layoutOf(args?.workspaceId);
      case "pane_list":
        return typeof args?.tabId === "string" ? PANES.filter((p) => p.tabId === args.tabId) : PANES;
      case "engines_list":
        return ENGINES;
      case "engines_detect":
        return { found: [], slowShell: null, shellsAsked: [] };
      case "models_list":
        return MODELS[String(args?.engineId)] ?? [];
      case "status_list":
        return PANES.filter((p) => STATUS[p.id]).map((p): StatusRecord => ({
          key: `pane:${p.id}`,
          status: STATUS[p.id]?.status ?? "idle",
          reason: null,
          since: START,
          ts: START,
          tool: null,
          title: STATUS[p.id]?.title ?? null,
          preview: STATUS[p.id]?.preview ?? null,
        }));
      case "status_snapshot":
        return this.dashboard();
      case "pty_attach": {
        const pty = this.ptys.get(String(args?.paneId));
        if (!pty) throw { code: "not_found", message: "Not running" };
        pty.channel = args?.channel as FrameChannel;
        const snap: AttachSnapshot = { replay: base64(pty.screen), live: true, state: null };
        return snap;
      }
      case "pty_spawn": {
        const id = String(args?.paneId);
        const shell = id === "pn_shell" ? new DemoShell() : null;
        const pty: Pty = { channel: args?.channel as FrameChannel, seq: 0, screen: "", shell };
        this.ptys.set(id, pty);
        this.later(80, () => this.frame(pty, shell ? shell.greeting() : (AGENT_SCREENS[id] ?? "")));
        return { pid: 4000 + this.ptys.size, startedAt: Date.now() };
      }
      case "pty_write": {
        const pty = this.ptys.get(String(args?.paneId));
        const data = String(args?.data ?? "");
        if (pty?.shell) this.frame(pty, pty.shell.write(data));
        else if (pty && data.includes("\r"))
          this.frame(pty, "\r\n\x1b[2m(This agent is simulated. In Vibezeo you'd be talking to it here.)\x1b[0m\r\n");
        return null;
      }
      case "thread_list":
        return [...this.threads.values()].map((t) => t.thread);
      case "thread_get":
        return this.detail(this.thread(args));
      case "thread_events": {
        const page: ThreadEventPage = { items: this.thread(args).events, nextBeforeId: null };
        return page;
      }
      case "thread_subscribe":
        this.thread(args).channels.add(args?.channel as { onmessage: (e: ThreadEventEnvelope) => void });
        return null;
      case "thread_create": {
        const a = (args?.args ?? {}) as { workspaceId?: string | null; engineId?: string | null };
        const t = this.newThread("", a.engineId ?? "claude", a.workspaceId ?? null, Date.now());
        this.changed(t);
        return t.thread;
      }
      case "thread_save_draft":
        this.thread(args).draft = JSON.stringify(args?.draft ?? {});
        return null;
      case "thread_send": {
        const t = this.thread(args);
        t.draft = "{}";
        const text = String(args?.text ?? "");
        if (!t.thread.title) t.thread.title = text.split("\n")[0]?.slice(0, 60) ?? "New thread";
        return this.send(t, text);
      }
      case "approval_resolve": {
        const t = [...this.threads.values()].find((x) => x.awaiting?.approvalId === args?.approvalId);
        if (!t?.awaiting) return null;
        const { turnId, approvalId } = t.awaiting;
        t.awaiting = null;
        const decision = args?.decision as { type: string; scope?: string };
        const allowed = decision.type === "allow";
        this.push(t, turnId, {
          type: "approval_resolved",
          id: approvalId,
          decision: allowed ? { type: "allow", scope: decision.scope === "thread" ? "thread" : "once" } : { type: "deny", reason: null },
        });
        t.thread.state = "working";
        t.fixed = true;
        this.changed(t);
        this.play(t, turnId, allowed ? afterAllow() : afterDeny());
        return null;
      }
      case "thread_stop": {
        const t = this.thread(args);
        if (t.running) this.endTurn(t, t.running, "cancelled");
        t.awaiting = null;
        return "stopped";
      }
      case "thread_update": {
        const t = this.thread(args);
        const { threadId: _id, ...patch } = args ?? {};
        Object.assign(t.thread, patch);
        this.changed(t);
        return t.thread;
      }
      case "thread_switch_engine": {
        const t = this.thread(args);
        const engineId = String(args?.engineId);
        if (engineId !== t.thread.engineId && t.turns.length > 0) {
          const label = ENGINES.find((e) => e.spec.id === engineId)?.spec.label ?? engineId;
          t.divider = `Continued on ${label} (Default) with a summary`;
        }
        t.thread.engineId = engineId;
        this.changed(t);
        return t.thread;
      }
      case "thread_archive": {
        const t = this.thread(args);
        t.thread.archived = Boolean(args?.archived);
        this.changed(t);
        return null;
      }
      case "thread_changes": {
        const t = this.thread(args);
        const changes: ThreadChanges = { source: t.turns.length ? "git" : "not_started", files: t.fixed ? DEMO_CHANGES : [] };
        return changes;
      }
      case "thread_file_read": {
        const file = DEMO_CHANGES.find((f) => f.path === args?.path);
        const text: FileText = {
          path: String(args?.path ?? ""),
          content: file ? file.patch.split("\n").filter((l) => !l.startsWith("-") && !l.startsWith("@@") && !l.startsWith("diff")).map((l) => l.replace(/^[+ ]/, "")).join("\n") : "",
          binary: false,
          truncated: false,
        };
        return text;
      }
      case "thread_mention_search":
        return ["src/checkout/redirect.ts", "src/checkout/redirect.test.ts", "src/api/orders.ts", "package.json"].map((path) => ({ path, kind: "file" }));
      case "thread_implement_plan": {
        const t = this.thread(args);
        const plan = t.plans.find((p) => p.version === Number(args?.version));
        if (!plan) throw { code: "not_found", message: `There's no plan version ${String(args?.version)}` };
        t.thread.mode = "act";
        return this.send(t, `Implement plan version ${plan.version} exactly as written:\n\n${plan.text}`);
      }
      case "thread_retry": {
        const t = this.thread(args);
        const last = [...t.events].reverse().find((e) => e.event.type === "user");
        return this.send(t, last?.event.type === "user" ? last.event.text : "");
      }
      case "app_open_external":
        if (typeof args?.url === "string" && args.url.startsWith("https://")) window.open(args.url, "_blank", "noopener");
        return null;
      case "fs_list":
        return this.files.list(String(args?.dir ?? ""));
      case "fs_read":
        return this.files.read(String(args?.path ?? ""));
      case "fs_write":
        return this.files.write(String(args?.path ?? ""), String(args?.text ?? ""), (args?.expectedHash as string | null | undefined) ?? null);
      case "fs_compare":
        return this.files.compare(String(args?.path ?? ""), String(args?.text ?? ""));
      case "fs_create":
      case "fs_rename":
      case "fs_delete":
        throw { code: "unavailable", message: "The browser demo doesn't create, rename or delete files." };
      case "fs_watch":
        return "demo-watch";
      case "git_status":
        return this.files.gitStatus();
      case "git_diff":
        return this.files.gitDiff(String(args?.path ?? ""));
      case "git_branches":
        return this.files.gitBranches();
      case "git_stage":
      case "git_unstage":
      case "git_discard":
      case "git_commit":
      case "git_push":
      case "git_pull":
      case "git_switch":
      case "git_create_branch":
        throw { code: "unavailable", message: NO_GIT };
      // Nothing typed here is kept anywhere, so there's nothing to export or delete (F-TL-05).
      case "data_export":
      case "data_delete_local":
        throw {
          code: "unavailable",
          message: "The demo keeps nothing: what you do here stays in this page and goes when you close it.",
        };
      // Find work reads real terminals and threads on this computer (F-CN-05).
      case "find_work":
        throw {
          code: "unavailable",
          message: "Find work searches the terminals and threads running on your computer, so the demo has nothing to search.",
        };
      // A web page can't hold other pages the way the app's browser does.
      case "browser_open":
      case "browser_navigate":
        throw { code: "unavailable", message: "Web pages don't open inside this demo. In the app they open right here." };
      case "editor_tabs_get":
        return this.files.savedTabs();
      case "editor_tabs_save":
        this.files.saveTabs(args?.state as EditorTabs);
        return null;
      case "scratchpad_list":
        return this.notes.list();
      case "scratchpad_read":
        return this.notes.read(String(args?.id ?? ""));
      case "scratchpad_write":
        return this.notes.write(typeof args?.id === "string" ? args.id : null, String(args?.markdown ?? ""));
      case "scratchpad_delete":
        this.notes.delete(String(args?.id ?? ""));
        return null;
      default:
        // Everything else is quiet: lists are empty, actions do nothing.
        return cmd.endsWith("_list") || cmd.endsWith("s_get") ? [] : null;
    }
  }
}

/** Replaces the Tauri bridge with the simulation. */
export function installDemo(speed = 1): DemoCore {
  const core = new DemoCore(speed);
  setInvokeBackend(async (cmd, args) => core.handle(cmd, args));
  return core;
}

/** The Usage page in the demo: 12 empty weeks, nothing invented. */
function demoUsage() {
  const today = new Date();
  const days = Array.from({ length: 84 }, (_, i) => {
    const d = new Date(today.getTime() - (83 - i) * 86_400_000);
    return { date: d.toISOString().slice(0, 10), tokens: 0, byEngine: {} };
  });
  return {
    since: days[0]?.date ?? "",
    until: days[83]?.date ?? "",
    total: 0,
    days,
    engines: [],
    accounts: [],
    workspaces: [],
    skippedLines: 0,
    filesRead: 0,
    unsupported: [],
  };
}
