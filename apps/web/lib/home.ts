// The home page's content (F-WB-01), moved out of app/page.site.tsx so the
// sections can be generated and tested. Copy here is checked against the
// shipped build; numbers and release notes come from code, never from here.
import type { Release } from "@tandem/protocol/changelog";
import { RELEASES } from "./changelog";
import { DOWNLOADS, RELEASE } from "./release";
import { docsPath } from "./docs/pages";

/** What the released desktop app does (the milestones `lib/progress.ts` lists). */
export const FEATURES = [
  {
    title: "Real terminals, side by side",
    body: "Each agent runs in a real PTY pane over your own folders. Split, maximize and rearrange panes into layouts. Full-screen TUIs, colours and mouse input behave as they do in your usual terminal.",
  },
  {
    title: "Your accounts, untouched",
    body: "Each engine signs in by itself, inside its own provider home on your disk. Vibezeo never reads, copies or proxies an AI vendor's tokens.",
  },
  {
    title: "Live status for every agent",
    body: "Vibezeo knows when an agent is working, waiting for you, done or has failed, from Claude Code's hooks and from each engine's own output. The status shows on every pane and in the sidebar.",
  },
  {
    title: "One Dashboard",
    body: "Every live agent in one place, across all your workspaces: what each one is doing and which ones are waiting for you. One click jumps to the pane that needs you.",
  },
  {
    title: "Thread mode",
    body: "Work with Claude Code, Codex and other agents as a conversation: streamed replies, tool calls, and approvals you answer inline, once or for the whole thread. Plan mode writes numbered plans you can implement exactly, and the Changes panel shows every file a thread touched. Threads run Claude Code, Codex, Cursor Agent, Gemini CLI, Grok, Kiro CLI, OpenCode and any agent you add that speaks ACP, up to six side by side.",
  },
  {
    title: "Switch engines mid-thread",
    body: "Move a thread to another engine and it starts with a summary of what was said, marked in the conversation.",
  },
  {
    title: "Worktree lanes and handoff",
    body: "Run a task in its own git worktree and branch, then merge it back. Hand a session to another engine with a written summary, in a pane beside it or as a thread.",
  },
  {
    title: "A review queue across engines",
    body: "Agents that finish with changes wait in one queue, whichever engine ran them, labelled by what they touch: migrations, CI, lockfiles, .env files, auth paths and deleted tests. Approve exactly the diff you read, and the item comes back if it changes. Ask another engine for a second opinion, and its findings land as line comments you can send to the agent.",
  },
  {
    title: "Previews per worktree",
    body: "Give each worktree its own ports and environment, so parallel agents' dev servers and databases don't collide. Each worktree's app runs as a preview you open from the rail, and Compare previews puts two agents' versions side by side.",
  },
  {
    title: "Files, git and a browser",
    body: "A file tree and code editor beside your agents, a git panel to stage, commit, push and switch branches with your own git, and a browser in the sidebar or a pane. Pick elements on a page and send them to an agent; paths and URLs in terminals open with Ctrl+Click. A Scratchpad keeps Markdown notes beside it all, on your computer.",
  },
  {
    title: "SSH workspaces",
    body: "Open a folder on another machine and its terminals, agents, threads, files and git run there. Vibezeo connects with your own ssh, agent and config, pins the host's key, and keeps terminals running through a dropped connection (up to 10 minutes). Engines run signed in on the host; your sign-ins stay on your computer. Linux hosts, x86-64 and arm64.",
  },
  {
    title: "Teammates",
    body: "Agent mode keeps named teammates that run on the engines you already use. Each has a brief, notes it remembers between chats and skills it can read; after a turn it can suggest what to remember, for you to keep or discard. Teammates search their past chats and send each other mail. Make as many as you need.",
  },
  {
    title: "Find work by what it's about",
    body: "Type \u201cthe login tests\u201d into Find work in the command palette and it takes you to the pane, thread or teammate doing them, even if nothing is named that. The search runs on your computer.",
  },
  {
    title: "Dictate",
    body: "Hold Ctrl+Alt in any app, speak and let go: the words are typed where your cursor was. Speech is recognized on your computer by a model you choose to download, and audio is never saved. History, stats and a dictionary for names and terms stay on your computer too.",
  },
  {
    title: "Plugins with approvals",
    body: "Connect services such as Linear, Notion, Sentry or GitHub once, then choose which teammates and threads may use them. Reads run; writes, posts, spending and deletions ask you first, and runs nobody is watching hold them for your review. Credentials stay in your keychain and engines never see them.",
  },
  {
    title: "Tasks",
    body: "A task tracker on your computer that your agents read and update too, with statuses, priorities, labels, comments and a history of who changed what. Changes from anyone show up at once, and your unsaved edits survive them.",
  },
  {
    title: "Several accounts per engine",
    body: "Keep work and personal accounts apart, each signed in by itself, with usage, limits and context in view. Add as many as you need.",
  },
  {
    title: "Notifications that matter",
    body: "You get an OS notification when an agent needs you or finishes, plus an in-app history. Nothing pings you for the pane you're already looking at.",
  },
  {
    title: "Crash-safe restore",
    body: "Layouts, tabs and panes come back after a quit or a crash. Agents return paused, ready to resume their session, and nothing re-runs on its own. We test this by force-killing the app at random points.",
  },
  {
    title: "Signed updates",
    body: "Updates are signature-checked before they install. An older or tampered build is refused.",
  },
  {
    title: "Private by default",
    body: "Usage statistics and crash reports are off until you turn them on. Secrets live in your OS keychain, and logs are scrubbed of tokens and keys. Export your data to a zip, or delete it from your computer, from Settings.",
  },
] as const;

