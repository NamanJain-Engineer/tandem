# Next features (October 2026)

What to build after the 165 features in `build-kit/PRODUCT_SPEC.md`. Internal
document for the coordinating session and the owner.

Researched on 15 October 2026. The evidence is in
`docs/COMPETITIVE_ANALYSIS.md` (refreshed the same day), which is the only
place competitors are named. Here, evidence names Hacker News items, the
trackers of engines Vibezeo hosts, and vendors' official pages. Other
products are described, not named; each entry points to its named sources
in COMPETITIVE_ANALYSIS §6. Every number has a date. Reaction counts
were read on 15 October 2026. **[unverified]** marks a claim that has one
source, or whose sources disagree.

Read §0 first: what already exists, and what this list doesn't repeat.

---

## 0. Ground rules for this list

**Already built, so not proposed again:**
- worktree setup, review comments to the agent, overlap check and GitLab MRs (F-WT-01…04);
- checkpoints and rewind (F-EX-01). This also covers "diff since the last turn", row 12 of the old analysis, which is now closed;
- Best of N (F-EX-04) and the ports panel (F-EX-03);
- multi-account, usage polling and the usage dashboard (F-AC-01…07);
- handoff (F-CD-16), saved sessions and resume after restart (F-CD-03, F-SH-07);
- Dashboard and notifications (F-DB-01…03), the phone companion (F-RM-03…06) and SSH (F-RM-01);
- Auto (F-AU-01), task execution with independent review (F-TK-04) and Routines.

**Every proposal keeps the product's rules** (CLAUDE.md, DECISIONS):
- nothing leaves the machine without an explicit opt-in;
- Vibezeo never reads, copies or proxies an AI vendor's OAuth token, and engines sign in inside their own provider homes;
- secrets go only in the OS keychain;
- nothing is taken from another product: names, branding, copy or code.

Each entry says how it fits these rules or where it is close to them. No
proposal switches accounts automatically (F-AC-05, out of scope §6.7).

**IDs** are proposed, in the spec's existing areas, starting after each
area's last ID. The coordinator assigns the final IDs. Each new Tauri command
or event goes into ARCHITECTURE.md before it is built (CLAUDE.md, IPC).

**Scoring:** impact (I) and effort (E), 1–5 each, ranked by I − E and then by I.
- **E1:** under half a day.
- **E2:** about a day.
- **E3:** two to three days.
- **E4:** a week.
- **E5:** more than that.

**Plan** is the proposed plan; pricing is the owner's call (§4).

---

## 1. Ranked features

| Rank | Proposed ID | Feature | I | E | I−E | Plan |
|---|---|---|---|---|---|---|
| 1 | F-EN-07 | One instructions file for every engine | 4 | 1 | 3 | Free |
| 2 | F-DB-04 | Review queue | 5 | 3 | 2 | Free |
| 3 | F-EN-08 | Engine versions and updates under your control | 4 | 2 | 2 | Free |
| 4 | F-AC-08 | Usage forecast | 4 | 2 | 2 | Free |
| 5 | F-TH-17 | Session library across engines | 4 | 2 | 2 | Free (resume), Pro (full-text search) |
| 6 | F-WT-05 | Second opinion from another engine | 4 | 2 | 2 | Free |
| 7 | F-WT-06 | Worktree runtime: ports, env and Compose per worktree | 4 | 2 | 2 | Free |
| 8 | F-AC-09 | Runaway guard | 4 | 2 | 2 | Free |
| 9 | F-EN-09 | Engine catalog refresh (Antigravity CLI, Kiro CLI) | 3 | 1 | 2 | Free |
| 10 | F-SH-14 | Keep agents running when the window closes | 5 | 4 | 1 | Free |
| 11 | F-TL-11 | Agent sandbox (Linux first) | 5 | 4 | 1 | Free |
| 12 | F-WT-07 | Local merge queue | 4 | 3 | 1 | Free |
| 13 | F-WT-08 | CI results and PR comments back to the agent | 4 | 3 | 1 | Free |
| 14 | F-EN-10 | Any ACP agent as a thread engine | 3 | 2 | 1 | Pro (Free keeps Claude Code and Codex) |
| 15 | F-EN-11 | Local models as engines | 3 | 2 | 1 | Free |
| 16 | F-TK-05 | Claims: agents say what they're about to change | 3 | 3 | 0 | Free |
| 17 | F-EX-06 | Resource governor (Linux first) | 3 | 3 | 0 | Free |
| 18 | F-CD-22 | Multi-repo workspaces | 4 | 4 | 0 | Pro |
| 19 | F-CD-23 | Code intelligence in the editor and diffs | 3 | 3 | 0 | Free |
| 20 | F-EX-07 | Activity ledger | 3 | 3 | 0 | Free; Team export to the audit log |
| 21 | F-CD-24 | jj (Jujutsu) workspaces | 3 | 4 | −1 | Free |

Ranking notes:
- The order is by score. Ranks 10 and 11 score lower than rank 9, but they are the two largest pieces of work with the highest impact (I5). Each should get its own multi-day slice once the quick wins have shipped.
- jj moved up from "later" (old row 14). the largest free rival's top issue is jj workspaces (46 reactions), but the effort is still large.

---

### 1. F-EN-07 One instructions file for every engine

**What it does.** Every engine reads its own instructions file:
- `CLAUDE.md`;
- `AGENTS.md`;
- `GEMINI.md`;
- `.github/copilot-instructions.md`;
- Cursor Agent's rules (the `cursor-agent` engine).

A repository's rules drift apart, or reach only one engine. Per repository, an
**Instructions** panel shows which file each installed engine reads, whether
it exists, and how the files differ. **Use one file for all** keeps
`AGENTS.md` as the source. It then makes each engine's file a one-line
pointer to it, in that engine's own syntax:
- an `@AGENTS.md` import for Claude Code;
- a `contextFileName` setting for Gemini CLI;
- a symlink where an engine has neither.

It shows the diff and asks before changing any file. The panel also catches
personal files the repository doesn't commit, such as `CLAUDE.local.md`, and
offers to add them to the worktree copy patterns (F-WT-01).

**Who.** Everyone who uses more than one engine on a repository. Plan: Free.

**Evidence.**
- An engine started reading `AGENTS.md` when there is no `CLAUDE.md`: HN 49760187, 18 Sep 2026, 741 points / 285 comments.
- A follow-up found it did so only with telemetry on (since fixed): HN 49814947, 486 points / 284 comments.
- A small tool that syncs these files across machines: HN 49587839.
- A major editor now runs several vendors' agents side by side in one session (30 Sep 2026). Instructions that differ per engine are a direct cost.
- Sources: COMPETITIVE_ANALYSIS §6, E-EN-07.

**Impact 4, effort 1.** Detection is a table per engine in `registry.json`, plus a diff view.

**Risks and conflicts.**
- Each engine's file name and import syntax must be checked against its current docs before the build **[unverified per engine]**. Engines change these, so the table lives in the registry, not in code.
- Vibezeo edits only files the user confirms, never `.gitignore` (F-CD-15). No rule conflicts.

**Draft acceptance criteria.**
1. In a repository with `CLAUDE.md` and `AGENTS.md` that differ, the panel lists both, says which engines read each, and shows the diff.
2. "Use one file for all" with Claude Code and Gemini CLI installed proposes:
   - `CLAUDE.md` containing `@AGENTS.md`;
   - Gemini's setting pointing at `AGENTS.md`.

   Nothing is written until the user confirms. After confirming, both engines' files resolve to `AGENTS.md`'s text.
3. A `CLAUDE.local.md` that git ignores is listed as "Not in new worktrees". "Copy into worktrees" adds `CLAUDE.local.md` to `.vibezeo/worktree.json`'s `copy`.
4. An engine missing from the registry's instructions table is shown as "Unknown", and nothing is written for it.

