/** The browser demo's stand-in for Vibezeo's core (vibezeo.com/demo): the
 * real interface runs against this simulation, in memory. Nothing runs and
 * nothing leaves the browser. */
import type {
  TeammateSummary,
  TeammateTemplate,
  EditorTabs,
  AppBoot,
  AttachSnapshot,
  AuthState,
  DashboardItem,
  EngineSpec,
  EngineView,
  EntitlementView,
  FileContext,
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
  PlanTask,
  OverlapReport,
  RadarMark,
  SaveIssue,
  SharedSymbol,
  SiblingOverlap,
  Turn,
  WorkspaceSummary,
} from "@tandem/protocol";
import { LIMITS } from "@tandem/protocol/plans";
import { emitLocal } from "../ipc/events";
import { setInvokeBackend } from "../ipc/invoke";
import { FrameKind, encodeFrame, type FrameChannel } from "../ipc/pty";
import {
  THREAD_CHANGES,
  afterAllow,
  afterDeny,
  chatTurn,
  fixTurn,
  planText,
  planTurn,
  type Step,
} from "./agent";
import { DemoChecks, fenceDefanged, untrustedLabel } from "./ci";
import { DemoFiles, NO_GIT, type CheckoutSeed } from "./files";
import { DemoNotes } from "./notes";
import { DemoPlans, RUNS_IN_THE_APP } from "./plan";
import { DemoLanding } from "./landing";
import { DemoReview } from "./review";
import { DemoTasks } from "./tasks";
import { DemoShell, agentScreen, wrapWords, type ShellPlace } from "./shell";
import {
  STORY,
  askScreen,
  branchOf,
  engineLabel,
  previewOf,
  toolOf,
  worktree,
  worktreeIn,
  type StoryCommit,
  type StoryWorktree,
} from "./story";
import { MAIN, changedIn, diffsOf, fileDiff, lineCount, testRun, testsIn } from "./repo";
import { version } from "../../package.json";

type Args = Record<string, unknown> | undefined;

/** The sample repository's workspace and tab (`story.json`, D-2609). */
const WS = "ws_demo";
const TAB = "tb_demo";

/** Messages a visit can send, and their length (the page's limits). */
export const DEMO_MESSAGE_LIMIT = 12;
export const DEMO_MESSAGE_MAX = 500;
const LIMIT_REACHED = "Demo limit reached. Download Vibezeo for Linux to keep going, or reset the demo.";
const START = Date.now() - 42 * 60_000;

const BOOT: AppBoot = {
  // The release the site offers (OPERATING §6): the demo shows the download's version.
  version,
  build: "demo",
  platform: "linux",
  dev: false,
  firstRun: false,
  // Code mode first, with agents mid-task and one waiting; the thread with
  // its first message ready is one switch away (DESIGN §5.2.1).
  lastMode: "code",
  flags: [],
  dataDirDisplay: "~/.vibezeo",
};

/** Early access, as the released download runs it after "Start using
 * Vibezeo": no account, so no avatar email and no credits (D-810). */
const AUTH: AuthState = {
  signedIn: false,
  email: null,
  offline: true,
  hosted: false,
  pending: null,
  error: null,
};

/** What the released download runs with no account (D-128): every local
 * feature, unlimited (D-869, D-2900). */
const ENTITLEMENT: EntitlementView = {
  plan: "free",
  status: "none",
  limits: { ...LIMITS.free },
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

function spec(
  id: string,
  label: string,
  vendor: string,
  logo: string,
  chatAdapter: string | null,
): EngineSpec {
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
    instructions: null,
    note: null,
    updates: null,
  };
}

/** The app's four local templates (services::teammate_draft::templates). */
const TEMPLATES: TeammateTemplate[] = [
  {
    id: "reviewer",
    name: "Reviewer",
    brief:
      "You review changes before they land. Read the diff and its tests, check correctness, edge cases and security, and list findings most severe first with the file and line. Never approve something you couldn't follow; say what was unclear.",
  },
  {
    id: "release-notes",
    name: "Release notes writer",
    brief:
      "You draft release notes from merged changes. Group them for users as New, Improved and Fixed, in plain words. Verify every line against the repository. Never publish; hand the draft back for review.",
  },
  {
    id: "researcher",
    name: "Researcher",
    brief:
      "You answer questions about the codebase and its dependencies. Read before you conclude, cite the files and lines you relied on, and say plainly when you're not sure. Never change files unless asked.",
  },
  {
    id: "triage",
    name: "Triage",
    brief:
      "You triage incoming issues and failures. Reproduce what you can, find the likely cause and the owner, and suggest a priority with one line of reasoning. Never close an issue without saying why.",
  },
];

/** Agent mode's teammates in the demo (D-2603): Lead and three made from
 * the templates, each at a different point in its work. */
function demoTeammates(now: number): TeammateSummary[] {
  const row = (
    id: string,
    name: string,
    brief: string,
    engineId: string,
    over: Partial<TeammateSummary>,
    lead = false,
  ): TeammateSummary => ({
    teammate: {
      id,
      name,
      avatarHue: null,
      avatarImage: null,
      brief,
      engineId,
      model: null,
      effort: null,
      preferredAccountId: null,
      home: `~/.vibezeo/teammates/${id}`,
      places: ["/home/you/code/shop"],
      settings: {
        memoryCap: 200,
        learnMode: "ask",
        mayMessage: true,
        maySchedule: false,
        defaultAccess: "edits",
      },
      isLead: lead,
      pinned: lead,
      sortKey: id,
      createdAt: now - 86_400_000 * 3,
    },
    chatCapable: true,
    working: 0,
    waiting: false,
    failed: false,
    lastLine: null,
    lastAt: null,
    chats: 0,
    ...over,
  });
  return [
    row(
      "tm_lead",
      "Lead",
      "You coordinate the other teammates and keep the plan.",
      "claude",
      {
        lastLine: "Split the checkout fix between Reviewer and Triage",
        lastAt: now - 4 * 60_000,
      },
      true,
    ),
    row("tm_reviewer", "Reviewer", TEMPLATES[0]?.brief ?? "", "codex", {
      working: 1,
      lastLine: "Reading the diff in src/checkout/redirect.ts",
      lastAt: now - 60_000,
    }),
    row("tm_release", "Release notes writer", TEMPLATES[1]?.brief ?? "", "claude", {
      waiting: true,
      lastLine: "Needs a yes on the 0.9.0 notes",
      lastAt: now - 12 * 60_000,
    }),
    row("tm_research", "Researcher", TEMPLATES[2]?.brief ?? "", "gemini", {
      lastLine: "Mapped every caller of nextStep()",
      lastAt: now - 26 * 3_600_000,
    }),
  ];
}

/** The engines the demo has "installed": their names and sample versions
 * come from the story (D-2609), the rest from the registry. */
const ENGINES: EngineView[] = (
  [
    ["claude", "Anthropic", "C", "claude"],
    ["codex", "OpenAI", "X", "codex"],
    ["gemini", "Google", "G", null],
  ] satisfies Array<[string, string, string, string | null]>
).map(([id, vendor, logo, adapter]) => ({
  spec: spec(id, engineLabel(id), vendor, logo, adapter),
  install: { path: `/usr/local/bin/${id}`, version: STORY.engines[id]?.version ?? null, source: "shell" },
  chatCapable: adapter !== null,
  unsupportedReason: null,
  extraArgs: [],
  added: false,
}));

const MODELS: Record<string, ModelEntry[]> = {
  claude: [
    {
      id: "default",
      label: "Default",
      description: "The engine's own choice",
      isDefault: true,
      effortLevels: [],
      contextWindow: 200_000,
      source: "bundled",
    },
    {
      id: "sonnet",
      label: "Sonnet",
      description: "Fast and capable",
      isDefault: false,
      effortLevels: [],
      contextWindow: 200_000,
      source: "bundled",
    },
    {
      id: "opus",
      label: "Opus",
      description: "The most capable",
      isDefault: false,
      effortLevels: [],
      contextWindow: 200_000,
      source: "bundled",
    },
  ],
  codex: [
    {
      id: "default",
      label: "Default",
      description: "The engine's own choice",
      isDefault: true,
      effortLevels: ["low", "medium", "high"],
      contextWindow: 272_000,
      source: "bundled",
    },
  ],
};

