## 0.6.0

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