---

### 2. F-DB-04 Review queue

**What it does.** One queue across all engines of the agents whose work is
ready to review: done, with changes in their folder or worktree. Today that
lives in the reviewer's head. Each item shows:
- the agent's closing message;
- a diffstat;
- the per-turn diff from checkpoints (F-EX-01);
- the result of the repository's check command, if one is set;
- a **risk** label from local rules, with no AI involved. The rules count files changed, and flag changes to migrations, CI files, lockfiles, auth paths and public API files (paths from a repository setting), and deleted tests.

The queue sorts by risk or by finish time. The actions are:
- **Approve:** stamps the item with a hash of the diff. If the agent changes the diff later, the approval goes stale and the item returns.
- **Send back:** the F-WT-02 review bar.
- **Second opinion:** F-WT-05.
- **Merge:** F-CD-15, or the queue in F-WT-07.
- **Snooze.**

Approvals stay inline.

**Who.** Anyone running three or more agents. Plan: Free. The queue is the core loop, so it shouldn't be gated.

**Evidence.**
- "AI coding agents generate more code, but not more software": HN 50059523, 12 Oct 2026, 364 points / 309 comments. Commenters agree review is the bottleneck.
- "Ask HN: How do you do PR reviews now?": HN 49195510, 6 Aug 2026. Teams sort by blast radius and ask for a summary with each change.
- Several agents finishing at once, all needing review: HN 49049986, 25 Jul 2026.
- A small review-inbox launch whose commenters asked for approvals that go stale when the diff changes: HN 49995778, 7 Oct 2026.
- A multi-agent TUI whose author wants a full diff of what each agent changed one key away: HN 49107749, 30 Jul 2026, 98 points / 80 comments.
- Vendors ship "needs input / ready for review" views, but each only for its own agent.
- Sources: §6, E-DB-04.

**Impact 5, effort 3.** The status bus, checkpoints, diff view and review bar exist. What's new is the queue model, the risk rules, diff-hash approval and the view.

**Risks and conflicts.**
- Risk labels are heuristics and could be read as a verdict. Label them as "what changed", never "safe", and show the rule that fired.
- No conflicts: it's all local.

**Draft acceptance criteria.**
1. An agent pane that finishes with uncommitted changes in its worktree appears in the queue within 1 s of its done status. One that finishes with no changes doesn't.
2. A change that touches `db/migrations/0042.sql` and deletes a test file is labelled with both rules. A change to `README.md` alone has no risk label.
3. Approve, then the agent edits a file: the item comes back as "Changed since you approved".
4. Send back opens the review bar for that worktree. Merge opens F-CD-15's confirm with the overlap forecast.
5. The queue shows empty ("Nothing to review"), loading, error and populated states. It lists items from terminals, threads and teammate chats.

---

### 3. F-EN-08 Engine versions and updates under your control

**What it does.**
- **The table.** Settings › Engines lists each engine's installed version, how it was installed (npm, Homebrew, a native installer or the engine's own updater), and whether the engine updates itself.
- **Checking.** **Check for updates** compares against the package source. It runs only when clicked, or daily if the user turns that on, because it contacts the registry.
- **Pin.** Sets the engine's documented "don't self-update" switch in its launch environment, inside its provider home.
- **Roll back.** Reinstalls the previous version through the same package manager, after a confirm that shows the command.
- **Last good version.** Vibezeo records the version that last ran a session without adapter errors. When a thread adapter or status hook starts failing right after an update, a banner names both versions and offers the rollback.
- **Background daemons.** Processes that engines start on their own are listed, each with Stop.

**Who.** Everyone. Engine updates are the main way wrappers break. Plan: Free.

**Evidence.**
- Codex CLI's persistent, self-updating background daemon: HN 50060694, 12 Oct 2026, 606 points / 292 comments. One user found 12 GB of old versions.
- Codex #48195 "make `daemon_auto_start` opt-in": 179 👍, opened 25 Sep 2026.
- Engine updates breaking hosts and wrappers recur on rivals' trackers (old analysis §2, pattern 7). A major editor's tracker asks to auto-pick the latest engine SDK.
- Sources: §6, E-EN-08.

**Impact 4, effort 2.**

**Risks and conflicts.**
- The update check sends the engine's package name to a public registry, so it is off unless the user turns it on (rule: nothing leaves without opt-in).
- The self-update switches differ by engine and change over time. They go in `registry.json` and are checked before the build **[unverified per engine]**.
- Rollback runs a package-manager command, so it must be shown and confirmed, like F-EN-03's install.

**Draft acceptance criteria.**
1. With a fake engine at 1.2.0 installed via npm, the table shows "1.2.0 · npm", and makes no network request until Check for updates is clicked.
2. Pin sets the registry's pin variable in the launch environment of new panes for that engine. Existing panes are unchanged.
3. After the fake engine moves from 1.2.0 to 1.3.0 and its adapter then reports a parse error, the banner reads "Codex updated from 1.2.0 to 1.3.0, and threads have failed since." It offers "Roll back to 1.2.0…", which shows the command before running it.
4. A process the engine left running outside any pane is listed under "Running in the background" with Stop, and Stop asks first.

---

### 4. F-AC-08 Usage forecast

**What it does.** The usage chip (F-AC-04) already shows how full a window
is. This adds:
- **Run-out forecast.** From the last hour's burn rate, it says when the window will run out: "At this pace, Claude's 5-hour window runs out at 3:40 PM, 1 h 30 min before it resets."
- **Weekly view.** "Weekly: on track", or "out by Thursday".
- **Per-session cost.** Using the local logs F-AC-07 already parses, it shows which running session is spending the window ("fix-auth: 61% of the last hour").
- **Notifications.** An opt-in notification fires at a forecast run-out, not only at 80%.
- **Hints.** In the launch sheet, a hint names an account or engine that has room. It never switches accounts.

**Who.** Subscribers on capped plans: that's nearly everyone. Plan: Free.

**Evidence.**
- Claude Code #16157 "instantly hitting usage limits with Max": 695 👍. #38335: 476 👍.
- Codex #24182 "show 5-hour and weekly limits persistently": 19 👍.
- OpenCode #38281 (a quota plugin): 63 👍.
- Plans keep tightening and moving to weekly pools:
  - a reported cut to one vendor's $200 tier from 29 Oct 2026 **[unverified, secondary]**;
  - an AI editor's move to weekly pools on 12 Oct 2026;
  - HN 49348751, 18 Aug 2026, 295 points / 267 comments.
- "Ask HN: How do you handle hitting usage limits?": HN 49904074, 30 Sep 2026.
- Three rival workspaces shipped usage meters in Sep–Oct 2026, but none forecasts a run-out.
- Sources: §6, E-AC-08.

**Impact 4, effort 2.** It is arithmetic over data already polled and parsed.

**Risks and conflicts.**
- A linear forecast can mislead after a burst. Show the window it used ("based on the last 60 min") and hide the forecast with under 10 minutes of data.
- Hints must never become automatic switching (F-AC-05).

**Draft acceptance criteria.**
1. A fixture where a window goes from 40% to 60% in 30 minutes, with a reset in 2 h, forecasts run-out in 1 h and shows "runs out before it resets".
2. With under 10 minutes of samples, the forecast reads "Not enough data yet".
3. Two sessions on one account with logged tokens 3:1 show 75% and 25% of the last hour.
4. The forecast notification is off by default. When on, it fires once per window when the forecast run-out first falls before the reset.
5. The launch sheet's hint names another account with more room, and launching still uses the account the user picked.

---

### 5. F-TH-17 Session library across engines

