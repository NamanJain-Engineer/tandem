# Changelog

Release notes per version, the one source for the website's changelog, the
app's "What's new" and the update feed (F-WB-03). Each version is a `## x.y.z`
section: an optional summary, then `### Added`, `### Changed`, `### Fixed` or
`### Security`, whose bullets start with platform tags (`[mac]`, `[win]`,
`[lin]` or `[all]`). `scripts/check-changelog.ts` checks the format, and a
release fails when its version has no section.

## Unreleased

Everything that runs on your computer is free and unlimited, with no
account. Features that need Vibezeo's servers (cloud dictation, Enhance and
Polish, dictionary sync, Conductor, Auto, Create with AI and the phone relay)
come with Vibezeo accounts.

### Changed

- [all] **No limits on your computer.** As many live panes, workspaces,
  teammates, plugin connections and accounts per engine as you need, six
  threads side by side, and every chat engine in Thread mode: Cursor Agent,
  Grok and OpenCode threads run with no account.
- [all] **Routines, task runs and the Night Shift are free.** They run with
  no account and no sign-in, in the app or in the background.
- [all] **Session search is free.** Turn on Search session text in Settings ›
  Privacy to search what your Claude Code and Codex sessions say.
- [all] **No plans to choose.** The plans dialog, See plans, Upgrade buttons,
  Pro badges, limit cards and the rail's plan footer are gone. The welcome
  screen says "Free · no account · nothing sent to Vibezeo". Features that
  need Vibezeo's servers say "Comes with Vibezeo accounts", with "Tell me when
  accounts open" where it helps.

## 0.9.0

Linux (x86-64): a .deb for Debian and Ubuntu, and an .rpm for Fedora.
Vibezeo is free, with no paid plans. Like 0.8.0, this build runs on your
computer, with no account and nothing sent to our servers. This release is about bringing parallel agents'
work home: land finished worktrees one after another behind your checks,
bring a pull request's CI failures and review comments back to the agent,
see when two agents change the same functions, keep each engine's version
under your control, find every past session in one library, and run agent
panes in a sandbox.

Limits in this build, left from the earlier Free plan: 4 live panes, 3
workspaces, one thread at a time, one account per engine, threads with Claude
Code and Codex, one teammate besides Lead, and one plugin connection. The next
release removes them.

### Added

