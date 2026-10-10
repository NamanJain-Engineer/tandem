/** What's in the 0.5.0 release, for the home page's progress strip and the
 * changelog. Figures are counted in the code the release was built from
 * (product branch): `scripts/gate.sh` output (1384 Rust and 1130 TypeScript
 * tests) and the files in `apps/desktop/e2e/specs`. Update them with each
 * release. */
export const PROGRESS = {
  milestones: 14,
  automatedTests: 2514,
  endToEndSuites: 109,
  engines: 11,
} as const;

export type Milestone = {
  id: string;
  title: string;
  items: readonly string[];
};

/** Newest first. Each entry is a closed milestone of the build plan. */
export const MILESTONES: readonly Milestone[] = [
  {
    id: "M14",
    title: "SSH workspaces",
    items: [
      "Open a folder on an SSH host: its terminals, agents, threads, files and git run there, with your own ssh, agent and config.",
      "The host's key is pinned the first time; if it changes, Vibezeo stops until you trust the new one.",
      "A small helper on the host, checked against its SHA-256, keeps terminals running through a dropped connection (up to 10 minutes) and reattaches them.",
      "Engines run signed in on the host, and your sign-ins, plugin credentials and the local gateway stay on your computer.",
      "WSL folders, written for the Windows download (not yet tried on Windows).",
    ],
  },
  {
    id: "M12",
    title: "Plugins and approvals",
    items: [
      "Connect a service once in Settings › Plugins: Web fetch, Notion, Sentry, Cloudflare, Linear, Supabase, Stripe and PostHog, and GitHub and Slack with a token.",
      "Grants per teammate, per thread or for every thread.",
      "Risk classes: reads run; writes, posts, spending and deletions ask you first, and a spend's amount is checked again before it runs.",
      "Held actions: what a run nobody is watching wants to change waits for your approval.",
      "Credentials in your system keychain, never shown to engines.",
    ],
  },
  {
    id: "M11",
    title: "Tasks and routines",
    items: [
      "A task tracker on your computer, with statuses, priorities, labels, comments and history, updated live as anyone changes it.",
      "Agents in threads, teammate chats and terminal panes read and update the same tasks through Vibezeo's tasks server.",
      "Running tasks in their own lanes, and routines that run teammates on a schedule, are built for Pro and switch on with Vibezeo accounts.",
    ],
  },
  {
    id: "M10",
    title: "Teammates",
    items: [
      "Named teammates in Agent mode that run on the engines you already use, each with a brief, notes it remembers between chats and skills it can read.",
      "Chats you can come back to, and after a turn a suggestion of what to remember, for you to keep or discard.",
      "Teammates search their past chats and send each other mail, through one local gateway.",
      "Lead is made for you; Free includes one teammate besides Lead.",
    ],
  },
  {
    id: "M9",
    title: "Files, git and a browser",
    items: [
      "A Files tab for the open thread's folder, the workspace or a lane: a tree that follows .gitignore and a code editor that asks before overwriting a file changed on disk.",
      "A git panel: stage, unstage, discard, commit, push, pull, publish and switch branches, with your own git, config and hooks.",
      "A browser in the sidebar or in a pane, with Phone and Tablet sizes; its pages get no access to the app.",
      "An element picker that sends what you pick on a page to a pane or a thread.",
      "File paths and URLs in terminals open with Ctrl+Click, following the shell's current folder.",
      "A Scratchpad in the sidebar: local Markdown notes with lists and checklists, saved as you type.",
    ],
  },
  {
    id: "M8",
    title: "Dictate and onboarding",
    items: [
      "Push-to-talk dictation into any app: hold Ctrl+Alt, speak and let go.",
      "Speech models that run on your computer (Parakeet and Whisper), downloaded when you choose and checked against pinned SHA-256s.",
      "A widget while you talk, history and stats, and a dictionary for names and terms.",
      "A mic in the message box, and a Dictate step in the first-run setup.",
    ],
  },
  {
    id: "M7",
    title: "Thread mode",
    items: [
      "Claude Code, Codex, Cursor Agent, Grok and OpenCode as conversations: streamed replies, folded tool calls, Stop and queued follow-ups (Cursor Agent, Grok and OpenCode threads on Pro).",
      "Approvals answered inline, once or for the whole thread.",
      "Plan mode with numbered plan versions, and Implement for the one you pick.",
      "A Changes panel with every file a thread changed since it started, and the files it read.",
      "Up to six threads side by side on Pro (one at a time on Free).",
      "Switching engines mid-thread, with a summary of the conversation.",
    ],
  },
  {
    id: "M6",
    title: "Lanes, handoff and accounts",
    items: [
      "Worktree lanes: a task in its own git worktree and branch, merged back or cleaned up.",
      "A launch sheet with saved presets and lineups.",
      "Handoff between engines with a written summary.",
      "Several accounts per engine, each signed in by itself, with usage, limits and context meters.",
    ],
  },
  {
    id: "M5",
    title: "Dashboard, notifications, restore and updates",
    items: [
      "A Dashboard of every live agent across workspaces, with search, filters and grouping by status or project.",
      "A notification center, plus OS notifications when an agent waits for you, finishes or fails while you're elsewhere.",
      "Crash-safe restore: layouts and panes come back, and agents return paused, ready to resume their session.",
      "Signed auto-updates that refuse tampered or older builds.",
      "A release pipeline with third-party notices and software bills of materials.",
      "First-run setup.",
    ],
  },
  {
    id: "M4",
    title: "Engines, provider homes and live status",
    items: [
      "Detects 11 coding-agent CLIs, and shows how to install the missing ones.",
      "A separate provider home per engine account; each engine signs in by itself.",
      "Live status from Claude Code's hooks, and from each engine's own output for the rest.",
      "A launch grid, auto-arrange and a card view.",
    ],
  },
  {
    id: "M3",
    title: "Code mode",
    items: [
      "Real PTY terminals in split layouts, with drag and drop, swap and maximize.",
      "Full-screen terminal apps such as vim and htop render as in a native terminal, checked against golden snapshots.",
      "Sixteen live terminals at once, with batched rendering.",
      "A command palette, keyboard shortcuts, find in terminal, paste and file drops.",
    ],
  },
  {
    id: "M2",
    title: "Desktop app and sign-in",
    items: [
      "The Tauri desktop app, its design system, settings and themes.",
      "Sign in with a Vibezeo account, or use the app offline.",
      "Usage statistics and crash reports off by default; secrets in the OS keychain.",
    ],
  },
  {
    id: "M1",
    title: "Accounts backend",
    items: [
      "Accounts with email verification, signed entitlements and device management.",
      "Plans and billing, in test mode.",
      "Rate limits and abuse protection.",
    ],
  },
  {
    id: "M0",
    title: "Foundations",
    items: [
      "Secret redaction for every log line, path containment and the local store.",
      "CI with lint, test and security gates.",
    ],
  },
];
