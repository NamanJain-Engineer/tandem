# Changelog

Release notes per version, the one source for the website's changelog, the
app's "What's new" and the update feed (F-WB-03). Each version is a `## x.y.z`
section: an optional summary, then `### Added`, `### Changed`, `### Fixed` or
`### Security`, whose bullets start with platform tags (`[mac]`, `[win]`,
`[lin]` or `[all]`). `scripts/check-changelog.ts` checks the format, and a
release fails when its version has no section.

## 0.4.0

Linux (x86-64). macOS and Windows are next. Like 0.3.0, this build runs the
Free plan on your computer, with no account and nothing sent to our servers.

Plan limits in this build (Free): 4 live panes, 3 workspaces, one thread at a
time, one account per engine, threads with Claude Code and Codex, and one
teammate besides Lead.

### Added

- [lin] **Teammates.** Agent mode now holds named teammates that run on the
  engines you already use. Each has a brief, notes it remembers between chats
  (within a budget you set), skills it can read, and chats you can come back
  to. After a turn, a teammate can suggest what to remember, for you to keep
  or discard. Teammates can search their past chats and send each other mail.
  Lead is made for you, and Free includes one teammate besides Lead.
- [lin] **Find work.** "Find work…" in the command palette (Ctrl+K) finds an
  open pane or thread from a description such as "the login tests" and takes
  you to it. The search runs on your computer.
- [lin] **Your data.** Settings › Privacy exports your data to a zip (every
  table as JSON, teammates' memory and skills, your notes), leaving out engine
  sign-ins and secrets. It can also delete Vibezeo's data from this computer;
  engine sign-ins stay unless you tick "Also remove engine sign-ins".
- [lin] **Your shortcuts.** Settings › Shortcuts lists every app shortcut and
  records new keys. It says when a combination is already used, with Swap, and
  refuses keys the system or text editing owns. Ctrl+/ shows the shortcuts in
  effect.
- [lin] **Usage.** "Usage…" in the workspace menu shows 12 weeks of tokens per
  day, per engine and per account, read from Claude Code's and Codex's own
  logs on your computer.
- [lin] **What's new.** From the next update on, the app shows once what
  changed in it. (0.3.0 kept no record of what you'd seen, so this update
  shows nothing.)

### Fixed

- [lin] The plan at the foot of the left rail says FREE. In 0.3.0 it said
  OFFLINE, which read like a connection problem; this build simply runs Free.

### Security

- [lin] A teammate's memory, skills, roster and mail reach its engine marked as
  data to read, never as instructions to follow.

## 0.3.0

Linux (x86-64). macOS and Windows are next. This build runs the Free plan on
your computer, with no account and nothing sent to our servers. It carries
everything from 0.2.0 (never published on its own).

Plan limits in this build (Free): 4 live panes, 3 workspaces, one thread at a
time, one account per engine, and threads with Claude Code and Codex.

### Added

- [lin] **Files.** A Files tab in the sidebar for the open thread's folder, the
  active workspace or a lane: a lazy tree that follows `.gitignore`, a code
  editor with syntax highlighting, search and go to line, and a prompt when a
  file changed on disk while you edited it. Files inside a workspace are
  editable; anything else opens read-only.
- [lin] **Git panel.** See what changed, stage, unstage, discard, commit, push,
  pull, publish a branch, switch branches and start a new one. It runs your
  own `git`, with your config, hooks and credential helpers.
- [lin] **A browser beside your agents.** Open a page in the sidebar or in a pane,
  with Back, Forward, Reload and Phone and Tablet sizes; a browser pane's page
  comes back after a relaunch. Pages get no access to the app.
- [lin] **Element picker.** Pick elements on a page (React components named where
  the page has them) and send what you picked to a pane or a thread.
- [lin] **Terminal links.** Paths and URLs a program prints open with Ctrl+Click:
  files in the Files tab at their line, following the shell's `cd`; URLs in
  the browser.
- [lin] **Scratchpad.** Local Markdown notes in the sidebar: Ctrl+B, I and U,
  lists and checklists, saved as you type. They stay on your computer.
- [lin] **Thread mode engines by plan.** Threads on the Free plan run Claude Code
  and Codex. Cursor Agent, Grok and OpenCode threads are on Pro, which this
  build can't sell yet; all three still run in terminal panes on Free.

## 0.2.0

Linux (x86-64). macOS and Windows are next. Never published: 0.3.0 carries
all of it.

### Added

- [lin] **Thread mode.** Work with Claude Code, Codex, Cursor Agent, Grok and
  OpenCode as a conversation instead of a terminal UI: streamed replies, tool
  calls folded into groups, approvals answered inline (once, or for the whole
  thread), Stop, queued follow-ups, attachments and `@` mentions.
- [lin] **Plan, then implement.** In Plan mode every reply is a numbered plan
  version; Implement sends exactly the version you pick.
- [lin] **Changes and Files.** Every file a thread changed since it started, with
  its diff, and the files it read or edited.
- [lin] **Up to six threads side by side** on Pro, each with its own composer
  (Free shows one at a time).
- [lin] **Switch engines mid-thread.** The new engine starts with a summary of the
  conversation; a divider marks the switch.
- [lin] **Worktree lanes.** Run a task in its own git worktree and branch, merge it
  back or clean it up.
- [lin] **Launch sheet and presets.** Start several agents at once from a saved
  lineup.
- [lin] **Handoff.** Hand a session to another engine with a written summary, in a
  pane beside it, a new worktree, or a thread.
- [lin] **Engine accounts.** Separate accounts per engine, each signed in by
  itself, with usage, limits and context meters in view.
- [lin] Interrupted threads say "Interrupted — Resume?" after a crash, and are never
  marked done.

## 0.1.0

Not released yet. The first public build will collect the milestones up to
then: Code mode with agents in panes, the Dashboard and notifications,
session restore, and signed auto-update.