**What it does.**
- **The library.** Lists every coding session on this computer, including ones started in a plain terminal outside Vibezeo. Vibezeo reads the engines' own local session stores read-only: Claude Code's `projects/*.jsonl`, Codex's `sessions/`, and OpenCode's storage, in every provider home and the default homes.
- **Listing.** Sessions are grouped by repository, with engine, account, date, title or first prompt, and size.
- **Open.** Opens a session as a read-only transcript.
- **Resume.** Starts the engine's own resume (`claude --resume <id>`, `codex resume <id>`) in the right folder and provider home.
- **Continue with another engine.** Starts a handoff (F-CD-16) with that transcript as the source.
- **Full-text search.** A local index (SQLite FTS5) behind a setting. The setting can be turned off, and the index deleted, in Settings › Privacy.

**Who.** Anyone who has lost a session, or switches engines. Plan:
- Free: listing and resume.
- Pro: full-text search across engines. This is the owner's call (§4).

**Evidence.**
- An AI editor's top agent issue is history for external agents: 260 reactions (closed).
- A terminal's tracker asks to resume CLI-agent conversations after restart.
- A major editor began continuing Codex chats started elsewhere and listing other CLIs' sessions (7 Oct 2026).
- Codex added `/import` from other engines (11 Aug 2026).
- A YC startup that converts and searches sessions across harnesses: HN 49743049, 17 Sep 2026, 68 points.
- Claude Code #43303 and #23800: resume losing history.
- One Mac-only rival shipped a searchable session vault with forkable checkpoints (14 Sep and 12 Oct 2026).
- Sources: §6, E-TH-17.

**Impact 4, effort 2.** Two parsers exist already (`transcript.rs`, `usage_log.rs`). OpenCode's format is new work.

**Risks and conflicts.**
- **OAuth tokens.** The reader must never open credential files: Claude's `.credentials.json` and Codex's `auth.json` are explicitly excluded. An invariant test enforces this, in the spirit of TL-06.
- **Prompt text on disk.** The index stores prompt text in a local database. That is storage, not logging, so it is allowed. It must never reach logs or telemetry, it can be turned off, and Delete all clears it.
- **Format changes.** Session formats aren't public APIs. A parse failure skips the file and counts it, as F-AC-07 does.

**Draft acceptance criteria.**
1. Fixture stores for Claude Code (two provider homes) and Codex list four sessions under their repositories with engine, account and date. A file that fails to parse is counted as skipped.
2. Resume on a Claude session opens an agent pane in the session's folder, running `claude --resume <id>` with that account's `CLAUDE_CONFIG_DIR`.
3. "Continue with another engine" opens F-CD-16's sheet with the transcript as the source.
4. With search on, a word from a fixture prompt finds its session. Turning search off deletes the index file.
5. No file named `.credentials.json` or `auth.json` is ever opened by the library: an invariant test with a tracing filesystem fake.

---

### 6. F-WT-05 Second opinion from another engine

**What it does.** **Ask for a second opinion…** works on any worktree's
diff, a thread's changes, or a checkpoint diff:
1. The user picks another engine and account.
2. Vibezeo starts it **read-only** in that folder: a Read only thread (F-TH-09), or the terminal engine's plan or read-only mode.
3. It sends a fixed review prompt with the diff and the original task.
4. It parses the answer's `file:line: comment` lines into the F-WT-02 review bar. The user edits or deletes them, then sends them to the original agent with one click.

This puts F-TK-04's independent review on every change, not just task runs.

**Who.** Developers who keep two subscriptions so the models check each other. Plan: Free. Free's Thread engines are Claude Code and Codex, the usual pair, and other engines work through the terminal path.

**Evidence.**
- Cross-model review is a common habit. Users keep two vendors' plans "because the models catch each other's misses" (HN 49348751, 18 Aug 2026). Two small launches are built around it (HN 49168002, HN 49595212).
- Line comments that feed the agent are requested on an AI editor's tracker (11 reactions) and shipped in one IDE's agent product.
- A survey reports one engine preferred for quality and another for cost and limits **[unverified]**.
- Sources: §6, E-WT-05.

**Impact 4, effort 2.** It reuses Read only threads, the review bar and F-TK-04's review prompt.

**Risks and conflicts.**
- The reviewer uses the second account's quota, so the sheet says which account it will use.
- A terminal engine without a read-only mode can't be offered as a reviewer, and the sheet says why.
- No rule conflicts: the user's own engines, on their own accounts.

**Draft acceptance criteria.**
1. On a worktree with changes, choosing Codex opens a Read only Codex thread in that worktree whose first message contains the diff and the template's headings.
2. A reply with `src/a.ts:42: handle expired tokens` and `b.ts:3: typo` adds two comments to the worktree's review bar, in file order.
3. Lines that don't match the format are kept in a "General" comment, not dropped.
4. An engine with no read-only mode isn't offered, with the reason shown.
5. The reviewer session can't write. An attempted edit raises an approval, and Read only denies it (F-TH-09).

---

### 7. F-WT-06 Worktree runtime: ports, env and Compose per worktree

**What it does.** F-WT-01 copies files and runs setup. Running five dev
servers still collides on ports, databases and Compose projects. This adds:
- **Ports.** `"ports": 10` in `.vibezeo/worktree.json` gives each worktree a stable index and a free block of 10 ports.
- **Env.** `"env": { "DATABASE_URL": "postgres://localhost/app_{{slug}}" }` sets variables, with `{{slug}}`, `{{index}}`, `{{port}}` and `{{port+N}}` filled in.
- **Every pane gets them.** Each pane in the worktree's tabs gets these variables, plus `VIBEZEO_WORKTREE`, `VIBEZEO_PORT` and `COMPOSE_PROJECT_NAME=<repo>-<slug>`.
- **Ports panel.** It groups by worktree (F-EX-03). Open uses `http://<slug>.localhost:<port>`, so browser tabs and cookies stay apart.
- **Teardown.** The trusted teardown (F-WT-01) gets the same variables, so `docker compose down` removes the right stack.

**Who.** Web and backend developers running several agents with dev servers. Plan: Free.

**Evidence.**
- "Git worktrees are not an isolation boundary for coding agents": HN 49110389, 30 Jul 2026, 32 points / 35 comments. Its top comments name ports, databases, `.env` and Compose names.
- "Setup Script Should Support Git Worktrees": HN 49085449, 28 Jul 2026. The author lists ports, databases, cache keys, queues and Compose resources.
- Per-branch hostnames because a frontend talked to the wrong worktree's backend: HN 49063648, 27 Jul 2026.
- A throwaway database per agent: HN 49100518, 29 Jul 2026.
- A TUI rival's top issue asks for worktree setup with port isolation (8 reactions).
- One Mac rival shipped auto port forwarding on 27 Aug 2026. A major editor added symlinked `node_modules` for worktrees on 30 Sep 2026.
- Sources: §6, E-WT-06.

**Impact 4, effort 2.** It extends an existing file, an existing environment injection (F-EN-04) and an existing panel.

