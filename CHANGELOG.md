# Changelog

Release notes per version, the one source for the website's changelog, the
app's "What's new" and the update feed (F-WB-03). Each version is a `## x.y.z`
section: an optional summary, then `### Added`, `### Changed`, `### Fixed` or
`### Security`, whose bullets start with platform tags (`[mac]`, `[win]`,
`[lin]` or `[all]`). `scripts/check-changelog.ts` checks the format, and a
release fails when its version has no section.

## 0.7.0

Linux (x86-64): a .deb for Debian and Ubuntu, and an .rpm for Fedora. macOS
and Windows are next. Like 0.6.0, this build runs the Free plan on your
computer, with no account and nothing sent to our servers.

Plan limits in this build (Free): 4 live panes, 3 workspaces, one thread at a
time, one account per engine, threads with Claude Code and Codex, one teammate
besides Lead, and one plugin connection.

### Added

- [lin] **Worktree setup.** "Worktree setup…" in a repository's menu (or
  any of its worktrees') says what each new worktree gets, saved as
  `.vibezeo/worktree.json` in the repository so you can commit and share it.
  Files matching its patterns, written as in `.gitignore` (such as `.env*`),
  are copied from your main checkout into every new worktree; files the
  worktree already has are never overwritten, and the copy stops at 2,000
  files or 512 MB. A setup command, such as `pnpm install`, runs in a new
  terminal in worktrees you make with New worktree or the launch sheet (it
  counts toward Free's 4 live panes), and a teardown command runs before
  Discard worktree deletes the folder, for up to 2 minutes. Commands run only
  once you've trusted them on this computer: saving them in Worktree setup
  trusts them, and a file that arrives or changes some other way, such as in
  a pull, is shown for review first, with nothing run until you choose
  "Trust and run".
- [lin] **Review comments to the agent.** In a worktree's diff, comment on
  any line, then "Send to agent" types all the comments into the agent
  running in that worktree's pane as one message, with each file, line and
  the line's text. With no agent running there, "Copy review" copies the
  same text.
- [lin] **Overlap check.** "Check overlap…" in a worktree's menu lists the
  other worktrees of the repository that changed the same files, and
  forecasts whether merging into the repository's current branch would
  conflict. The merge confirmation shows the same forecast. The forecast
  uses your git (2.38 or later), leaves your checkout untouched, and covers
  committed work only.
- [lin] **Merge requests on GitLab.** For a worktree whose `origin` is on a
  GitLab host, "Create pull request" becomes "Create merge request" and runs
  `glab mr create` (needs the GitLab CLI). Worktrees aren't available in WSL
  folders yet; on SSH hosts they don't have setup, the overlap check or
  merge requests.
- [lin] **Worktrees on SSH hosts.** In a git repository on a host, New
  worktree and the launch sheet's Fleet preset make each worktree on the
  host, under `<repo>/.tandem/worktrees/<name>` on a `tandem/<name>` branch,
  as they do locally. Diff, merge and cleanup run there with the host's own
  git; a worktree's pull request needs the GitHub CLI (`gh`) on the host.
- [lin] **Best of N on SSH hosts.** "Best of N…" works in a repository on a
  host: each agent gets its own worktree there, Compare shows their
  changes, your check command runs in each worktree in your login shell on
  the host, and "Keep this one…" commits the winner with the host's git and
  identity, merges it and removes the other worktrees (their branches
  stay). The agents count against Free's 4 live panes.
- [lin] **Plugins and tasks for agents on SSH hosts.** A thread on a host
  can use the plugins you grant it and your tasks, as a local thread does.
  Its tool calls come back to this computer over the same connection:
  plugin approvals show here, plugin sign-ins and your task list stay here,
  and no port is opened on either machine. An agent terminal on a host
  (Claude Code or Codex) gets the task tools too, for that run only,
  without anything written into the host's own config, and they keep
  working when the connection drops and comes back. As on this computer,
  agent terminals don't get plugins. On Free you can connect one plugin;
  the task tools are included, and running a task with "Execute…" needs
  Pro.
- [lin] **Hosts behind a jump host.** Hosts reached through `ProxyJump` or
  `ProxyCommand` in your SSH config now connect. Vibezeo reads the host's
  key through the jump for you to check before connecting, and the jump
  host itself is reached and checked as your own config and `known_hosts`
  say.
- [lin] **Your phone, over your own network.** Settings › Remote › Local
  network › "Pair over your local network" (off until you turn it on) lets a
  phone pair with no account, on the Free plan. "Pair a phone on this network"
  shows a QR code (valid 10 minutes); scanning it with the phone's camera
  opens Vibezeo's phone web app, which your computer serves at
  `http://<its address>:47800/remote`. Everything between them is end-to-end
  encrypted. Vibezeo listens only on your computer's home or office network
  and VPN addresses (such as Tailscale's), never on a public address. The
  phone and computer must be on the same network or VPN, and a firewall must
  allow port 47800. The Android and iOS apps aren't out yet.
- [lin] A paired phone can see each agent's status and latest activity; deny
  approvals, or allow them (an allow waits for you to confirm in Settings ›
  Remote unless "Allow approvals from phone for the next 8 hours" is on); read
  threads and teammate chats and send to them; type into agent panes, never
  plain shells; and, with Pro's routines, run a routine now. Viewing comes
  with pairing; sending, typing, approvals and routines are each off per phone
  until you turn them on in Settings › Remote. The page refreshes while it's
  open; there are no notifications when it's closed.
- [lin] To unpair, use "Unpair this phone" on the phone, or Revoke (or "Reset
  remote access…") in Settings › Remote; the computer forgets the phone's keys
  and refuses it. Turning the switch off disconnects phones without unpairing
  them.

### Changed

- [lin] **Dictate starts lighter.** Its widget window is created on your
  first take and closed after 10 minutes without one, so Vibezeo starts with
  about 120 MB less memory, unless Show widget is set to Always. The first
  take after launch can show the pill a moment later; recording starts at
  once either way.

### Fixed

- [lin] "Create pull request" now types `gh pr create` into its terminal, and
  a launch-sheet shell with a command now runs it. In 0.6.0 those terminals
  could open at the prompt without the command being typed.
- [lin] Two hosts in Settings › Remote that reach the same machine (two
  aliases for one `user@host:port`, or two hosts behind one jump) no longer
  drop each other: disconnecting or removing one used to cut the other's
  connection too.
- [lin] A `tandem://` link opened while Vibezeo was already running was
  handled twice, so on Wayland the Dictate toggle link started a take and
  stopped it at once.
- [lin] Settings › Dictate's Microphone showed nothing when the system
  default was in use; it now says "System default".

## 0.6.0

Linux (x86-64): a .deb for Debian and Ubuntu, and an .rpm for Fedora. macOS
and Windows are next. Like 0.5.0, this build runs the Free plan on your
computer, with no account and nothing sent to our servers.

Plan limits in this build (Free): 4 live panes, 3 workspaces, one thread at a
time, one account per engine, threads with Claude Code and Codex, one teammate
besides Lead, and one plugin connection.

### Added

- [lin] **Checkpoints.** Before each thread turn and before a prompt you send
  to an agent pane, Vibezeo saves a copy of the folder's files in its own data
  folder, never in your repository. The sidebar's Checkpoints tab shows what
  differs from now and restores all files or the ones you choose, with Undo.
  On by default: 100 checkpoints per folder, kept 14 days, files over 5 MB
  skipped; outside a git repository it covers up to 20,000 files and honors
  `.gitignore`. Needs git installed. Not available in folders on SSH hosts
  yet. Settings › Checkpoints changes the limits or turns them off.
- [lin] **Command recall.** Ctrl+Alt+J searches the commands you ran in
  Vibezeo terminals (bash, zsh, fish and PowerShell with shell integration),
  with their folder and exit code, and puts one at the prompt without running
  it. Commands typed with a leading space aren't recorded, and text that looks
  like a secret is stored redacted. Kept 90 days on this computer; Settings ›
  Terminal turns it off or clears it.
- [lin] **Ports.** The sidebar's Ports tab lists the TCP ports that processes
  started in Vibezeo panes are listening on, with Open in a browser tab, Copy
  URL and Stop (it asks first, and the pane stays open). Servers started
  outside Vibezeo aren't listed.
- [lin] **Best of N.** "Best of N…" in the command palette gives one prompt to
  2 or 3 agents, each in its own worktree from the current commit. Compare
  shows each agent's diff, runs your check command in every worktree, and
  "Keep this one…" commits and merges the winner and removes the other
  worktrees (their branches stay). Needs a local git repository with at least
  one commit; the agents count against Free's 4 live panes.
- [lin] **Prompt library.** Ctrl+Alt+P inserts a saved prompt into the focused
  message box or pane without sending it. Prompts are Markdown files (up to
  64 KB) in a repository's `.tandem/prompts/`, shared through git, or in your
  own `~/.tandem/prompts/`; `{{branch}}`, `{{file}}`, `{{selection}}` and
  `{{clipboard}}` are filled in, and any other `{{name}}` is asked for.
- [lin] **Skills in your engines.** "Also install for" in a skill's ⋯ menu
  copies a teammate's skill into the skills folder of a Claude Code or Codex
  account, so that engine can use it outside Vibezeo too, and saving the
  skill in Vibezeo updates the copy. It's a copy, never a link. If the engine
  already has its own skill with that name, Vibezeo leaves it as it is and
  the row says so.
- [lin] **A teammate in a terminal.** "Launch in terminal" on a teammate's
  page opens it as an agent pane in Code mode, in its Home or one of its
  places and named after it, with its brief typed whole as the first prompt.
  The place opens as a workspace, so it counts toward Free's 3 workspaces,
  and the pane toward its 4 live panes.
- [lin] **Find work finds teammates.** "Find work…" in the command palette
  finds teammates as well as panes and threads, by name, the first line of
  the brief and what the teammate said last.
- [lin] **Files and git on SSH hosts.** The Files tab and the Changes panel
  follow a host's folder as it changes, whatever changed it: an agent, a
  terminal or another program on the host. Push, pull, publishing a branch,
  switching branches and new branches run the host's own git with the host's
  own credentials (its SSH keys or credential helper); nothing from this
  computer is used.
- [lin] **Fedora.** An .rpm package, beside the .deb for Debian and Ubuntu:
  `sudo dnf install ./Vibezeo-0.6.0-1.x86_64.rpm`.

### Changed

- [lin] **Hidden terminals use less memory.** A terminal you haven't seen for
  30 seconds, on another tab or behind a maximized pane, hands its screen to
  Vibezeo's core. It's rebuilt when you come back, with its colours, cursor,
  scroll region and command marks.

### Fixed

- [lin] Settings › Remote, the plans and Find work no longer ask you to sign
  in, or promise hosted features, in this build, which has no Vibezeo
  accounts. Pairing a phone, Conductor and Find work's hosted ranking are Pro
  features that come with accounts.

## 0.5.0

Linux (x86-64). macOS and Windows are next. Like 0.4.0, this build runs the
Free plan on your computer, with no account and nothing sent to our servers.

Plan limits in this build (Free): 4 live panes, 3 workspaces, one thread at a
time, one account per engine, threads with Claude Code and Codex, one teammate
besides Lead, and one plugin connection.

### Added

- [lin] **Dictate.** Hold Ctrl+Alt in any app, speak and let go: the words are
  typed where your cursor was. Speech is recognized on your computer, by a
  model you choose to download in Settings › Dictate: Parakeet v3 (671 MB,
  recommended), Whisper Turbo (574 MB), Whisper Base (English, 148 MB) or
  Distil-Whisper (English, 1.5 GB). Each file is checked against a pinned
  SHA-256 before it's used. Audio isn't saved. Each take's text is kept in
  History on this computer, with stats and a dictionary for names and terms.
  The message box has a mic, and the setup has a Dictate step. On X11 it needs
  nothing else. On Wayland it uses the desktop's global-shortcuts portal, or a
  command you bind to a system shortcut, and typing needs `wtype` and
  `wl-clipboard`. Cloud dictation and Enhance or Polish are Pro features that
  switch on with Vibezeo accounts; this build recognizes speech only on your
  computer.
- [lin] **Plugins.** Settings › Plugins connects a service once; then you
  choose which teammates and threads may use it. Every action has a risk
  class: reads run, and writes, posts, spending and deletions ask you first
  in the chat. Runs nobody is watching hold them in Held actions for you to
  approve. Credentials stay in your system keychain, and engines never see
  them. In this build: Web fetch (built in); Notion, Sentry, Cloudflare,
  Linear, Supabase, Stripe and PostHog with their own sign-in (Linear,
  Supabase, Stripe and PostHog also with an API key); GitHub and Slack with a
  token. Gmail and Vercel are listed as not available yet. Free includes one
  connection.
- [lin] **Tasks.** A task tracker on your computer: the Tasks tab in the
  sidebar and in Agent mode, with statuses, priorities, labels, comments and
  a history of every change. Agents in your threads and teammate chats, and
  Claude Code and Codex in terminal panes, read and update the same tasks, and
  each change records who made it. Running tasks for you, and routines, are
  Pro features that come with Vibezeo accounts.
- [lin] **SSH workspaces.** Settings › Remote › Add host, then open a folder on
  that machine: its terminals, agents, threads, files and git run there.
  Vibezeo connects with your own `ssh`, agent and config, shows the host's key
  fingerprint the first time and pins it, and installs a small helper whose
  SHA-256 is checked on the host before it's used. Terminals keep running for
  up to 10 minutes if the connection drops, and reattach when it's back.
  Engines run signed in on the host; your sign-ins, plugin credentials and the
  local gateway stay on this computer. Threads on a host ask for approval just
  as local ones do. Hosts: Linux x86-64 and arm64. Not in remote folders yet:
  hosts behind ProxyJump or ProxyCommand, git worktrees, plugins and task
  tools in threads, live file watching, and push, pull and branch switching
  from the Changes panel.
- [lin] **Diagnostics.** "Diagnostics…" in the app menu shows, live, how each
  part of the app is doing, how long each terminal took to start and what it
  has waiting, the event queues, uptime and frame rate, with Copy diagnostics.
  It's measured on your computer and never sent anywhere.
- [lin] **Tray icon.** While Dictate is on, a tray icon shows its status, such
  as "Dictate: ready · hold Ctrl+Alt", with Open Vibezeo and Quit Vibezeo.

### Changed

- [lin] When a background part of Vibezeo keeps failing (three restarts in
  ten minutes), a banner names it and offers Copy diagnostics.
- [lin] Screen readers hear new notifications politely, at most one every two
  seconds, with a burst folded into "And N more".
- [lin] The light theme's danger red, added-line green, waiting colour and
  focus ring meet WCAG contrast.

### Fixed

- [lin] The download now includes `tandem-hook`, the small helper that Claude
  Code's hooks and Codex's notifications report through, so their panes show
  exactly when they're working, need you or finish. 0.4.0 left it out, and
  those panes' status came only from what the terminal showed.
- [lin] The status bar no longer says Online in a build that has no Vibezeo
  service to be online to.
- [lin] The New chat button and the unread dot are announced correctly to
  screen readers.

### Security

- [lin] Plugin writes from runs nobody is watching are held for you, never
  run, and say which teammate mail, task or routine they came from.
- [lin] SSH hosts: passwords and key passphrases are typed into a dialog and
  handed to `ssh` through a one-time, loopback-only helper. They're never
  saved or logged, so you're asked again when it reconnects. A changed host
  key stops the connection until you trust the new one.

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