/** A worktree's workspace and tab ids. */
const wsOf = (slug: string) => `ws_${slug.replace(/-/g, "_")}`;
const tabOf = (slug: string) => `tb_${slug.replace(/-/g, "_")}`;
const ROOT = `/home/you/${STORY.repo}`;
/** Where the core puts a worktree: `<repo>/.tandem/worktrees/<slug>`. */
const rootOf = (slug: string) => `${ROOT}/.tandem/worktrees/${slug}`;

/** Each worktree has a tab of its own, as `lanes::create` makes it. */
const TABS: Tab[] = [
  { id: TAB, workspaceId: WS, name: "Tab 1", sortKey: "a0", createdAt: START },
  ...STORY.worktrees.map((w): Tab => ({
    id: tabOf(w.slug),
    workspaceId: wsOf(w.slug),
    name: "Tab 1",
    sortKey: "a0",
    createdAt: START,
  })),
];

/** Each worktree's agent pane, named for the engine (one engine a worktree). */
const AGENT_PANE: Record<string, string> = { claude: "pn_claude", codex: "pn_codex", gemini: "pn_gemini" };
const agentPane = (w: StoryWorktree) => AGENT_PANE[w.engine] ?? `pn_${w.engine}`;
const WORKING = worktreeIn("working");
const WAITING = worktreeIn("waiting");

/** A pane as the core keeps it; an agent pane has its first name (F-CD-12). */
function pane(
  id: string,
  kind: string,
  engineId: string | null,
  slug: string,
  humanName: string | null = null,
): PaneRecord {
  return {
    id,
    tabId: tabOf(slug),
    workspaceId: wsOf(slug),
    kind,
    cwd: rootOf(slug),
    engineId,
    accountId: null,
    humanName,
    title: null,
    restoreState: "live",
    browserUrl: null,
    createdAt: START,
  };
}

/** The story's three agents, each in its own worktree, and a shell beside
 * the working one (D-1899: worktree agents never share a grid). */
const PANES: PaneRecord[] = [
  ...STORY.worktrees.map((w) => pane(agentPane(w), "agent", w.engine, w.slug, w.agentName)),
  pane("pn_shell", "terminal", null, WORKING.slug),
];

function workspace(
  id: string,
  name: string,
  root: string,
  branch: string,
  sortKey: string,
  lane: { parentId: string; base: string } | null = null,
): WorkspaceSummary {
  return {
    id,
    kind: "local",
    hostId: "local",
    root,
    wslDistro: null,
    name,
    parentId: lane?.parentId ?? null,
    laneBranch: lane ? branch : null,
    laneBase: lane?.base ?? null,
    sortKey,
    pinned: false,
    collapsed: false,
    sessionsCollapsed: false,
    createdAt: START,
    missing: false,
    branch,
    liveCount: PANES.filter((p) => p.workspaceId === id).length,
    tabs: TABS.filter((t) => t.workspaceId === id),
  };
}

/** One repository and its worktrees, nothing else (D-1899). */
const WORKSPACE = workspace(WS, STORY.repo, ROOT, STORY.base, "a0");
const WORKSPACES: WorkspaceSummary[] = [
  WORKSPACE,
  ...STORY.worktrees.map((w, i) =>
    workspace(wsOf(w.slug), w.slug, rootOf(w.slug), branchOf(w.slug), `a${i + 1}`, {
      parentId: WS,
      base: STORY.base,
    }),
  ),
];

/** Each checkout of the sample repository (repo.ts): `shop` on main, and
 * each worktree on its branch. A finished agent's changes are committed; a
 * working or waiting agent's aren't yet. */
const CHECKOUTS: CheckoutSeed[] = [
  { id: WS, root: ROOT, branch: STORY.base, committed: {}, uncommitted: {} },
  ...STORY.worktrees.map((w): CheckoutSeed => ({
    id: wsOf(w.slug),
    root: rootOf(w.slug),
    branch: branchOf(w.slug),
    committed: w.state === "done" ? changedIn(w.slug) : {},
    uncommitted: w.state === "done" ? {} : changedIn(w.slug),
  })),
];

/** The worktree Code mode opens on: the working agent's, for the motion. */
export const DEMO_ACTIVE = wsOf(WORKING.slug);

/** Every worktree's workspace, in the rail's order. */
export const DEMO_WORKTREES = STORY.worktrees.map((w) => wsOf(w.slug));

/** The agent waiting on the visitor when the demo opens, for the coach's
 * first hint (DESIGN §5.2.2). */
export const DEMO_WAITING = {
  workspaceId: wsOf(WAITING.slug),
  branch: branchOf(WAITING.slug),
  /** Its repository's rail row, which holds it. */
  repoRoot: ROOT,
  paneId: agentPane(WAITING),
  engine: engineLabel(WAITING.engine),
  /** Its agent's first name, as the coach calls it (F-CD-12). */
  name: WAITING.agentName,
};

function layoutOf(workspaceId: unknown): TabLayout[] {
  const tab = TABS.find((t) => t.workspaceId === workspaceId);
  if (!tab) return [];
  const panes = PANES.filter((p) => p.tabId === tab.id);
  const [first, second] = panes;
  const tree: LayoutNode | null = !first
    ? null
    : second
      ? {
          type: "split",
          dir: "row",
          ratio: 0.6,
          a: { type: "leaf", paneId: first.id },
          b: { type: "leaf", paneId: second.id },
        }
      : { type: "leaf", paneId: first.id };
  return [{ tab, tree, activePaneId: first?.id ?? null, maximizedPaneId: null }];
}

/** "Send failure to agent" as the agent's input shows it: the prompt's
 * first line, and what follows it summed up (the log is long). */
function ciEcho(pr: number): string {
  const log = STORY.pr.checks.find((c) => c.bucket === "fail")?.log?.length ?? 0;
  return `\n> A CI check failed on this worktree's pull request #${pr}. Find the cause, fix it, commit, and say what you changed.\n\x1b[2m  (Then the check's name, its link and its log's last ${log} lines, behind an untrusted label.)\x1b[0m\n\n`;
}

/** What a story step looks like in each engine's terminal. */
const STYLE: Record<string, { mark: string; read: string; edit: string; run: string }> = {
  claude: { mark: "●", read: "Read", edit: "Update", run: "Bash" },
  codex: { mark: "•", read: "Reading", edit: "Editing", run: "Running" },
  gemini: { mark: "✦", read: "Read", edit: "Edit", run: "Shell" },
};

/** An agent's still frame from its story: what it read, edited and ran,
 * then where it is now (done, working, or asking). */
function screenOf(w: StoryWorktree): string {
  const st = STYLE[w.engine] ?? { mark: "•", read: "Read", edit: "Edit", run: "Run" };
  const lines: string[] = [];
  for (const step of w.steps) {
    if ("read" in step) lines.push(`${st.mark} ${st.read} ${step.read}`);
    else if ("edit" in step) lines.push(`${st.mark} ${st.edit} ${step.edit} (+${step.add} -${step.del})`);
    else {
      lines.push(`${st.mark} ${st.run} ${step.run}`);
      if (step.passed !== undefined) lines.push(`  ✓ ${step.passed} tests passed`);
    }
  }
  lines.push("");
  if (w.state === "done") lines.push(w.closing ?? "Done.");
  else if (w.state === "waiting") lines.push(...askScreen(w));
  else lines.push("Working…");
  return agentScreen(engineLabel(w.engine), w.task, lines);
}

const AGENT_SCREENS: Record<string, string> = Object.fromEntries(
  STORY.worktrees.map((w) => [agentPane(w), screenOf(w)]),
);

/** Each agent pane's status record as the app keeps it: no preview (none
 * of the app's sources sets one for a pane), and the tool Claude Code's
 * hooks name while it works (D-2624). */
const STATUS: Record<string, { status: StatusRecord["status"]; title: string; tool: string | null }> =
  Object.fromEntries(
    STORY.worktrees.map((w) => [agentPane(w), { status: w.state, title: w.task, tool: previewOf(w) }]),
  );

