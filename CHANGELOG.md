# Changelog

Release notes per version. `release.yml` refuses a tag whose version has no
section here.

## 0.3.0

Linux (x86-64). macOS and Windows are next. This build runs the Free plan on
your computer, with no account and nothing sent to our servers. It carries
everything from 0.2.0 (never published on its own) and adds:

- **Files.** A Files tab in the sidebar for the open thread's folder, the
  active workspace or a lane: a lazy tree that follows `.gitignore`, a code
  editor with syntax highlighting, search and go to line, and a prompt when a
  file changed on disk while you edited it. Files inside a workspace are
  editable; anything else opens read-only.
- **Git panel.** See what changed, stage, unstage, discard, commit, push,
  pull, publish a branch, switch branches and start a new one. It runs your
  own `git`, with your config, hooks and credential helpers.
- **A browser beside your agents.** Open a page in the sidebar or in a pane,
  with Back, Forward, Reload and Phone and Tablet sizes; a browser pane's page
  comes back after a relaunch. Pages get no access to the app.
- **Element picker.** Pick elements on a page (React components named where
  the page has them) and send what you picked to a pane or a thread.
- **Terminal links.** Paths and URLs a program prints open with Ctrl+Click:
  files in the Files tab at their line, following the shell's `cd`; URLs in
  the browser.
- **Scratchpad.** Local Markdown notes in the sidebar: Ctrl+B, I and U,
  lists and checklists, saved as you type. They stay on your computer.
- **Thread mode engines by plan.** Threads on the Free plan run Claude Code
  and Codex. Cursor Agent, Grok and OpenCode threads are on Pro, which this
  build can't sell yet; all three still run in terminal panes on Free.

Plan limits in this build (Free): 4 live panes, 3 workspaces, one thread at a
time, one account per engine, and threads with Claude Code and Codex.

## 0.2.0

Linux (x86-64). macOS and Windows are next.

- **Thread mode.** Work with Claude Code, Codex, Cursor Agent, Grok and
  OpenCode as a conversation instead of a terminal UI: streamed replies, tool
  calls folded into groups, approvals answered inline (once, or for the whole
  thread), Stop, queued follow-ups, attachments and `@` mentions.
- **Plan, then implement.** In Plan mode every reply is a numbered plan
  version; Implement sends exactly the version you pick.
- **Changes and Files.** Every file a thread changed since it started, with
  its diff, and the files it read or edited.
- **Up to six threads side by side** on Pro, each with its own composer
  (Free shows one at a time).
- **Switch engines mid-thread.** The new engine starts with a summary of the
  conversation; a divider marks the switch.
- **Worktree lanes.** Run a task in its own git worktree and branch, merge it
  back or clean it up.
- **Launch sheet and presets.** Start several agents at once from a saved
  lineup.
- **Handoff.** Hand a session to another engine with a written summary, in a
  pane beside it, a new worktree, or a thread.
- **Engine accounts.** Separate accounts per engine, each signed in by
  itself, with usage, limits and context meters in view.
- Interrupted threads say "Interrupted — Resume?" after a crash, and are never
  marked done.

## 0.1.0

Not released yet. The first public build will collect the milestones up to
then: Code mode with agents in panes, the Dashboard and notifications,
session restore, and signed auto-update.