- [lin] **Land worktrees.** "Land worktrees…" in a repository's menu, or any
  of its worktrees' menus, merges finished worktrees into the repository's
  current branch one after another, without a pull request for each. It
  lists the worktrees with commits the branch doesn't have, safest first,
  and takes a check command, such as your tests, which it remembers for the
  repository. For each worktree in turn it merges the branch into the
  worktree, runs the check there with the worktree's ports and environment,
  then merges the worktree into the branch with a merge commit, so what
  lands is exactly what was checked. The queue stops and says why when a
  merge conflicts (it's aborted, so nothing is left half-merged), a check
  fails, the branch or the worktree changed during the landing, or a merge
  would replace a file that isn't committed, such as an ignored `.env`. A
  failed check offers "Send failure to agent", which types the end of its
  output into the worktree's agent when you click. Landing never pushes or
  fetches, and until you push, "Undo landing" moves the branch back after
  you confirm.
- [lin] **CI results and review comments back to the agent.** Allow a
  repository with "Check pull request" in a worktree's ⋯ menu, and Vibezeo
  reads its pull requests through your own GitHub CLI (`gh`), which signs
  itself in: Vibezeo never sees or stores a token. A worktree with an open
  pull request shows its checks on its rail row, such as "1 check failed:
  test". On a failed check, "Send failure to agent" types the check's name,
  its link and the end of the failing job's log into the worktree's agent;
  with no agent there, "Copy failure" copies it. In Open diff, "Import
  review comments" brings the pull request's line comments into the review
  bar, up to 40 at a time, to send to the agent with your own. Logs and
  comments reach the agent marked as data, not instructions, and only when
  you click. GitLab merge requests aren't read yet.
- [lin] **Intent radar.** Git only notices two agents clashing when they
  touch the same lines. The intent radar reads the TypeScript, Rust and
  Python each worktree changed and notes which functions, classes and other
  definitions it added, changed or removed, and which names its new lines
  use. When two worktrees of a repository share a symbol (both changed it,
  or one renamed or removed what the other's new code uses), the rail row
  says so, such as "Shares Session", Check overlap… lists the symbols with
  what each side did, and a notice offers "Warn both agents", which types a
  short note into each agent when you click. It updates after every agent
  turn and only reads files.
- [lin] **Engine versions and updates under your control.** Settings ›
  Engines shows each installed engine's version and how it was installed.
  "Check for updates" asks npm for the newest version, only when you click
  it and only for engines installed with npm. "Pin this version" starts new
  panes of Claude Code or Codex with the engine's own documented switch
  against updating. Vibezeo remembers the last version that finished a
  thread turn: if an engine updates and then breaks on this computer (it
  won't start, or it stops mid-turn), the Engines page and the thread say
  so, and "Roll back to…" shows the command that installs the earlier
  version the same way, before anything runs. "Running in the background"
  lists your own engine processes that have no terminal and that Vibezeo
  didn't start, such as a leftover daemon, and Stop ends one after you
  confirm.
- [lin] **Session library.** "Session library…" in the command palette
  lists the Claude Code and Codex sessions on this computer, from every
  account and from each engine's own home, including sessions you started
  in a terminal outside Vibezeo. They're grouped by repository, with their
  first prompt. Open shows the prompts and replies, read-only. Resume opens
  an agent pane in the session's folder with the engine's own resume, and
  "Continue with another engine…" opens the handoff sheet with a summary of
  the session's last messages. Vibezeo only reads the engines' session
  files and never opens their sign-in files. Searching what sessions say
  comes in the next release.
- [lin] **Sandbox agent panes.** Settings › Engines › Sandbox agent panes,
  off by default, runs each new agent pane's engine in a sandbox made with
  bubblewrap. It writes only to the pane's folder, its repository's git
  data, the engine's own home in Vibezeo and `/tmp`, and it can't read
  `~/.ssh`, `~/.aws`, `~/.gnupg`, your other credential folders or
  Vibezeo's own data. Git's hooks and `.git/config` stay read-only, and the
  network stays on. Claude Code, Codex and OpenCode run sandboxed; engines
  that keep their sign-in in your home folder start outside it, and the
  pane's header says why. Install bubblewrap with your package manager
  (`sudo apt install bubblewrap`); Vibezeo never installs it.

## 0.8.0

Linux (x86-64): a .deb for Debian and Ubuntu, and an .rpm for Fedora. Like
0.7.2, this build runs the Free plan on your computer, with no account and
nothing sent to our servers. This release is about running agents side by
side and checking their work: ports, environments and previews for each
worktree, one review queue across engines, second opinions from another
engine, and warnings before an agent runs away or an account runs out.

Plan limits in this build (Free): 4 live panes, 3 workspaces, one thread at a
time, one account per engine, threads with Claude Code and Codex, one teammate
besides Lead, and one plugin connection.

### Added

- [lin] **Review queue.** Agents that finish with changes wait in one list,
  across every engine, with what each one touched. It's the Review view in
  the Dashboard panel ("Open review queue" in the command palette). An agent
  or thread joins within a second of finishing when its worktree or git
  folder has changes. One with no changes, outside a git repository or on an
  SSH host doesn't, and neither does a Read only thread, such as a second
  opinion's reviewer. Each item shows who did the work, on which engine and
  branch, its last reply or screen lines, how many files it changed, and
  labels for what it touches: 20 or more files, migrations, CI, lockfiles,
  `.env` files, auth paths, your public API paths and deleted tests. The
  labels come from fixed rules on your computer, with no AI. Sort by risk or
  by finish time.
- [lin] In the review queue, Approve remembers exactly what you approved: if
  the agent changes anything after that, the item comes back as "Changed
  since you approved". Open diff shows the whole change, and Last turn what
  changed since the agent's last checkpoint. Snooze hides an item for an
  hour, or until its agent finishes again. On a worktree, Send back opens
  its diff with review comments, Second opinion asks another engine, and
  Merge opens the merge confirm with its forecast. "Repository checks…" on
  an item sets the repository's public API paths and a check command, such
  as `pnpm test`, that runs in the agent's folder each time an item joins,
  for up to 10 minutes. Both are kept in Vibezeo's settings on this
  computer, never in the repository. The queue starts empty each time
  Vibezeo opens, apart from threads that had already finished.
- [lin] **Second opinion.** "Ask for a second opinion…" on a worktree has
  another engine read its changes and leave line comments you can edit and
  send on. It's in the worktree's ⋯ menu and at the top of its diff. The
  reviewer is a Read only thread in the worktree, on your own engine and
  account; any edit it tries is denied. It gets the branch, the task if you
  give one, and the diff, up to 150,000 characters. Each finding on a line
  the diff shows becomes a comment on that line, and everything else it
  says goes into one General comment, so nothing is dropped. Edit or remove
  them like your own comments, then "Send to agent". Only engines that run
  as threads can review: on Free, Claude Code or Codex. One review runs per
  worktree at a time.
- [lin] **Ports and environment per worktree.** "Worktree setup…" can give
  each worktree its own ports and environment variables, so parallel
  agents' dev servers and databases don't collide. With "Ports per
  worktree" (1 to 100), each worktree gets a block of that many ports in
  41000–48999 that no other worktree on this computer has, kept until you
  discard it; its terminals and agents see the first as `VIBEZEO_PORT` and
  the block as `VIBEZEO_PORTS`. If a port in the block is busy when the
  worktree is made, it takes the next free block and says so.
  "Environment" takes one `NAME=value` per line for every terminal and
  agent in the worktree, with `{{slug}}`, `{{index}}`, `{{repo}}`,
  `{{port}}` and `{{port+1}}`… filled in, such as
  `DATABASE_URL=postgres://localhost/app_{{slug}}`. Each worktree also gets
  `COMPOSE_PROJECT_NAME`, so `docker compose` runs a separate stack in each,
  and the setup command, the teardown and Best of N's check command see the
  same variables. The tooltip on a worktree's row shows its ports.
- [lin] The environment is trusted with the setup and teardown commands:
  until you trust them on this computer, terminals get only Vibezeo's own
  variables. Names that change how shells start or where agents sign in or
  send their requests (such as `PATH`, `LD_PRELOAD`, `CLAUDE_CONFIG_DIR`,
  `*_BASE_URL` and proxies) aren't allowed, and nothing in the file can
  replace what Vibezeo sets for an agent. A value that looks like a password
  or key gets a warning, since the file is meant to be committed. Worktrees
  on SSH hosts don't get ports, environments or previews.
- [lin] **Previews per worktree.** Run your app in every worktree at once,
  each on its own ports, and try each agent's version from the rail. Add a
  `preview` command to `.vibezeo/worktree.json`, such as
  `pnpm dev --port {{port}}` (it needs ports per worktree). "Start preview"
  in a worktree's ⋯ menu runs it in a terminal named Preview, and a
  worktree you make with New worktree or the launch sheet starts it by
  itself, after its setup if it has one. Once the port answers, the
  worktree's row shows Preview, which opens it in a sidebar browser tab.
  Best of N's Compare gets Previews, which starts every agent's preview and
  opens a tab for each. "Compare previews…" in a worktree's ⋯ menu (or
  "Side by side…" in Compare) opens two worktrees' previews side by side in
  a new tab, each pane named after its branch. A preview runs only once
  you've trusted the file's commands, and each one counts toward Free's 4
  live panes.
- [lin] **Runaway guard.** When an agent may have run away, its pane shows a
  card saying why, and a notification goes out. By default it trips when an
  agent works for 90 minutes without its turn ending, uses 30 points of its
  account's usage window in an hour (Codex, which reports its windows),
  starts 20 subagents since your last prompt (Claude Code), or ends three
  turns in a row with the same screen. Pause sends the agent Esc; if it's
  still working 5 seconds later, Vibezeo stops its program until you click
  Resume. An agent on an SSH host only gets the Esc, and when an agent
  can't be paused, the card says so. Dismiss keeps that limit quiet until
  your next prompt. Settings › Teammates › Runaway guard changes the limits,
  turns any of them off, or has the guard pause the agent itself. Threads
  and teammate chats are watched for working time only, with a
  notification. The guard never sends a prompt and never switches accounts.
- [lin] **Usage forecast.** For engines that report usage windows (Codex
  does; Claude Code doesn't), the usage chip in the title bar forecasts
  whether each window runs out before it resets. From the last hour's
  readings, each window in its popover says "Not enough data yet", "On
  track" or "Runs out at 3:40 PM, 30 min before it resets". "Who's using
  it" lists the sessions that spent each Claude Code or Codex account's
  tokens in the last hour, with their shares, named after the pane running
  each one and read from the engines' own logs on your computer (token
  counts only). "Warn me before usage runs out" in Settings › Notifications
  (off by default) sends one notification when a window is forecast to run
  out before it resets, and the launch sheet says when an agent's account
  is at 80% or more or forecast to run out. Vibezeo never switches accounts
  for you.
- [lin] **One instructions file for every engine.** "Instructions…" in a
  local folder's menu in the rail shows which instructions file each
  installed engine reads, whether git tracks it, and how it differs from
  `AGENTS.md`. "Use one file for all" makes `AGENTS.md` the source (with
  none yet, you pick the file it starts from): each installed engine that
  needs it gets a one-line file holding `@AGENTS.md` (`CLAUDE.md` for
  Claude Code, `GEMINI.md` for Gemini CLI), while Codex, Copilot CLI,
  Cursor Agent and Antigravity CLI read `AGENTS.md` themselves. Engines
  whose files haven't been checked show as Unknown and get nothing. You see
  every file it would create or replace before anything is written, and
  Undo puts them back as they were. "Copy into worktrees" adds a personal
  file git ignores, such as `CLAUDE.local.md`, to the worktree setup's copy
  list. Vibezeo never commits these files.
- [lin] **Antigravity CLI and Kiro CLI.** Two more coding agent CLIs join
  the launch grid, so Vibezeo now detects 13. They run in terminal panes
  only, as their vendors ship them, never as threads or in the background.
  Each Kiro CLI account signs in inside its own home (Free has one account
  per engine); Antigravity CLI keeps one account per computer, because it
  stores its sign-in in the system keyring. Their panes show when they're
  working or done, but not yet when they're waiting for you. Gemini CLI
  stays listed, with a note in Settings › Engines and the launch grid's
  not-installed list: Google serves it to paid API keys and Gemini Code
  Assist Standard or Enterprise, and personal Google accounts use
  Antigravity CLI.

### Changed

- [lin] **Terminals on screen open first.** When you switch tabs while
  another tab's terminals are still opening, the ones in the tab you're
  looking at now open before the hidden tab's, which follow in order, so
  the agents you're watching show their first output sooner.
- [lin] **Worktree setup clones files on btrfs and XFS.** On filesystems
  with copy-on-write, the files Worktree setup copies into a new worktree
  are cloned, so a large `node_modules` or dev database arrives almost at
  once and takes no extra space until one side changes. The message after
  the worktree is made says how many were cloned. Elsewhere they're copied
  as before.
- [lin] **The Ports tab groups servers by worktree.** A server started in a
  worktree's terminal is listed under the worktree's branch, with its ports
  when it has a block. Open loads it at `http://<worktree>.localhost:<port>`
  where your computer resolves names under `.localhost`, so each worktree's
  cookies and sign-ins stay apart; elsewhere at `localhost`, as before.

### Fixed

- [lin] Terminals hidden for 30 seconds now let go of their scrollback, as
  they were meant to since 0.6.0: until now, Vibezeo's window kept each one
  alive with every line it had shown. Vibezeo reuses the memory this frees
  rather than returning it to the system, so a system monitor won't show it
  drop.
- [lin] A worktree you make with New worktree opens with just the terminal
  running its trusted setup or preview. It could also get an idle shell
  beside it, which on Free could take the last of the 4 live panes and leave
  the setup or preview waiting for a free one.

## 0.7.2

Linux (x86-64): a .deb for Debian and Ubuntu, and an .rpm for Fedora. Like
0.7.1, this build runs the Free plan on your computer, with no account and
nothing sent to our servers.

Plan limits in this build (Free): 4 live panes, 3 workspaces, one thread at a
time, one account per engine, threads with Claude Code and Codex, one teammate
besides Lead, and one plugin connection.

### Changed

- [lin] **The Dictate toggle link works only while the global shortcut is
  down.** `tandem://dictate/toggle` is the fallback when Dictate's global
  shortcut can't work: there's no global-shortcuts portal, or the portal
  failed. While the shortcut works, Wayland included, the link now does
  nothing. Browsers can open `tandem://` links, so this stops a web page from
  starting a take. Settings › Dictate shows the link's command only when you
  need it. If you bound the link while your shortcut worked, use the shortcut.
- [lin] **Dictate says when its shortcut stops working.** If the global
  shortcut stops working while the app runs, for example when a Wayland
  portal session ends, Dictate says so, and the toggle link takes over.
- [lin] **Your last 5 takes on the Dictate card.** The Dictate card lists
  your five most recent takes with their times; click one to open History.
- [lin] **Reconnecting, then Listening.** When your microphone goes silent
  during a take and Dictate reopens it, the widget says Reconnecting, then
  goes back to Listening once sound returns or your backup microphone takes
  over.

### Fixed

- [lin] **Dictate stops at 10 minutes however a take started.** When the
  global shortcut couldn't start, takes started from a message box's mic
  button or the toggle link had no time limit: the widget counted down to 0
  and recording went on. This happens on Wayland without a global-shortcuts
  portal, as in Ubuntu 24.04's default session. Those takes now stop at
  10 minutes, after the same 30-second countdown.
- [lin] Esc now cancels a take started from the mic button or the toggle
  link, as it does one started with the shortcut. On Wayland, where Esc
  can't be heard outside the app, the widget's × cancels it.
- [lin] A quick tap of the Dictate shortcut during a take started from the
  mic button or the toggle link stops it and types what you said, as a tap
  does for a hands-free take started with a double-tap. Before, it started
  over and the words were lost.

## 0.7.1

Linux (x86-64): a .deb for Debian and Ubuntu, and an .rpm for Fedora. Like
0.7.0, this build runs the Free plan on your computer, with no account and
nothing sent to our servers.

Plan limits in this build (Free): 4 live panes, 3 workspaces, one thread at a
time, one account per engine, threads with Claude Code and Codex, one teammate
besides Lead, and one plugin connection.

### Changed

- [lin] **The pane you're in comes first at launch.** When Vibezeo opens a
  workspace with many panes, the focused pane connects to its shell before
  the next panes open, so it's ready to type in sooner: about a second
  sooner with 16 terminals on our test machine.

### Fixed

- [lin] Agents started together, such as six from the launch sheet, took
  about twice as long to show their first output in 0.6.0 and 0.7.0, because
  each new terminal held its output until the call that started its process
  had returned. It now shows output as it arrives.

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
- [lin] Dialogs, the command palette and command recall opened shifted up
  and to the left by half their size, then jumped to their place as they
  finished opening. They now open in place.
- [lin] Typing a confirmation, such as a worktree's branch name to discard
  it, could lose what you'd typed when a terminal started behind the
  dialog. Terminals now leave the keyboard alone while a confirm is open.

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