/** How Vibezeo builds on Claude: what's shipped, how it's built, and what's built but not live yet. */
export const CLAUDE = [
  {
    tag: "Shipped",
    title: "Claude Code, integrated first",
    body: "Claude Code is the engine Vibezeo integrates most deeply. Vibezeo installs Claude Code's own hooks in a separate home for each account, so it knows exactly when a session starts, runs a tool, needs you or finishes. After a restart it reopens the same Claude Code session with --resume.",
  },
  {
    tag: "How we build",
    title: "Built with Claude Code",
    body: "Vibezeo is built with Claude Code. Most of the code, including the desktop app, the backend, this site and their tests, was written with it, and the commit history records it.",
  },
  {
    tag: "Comes with Vibezeo accounts",
    title: "Hosted features on the Claude API",
    // DESIGN §9.5: the one phrase for hosted features, word for word.
    body: "Vibezeo's hosted AI runs on the Claude API. Session titles, teammate drafts, Enhance and Polish for dictated prompts, and the Auto supervisor are built in the app and the backend, which calls Claude and stores no prompts or responses. It uses Vibezeo's servers and switches on when Vibezeo accounts open.",
  },
] as const;

/** What comes next, in order. Each item says what's built and what it waits for. */
export const ROADMAP = [
  {
    title: "Code intelligence in the editor and diffs",
    body: "Go to definition, find references, hover and errors in the Files editor, and the same jumps from a diff, using the language servers you already have, once you trust the repository. Built and in review now.",
  },
  {
    title: "Lead keeps a plan moving",
    body: "Once a plan's runs start, Lead can answer their routine questions for you and pass the rest on to you. Built and in review now.",
  },
  {
    title: "Secrets for agents' commands",
    body: "An agent runs a command that needs your secrets with vibezeo run, after you approve it, and the output comes back to it with the values masked. Built and in review now.",
  },
  {
    title: "Local models as engines",
    body: "Agents that run on a model served from your own computer. Built and in review now.",
  },
  {
    title: "Activity ledger",
    body: "What an agent session did, in order: the turns it was given, the commands it ran, the files it changed, and the approvals it asked for and who answered. Built and in review now.",
  },
  {
    title: "Your phone, from anywhere",
    body: "Your phone already pairs with Vibezeo over your own network. The Vibezeo Remote app for iOS and Android, with encrypted push and a relay that reaches your agents away from home, is built and comes with the app stores and accounts.",
  },
  {
    title: "macOS, Windows and WSL",
    body: "The same app on macOS and Windows, with WSL folders in the Windows download. The code is ready; the downloads wait for signed builds tested on real machines.",
  },
  {
    title: "Conductor, when accounts open",
    body: "Talk to Vibezeo and it works the app for you: Conductor starts agents, prompts them and finds work. It's built, along with the other features that need our servers, such as cloud dictation and the phone relay, and they switch on with Vibezeo accounts.",
  },
] as const;

/** The hero (D-1841): ownership first. Every phrase is true of the shipped
 * build: the engines run unmodified on their own sign-ins, the app runs on
 * the computer, the app needs no account, and this build sends nothing
 * to Vibezeo's servers (privacy page, FAQ). No other product is named. */