**Risks and conflicts.**
- A port block can be taken by another program after it's assigned. Check it at worktree creation and on pane launch, and say when a port is busy.
- `*.localhost` resolves to loopback in current browsers and the built-in browser pane. Test it in the webview.
- No rule conflicts. Env values in the file are committed, so the panel warns if a value looks like a secret (the F-TL-02 redactor's patterns).

**Draft acceptance criteria.**
1. With `"ports": 10`, two new worktrees get disjoint blocks, and shells in each see different `VIBEZEO_PORT` values. The blocks survive a relaunch.
2. `"env": {"DB": "app_{{slug}}"}` gives a shell in the `fix-auth` worktree `DB=app_fix-auth` and `COMPOSE_PROJECT_NAME=<repo>-fix-auth`.
3. A port in the block already in use when the worktree is created moves the worktree to the next free block, and says so.
4. The ports panel groups a server started in a worktree pane under that worktree. Open loads `http://fix-auth.localhost:<port>`.
5. Teardown runs with the same variables. A teardown printing `$COMPOSE_PROJECT_NAME` prints the worktree's name.
6. An env value matching a secret pattern shows "This looks like a secret. Don't commit it in worktree.json."

---

### 8. F-AC-09 Runaway guard

**What it does.** Limits per agent, set per workspace, that **notify by
default**. The user can choose to **pause** instead. The limits:
- time spent working without a turn ending (default 90 min);
- share of an account's window used by one session (from F-AC-08's per-session cost);
- subagents spawned in a session (from status hooks where the engine reports them, e.g. Claude Code's subagent events);
- the same output repeated in a loop (identical turn endings three times).

What pause means:
- a thread pause interrupts the turn;
- a terminal pause sends the engine's interrupt key and, if that fails, stops the process group, which Resume continues (SIGSTOP/SIGCONT on Linux and macOS; Windows only notifies).

**Who.** Anyone who leaves agents running; teams with spend caps. Plan: Free.

**Evidence.**
- Claude Code #75314: ten background tasks stuck for 34+ hours, about 1M tokens.
- "Codex agents went rogue, $78,000": HN 49861047, 26 Sep 2026. It claimed 826 child agents **[unverified: flagged post, fresh account]**. The thread also has a separate report of 437 subagents in a review loop.
- A single forum report of "zombie subagents" in an AI editor, 30 Sep 2026 **[unverified]**.
- Large employers reportedly capping employees' AI coding spend: HN 49997161, 7 Oct 2026, 377 points **[unverified]**.
- Sources: §6, E-AC-09.

**Impact 4, effort 2.** It reads the status bus and usage data.

**Risks and conflicts.**
- Stopping a process mid-write could leave files half-written. Pause is opt-in, checkpoints (F-EX-01) make it reversible, and the default is notify.
- Never switches accounts or engines.

**Draft acceptance criteria.**
1. A fake engine working for 91 minutes with the default limit produces one notification "fix-auth has been working for 90 min" with Pause and Dismiss, and nothing is paused.
2. With "Pause" chosen and a subagent limit of 20, the 21st subagent event pauses the session, and Resume continues it.
3. Three identical turn endings in a row raise "fix-auth may be looping".
4. Pausing a terminal agent sends the interrupt first. Only if the engine is still working 5 s later is the process group stopped, and Resume continues it.

---

### 9. F-EN-09 Engine catalog refresh

**What it does.**
- **Antigravity CLI.** Adds it as a terminal engine, alongside Gemini CLI for API-key users. Google stopped serving consumer Gemini CLI users on 18 Jun 2026.
- **Kiro CLI.** Adds it as a terminal engine.
- **Re-checks** Grok's binary and flags, and Muse Code's availability on Linux.
- **Status hooks** are added where each engine documents them.

Old analysis row 9 is not built yet: `registry.json` has 11 engines and no Antigravity.

**Who.** Users of those engines. Plan: Free.

**Evidence.**
- Google stopped consumer Gemini CLI on 18 Jun 2026 in favour of Antigravity CLI (Google's developer blog).
- Kiro CLI 2.31 shipped on 13 Oct 2026.
- Meta's Muse Code ships for macOS and Linux (5 Aug 2026) **[secondary]**.
- A rival terminal added Kiro CLI, grok and Antigravity to its agent toolbar between July and October 2026.
- Rivals' trackers ask for each new harness within days of its launch: Antigravity (15), pi (30), DeepSeek (31).
- A TUI rival now reads model lists from the installed CLIs instead of hard-coding them.
- Sources: §6, E-EN-09.

**Impact 3, effort 1.**

**Risks and conflicts.**
- Antigravity's terms: a thread about third-party automation getting Google accounts suspended (HN 49548452, 3 Sep 2026, 338 points). Host it as a terminal only, unmodified and signed in by itself. Don't drive it headless from Thread mode until the owner has checked its terms (§4).
- Verify each binary name, install command and provider-home variable on a real install **[unverified]**.

**Draft acceptance criteria.**
1. With `agy` and `kiro-cli` on the login PATH (or the verified binary names), both are detected and launch in panes with their own provider homes.
2. Gemini CLI stays listed, with a note that it needs an API key or enterprise access.
3. Neither appears as a Thread engine.
4. An engine whose CLI lists its models has its model picker filled from that list, and from the registry only when the list fails.

---

### 10. F-SH-14 Keep agents running when the window closes

**What it does.**
- **On quit with agents working,** the user can keep them running: "Keep 3 agents running in the background?". The PTYs move to a local background holder. That can be `tandem-host` running locally, since it already keeps sessions alive across SSH drops (F-RM-01). It runs as a systemd user service on Linux and a LaunchAgent on macOS, and survives the window closing but not a reboot.
- **Tray icon.** It shows the running agents and their states. OS notifications for waiting and done still fire.
- **Reopening** the app reattaches every pane with its scrollback.
- **Approvals** need the window or the phone. When the window is closed, an approval waits and notifies.

**Who.** Everyone running long jobs. Plan: Free. This is reliability, and the product principle is "restore after crash". Background *routines* stay Pro (F-RT-03).

**Evidence.**
- A multiplexer's comparison page says desktop managers stop agents when the app quits, while multiplexers survive.
- Three rivals keep sessions in a daemon or host process: one desktop workspace, one remote-first client (21k stars), and a major editor's agent host (30 Sep 2026).
- Requests on rivals' trackers:
  - processes kept alive across relaunch (26 reactions);
  - a headless server with a desktop client (5);
  - detached background sessions in a major editor (23).
- Today Vibezeo restores agents as paused cards (F-SH-07), so a quit ends their turns.
- Sources: §6, E-SH-14.

**Impact 5, effort 4.**

**What changes in the architecture.**
- PTY ownership moves out of the app process. `tandem-pty` sessions are served by the holder over a local socket, which only the user can open (0600, peer-credential check).
- The app becomes a client of the holder. The SSH path already works this way.

**Risks and conflicts.**
- An agent still running while the user thinks Vibezeo is closed. Keep the tray icon visible, make the choice explicit and per quit (with a remembered default), and hold every approval.
- No rule conflicts. Nothing leaves the machine.

**Draft acceptance criteria.**
1. Quitting with two working agents asks "Keep 2 agents running in the background?". Choosing Keep leaves both processes running after the window process exits.
2. Reopening reattaches both panes, and the output written while closed is shown in order.
3. While closed, an agent reaching an approval produces an OS notification, and the approval is not answered.
4. Choosing "Stop them" ends both, as quit does today.
5. Only the user's own processes can connect to the holder's socket. Another user's connection is refused (a test with a second uid where CI allows, otherwise a peer-credential unit test).

---

### 11. F-TL-11 Agent sandbox (Linux first)

**What it does.** Agent panes and threads can run in an engine-agnostic sandbox, with three policies: Off, Workspace or Strict. On Linux it uses bubblewrap where installed, and Landlock otherwise.
- **Write access** is limited to the workspace or worktree, the engine's provider home and a private temp folder.
- **Read access is denied** to:
  - `~/.ssh`, `~/.aws`, `~/.config/gh`;
  - the keyrings;
  - the other provider homes;
  - other worktrees.
- **Git internals.** `.git/hooks` and `.git/config` are read-only, so an agent can't plant a hook that later runs outside the sandbox.
- **Network:** on, or off (Strict).
- **Denials.** A denied write shows in the pane's header with the path.

**Who.** Everyone who uses an engine's "auto" or full-access mode; teams. Plan: Free. Team admins can require a policy (F-BL-06 policy).

**Evidence.**
- "Breaking Claude Code Opus 5 Auto Mode": HN 49506819, 31 Aug 2026, 399 points. Auto mode is not a sandbox.
- Auto mode made the default: HN 49239021, 10 Aug 2026, 292 points / 313 comments.
- An agent writing `.git/hooks` that later run outside its sandbox: comments on HN 49110389.
- Three rivals shipped local command sandboxes between 7 and 13 Oct 2026: a major editor (all three OSes), an AWS IDE, and a vendor CLI (GA).
- OpenCode #2242 "sandboxing": 79 👍.
- Sources: §6, E-TL-11.

**Impact 5, effort 4.**

**What changes in the architecture.**
- F-EN-04's launch gains a sandbox wrapper per OS, behind a trait, with fakes for tests.
- macOS would use a Seatbelt profile **[verify feasibility]**. Windows has no equivalent yet. This conflicts with the "same features on every OS" principle, so the owner decides whether Linux-first is acceptable (§4).

**Risks and conflicts.**
- Engines and tools break in surprising ways (Docker's socket, language servers, browsers). Start with Workspace mode, and give a clear "Allow this path" fix per denial.
- Must keep the provider home writable, or the engine's own sign-in refresh fails. Vibezeo still never reads the tokens.

**Draft acceptance criteria.**
1. In Workspace mode on Linux, an agent pane can create a file in its worktree and can't create one in `$HOME`. The pane header shows the denial with the path.
2. Reading `~/.ssh/id_ed25519` from a sandboxed pane fails. So does reading another account's provider home.
3. Writing `.git/hooks/pre-commit` from a sandboxed worktree pane fails.
4. With Strict, `curl https://example.com` fails, and the engine's own process can still start. Engines that need the network for inference are flagged "needs network": Strict is offered only for local-model engines (F-EN-11).
5. Without bubblewrap and without Landlock, the setting says "Sandbox unavailable on this system" and launches unsandboxed only after the user confirms.

---

### 12. F-WT-07 Local merge queue

**What it does.** **Land worktrees…** takes the worktrees ready to merge,
ordered by the overlap forecast (F-WT-03) with the fewest conflicts first. For
each one in turn, it:
1. merges the current base into the worktree;
2. runs the repository's check command in that worktree;
3. merges into the base with `--no-ff` if the check passes;
4. stops at the first conflict or failure, offering "Send the failure to the agent".

It never pushes. The base's state before the run is recorded, and **Undo
landing** resets the base branch to it, after a confirm.

**Who.** Developers landing many agent branches a day without paying for CI on each. Plan: Free.

**Evidence.**
- A local merge queue for agent branches: HN 49104747, 30 Jul 2026, 42 points / 22 comments. 90 commits a day, a local test suite, rollback.
- Users of one engine have a "merge-queue thread" land the others one at a time (comments on that post).
- A rival's tracker asks for local merge without a PR.
- Sources: §6, E-WT-07.

**Impact 4, effort 3.**

**Risks and conflicts.**
- Undo rewrites the base branch's ref, so only the local branch, never a pushed one. Refuse when the base has been pushed since the run.
- Merges stay `--no-ff`, as in F-CD-15.
- No rule conflicts.

**Draft acceptance criteria.**
1. Three worktrees with passing checks land in forecast order. The base has three merge commits and no push happened.
2. A worktree whose check fails stops the queue at it. The later ones are untouched, and the failure's output tail is shown with "Send to agent".
3. A conflict stops the queue with the base unchanged for that worktree (the merge is aborted).
4. Undo landing restores the base branch to the recorded commit after a confirm. It's refused when `origin/<base>` contains a landed merge.

---

### 13. F-WT-08 CI results and PR comments back to the agent

**What it does.** For a worktree with an open PR or MR, read through the
user's own `gh` or `glab` (which keep their own tokens), the lane shows its
checks.
- **Failed checks.** When one fails, **Send failure to agent** types the failing job's log tail into the worktree's agent, fenced as untrusted content, with the job name.
- **Review comments.** **Import review comments** pulls the PR's review comments into the F-WT-02 review bar.
- **Polling** happens every 2 minutes while the lane is open and the app is focused, and on Refresh.

**Who.** Teams on GitHub or GitLab. Plan: Free.

**Evidence.**
- An engineering blog on agents supervising agents (31 Aug 2026): CI logs and bot comments are untrusted input; keep a human on merge.
- Vendors and an AI editor ship cloud CI auto-fix and PR review bots (Sep 2026).
- A terminal's tracker asks for full git staging and commit in its changes panel (44 reactions).
- Sources: §6, E-WT-08.

**Impact 4, effort 3.**

**Risks and conflicts.**
- **Prompt injection** from CI logs and bot comments. Fence them as untrusted (as F-TK-04 does with comments). Never send them automatically: always a click.
- Vibezeo never touches gh's token. It only runs `gh`, and the CLI authenticates itself.
- Requests go to GitHub or GitLab only because the user already uses `gh`/`glab` for this repository. The first use still asks once per repository, because it's a new network action by Vibezeo.

**Draft acceptance criteria.**
1. With a fake `gh` returning one failed check, the lane shows "1 check failed: test". Send failure to agent types a prompt containing the job name and the log tail between untrusted fences.
2. Import review comments adds each PR line comment to the review bar with its file and line.
3. No `gh` call is made for a repository before the user allows "Check pull requests for this repository".
4. Nothing is ever sent to the agent without a click.

---

### 14. F-EN-10 Any ACP agent as a thread engine

**What it does.** **Settings › Engines › Add an ACP agent:** the user gives a
label, command, arguments and environment. Vibezeo runs `initialize` to test
it, then offers it as a Thread engine. Built-in entries cover engines that
document an ACP mode **[verify each]**: Gemini CLI, Goose and Kiro.
Antigravity CLI gets an entry once its ACP mode ships.

**Who.** Users of agents beyond the three built-in ACP engines. Plan: Pro (Free's Thread engines stay Claude Code and Codex, §3.2).

**Evidence.**
- Antigravity CLI #31 "Add ACP stdio JSON-RPC mode": 405 👍, that tracker's top request, plus #195.
- Goose #12255, launching external ACP clients.
- An IDE's agent product and an editor's agent registry already run any ACP agent.
- Sources: §6, E-EN-10.

**Impact 3, effort 2.** The ACP adapter (F-TH-06) exists. This adds config and a test.

**Risks and conflicts.** ACP shows only what each adapter exposes (COMPETITIVE_ANALYSIS §3). Label these threads "via ACP".

**Draft acceptance criteria.**
1. Adding the fake engine's ACP mode by command passes the test and appears in the Thread engine picker.
2. A command that doesn't answer `initialize` within 10 s shows "Not an ACP agent", and nothing is saved.
3. On Free, the added engine shows the plan's limit, as other non-included engines do.

---

### 15. F-EN-11 Local models as engines

**What it does.**
- **Detection.** Finds local model servers listening on localhost: Ollama, llama.cpp's server and LM Studio, on their default ports or ones the user adds.
- **Models.** Lists their models.
- **Launch.** Offers "Launch with a local model", which runs an engine that supports a local provider, starting with OpenCode and Codex's open-model mode **[verify flags]**. The engine is configured for that server in its own provider home.
- **Badge.** These panes carry an "On this computer" badge, and are the only ones offered Strict sandboxing (F-TL-11).

**Who.** Privacy-bound developers, people over their limits, and r/LocalLLaMA readers. Plan: Free.

**Evidence.**
- A 30B model built for always-on local agents: HN 49241679, 10 Aug 2026, 1,209 points / 638 comments.
- OpenCode with Ollama setup pain: HN 49652122, 11 Sep 2026.
- An offline agent: HN 49245437, 169 points.
- A vendor CLI added local-model discovery on 7 Oct 2026. Its tracker asks to switch to local models within one session (34 👍).
- Sources: §6, E-EN-11.

**Impact 3, effort 2.**

**Risks and conflicts.**
- Tool-calling quality varies by model. Show the model's name and size, and promise nothing.
- Probing localhost ports sends nothing off the machine.

**Draft acceptance criteria.**
1. A fake server answering Ollama's model-list endpoint on 127.0.0.1 is detected, and its models are listed.
2. Launch writes the engine's local-provider config only inside a dedicated provider home, and the pane shows "On this computer".
3. No probe targets a non-loopback address.

---

### 16. F-TK-05 Claims: agents say what they're about to change

**What it does.**
- **New MCP tools.** `tandem-tasks` gains `claim(paths, intent)`, `release(paths)` and `list_claims()`.
- **Briefing.** A lane's briefing (F-CD-15) asks the agent to claim before editing, and to check others' claims.
- **Inferred claims.** Status hooks (PreToolUse edit paths) and checkpoints add claims for agents that don't call the tool.
- **Overlaps.** When two agents' claims overlap, both panes and the Review queue get a notice: "add-oauth and fix-auth both claim src/auth/session.ts". The agent that claimed second sees it in its next `list_claims`.

**Who.** Three or more agents in one repository. Plan: Free.

**Evidence.**
- A launch whose point is that the worst conflicts are intent conflicts git doesn't flag: HN 49789356, 21 Sep 2026, 45 points. A commenter is sceptical that scope is known in advance.
- One engine's first-party cross-session messaging: HN 49222824, 8 Aug 2026, 173 points. The thread asks whether it works across harnesses; it doesn't.
- An AI editor built its own VCS for agent swarms: HN 48982535, 20 Jul 2026, 278 points.
- Sources: §6, E-TK-05.

**Impact 3, effort 3.** It's an honest 3: agents may ignore the instruction, so the inferred claims carry most of the value.

**Risks and conflicts.** Agents could read the claim intents as instructions (prompt injection between agents). Return them as data, fenced, as task comments are.

**Draft acceptance criteria.**
1. Two agents claiming overlapping paths produce one overlap notice naming both and the paths.
2. An agent that edits `a.ts` without claiming gets an inferred claim from its PreToolUse event.
3. Claims are released when the worktree is merged or removed.
4. `list_claims` returns other agents' intents inside untrusted fences.

---

### 17. F-EX-06 Resource governor (Linux first)

**What it does.**
- **Cgroups.** Each agent pane's process tree runs in its own cgroup: a systemd user scope (`systemd-run --user --scope`).
- **Ports and Dashboard cards** show each pane's CPU and memory, grouped by worktree.
- **Memory cap.** An optional cap per agent, with the OOM event shown in the pane.
- **Freeze** for a pane, and **Thaw**.
- **One check at a time.** A setting runs check commands (Best of N, merge queue, review queue) one after another instead of all at once.

**Who.** Laptop users running 4+ agents with builds. Plan: Free.

**Evidence.**
- Four or five agents building and testing at once force-quit an 8 GB machine: HN 49104747, 30 Jul 2026.
- Running 3–4 local database stacks makes a laptop overheat: HN 49100518, 29 Jul 2026.
- Sources: §6, E-EX-06.

**Impact 3, effort 3.**

**What changes in the architecture.** The pane launch goes through a platform trait that can place a process tree in a resource group. On macOS and Windows, the first version only measures.

**Risks and conflicts.** Systems without systemd user sessions. Measure from `/proc` there and say capping is unavailable.

**Draft acceptance criteria.**
1. On Linux with systemd, a pane's processes are in a scope named after the pane, and its CPU and memory are shown within 3 s.
2. A 512 MB cap on a pane that allocates 1 GB ends that process, and the pane shows "Stopped: memory cap 512 MB". Other panes keep running.
3. Freeze stops the scope's processes and Thaw resumes them.
4. With "one check at a time", three Best of N checks run one after another.

---

### 18. F-CD-22 Multi-repo workspaces

**What it does.** A workspace can hold several repositories:
- **Worktrees.** One agent launch makes a worktree with the same `tandem/<slug>` branch in each repository, with a parent folder holding them all.
- **Diff.** One diff view per repository.
- **PRs.** "Create pull requests" opens one per repository, each linking the others.

**Who.** Microservice and frontend/backend teams. Plan: Pro.

**Evidence.**
- The largest free rival's #2 request is multi-repo diffs: 43/71 reactions (15 Oct 2026).
- A major editor's tracker has cross-repo tracking with worktrees (34), and it shipped experimental multi-folder sessions on 30 Sep 2026.
- Two rivals shipped multi-repo work in Sep 2026, and two more trackers ask for it.
- Sources: §6, E-CD-22.

**Impact 4, effort 4.** Impact was 3 on 10 October; it's 4 now because more evidence has come in.

**What changes in the architecture.** A workspace today is one folder. The workspace model would gain a list of repositories, and every git feature would loop over them.

**Draft acceptance criteria.**
1. A two-repository workspace launch creates `tandem/<slug>` in both and one parent folder the agent starts in.
2. The diff view lists changes per repository.
3. "Create pull requests" runs `gh pr create` in each, and each body links the others.

---

### 19. F-CD-23 Code intelligence in the editor and diffs

**What it does.** The Files editor (F-CD-19) and the diff views (F-WT-02,
the review queue) gain:
- go to definition;
- find references;
- hover types;
- diagnostics.

They come from the language servers the user already has installed
(`rust-analyzer`, `typescript-language-server`, `pyright` and others),
started per workspace or worktree. The reviewer can follow a changed function
to its callers without leaving the diff.

**Who.** Reviewers of agent changes. Plan: Free.

**Evidence.**
- Codex #8745 "LSP integration": 503 👍, the top request on that tracker.
- The largest free rival's tracker asks for LSP (39), and another rival shipped editor language intelligence on 10 Oct 2026.
- Sources: §6, E-CD-23.

**Impact 3, effort 3.**

**Risks and conflicts.** Language servers can be heavy. Start them on first
use per language and stop them when idle. They run locally.

**Draft acceptance criteria.**
1. With `rust-analyzer` on PATH, go to definition on a symbol in a Rust file opens its definition.
2. A diff line's symbol offers "Find references" across the worktree.
3. Without a server for the language, the editor works as today, with "No language server for Rust" in its status line.

---

### 20. F-EX-07 Activity ledger

**What it does.** Per agent session, a local timeline of everything that
happened:
- prompts sent (as references, without the text unless the user opens the transcript);
- tool calls from hooks and adapters;
- commands run (F-EX-02);
- files changed (F-EX-01);
- approvals and who answered them;
- subagents;
- the network connections the process tree opened (on Linux, sampled from `/proc`).

Each event links to its checkpoint. **Export** writes JSONL. On Team, the user
can choose to attach an export to the audit log (F-TL-10).

**Who.** Anyone who needs to know "what did it do?" after an incident; teams. Plan: Free; Team export.

**Evidence.**
- The ask comes up mainly after incidents:
  - HN 49861047, where the poster's records were gone **[unverified post]**;
  - a commenter building "session scanners", HN 49565799 **[thin]**.
- Sources: §6, E-EX-07.

**Impact 3, effort 3.** Most of the data exists. This joins it.

**Risks and conflicts.**
- The ledger must follow the logging rule: no prompt text, file contents or tokens. Store references and redacted commands only.
- Network sampling is local.

**Draft acceptance criteria.**
1. A session that runs two commands, edits a file and gets one approval shows those four events in order, each linked to its checkpoint or command.
2. The ledger's database has no prompt text: an invariant test with a known sentence.
3. Export writes valid JSONL with the same events.

---

### 21. F-CD-24 jj (Jujutsu) workspaces

**What it does.** In a repository co-located with jj, a lane can be a
`jj workspace` instead of a git worktree, and the diff, overlap and merge
features run through jj.

**Who.** jj users. Plan: Free.

**Evidence.**
- The largest free rival's top issue is jj workspaces (46 reactions), and two other trackers ask too (8 and 2).
- jj workspaces came up as an alternative in the merge-queue thread, HN 49104747.
- Sources: §6, E-CD-24.

**Impact 3, effort 4.** Every git path (F-CD-15, F-WT-01…04, F-EX-01's
store stays git) needs a jj variant behind the existing git trait.

**Draft acceptance criteria.**
1. In a co-located jj repository, New worktree runs `jj workspace add` and the lane works like a git lane.
2. Diff vs base and the overlap check give the same results as for a git lane on the same changes.

---

## 2. Recommended build order: the top 10 in eight slices

Each slice fits about one session-day, apart from slice 8, which needs two.
Each ends on the gate with its E2E spec, BUILD_LOG and CHANGELOG. The order
puts the cheap, broad wins first, then the review loop, then the large
reliability work.

| Slice | Features | Why together | Days |
|---|---|---|---|
| 1 | #1 F-EN-07 instructions, #9 F-EN-09 engine catalog | Both are registry tables plus small UI, in `tandem-engines` | 1 |
| 2 | #3 F-EN-08 engine versions | Settings › Engines, launch environment, one banner | 1 |
| 3 | #4 F-AC-08 forecast, #8 F-AC-09 runaway guard | Both read the usage polling and status bus. The guard uses the forecast's per-session cost | 1 |
| 4 | #5 F-TH-17 session library | Parsers exist; adds OpenCode's format, resume and the index | 1 |
| 5 | #7 F-WT-06 worktree runtime | Extends `worktree.json`, launch environment and the ports panel | 1 |
| 6 | #6 F-WT-05 second opinion | Read only threads and the review bar exist | 1 |
| 7 | #2 F-DB-04 review queue | Needs slice 6's button and F-EX-01's per-turn diff | 1–1.5 |
| 8 | #10 F-SH-14 keep agents running | The holder process and reattach. 8a moves PTYs into the holder; 8b adds the quit dialog, tray and reattach | 2 |

Next after these: F-TL-11 sandbox (two days, Linux), then F-WT-07 merge
queue and F-WT-08 CI feedback (one slice each). Together with the review
queue, they complete the loop from agents finishing to work landing.

---

## 3. Bold bets

The owner asked for ambitious, original features, beyond the gaps. Nobody has
asked for these exact things yet. Each bet below says:
- the problem;
- why it's possible now;
- why Vibezeo is the one to do it;
- what the architecture would need;
- a first slice that could ship in a few days.

All of them keep the four rules.

### B1. The Night Shift: work that waits for your limits to reset

**Problem.** Capped subscriptions refill on 5-hour and weekly windows. A
developer's best hours burn the window, and the night's reset goes unused.
Today, "Resume at reset" (F-AC-05) only continues one stopped session.

**The bet.**
- **Queue.** Queue tasks for later: "Tonight: write tests for the billing module, upgrade the lint config, try the flaky-test fix three ways."
- **Schedule.** Vibezeo plans them across your own accounts' reset times, and you approve the plan.
- **Run.** Each runs in a sandboxed worktree with held actions.
- **Morning brief.** At 8 the phone shows "4 done, 1 stuck, 2 ready to review", opening on the review queue.

It never switches an account on its own. The plan names each task's account,
and you approve the plan.

**Why now.**
- Limits are tighter and change often (§1 #4 evidence).
- Agents can run for hours (Claude Code background sessions, Codex's daemon).
- People already run agents overnight: HN 49653369 "Those running agents 24/7".

**Why Vibezeo.** Routines (F-RT), usage polling across vendors (F-AC-04), held
drafts for unattended runs (F-PL-08), task execution (F-TK-04) and the phone
companion are all built. No vendor will schedule work onto a rival's
subscription.

**What changes.**
- A planner over the usage windows.
- Routines that wait for a reset instead of a clock time.
- F-SH-14's holder, so the window can be closed.

**First slice (3 days).** "Run at reset" on a task:
1. pick an account;
2. it starts as a background routine when that account's window resets;
3. the morning summary is a notification.

### B2. Previews per worktree: each agent gets its own local preview

**Problem.** Five agents, five branches, one `localhost:3000`. Checking any
agent's work means stopping a server, switching branches and waiting for a
rebuild.

**The bet.** Every worktree gets a running preview automatically:
- its own hostname (`fix-auth.localhost`);
- its own ports and Compose stack (F-WT-06);
- its own copy of the dev database, made in milliseconds as a copy-on-write clone (reflinks on btrfs or XFS, or a Postgres template database);
- dependencies cloned by reflink, not reinstalled.

The review queue shows each item's preview with its diff: one click to try
it, one to compare two agents' versions side by side in the browser pane.

**Why now.**
- `*.localhost` resolves to loopback in browsers.
- Reflink copies are standard on btrfs and XFS (Fedora's default is btrfs).
- `cp --reflink=auto` makes a 1 GB `node_modules` clone near-instant.

**Why Vibezeo.**
- Linux first: the filesystems that make this cheap are Linux's.
- The browser pane, ports panel, worktree setup and Best of N are built.
- Cloud agents can't preview your local stack, and vendor desktop apps don't run your dev servers.

**What changes.**
- A preview record per worktree (command, port, health check).
- Reflink-aware copying in F-WT-01, with a fallback to an ordinary copy.

**First slice (2–3 days).**
1. The worktree's `preview` command runs with F-WT-06's port.
2. The lane shows "Preview" when the port answers.
3. Compare in Best of N gets a browser tab per lane.

### B3. The Foreman: a local, cross-vendor lead for a whole goal

**Problem.** Running five agents means being their manager: splitting the
goal, briefing each one, checking in, reviewing and merging. Cloud products
now sell a coordinating agent:
- an AI editor's coordinator over parallel cloud agents (10 Sep 2026);
- a terminal's "foreman" over Claude Code and Codex workers (Aug 2026).

Sources are in COMPETITIVE_ANALYSIS §6, E-B3.

Both run in the vendor's cloud, on the vendor's billing.

**The bet.** Give Lead (the built-in teammate) a goal:
1. It writes a plan as tasks in `tandem-tasks`.
2. It proposes an engine and account for each, from their strengths and remaining quota.
3. After **you approve the plan**, it launches each in a worktree with claims (F-TK-05).
4. It answers routine questions from the repository's instructions and escalates the rest to you.
5. It asks a second engine to review each result (F-WT-05).
6. It lands passing work through the merge queue (F-WT-07).

You see one thread with Lead, and the review queue fills as work finishes.

**Why now.** Every vendor ships subagents and agent teams, but each inside its
own harness only. Cross-vendor coordination is open ground:
- HN 49222824 asks for cross-harness messaging;
- the terminal's foreman runs Claude Code and Codex, but in its own cloud at a markup.

**Why Vibezeo.** Teammates with memory and mail, tasks over MCP, approvals,
multi-account across vendors and worktrees are all local and built. It runs
on the user's own subscriptions with nothing hosted.

**What changes.**
- A planning mode for teammates that writes tasks.
- Task execution taking engine and account per task.
- A supervisor loop on the user's own engine. Today's Auto calls the hosted relay; a local mode needs no server.

**First slice (3 days).** "Plan with Lead": Lead turns a goal into tasks with
a suggested engine for each, and "Execute all" runs them through F-TK-04's
queue. Review and merge stay manual.

### B4. Walk-and-review: review the fleet by voice, on the phone

**Problem.** Review is the bottleneck (§1 #2), and it chains you to the desk.

**The bet.**
- **Walk the queue.** Put on headphones and say "Hey Conductor, what's ready?". Conductor walks the review queue: what each agent did, the risk labels, and the checks.
- **Act by voice:**
  - "Show me" puts the diff on the phone;
  - "Send it back: handle expired tokens too" goes to the agent as a review comment;
  - "Ask Codex for a second opinion";
  - "Approve".
- **Merges** still need the desktop's confirm (TL-06 rule 5), so the session ends with "3 approved, ready to merge when you're back".
- **A local mode** uses Dictate's local Whisper and a local text-to-speech voice, so nothing leaves the machine.

**Why now.**
- Local speech recognition and synthesis are good enough on a laptop CPU (Dictate ships Whisper today).
- Voice for agents is an open ask: Codex #3000, voice transcription, 241 👍; HN 49610888.

**Why Vibezeo.** Conductor (47 tools), Dictate, the phone companion with
end-to-end encryption, approvals with step-up, and the review queue. Nobody
else has all five.

**What changes.**
- Conductor tools over the review queue.
- A local voice pipeline beside the hosted one: local STT is built; local TTS is new.
- A diff view in the phone app.

**First slice (3 days).** Conductor tools `review_next`, `review_send_back`
and `review_approve`, plus the phone's read-only diff for the current item.
Hosted voice for now; local voice in a later slice.

### B5. Agent-proof secrets

**Problem.** Agents read `.env` files and paste keys into logs, transcripts
and commits. Secrets in worktrees are copied around by design (F-WT-01).
Several products now sell a credential layer:
- a credential gateway for agents: HN 49023427, 110 points, and HN 49363710;
- placeholder secrets inside microVMs: HN 49168002.

**The bet.**
- **References, not values.** `.env` holds a reference, `API_KEY=vibezeo-secret://stripe-test`, and the real value lives in the OS keychain.
- **Run.** `vibezeo run -- pnpm dev` (or the pane's run button) puts the value into that one process's environment when it starts.
- **What the agent sees.** Agents see the reference, never the value, in files, in their context and in the worktree copy.
- **Redaction.** The terminal redactor masks the value if a process prints it.
- **Honest limits.** An agent that runs a command under `vibezeo run` could still print the variable. The redactor and an approval for `vibezeo run` started by an agent are the mitigations. This narrows the leak; it doesn't close it.

**Why now.** Agents in auto mode by default (HN 49239021) plus worktree
copying make plaintext `.env` files a growing risk.

**Why Vibezeo.**
- The keychain-only rule is already in its DNA.
- The gateway already injects credentials into MCP calls without engines seeing them (F-PL-04).
- The redactor (F-TL-02) exists.

**What changes.**
- A small `vibezeo run` binary, the user's secret store UI and per-pane env injection.
- An approval rule for agent-initiated runs.

**First slice (2 days).** A Secrets page that stores named values in the
keychain; `vibezeo-secret://` references resolved for commands run from a
pane's Run button; redaction of those values in that pane.

### B6. Intent radar: conflicts before they're written

**Problem.** Git flags two agents editing the same line. It doesn't flag one
agent renaming a class while another extends it (HN 49789356).

**The bet.**
- **Watch symbols.** As agents edit, Vibezeo parses each worktree's changed files with tree-sitter and keeps a live map of the symbols each agent touched.
- **Warn early.** When two worktrees touch the same symbol (one renames `Session`, the other calls `Session.refresh`), both agents and the user get a warning while the work is still cheap to change, not at merge.

**Why now.** tree-sitter grammars cover every major language and parse in
milliseconds. Checkpoints already snapshot each turn.

**Why Vibezeo.** It runs every agent's worktree on one machine, with file
watching (notify), checkpoints, the overlap check and claims (F-TK-05) built.
Vendor tools see only their own agent.

**What changes.** A symbol index per worktree, updated from checkpoints.

**First slice (3 days).** Changed symbols per worktree for TypeScript, Rust
and Python, plus the overlap check (F-WT-03) listing shared symbols alongside
shared files.

### My pick: the three that would most surprise and delight

1. **Previews per worktree (B2).** The moment a developer sees five agents' versions of the app running side by side, each at its own `*.localhost` address, with its own data, and nothing typed to set it up, the tool feels magic. It turns "review the diff" into "try the result", which is what people actually want to judge. It uses Linux's strengths (reflinks), and no cloud or vendor app can do it on your own machine.
2. **The Night Shift (B1).** It turns a source of frustration, usage limits, into a feature: idle quota becomes finished work by morning, reviewed from the phone over coffee. Only a vendor-neutral, multi-account, local tool can do it, and it shows what Vibezeo is: your subscriptions, your machine, working while you sleep.
3. **Review queue + second opinion (§1 #2 and #6), with walk-and-review (B4) as its voice.** Every heavy user's real bottleneck is review. A queue that knows what changed, whose approvals go stale when the diff moves, and that asks a rival model to check the work, turns ten agents from a burden into leverage. Doing it by voice on a walk is the delight.

---

## 4. Owner-only items (not for agents to build)

| Item | Why it's yours | Evidence |
|---|---|---|
| **Re-check Anthropic's terms for Thread mode** (`claude -p`, stream-json) | Legal reading. On 7 Oct 2026 a support article says the Agent SDK, `claude -p` and third-party apps still use subscription limits. The legal page says hosting must run the **unmodified** binary and may not "intermediate" credentials. Vibezeo complies on both counts, but the two texts don't fully agree on "third-party apps" | support.claude.com/en/articles/15036540; code.claude.com/docs/en/legal-and-compliance |
| **Antigravity CLI's terms** before any headless use | Account-suspension fears | HN 49548452 (338 pts); antigravity.google/docs/cli/headless |
| **Don't join "Sign in with ChatGPT for third-party tools"** | Joining would make Vibezeo hold OpenAI OAuth tokens, which breaks the rule. Codex's own CLI already signs in by itself | Reported launch ≈29 Sep 2026 [secondary]: thenewstack.io/sign-in-with-chatgpt |
| **Free plan caps** (4 panes, 3 workspaces) | Pricing. The largest local rivals are free and uncapped, and one AI editor's new entry tier is $8 | COMPETITIVE_ANALYSIS §1.1, §1.2 |
| **Is full-text session search Pro?** (F-TH-17) | Pricing | — |
| **Linux-first for the sandbox and the resource governor** | Conflicts with "same features on every OS, same day" (PRODUCT_SPEC §2) | §1 #11, #17 |
| **Linux ARM64 builds** | Out of scope today (§6.11). Claude's and Codex's Linux desktops both ship arm64 | Claude Linux beta (basic-tutorials.com/news/claude-desktop-for-linux-anthropic-launches-the-official-beta); Codex 11 Aug changelog |
| **Positioning on neutrality** | A leading AI editor moved Claude and GPT to API price on 12 Oct 2026, after its sale to a model maker, and OpenAI's models leave its picker on 12 Nov 2026 [one source disputes the date]. "Any vendor, your own plan" is now timely. Site copy must name no competitor | COMPETITIVE_ANALYSIS §1.2 |
| **Community and launch** | The category is saturated with small tools: "every day I see a paid or OSS version of this". VC-backed tools draw distrust (a multiplexer's YC thread, HN 49201003, 281 pts). An open, local, no-account stance is an asset, so lead with it | HN 49023427, 49201003 |
