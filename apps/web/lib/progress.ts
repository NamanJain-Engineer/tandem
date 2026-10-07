/** What's been built so far, for the home page's progress strip and the
 * changelog. Figures come from the repository (`scripts/gate.sh` output and
 * `apps/desktop/e2e/specs`); update them when a milestone closes. */
export const PROGRESS = {
  milestones: 6,
  automatedTests: 792,
  endToEndSuites: 41,
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