export const HERO = {
  title: ["Your agents.", "Your machine.", "Your plan."],
  /** The phrase drawn in the teal → sky highlight. */
  highlight: "Your machine.",
} as const;

/** The four ownership facts under the hero, each a claim about this build. */
export const OWNERSHIP = [
  { icon: "terminal", text: "Agent CLIs run unmodified" },
  { icon: "key", text: "On your own subscriptions" },
  { icon: "files", text: "Runs on your computer" },
  { icon: "lock", text: "No account, nothing sent to us" },
] as const;

export const PRINCIPLES = [
  {
    title: "Your machine, your accounts",
    body: "Nothing leaves your machine unless you opt in, and engine tokens are never touched.",
  },
  {
    title: "Reliability first",
    body: "Correct terminal behaviour, restore after a crash, and no silent failures.",
  },
  {
    title: "Explicit authority",
    body: "Vibezeo never answers an agent's request on your behalf, and spending always asks first.",
  },
  {
    title: "One codebase",
    body: "The goal is that Linux, macOS and Windows get the same features on the same day. Linux is first.",
  },
] as const;

export const FAQ = [
  {
    q: "Can I use Vibezeo today?",
    a: `Yes. Download Vibezeo ${RELEASE.version} for Linux from the download page. It's free, runs on your computer with no account, and doesn't contact our servers.`,
  },
  {
    q: "How does Vibezeo use Claude?",
    a: "Claude Code is the engine Vibezeo integrates most deeply, and Vibezeo itself is built with Claude Code. Its hosted AI features run on the Claude API: session titles and Enhance and Polish are built, and go live when accounts open.",
  },
  {
    q: "Which platforms does it support?",
    a: "Linux x86-64, as a .deb for Debian and Ubuntu and an .rpm for Fedora.",
  },
  {
    q: "Does Vibezeo see my API keys or subscription tokens?",
    a: "No. Each engine CLI signs in by itself, in its own folder on your disk. Vibezeo starts the CLI and shows its terminal, and never reads, copies or forwards the engine's credentials.",
  },
  {
    q: "Do I need a new AI subscription?",
    a: "No. Vibezeo runs the coding agents you already have, on your existing plans.",
  },
  {
    q: "Is Vibezeo free?",
    a: "Yes. Vibezeo is free, with no paid plans and no limits on what runs on your computer. Your agents run on the AI subscriptions you already have.",
  },
] as const;

export type FeatureTitle = (typeof FEATURES)[number]["title"];

export function feature(title: FeatureTitle): (typeof FEATURES)[number] {
  const f = FEATURES.find((x) => x.title === title);
  if (!f) throw new Error(`no feature ${title}`);
  return f;
}

/** A body's first sentence: up to the first ". " (or the whole body). */
export function firstSentence(text: string): string {
  const cut = text.indexOf(". ");
  return cut < 0 ? text : text.slice(0, cut + 1);
}

/** The feature the hero's scene shows: Code mode's real terminals (D-1881). */
export const HERO_FEATURES = ["Real terminals, side by side"] as const satisfies readonly FeatureTitle[];

/** #features: what it's like, as scenes of the app (DESIGN §5.1, D-1881).
 * Each row's features are shown by its scene; every feature appears in
 * exactly one place on the home page: the hero, a scene row, the capability
 * index or its "Everything else" list (a test checks). `release` gates a
 * row on the release that ships what it shows (D-128). */
export const SCENE_ROWS = [
  {
    id: "dashboard",
    eyebrow: "Dashboard",
    title: "See who needs you",
    body: "Working, waiting, done or failed: every pane, the rail and the Dashboard show it, from each engine's own signals.",
    features: ["One Dashboard", "Live status for every agent"],
    docs: docsPath("code-mode"),
    release: "0.5.0",
  },
  {
    id: "review",
    eyebrow: "Review",
    title: "Review before you merge",
    body: "Finished work waits in one queue with its diff. Ask another engine for a second opinion before you approve.",
    features: ["A review queue across engines"],
    docs: docsPath("code-mode"),
    release: "0.8.0",
  },
] as const satisfies ReadonlyArray<{ features: readonly FeatureTitle[]; [key: string]: unknown }>;

/** The capability index (#capabilities): eight, one line each, as DESIGN
 * §9.1 lists them; the full copy is in the docs (progressive disclosure). */