type Pty = {
  channel: FrameChannel;
  seq: number;
  screen: string;
  shell: DemoShell | null;
  /** Its terminal's width, once the pane has said (pty_resize). */
  cols?: number;
  /** An agent's text before it's wrapped, which its terminal UI draws again
   * at each new width. */
  logical?: string;
};

type DemoThread = {
  thread: Thread;
  turns: Turn[];
  events: ThreadEventEnvelope[];
  channels: Set<{ onmessage: (e: ThreadEventEnvelope) => void }>;
  running: string | null;
  /** Steps waiting on the visitor's answer. */
  awaiting: { approvalId: string; turnId: string } | null;
  /** The turn that makes the demo's fix, and whether its edits are in. */
  fixTurn: string | null;
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

const DONE = worktreeIn("done");
/** The worktree with the pull request (F-WT-08). */
const PR_LANE = wsOf(STORY.pr.worktree);

/** The finished agent's work, as the review queue holds it (F-DB-04). */
const REVIEW_SEED = {
  ws: { id: wsOf(DONE.slug), name: DONE.slug },
  repo: { root: ROOT, check: STORY.check },
  branch: branchOf(DONE.slug),
  paneId: agentPane(DONE),
  // The core titles an item with its pane's first name (review_queue.rs).
  title: DONE.agentName,
  engine: DONE.engine,
  closing: DONE.closing ?? "",
  files: diffsOf(DONE.slug),
  checkOutput: testRun({ ...MAIN, ...changedIn(DONE.slug) }).join("\n"),
};

/** The simulated core. One per page. */
export class DemoCore {
  private ptys = new Map<string, Pty>();
  private threads = new Map<string, DemoThread>();
  /** The rail starts a little wider than its default (a width anyone can
   * drag to), so each worktree's name shows in full beside its status. */
  private settings: Record<string, unknown> = { theme: "dark", rail_width: 280 };
  /** The sample repository's checkouts, for Files, git and the shell (in memory). */
  readonly files = new DemoFiles(CHECKOUTS);
  readonly notes = new DemoNotes();
  /** The Tasks tab, kept in the page (F-TK-03). */
  readonly tasks = new DemoTasks();
  /** Plan with Lead (F-TK-06), simulated. */
  readonly plans = new DemoPlans(this.tasks, (ms, fn) => this.later(ms, fn));
  /** The review queue (F-DB-04): the finished agent's work in its worktree. */
  private readonly review = new DemoReview(REVIEW_SEED);
  /** Each agent pane's state; the waiting one can finish in the page. */
  private status: Record<string, (typeof STATUS)[string]> = structuredClone(STATUS);
  private since: Record<string, number> = {};
  /** Worktrees whose agent has committed its work: the finished ones, then
   * any that finish here. */
  private committed = new Set(STORY.worktrees.filter((w) => w.state === "done").map((w) => w.slug));
  /** Commits made here, by worktree, newest first (the CI fix), and how
   * many of them are pushed. */
  private madeHere: Record<string, StoryCommit[]> = {};
  private pushedHere: Record<string, number> = {};
  /** Worktrees landed in main, and main's commits, newest first. */
  private landed = new Set<string>();
  private mainLog: StoryCommit[] = [...STORY.history];
  /** main as each landing found it, by its commit, for Undo landing. */
  private beforeLanding = new Map<
    string,
    { head: Map<string, string>; log: StoryCommit[]; landed: Set<string> }
  >();
  /** The main each worktree last had merged into it, at a landing. */
  private mergedMain = new Map<string, Record<string, string>>();
  /** Land worktrees (F-WT-07), on this repository's checkouts. */
  private readonly landing = new DemoLanding(
    {
      repoId: WS,
      repoName: STORY.repo,
      base: STORY.base,
      head: () => this.mainLog[0]?.sha ?? "",
      dirty: (id) => this.files.dirty(id),
      lanes: () =>
        STORY.worktrees
          .filter((w) => this.committed.has(w.slug) && !this.landed.has(w.slug))
          .map((w) => ({ id: wsOf(w.slug), branch: branchOf(w.slug), ahead: this.commitsOf(w.slug).length })),
      check: (id, command) => this.check(id, command),
      land: (id) => this.land(id),
      undo: (before) => this.undoLanding(before),
    },
    (ms, fn) => this.later(ms, fn),
    // Used here before, by a landing since undone: Undo keeps the check.
    STORY.check,
  );
  /** A worktree's checks changed: the page's rail reads them again, as
   * the app's polling would within two minutes (main.tsx). */
  onChecks: (laneId: string) => void = () => undefined;
  /** The pull request's checks and review comments (F-WT-08). */
  private readonly ci = new DemoChecks(
    PR_LANE,
    (ms, fn) => this.later(ms, fn),
    (id) => this.onChecks(id),
  );
  /** Whether checkout-fix's agent has fixed what CI found. */
  private ciFixed = false;
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
    this.review.dispose();
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
      teammateId: null,
      conversationRef: null,
    };
    const t: DemoThread = {
      thread,
      turns: [],
      events: [],
      channels: new Set(),
      running: null,
      awaiting: null,
      fixTurn: null,
      fixed: false,
      divider: null,
      draft: "{}",
      plans: [],
    };
    this.threads.set(thread.id, t);
    return t;
  }

  /** Two finished threads to look at, whose work is in main's history,
   * and an empty one to try. */
  private seed(): void {
    const tool = (
      id: string,
      verb: Extract<ThreadEvent, { type: "tool" }>["verb"],
      argSummary: string,
      extra: Partial<Extract<ThreadEvent, { type: "tool" }>> = {},
    ): ThreadEvent => ({
      type: "tool",
      id,
      verb,
      argSummary,
      status: "ok",
      output: null,
      outputTruncated: false,
      outputFile: null,
      diff: null,
      detail: null,
      ...extra,
    });
    const before = changedIn("before");
    const diff = (path: string) => fileDiff(path, before[path] ?? null, MAIN[path] ?? "").patch;

    const CART = "src/cart/cart.ts";
    const CART_TEST = "src/cart/cart.test.ts";
    const done = this.newThread("Show the cart's total before checkout", "codex", WS, START);
    const turn = this.startTurn(
      done,
      "The cart page should show the total before checkout. Add it to the cart, with tests.",
    );
    const run = testRun({ [CART_TEST]: MAIN[CART_TEST] ?? "" }, "src/cart/cart");
    const steps: ThreadEvent[] = [
      {
        type: "reasoning",
        text: "The cart has its items and prices but nothing adds them up. A subtotal beside the Cart type can serve the cart page and checkout both.",
        durationMs: 2400,
      },
      tool("s1", "read", CART, { output: `(${lineCount(before[CART] ?? "")} lines)` }),
      // Codex reports each file change as an edit, with its diff and file.
      tool("s2", "edit", CART, { diff: diff(CART), paths: [`${ROOT}/${CART}`] }),
      tool("s3", "edit", CART_TEST, { diff: diff(CART_TEST), paths: [`${ROOT}/${CART_TEST}`] }),
      tool("s4", "bash", "pnpm test src/cart/cart", { output: run.join("\n") }),
      {
        type: "assistant",
        markdown: `Added \`subtotal\` to \`${CART}\`: the items' total in cents, which the cart page shows before checkout. ${testsIn(MAIN[CART_TEST] ?? "")} tests in \`${CART_TEST}\` cover it.`,
      },
      {
        type: "usage",
        input: 22_310,
        output: 1_840,
        cacheRead: 0,
        cacheWrite: 0,
        contextFillPct: 11.4,
        costUsd: null,
      },
    ];
    for (const e of steps) this.push(done, turn, e);
    this.endTurn(done, turn, "completed");
    done.thread.state = "done";
    done.thread.updatedAt = START + 20 * 60_000;

    // A thread moved from Claude Code to Codex (F-TH-16).
    const QUERY = "src/search/query.ts";
    const slow = (before[QUERY] ?? "").split("\n").findIndex((l) => l.includes("db.all")) + 1;
    const moved = this.newThread("Speed up the product search", "claude", WS, START + 30_000);
    const first = this.startTurn(moved, "Product search takes two seconds. Find the slow part.");
    this.push(
      moved,
      first,
      tool("m1", "read", QUERY, { output: `(${lineCount(before[QUERY] ?? "")} lines)` }),
    );
    this.push(moved, first, {
      type: "assistant",
      markdown: `The search loads every product and filters them in memory (\`${QUERY}\`, line ${slow}). Asking the database for names that start with the text would let it use its index on \`name\`.`,
    });
    this.endTurn(moved, first, "completed");
    moved.thread.engineId = "codex";
    moved.divider = "Continued on Codex (Default) with a summary";
    const second = this.startTurn(moved, "Make that change and measure it.");
    this.push(moved, second, tool("m2", "edit", QUERY, { diff: diff(QUERY) }));
    this.push(moved, second, {
      type: "assistant",
      markdown:
        "Search now asks the database for names that start with the text, so it uses the index on `name`. The same query takes **40 ms** instead of two seconds on the sample data.",
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
      // The fix's edits are in the folder by the time it asks to run the tests.
      if (t.fixTurn === turnId) this.applyFix(t);
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

  private record(paneId: string): StatusRecord {
    const s = this.status[paneId];
    const at = this.since[paneId] ?? START;
    return {
      key: `pane:${paneId}`,
      status: s?.status ?? "idle",
      reason: null,
      since: at,
      ts: at,
      tool: s?.tool ?? null,
      title: s?.title ?? null,
      preview: null,
    };
  }

  /** Every file in a checkout now, by path. */
  private filesOf(id: string): Record<string, string> {
    return Object.fromEntries(this.files.paths(id).map((p) => [p, this.files.text(id, p) ?? ""]));
  }

  /** A landing's check in a worktree, with main merged in: the demo runs
   * `pnpm test` (with or without a filter) and nothing else. */
  private check(id: string, command: string): { output: string } | { refused: string } {
    const run = /^(?:pnpm|npm) (?:run )?test(?: (\S+))?$/.exec(command.trim());
    if (!run) {
      return {
        refused: `The demo can run only ${STORY.check} as a check. In Vibezeo the check runs in each worktree's folder, whatever it is.`,
      };
    }
    return {
      output: testRun({ ...Object.fromEntries(this.files.head(WS)), ...this.filesOf(id) }, run[1] ?? "").join(
        "\n",
      ),
    };
  }

  /** What a worktree's diff is against: where its branch and main meet, as
   * git's merge base. A landed worktree is in main, so only what it hasn't
   * committed shows; one that main was merged into, at a landing since
   * undone, meets main there; the rest left main as the story began. */
  private baseOf(id: string): Record<string, string> {
    const w = STORY.worktrees.find((x) => wsOf(x.slug) === id);
    if (w && this.landed.has(w.slug)) return Object.fromEntries(this.files.head(id));
    return this.mergedMain.get(id) ?? MAIN;
  }

  /** Lands a worktree: main is merged into it, then it into main, as its
   * merge commit. The worktrees keep main merged into them. */
  private land(id: string): string {
    const w = STORY.worktrees.find((x) => wsOf(x.slug) === id);
    if (!w) throw { code: "not_found", message: "That worktree isn't in this repository" };
    const head = this.mainLog[0]?.sha ?? "";
    if (!this.beforeLanding.has(head))
      this.beforeLanding.set(head, {
        head: this.files.head(WS),
        log: [...this.mainLog],
        landed: new Set(this.landed),
      });
    this.mergedMain.set(id, Object.fromEntries(this.files.head(WS)));
    const merged = new Map([...this.files.head(WS), ...Object.entries(this.files.committedChanges(id))]);
    this.files.setHead(id, merged);
    this.files.setHead(WS, merged);
    this.mainLog = [
      { sha: w.mergeCommit, subject: `Merge branch '${branchOf(w.slug)}'` },
      ...this.commitsOf(w.slug),
      ...this.mainLog,
    ];
    this.landed.add(w.slug);
    return w.mergeCommit;
  }

  /** A worktree's commits, newest first: its story's, then any made here. */
  private commitsOf(slug: string): StoryCommit[] {
    return [...(this.madeHere[slug] ?? []), ...(STORY.worktrees.find((w) => w.slug === slug)?.commits ?? [])];
  }

  /** Send failure to agent in the pull request's worktree (F-WT-08): its
   * agent reads the failing test file, fixes it, runs every test and
   * commits. The commit waits for a push; the checks run again after it. */
  private fixCi(pty: Pty, paneId: string): void {
    const w = worktree(STORY.pr.worktree);
    const { fix, number } = STORY.pr;
    const st = STYLE[w.engine] ?? { mark: "•", read: "Read", edit: "Edit", run: "Run" };
    const id = wsOf(w.slug);
    this.ciFixed = true;
    this.say(pty, ciEcho(number));
    this.setStatus(paneId, "working");
    let at = 0;
    for (const step of fix.steps) {
      if ("read" in step) {
        at += 700;
        this.later(at, () => {
          this.say(pty, `${st.mark} ${st.read} ${step.read}\n`);
          this.setStatus(paneId, "working", toolOf(step));
        });
      } else if ("edit" in step) {
        at += 900;
        this.later(at, () => {
          this.say(pty, `${st.mark} ${st.edit} ${step.edit} (+${step.add} -${step.del})\n`);
          this.setStatus(paneId, "working", toolOf(step));
          this.files.edit(id, step.edit, changedIn("ci-fix")[step.edit] ?? "");
        });
      } else {
        at += 700;
        this.later(at, () => {
          this.say(pty, `${st.mark} ${st.run} ${step.run}\n`);
          this.setStatus(paneId, "working", toolOf(step));
        });
        at += 1400;
        this.later(at, () => {
          const passed = /Tests {2}(\d+) passed/.exec(testRun(this.filesOf(id)).at(-1) ?? "")?.[1] ?? "0";
          this.say(pty, `  ✓ ${passed} tests passed\n`);
        });
      }
    }
    this.later(at + 600, () => {
      this.say(
        pty,
        `${st.mark} ${st.run} git commit\n  [${branchOf(w.slug)} ${fix.commit.sha}] ${fix.commit.subject}\n\n${fix.closing}\n`,
      );
      this.files.commit(id);
      this.madeHere[w.slug] = [fix.commit, ...(this.madeHere[w.slug] ?? [])];
      // The review queue's item is the worktree's diff: it has the fix now.
      this.review.update(
        Object.entries(this.files.committedChanges(id))
          .sort(([a], [b]) => a.localeCompare(b))
          .map(([path, text]) => fileDiff(path, MAIN[path] ?? null, text)),
        testRun(this.filesOf(id)).join("\n"),
      );
      this.setStatus(paneId, "done");
    });
  }

  private undoLanding(before: string): void {
    const was = this.beforeLanding.get(before);
    if (!was) return;
    this.files.setHead(WS, was.head);
    this.mainLog = was.log;
    this.landed = was.landed;
    this.beforeLanding.clear();
  }

  /** The waiting agent's question, answered in its terminal: yes runs its
   * tests and commits its work, which can then land; no leaves it as is. */
  private answerWaiting(pty: Pty, yes: boolean): void {
    const w = WAITING;
    const paneId = agentPane(w);
    const st = STYLE[w.engine] ?? { mark: "•", run: "Run" };
    if (!yes) {
      this.say(
        pty,
        `\n${st.mark} Okay, I won't run ${w.ask ?? "them"}. The change is in place for you to try.\n`,
      );
      this.setStatus(paneId, "done");
      return;
    }
    // It runs the command: working until it's done.
    this.setStatus(paneId, "working");
    const id = wsOf(w.slug);
    const filter = (w.ask ?? "").replace(/^pnpm test ?/, "");
    const run = testRun(this.filesOf(id), filter);
    const passed = /Tests {2}(\d+) passed/.exec(run.at(-1) ?? "")?.[1] ?? "0";
    const lines = [
      `${st.mark} ${st.run} ${w.ask ?? ""}`,
      `  ✓ ${passed} tests passed`,
      `${st.mark} ${st.run} git commit`,
      ...[...w.commits].reverse().map((c) => `  [${branchOf(w.slug)} ${c.sha}] ${c.subject}`),
      "",
      w.closing ?? "Done.",
    ];
    this.later(600, () => {
      this.say(pty, `\n${lines.join("\n")}\n`);
      this.files.commit(id);
      this.committed.add(w.slug);
      this.setStatus(paneId, "done");
    });
  }

  /** A pane's next status, and the tool its engine's hooks name with it:
   * Claude Code's at work, none otherwise (D-2624). */
  private setStatus(paneId: string, status: StatusRecord["status"], tool: string | null = null): void {
    const s = this.status[paneId];
    if (!s) return;
    this.status[paneId] = { ...s, status, tool };
    this.since[paneId] = Date.now();
    emitLocal("status:changed", { records: [this.record(paneId)], removed: [] });
  }

  /** The radar's indexes (F-WT-10): the worktrees parsed since their agent's
   * last edits. coupon-rules' is from the checkpoint before Codex's turn,
   * so its overlap with checkout-fix shows once Check overlap or a new
   * turn's checkpoint parses it again. */
  private parsed = new Set(STORY.worktrees.filter((w) => w.state !== "working").map((w) => w.slug));
  /** Pairs and symbols already announced (radar.rs WARNED). */
  private warned = new Set<string>();

  private slugOf(id: string): string | undefined {
    return STORY.worktrees.find((w) => wsOf(w.slug) === id)?.slug;
  }

  /** What `a` shares with `b`, from their parsed indexes: both changed the
   * story's symbol. */
  private shared(a: string, b: string): SharedSymbol[] {
    const o = STORY.overlap;
    if (a === b || !o.between.includes(a) || !o.between.includes(b)) return [];
    if (!this.parsed.has(a) || !this.parsed.has(b)) return [];
    return [{ name: o.symbol, qualified: o.symbol, here: "changed", there: "changed" }];
  }

  private radarMarks(laneIds: string[]): RadarMark[] {
    return laneIds.flatMap((id) => {
      const slug = this.slugOf(id);
      if (!slug) return [];
      const symbols = [
        ...new Set(STORY.worktrees.flatMap((w) => this.shared(slug, w.slug).map((x) => x.qualified))),
      ].sort();
      return symbols.length ? [{ laneId: id, symbols }] : [];
    });
  }

  /** UI-153 (F-WT-03, F-WT-10): the check parses this worktree and its
   * siblings again, then reports the files and symbols they share. */
  private overlap(laneId: string): OverlapReport {
    const slug = this.slugOf(laneId);
    if (!slug) throw { code: "not_found", message: "That worktree isn't open in the demo." };
    for (const w of STORY.worktrees) this.parsed.add(w.slug);
    const changed = (s: string) => [
      ...new Set([
        ...Object.keys(this.files.committedChanges(wsOf(s))),
        ...this.files.changes(wsOf(s)).map((f) => f.path),
      ]),
    ];
    const mine = changed(slug);
    const siblings: SiblingOverlap[] = STORY.worktrees
      .filter((w) => w.slug !== slug)
      .flatMap((w) => {
        const files = changed(w.slug)
          .filter((f) => mine.includes(f))
          .sort();
        const symbols = this.shared(slug, w.slug);
        return files.length || symbols.length
          ? [{ laneId: wsOf(w.slug), branch: branchOf(w.slug), files, symbols }]
          : [];
      });
    return {
      files: mine.length,
      siblings,
      forecast: { kind: "clean" },
      uncommitted: this.files.dirty(laneId),
      unparsed: 0,
    };
  }

  /** The note Warn both agents types into `laneId`'s agent (radar.rs `note_text`). */
  private radarNote(laneId: string, otherId: string): string {
    const a = this.slugOf(laneId);
    const b = this.slugOf(otherId);
    if (!a || !b || a === b)
      throw { code: "invalid", message: "Those aren't two worktrees of one repository" };
    const found = this.shared(a, b);
    if (found.length === 0)
      throw { code: "not_found", message: "These worktrees don't share symbols any more" };
    const what = (c: SharedSymbol["here"], to?: string) =>
      c === "renamed" && to
        ? `renamed to ${to}`
        : (
            {
              added: "added",
              changed: "changed",
              removed: "renamed or removed",
              renamed: "renamed or removed",
              used: "used",
            } as const
          )[c];
    const data = [
      `Other worktree: ${branchOf(b)}`,
      ...found.map(
        (s) => `- ${s.qualified}: ${what(s.here, s.renamedTo)} here, ${what(s.there, s.renamedTo)} there`,
      ),
    ].join("\n");
    // radar.rs `note_text`: the symbols behind the label and fence (D-3022).
    return [
      "Heads-up from Vibezeo: another agent, working in a parallel worktree of this repository, is changing code that your changes also touch. Check that your work still fits theirs (a renamed or changed symbol you use, or the same one changed twice), and adjust before you finish.",
      "",
      untrustedLabel("the overlapping symbols"),
      fenceDefanged("text", data),
    ].join("\n");
  }

  /** Claim these files (radar.rs `claim`): the files where this worktree
   * changed what it shares, which in the demo is the story's one file
   * (D-128; the review of 9a4e93ab, S7). */
  private radarClaim(laneId: string, otherId: string): string[] {
    const a = this.slugOf(laneId);
    const b = this.slugOf(otherId);
    if (!a || !b || a === b)
      throw { code: "invalid", message: "Those aren't two worktrees of one repository" };
    const touched = this.shared(a, b).some(
      (s) => s.here === "changed" || s.here === "removed" || s.here === "renamed",
    );
    if (!touched)
      throw { code: "not_found", message: "This worktree didn't change the symbols it shares any more" };
    return [STORY.overlap.file];
  }

  /** A new turn in an agent pane takes a checkpoint (F-EX-01), after which
   * the radar parses its worktree again and announces a pair that newly
   * shares symbols, once (radar.rs `on_checkpoint`). True, so a caller can
   * go on to answer the prompt. */
  private checkpoint(paneId: string): boolean {
    const pane = PANES.find((p) => p.id === paneId);
    const slug = pane ? this.slugOf(pane.workspaceId) : undefined;
    if (!slug) return true;
    this.parsed.add(slug);
    for (const w of STORY.worktrees) {
      const found = this.shared(slug, w.slug);
      if (found.length === 0) continue;
      const names = found.map((x) => x.qualified);
      const key = [[slug, w.slug].sort().join("|"), names.join(",")].join("|");
      if (this.warned.has(key)) continue;
      this.warned.add(key);
      emitLocal("radar:overlap", {
        repoId: WS,
        a: { laneId: wsOf(slug), branch: branchOf(slug) },
        b: { laneId: wsOf(w.slug), branch: branchOf(w.slug) },
        symbols: names,
      });
    }
    return true;
  }

  /** Keys at the waiting agent's question: Enter allows and Esc alone says
   * no, as the coach says (D-2633). Any other key does nothing here, an
   * escape sequence among them (an arrow, Alt and a key). */
  private answer(pty: Pty, data: string): void {
    if (data === "\x1b") return this.answerWaiting(pty, false);
    if (data.startsWith("\x1b")) return;
    if (data.includes("\r")) this.answerWaiting(pty, true);
  }

  /** The thread's fix lands in its workspace's folder: `shop/thread/`'s
   * files, uncommitted, as an agent leaves them. */
  private applyFix(t: DemoThread): void {
    if (t.fixed) return;
    t.fixed = true;
    for (const [path, text] of Object.entries(changedIn("thread")))
      this.files.edit(t.thread.workspaceId ?? WS, path, text);
  }

  /** The checkout a Files or git request is about: its workspace's, or
   * its thread's workspace's. */
  private checkout(args: Args): string {
    const c = args?.context as FileContext | undefined;
    if (c?.type === "thread") return this.threads.get(c.threadId)?.thread.workspaceId ?? "scratch";
    return c?.type === "workspace" ? c.workspaceId : WS;
  }

  /** The repository a landing command is about: the one asked, or a
   * worktree's (every worktree's ⋯ opens its repository's landing). */
  private repoOf(args: Args): string {
    const id = String(args?.workspaceId ?? "");
    const ws = WORKSPACES.find((w) => w.id === id);
    if (!ws) throw { code: "not_found", message: "That folder isn't open in the demo." };
    return ws.parentId ?? ws.id;
  }

  /** What a shell in `workspaceId`'s folder can see of its checkout. */
  private place(workspaceId: string): ShellPlace {
    const ws = WORKSPACES.find((w) => w.id === workspaceId) ?? WORKSPACE;
    const story = STORY.worktrees.find((w) => wsOf(w.slug) === ws.id);
    const files = () =>
      Object.fromEntries(this.files.paths(ws.id).map((p) => [p, this.files.text(ws.id, p) ?? ""]));
    return {
      dir: ws.root.replace(/^\/home\/you/, "~"),
      path: ws.root,
      branch: ws.branch ?? STORY.base,
      paths: () => this.files.paths(ws.id),
      text: (path) => this.files.text(ws.id, path),
      changes: () => this.files.changes(ws.id),
      diff: (path) => this.files.gitDiff(ws.id, path).patch,
      log: () => [
        ...(story && this.committed.has(story.slug) && !this.landed.has(story.slug)
          ? this.commitsOf(story.slug)
          : []),
        ...this.mainLog,
      ],
      test: (filter) => testRun(files(), filter),
    };
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
    if (text.length > DEMO_MESSAGE_MAX)
      throw { code: "invalid", message: `Demo messages are up to ${DEMO_MESSAGE_MAX} characters` };
    if (this.sent >= DEMO_MESSAGE_LIMIT) {
      this.notify();
      throw { code: "limit_reached", message: LIMIT_REACHED };
    }
    this.sent += 1;
    this.notify();
    const turnId = this.startTurn(t, text);
    this.changed(t);
    const first =
      !t.fixed && t.thread.workspaceId !== null && t.turns.length === 1 && t.thread.mode === "act";
    if (t.thread.mode === "plan") {
      const plan = planText(text);
      this.play(t, turnId, planTurn(plan), () => {
        const version = t.plans.length + 1;
        t.plans.push({ version, turnId, text: plan, createdAt: Date.now() });
        this.push(t, turnId, { type: "plan_revision", version });
      });
    } else {
      if (first) t.fixTurn = turnId;
      this.play(t, turnId, first ? fixTurn(this.id("apr"), Date.now() + 10 * 60_000, ROOT) : chatTurn(text));
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

  /** A pane's terminal: a shell's once its pane spawned it, an agent's from
   * the start, since its agent runs whether or not its tab was opened (what
   * it prints is replayed when its pane attaches). */
  private ptyOf(paneId: string): Pty | undefined {
    const pty = this.ptys.get(paneId);
    if (pty || AGENT_SCREENS[paneId] === undefined) return pty;
    const logical = (AGENT_SCREENS[paneId] ?? "").replace(/\r\n/g, "\n");
    const agent: Pty = { channel: { onmessage: () => undefined }, seq: 0, screen: "", shell: null, logical };
    agent.screen = this.rendered(agent);
    this.ptys.set(paneId, agent);
    return agent;
  }

  /** An agent's text as its terminal UI lays it out at its width. */
  private rendered(pty: Pty): string {
    return wrapWords(pty.logical ?? "", pty.cols ?? 80).replace(/\n/g, "\r\n");
  }

  /** An agent's words in its pane, wrapped at its width as an agent's
   * terminal UI wraps them (a shell's output isn't: a terminal cuts it). */
  private say(pty: Pty, text: string): void {
    if (pty.logical !== undefined) pty.logical += text;
    this.frame(pty, wrapWords(text, pty.cols ?? 80).replace(/\n/g, "\r\n"));
  }

  private frame(pty: Pty, text: string): void {
    pty.seq += 1;
    pty.screen += text;
    pty.channel.onmessage(encodeFrame(FrameKind.Data, pty.seq, encoder.encode(text)));
  }

  private dashboard(): DashboardItem[] {
    const panes = PANES.filter((p) => this.status[p.id]).map((p): DashboardItem => {
      const s = this.status[p.id] ?? { status: "idle", title: "", tool: null };
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
        branch: WORKSPACES.find((w) => w.id === p.workspaceId)?.branch ?? null,
        // dashboard.rs: the record's preview, else its tool.
        preview: s.tool,
        pendingApproval: null,
      };
    });
    const threads = [...this.threads.values()].map((t): DashboardItem => {
      const engine = ENGINES.find((e) => e.spec.id === t.thread.engineId);
      const status =
        t.thread.state === "working"
          ? "working"
          : t.thread.state === "waiting"
            ? "waiting"
            : t.thread.state === "done"
              ? "done"
              : "idle";
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
        workspace: WORKSPACES.find((w) => w.id === t.thread.workspaceId)?.name ?? null,
        branch: WORKSPACES.find((w) => w.id === t.thread.workspaceId)?.branch ?? null,
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
        return {
          stage: "idle",
          current: BOOT.version,
          channel: "stable",
          version: null,
          notes: null,
          percent: null,
          errorStage: null,
          error: null,
          lastCheck: null,
          upToDate: true,
          minVersion: null,
        };
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
        return PANES.filter((p) => this.status[p.id]).map((p) => this.record(p.id));
      case "status_snapshot":
        return this.dashboard();
      case "pty_attach": {
        const pty = this.ptyOf(String(args?.paneId));
        if (!pty) throw { code: "not_found", message: "Not running" };
        pty.channel = args?.channel as FrameChannel;
        const snap: AttachSnapshot = {
          replay: base64(pty.screen),
          end: encoder.encode(pty.screen).length,
          live: true,
          state: null,
        };
        return snap;
      }
      case "pty_spawn": {
        const id = String(args?.paneId);
        // A shell answers from its worktree's checkout; an agent pane shows its still.
        const record = PANES.find((p) => p.id === id);
        const shell = record?.kind === "terminal" ? new DemoShell(this.place(record.workspaceId)) : null;
        const agent = shell ? undefined : this.ptyOf(id);
        if (agent) {
          // Its agent has been running: the pane shows all it printed.
          agent.channel = args?.channel as FrameChannel;
          this.later(80, () => {
            agent.seq += 1;
            agent.channel.onmessage(encodeFrame(FrameKind.Data, agent.seq, encoder.encode(agent.screen)));
          });
          return { pid: 4000 + Object.keys(AGENT_SCREENS).indexOf(id), startedAt: START };
        }
        const pty: Pty = { channel: args?.channel as FrameChannel, seq: 0, screen: "", shell };
        this.ptys.set(id, pty);
        this.later(80, () => this.frame(pty, shell ? shell.greeting() : ""));
        return { pid: 4000 + this.ptys.size, startedAt: Date.now() };
      }
      case "pty_write": {
        const paneId = String(args?.paneId);
        const pty = this.ptyOf(paneId);
        const data = String(args?.data ?? "");
        if (pty?.shell) this.frame(pty, pty.shell.write(data));
        else if (pty && paneId === agentPane(WAITING) && this.status[paneId]?.status === "waiting")
          this.answer(pty, data);
        else if (pty && data.includes("\r") && this.checkpoint(paneId))
          this.say(
            pty,
            "\n\x1b[2m(This agent is simulated. In Vibezeo you'd be talking to it here.)\x1b[0m\n",
          );
        return null;
      }
      case "pty_resize": {
        const pty = this.ptyOf(String(args?.paneId));
        if (!pty || typeof args?.cols !== "number" || args.cols === pty.cols) return null;
        pty.cols = args.cols;
        // An agent's terminal UI draws itself again at the new width.
        if (pty.logical !== undefined) {
          pty.screen = this.rendered(pty);
          pty.seq += 1;
          pty.channel.onmessage(
            encodeFrame(FrameKind.Data, pty.seq, encoder.encode(`\x1b[2J\x1b[3J\x1b[H${pty.screen}`)),
          );
        }
        return null;
      }
      case "thread_list":
        // Teammates' chats aren't simulated: their pages open on an empty chat.
        if (args?.kind === "teammate_chat") return [];
        return [...this.threads.values()].map((t) => t.thread);
      case "teammate_list":
        return demoTeammates(Date.now());
      case "teammate_templates":
        return TEMPLATES;
      case "teammate_suggestions":
        return ["Review the open diff", "What changed since yesterday?", "Write the release notes"];
      case "teammate_create":
      case "teammate_draft_with_ai":
        throw {
          code: "unavailable",
          message: "The browser demo doesn't make teammates. In the app you'd create one here.",
        };
      case "thread_get":
        return this.detail(this.thread(args));
      case "thread_mark_read":
        this.thread(args).thread.unread = false;
        return null;
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
          decision: allowed
            ? { type: "allow", scope: decision.scope === "thread" ? "thread" : "once" }
            : { type: "deny", reason: null },
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
        const changes: ThreadChanges = {
          source: t.turns.length ? "git" : "not_started",
          files: t.fixed ? THREAD_CHANGES : [],
        };
        return changes;
      }
      case "thread_file_read": {
        const t = this.thread(args);
        const path = String(args?.path ?? "");
        const text: FileText = {
          path,
          content: (t.thread.workspaceId && this.files.text(t.thread.workspaceId, path)) || "",
          binary: false,
          truncated: false,
        };
        return text;
      }
      case "thread_mention_search": {
        const t = this.thread(args);
        const query = String(args?.query ?? "").toLowerCase();
        return (t.thread.workspaceId ? this.files.paths(t.thread.workspaceId) : [])
          .filter((path) => path.toLowerCase().includes(query))
          .slice(0, 8)
          .map((path) => ({ path, kind: "file" }));
      }
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
        // Web links, and the prefilled email "Tell me when accounts open" writes (D-2914).
        if (typeof args?.url === "string" && /^(https:\/\/|mailto:)/.test(args.url))
          window.open(args.url, "_blank", "noopener");
        return null;
      case "fs_list":
        return this.files.list(this.checkout(args), String(args?.dir ?? ""));
      case "fs_read":
        return this.files.read(this.checkout(args), String(args?.path ?? ""));
      case "fs_write":
        return this.files.write(
          this.checkout(args),
          String(args?.path ?? ""),
          String(args?.text ?? ""),
          (args?.expectedHash as string | null | undefined) ?? null,
        );
      case "fs_compare":
        return this.files.compare(this.checkout(args), String(args?.path ?? ""), String(args?.text ?? ""));
      case "fs_create":
      case "fs_rename":
      case "fs_delete":
        throw { code: "unavailable", message: "The browser demo doesn't create, rename or delete files." };
      case "fs_watch":
        return "demo-watch";
      case "git_status": {
        const id = this.checkout(args);
        const panel = this.files.gitStatus(id);
        // The pull request's branch is on the remote: what's committed here
        // since is ahead of it until it's pushed (F-WT-08).
        if (id !== PR_LANE) return panel;
        const slug = STORY.pr.worktree;
        const ahead = (this.madeHere[slug]?.length ?? 0) - (this.pushedHere[slug] ?? 0);
        return { ...panel, status: { ...panel.status, upstream: `origin/${branchOf(slug)}`, ahead } };
      }
      case "git_diff":
        return this.files.gitDiff(this.checkout(args), String(args?.path ?? ""));
      case "git_branches":
        return this.files.gitBranches(this.checkout(args));
      case "git_push": {
        // Only the pull request's branch has a remote here: pushing the
        // fix starts CI on it (F-WT-08).
        if (this.checkout(args) !== PR_LANE) throw { code: "unavailable", message: NO_GIT };
        const slug = STORY.pr.worktree;
        const made = this.madeHere[slug]?.length ?? 0;
        if (made > (this.pushedHere[slug] ?? 0)) {
          this.pushedHere[slug] = made;
          this.ci.pushed();
        }
        return null;
      }
      case "git_stage":
      case "git_unstage":
      case "git_discard":
      case "git_commit":
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
      // Conductor's memory is a file on the user's computer (F-CN-06); the
      // demo has none, and the wake phrase needs the app's microphone (F-CN-07).
      case "conductor_memory_get":
        return { enabled: false, text: "", facts: [], proposals: [], path: "~/.tandem/conductor/memory.md" };
      case "conductor_memory_save":
      case "conductor_memory_clear":
      case "conductor_memory_resolve":
        throw {
          code: "unavailable",
          message: "Conductor's memory lives in a file on your computer, so the demo can't keep one.",
        };
      case "conductor_wake_status":
        return { state: { state: "unavailable", reason: "not_built" }, unavailable: "not_built" };
      // Find work reads real terminals and threads on this computer (F-CN-05).
      case "find_work":
        throw {
          code: "unavailable",
          message:
            "Find work searches the terminals, threads and teammates running on your computer, so the demo has nothing to search.",
        };
      // The review queue (F-DB-04): Claude Code's finished fix, kept in the page.
      // Worktrees as the app shows them before any pull request, overlap
      // or landing (F-WT-04, F-WT-10, F-WT-07; D-2609).
      // A worktree's diff (F-WT-02): what it changed since main as the
      // story starts, uncommitted work included.
      case "lane_diff": {
        const id = String(args?.laneId ?? "");
        const files = this.filesOf(id);
        const base = this.baseOf(id);
        return Object.keys(files)
          .filter((path) => files[path] !== base[path])
          .sort()
          .map((path) => fileDiff(path, base[path] ?? null, files[path] ?? ""));
      }
      // CI results and review comments back to the agent (F-WT-08).
      case "lane_checks":
        return this.ci.get(String(args?.laneId ?? ""));
      case "pr_checks_allow":
        this.ci.allow(Boolean(args?.allow));
        return null;
      case "lane_check_failure":
        return this.ci.failure(String(args?.laneId ?? ""), String(args?.name ?? ""));
      case "lane_review_comments":
        return this.ci.comments(String(args?.laneId ?? ""), (args?.seen as string[] | undefined) ?? []);
      case "merge_queue_plan":
        this.repoOf(args);
        return this.landing.plan();
      case "merge_queue_start":
        this.repoOf(args);
        return this.landing.start(
          (args?.laneIds as string[] | undefined) ?? [],
          (args?.check as string | null | undefined) ?? null,
        );
      case "merge_queue_get":
        this.repoOf(args);
        return this.landing.get();
      case "merge_queue_stop":
        this.repoOf(args);
        this.landing.stop();
        return null;
      case "merge_queue_undo":
        this.repoOf(args);
        this.landing.undo();
        return null;
      case "lane_radar":
        return this.radarMarks((args?.laneIds as string[] | undefined) ?? DEMO_WORKTREES);
      case "lane_overlap":
        return this.overlap(String(args?.laneId ?? ""));
      case "lane_radar_note":
        return this.radarNote(String(args?.laneId ?? ""), String(args?.otherId ?? ""));
      case "lane_radar_claim":
        return this.radarClaim(String(args?.laneId ?? ""), String(args?.otherId ?? ""));
      case "pty_type_prompt": {
        const paneId = String(args?.paneId ?? "");
        const pty = this.ptyOf(paneId);
        const text = String(args?.text ?? "");
        if (pty && !pty.shell && text.startsWith("A CI check failed")) {
          this.checkpoint(paneId);
          if (paneId === agentPane(DONE) && !this.ciFixed) this.fixCi(pty, paneId);
          else {
            // Sent again, the fix is already in and waits for a push.
            const then = this.ciFixed
              ? "it reads the failure again; here its fix is committed and waits for a push"
              : "it reads the failure and gets to work on it";
            this.say(
              pty,
              `${ciEcho(STORY.pr.number)}\x1b[2m(This agent is simulated. In Vibezeo ${then}.)\x1b[0m\n`,
            );
          }
        } else if (pty && !pty.shell) {
          // The radar's note, or a review (F-WT-02): typed in, and simulated from there.
          const then = text.startsWith("Heads-up from Vibezeo")
            ? "it reads the note and fits its work to the other worktree's"
            : "it reads what you sent and gets to work on it";
          this.say(pty, `\n${text}\n\x1b[2m(This agent is simulated. In Vibezeo ${then}.)\x1b[0m\n`);
          this.checkpoint(paneId);
        }
        return null;
      }
      case "review_queue":
        return this.review.view();
      case "review_item_diff":
        return this.review.diff(String(args?.key ?? ""));
      case "review_approve":
        return this.review.approve(String(args?.key ?? ""), String(args?.diffHash ?? ""));
      case "review_unapprove":
        return this.review.unapprove(String(args?.key ?? ""));
      case "review_snooze":
        return this.review.snooze(
          String(args?.key ?? ""),
          typeof args?.minutes === "number" ? args.minutes : null,
        );
      case "review_run_check":
        return this.review.runCheck(String(args?.key ?? ""));
      case "review_repo_get":
        return this.review.repoGet();
      case "review_repo_set":
        return this.review.repoSet(
          typeof args?.check === "string" ? args.check : null,
          Array.isArray(args?.apiPaths) ? (args.apiPaths as string[]) : [],
        );
      // Teammates' engines run on your computer (F-AG-10, F-AG-13).
      case "teammate_launch_in_terminal":
        throw {
          code: "unavailable",
          message: "The demo has no engines to launch. In the app, the teammate opens in a terminal pane.",
        };
      // The demo's folders aren't on a disk (F-EN-07).
      case "instructions_get":
      case "instructions_plan":
      case "instructions_apply":
      case "instructions_undo":
      case "instructions_copy_to_worktrees":
        throw {
          code: "unavailable",
          message:
            "The demo has no repository files to read. In the app, this lists each engine's instructions file.",
        };
      // Engines' versions and processes are on your computer (F-EN-08).
      case "engine_versions":
      case "engine_background":
        return [];
      case "engine_check_update":
      case "engine_pin":
      case "engine_rollback_command":
      case "engine_background_stop":
        throw {
          code: "unavailable",
          message: "The demo has no installed engines to check, pin or roll back.",
        };
      case "skill_sync":
        throw { code: "unavailable", message: "The demo has no engines to install skills for." };
      // ACP agents are commands on your computer (F-EN-10).
      case "acp_agents_list":
        return [];
      case "acp_agent_test":
      case "acp_agent_add":
      case "acp_agent_remove":
        throw {
          code: "unavailable",
          message:
            "The demo can't run commands. In the app, this adds any agent that speaks the Agent Client Protocol.",
        };
      // Model servers run on your computer (F-EN-11).
      case "local_models_list":
        return [];
      case "local_models_add_port":
      case "local_models_remove_port":
      case "local_model_start":
        throw {
          code: "unavailable",
          message:
            "The demo can't reach your computer. In the app, this finds Ollama and LM Studio running on it.",
        };
      // Engines' session files are on your computer (F-TH-17).
      case "library_list":
        return { sessions: [], skipped: 0 };
      case "library_transcript":
      case "library_resume":
      case "library_brief":
      case "library_continue":
      case "library_search":
      case "library_search_set":
        throw {
          code: "unavailable",
          message:
            "The demo has no engine sessions to read. In the app, this lists your Claude Code and Codex sessions.",
        };
      // A web page can't hold other pages the way the app's browser does.
      case "ssh_host_add":
      case "ssh_connect":
      case "ssh_list_dir":
        // The demo runs in a web page: there's no ssh to run.
        throw {
          code: "unavailable",
          message: "The browser demo doesn't connect to SSH hosts. The app does, with your own ssh.",
        };
      case "wsl_distros":
        // Same answer as the app on a system without WSL: the section hides.
        throw { code: "unavailable", message: "WSL folders open on Windows only", stage: "wsl" };
      case "browser_open":
      case "browser_navigate":
        throw {
          code: "unavailable",
          message: "Web pages don't open inside this demo. In the app they open right here.",
        };
      case "editor_tabs_get":
        return this.files.savedTabs(String(args?.contextKey ?? ""));
      case "editor_tabs_save":
        this.files.saveTabs(String(args?.contextKey ?? ""), args?.state as EditorTabs);
        return null;
      // Tasks are a feature of the download, so the demo keeps them in the
      // page. Running one starts an agent in a worktree on your computer,
      // which a web page can't (D-2905).
      case "tasks_meta":
        return this.tasks.meta();
      case "tasks_list":
        return this.tasks.list(
          Boolean((args?.filter as { include_archived?: boolean } | undefined)?.include_archived),
        );
      case "task_get":
        return this.tasks.get(String(args?.id ?? ""));
      case "task_save":
        return this.tasks.save((args?.input ?? {}) as SaveIssue);
      case "task_comment":
        return this.tasks.comment(
          String(args?.id ?? ""),
          String(args?.body ?? ""),
          typeof args?.parentId === "string" ? args.parentId : null,
        );
      case "plan_start":
        return this.plans.start(String(args?.workspaceId ?? ""), String(args?.goal ?? ""));
      case "plan_list":
        return this.plans.list();
      case "plan_create_tasks":
        return this.plans.createTasks(String(args?.id ?? ""), (args?.tasks ?? []) as PlanTask[]);
      case "plan_prepare":
        return this.plans.prepare(String(args?.id ?? ""));
      case "plan_execute":
      case "plan_supervise":
      case "plan_answer":
      case "plan_send_back":
      case "plan_review_send":
        return this.plans.execute();
      case "plan_discard":
        return this.plans.discard(String(args?.id ?? ""));
      case "task_archive":
      case "task_restore":
        return this.tasks.setArchived(
          String(args?.id ?? ""),
          cmd === "task_archive",
          typeof args?.expectedRevision === "number" ? args.expectedRevision : undefined,
        );
      // Dictate and plugins reach past the browser: the microphone and other
      // apps, and the user's own accounts.
      case "dictate_status":
        throw {
          code: "unavailable",
          message:
            "Dictate listens to your microphone and types into your other apps, so it works in the app, not in this demo.",
        };
      case "plugins_catalog":
        throw {
          code: "unavailable",
          message:
            "Plugins sign in to your own accounts, so they don't connect inside this demo. In the app, this page lists them.",
        };
      // Checkpoints, saved prompts and Best of N work on the folders on your
      // computer; the phone pairs over your own network. Each says so here
      // instead of answering with the wrong shape (D-833).
      case "checkpoints_list":
      case "checkpoint_create":
      case "checkpoint_diff":
      case "checkpoint_restore":
        throw {
          code: "unavailable",
          message:
            "Checkpoints save the folders on your computer, so they work in the app, not in this demo.",
        };
      case "checkpoints_usage":
        return { bytes: 0, folders: 0 };
      case "prompts_list":
        return { items: [], skipped: [], excludeHidesRepo: false };
      case "prompts_create":
      case "prompts_render":
        throw {
          code: "unavailable",
          message: "Saved prompts are files in your folders, so they work in the app, not in this demo.",
        };
      case "bestof_start":
        throw {
          code: "unavailable",
          message:
            "Best of N runs agents in git worktrees on your computer, so it works in the app, not in this demo.",
        };
      // Command recall lists what ran in real shells; these panes simulate them.
      case "history_search":
        return [];
      case "phone_arm_state":
      case "phone_arm_approvals":
        return { armedUntil: null };
      // Local-network pairing is off here: the shape the section reads, so it
      // doesn't sit loading, and an honest refusal to turn it on (F-RM-04).
      case "phone_lan_state":
        return { enabled: false, running: false, port: 47800, addresses: [], error: null };
      case "phone_lan_set":
      case "phone_pair_begin":
        throw {
          code: "unavailable",
          message:
            "A phone pairs with the app over your own network, so it works in the app, not in this demo.",
        };
      // Routines, task runs and Night Shift start agents on your computer,
      // free in the download (D-2900); a web page can't, so each says so
      // (D-833, D-2905).
      case "routine_save":
      case "routine_run_now":
      case "routine_preview_schedule":
      case "routines_background_set":
        throw {
          code: "unavailable",
          message: "Routines run your teammates on your computer, so they work in the app, not in this demo.",
        };
      case "task_execute":
      case "night_schedule":
        throw { code: "unavailable", message: RUNS_IN_THE_APP };
      case "night_accounts":
        return [];
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
