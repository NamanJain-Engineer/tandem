/** What's in the 0.2.0 release, for the home page's progress strip and the
 * changelog. Figures are counted in the code the release was built from
 * (product branch): `scripts/gate.sh` output (613 Rust and 622 TypeScript
 * tests) and the files in `apps/desktop/e2e/specs`. Update them with each
 * release. */
export const PROGRESS = {
  milestones: 8,
  automatedTests: 1235,
  endToEndSuites: 60,
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