export const CAPABILITIES = [
  { title: "Dictate", icon: "mic", docs: docsPath("dictate") },
  { title: "Worktree lanes and handoff", icon: "branch", docs: docsPath("code-mode") },
  { title: "Previews per worktree", icon: "compare", docs: docsPath("code-mode") },
  { title: "Files, git and a browser", icon: "files", docs: docsPath("code-mode") },
  { title: "SSH workspaces", icon: "server", docs: docsPath("remote") },
  { title: "Find work by what it's about", icon: "search", docs: docsPath("code-mode") },
  { title: "Plugins with approvals", icon: "plug", docs: docsPath("plugins") },
  { title: "Crash-safe restore", icon: "restore", docs: docsPath("troubleshooting") },
] as const satisfies ReadonlyArray<{ title: FeatureTitle; [key: string]: unknown }>;

/** "Everything else in the docs": the rest, as one line of links. */
export const MORE = [
  { title: "Thread mode", docs: docsPath("thread-mode") },
  { title: "Switch engines mid-thread", docs: docsPath("thread-mode") },
  { title: "Teammates", docs: docsPath("agent-mode") },
  { title: "Tasks", docs: docsPath("tasks") },
  { title: "Several accounts per engine", docs: docsPath("accounts-and-limits") },
  { title: "Your accounts, untouched", docs: docsPath("privacy") },
  { title: "Notifications that matter", docs: docsPath("settings") },
  { title: "Signed updates", docs: docsPath("troubleshooting") },
  { title: "Private by default", docs: docsPath("privacy") },
] as const satisfies ReadonlyArray<{ title: FeatureTitle; docs: string }>;

/** The four questions on the home page (DESIGN §5.1); the rest of FAQ is
 * answered on /about/ and /download/. */
export const HOME_FAQ = [
  "Can I use Vibezeo today?",
  "Do I need a new AI subscription?",
  "Does Vibezeo see my API keys or subscription tokens?",
  "Is Vibezeo free?",
] as const;

/** Release-note tiles: icon and docs page for each lead phrase in a
 * release's Added section. A release whose Added title isn't listed here
 * fails the site test, so a tile never renders without a link. */
export const RELEASE_TILE_MAP: Record<string, { icon: string; docs: string }> = {
  Dictate: { icon: "mic", docs: docsPath("dictate") },
  Plugins: { icon: "plug", docs: docsPath("plugins") },
  Tasks: { icon: "tasks", docs: docsPath("tasks") },
  "SSH workspaces": { icon: "server", docs: docsPath("remote") },
  Diagnostics: { icon: "activity", docs: docsPath("troubleshooting") },
  "Tray icon": { icon: "tray", docs: docsPath("dictate") },
  Checkpoints: { icon: "restore", docs: docsPath("workflow-tools") },
  "Command recall": { icon: "history", docs: docsPath("workflow-tools") },
  Ports: { icon: "network", docs: docsPath("workflow-tools") },
  "Best of N": { icon: "layers", docs: docsPath("workflow-tools") },
  "Prompt library": { icon: "book", docs: docsPath("workflow-tools") },
  "Skills in your engines": { icon: "sparkles", docs: docsPath("agent-mode") },
  "A teammate in a terminal": { icon: "terminal", docs: docsPath("agent-mode") },
  "Find work finds teammates": { icon: "search", docs: docsPath("code-mode") },
  "Files and git on SSH hosts": { icon: "server", docs: docsPath("remote") },
  Fedora: { icon: "package", docs: docsPath("index") },
  "Worktree setup": { icon: "branch", docs: docsPath("code-mode") },
  "Review comments to the agent": { icon: "message", docs: docsPath("code-mode") },
  "Overlap check": { icon: "compare", docs: docsPath("code-mode") },
  "Merge requests on GitLab": { icon: "merge", docs: docsPath("code-mode") },
  "Worktrees on SSH hosts": { icon: "server", docs: docsPath("remote") },
  "Best of N on SSH hosts": { icon: "layers", docs: docsPath("remote") },
  "Plugins and tasks for agents on SSH hosts": { icon: "plug", docs: docsPath("remote") },
  "Hosts behind a jump host": { icon: "route", docs: docsPath("remote") },
  "Your phone, over your own network": { icon: "phone", docs: docsPath("remote") },
  "Review queue": { icon: "tasks", docs: docsPath("code-mode") },
  "Second opinion": { icon: "users", docs: docsPath("code-mode") },
  "Ports and environment per worktree": { icon: "network", docs: docsPath("code-mode") },
  "Previews per worktree": { icon: "compare", docs: docsPath("code-mode") },
  "Runaway guard": { icon: "shield", docs: docsPath("workflow-tools") },
  "Usage forecast": { icon: "activity", docs: docsPath("accounts-and-limits") },
  "One instructions file for every engine": { icon: "book", docs: docsPath("engines") },
  "Land worktrees": { icon: "merge", docs: docsPath("code-mode") },
  "CI results and review comments back to the agent": { icon: "message", docs: docsPath("code-mode") },
  "Intent radar": { icon: "layers", docs: docsPath("code-mode") },
  "Engine versions and updates under your control": { icon: "package", docs: docsPath("engines") },
  "Session library": { icon: "history", docs: docsPath("workflow-tools") },
  "Sandbox agent panes": { icon: "lock", docs: docsPath("engines") },
  "Antigravity CLI and Kiro CLI": { icon: "terminal", docs: docsPath("engines") },
  "Resource governor": { icon: "activity", docs: docsPath("engines") },
  "Agent-proof secrets": { icon: "lock", docs: docsPath("privacy") },
  "Multi-repo workspaces": { icon: "branch", docs: docsPath("code-mode") },
  "Any ACP agent in Thread mode": { icon: "plug", docs: docsPath("engines") },
  Claims: { icon: "compare", docs: docsPath("tasks") },
  "Plan with Lead": { icon: "tasks", docs: docsPath("tasks") },
  "Follow a plan through": { icon: "tasks", docs: docsPath("tasks") },
  "Land approved work in the plan's order": { icon: "merge", docs: docsPath("tasks") },
  "Plans are kept": { icon: "history", docs: docsPath("tasks") },
  "Chat threads claim the files they edit": { icon: "compare", docs: docsPath("tasks") },
};

