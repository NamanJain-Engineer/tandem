# Changelog

Release notes per version. `release.yml` refuses a tag whose version has no
section here.

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