export type ReleaseTile = { title: string; body: string; text: string; platforms: string[]; icon: string; docs: string };

/** The "New in" tiles: a release's Added entries whose text starts with a
 * bold lead phrase ("**Dictate.** Hold Ctrl+Alt…"), in CHANGELOG order. */
/** The release whose Added entries the "New in" tiles show: this one, or,
 * for a release that only changes and fixes things (0.7.1), the last one
 * that added something. RELEASES is newest first. */
export function tilesRelease(): Release | undefined {
  const at = RELEASES.findIndex((r) => r.version === RELEASE.version);
  return RELEASES.slice(Math.max(at, 0)).find((r) => r.entries.some((e) => e.category === "Added"));
}

export function releaseTiles(release: Release | undefined = tilesRelease()): ReleaseTile[] {
  if (!release) return [];
  return release.entries
    .filter((e) => e.category === "Added")
    .flatMap((e) => {
      const m = /^\*\*(.+?)\.?\*\*\s*(.*)$/.exec(e.text);
      if (!m) return [];
      const title = m[1] as string;
      const rest = m[2] as string;
      const meta = RELEASE_TILE_MAP[title];
      if (!meta) throw new Error(`"${title}" (CHANGELOG ${release.version}) has no entry in RELEASE_TILE_MAP`);
      return [{ title, body: firstSentence(rest), text: rest, platforms: e.platforms, icon: meta.icon, docs: meta.docs }];
    });
}

/** What "Linux first" means in this build, each fact checkable in the
 * download or its CHANGELOG section (a test checks the CHANGELOG ones). */
export function linuxFacts(): string[] {
  const linux = DOWNLOADS.find((d) => d.os === "linux" && d.available);
  const distros = linux?.requirement.split(": ")[1] ?? "";
  const kinds = [...new Set((linux?.assets ?? []).map((a) => a.file.slice(a.file.lastIndexOf("."))))];
  const named = kinds.map((k) => `${/^\.[aeiou]|^\.r/i.test(k) ? "an" : "a"} ${k}`);
  const packages = named.length > 1 ? `${named.slice(0, -1).join(", ")} and ${named.at(-1)}` : (named[0] ?? "");
  return [
    `${packages.charAt(0).toUpperCase()}${packages.slice(1)} for ${distros}`,
    "Dictate on X11 and Wayland",
    "SSH hosts on x86-64 and arm64",
  ];
}
