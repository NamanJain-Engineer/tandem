# Competitive analysis (October 2026, refreshed)

Internal document. Competitors are named here and only here. Never name them
on the site, in the app or in posts (CLAUDE.md "Never").

## How this was researched

**Dates.** First written on 10 October 2026 and refreshed on 15 October 2026.

**Sources.** Products' own changelogs, pricing pages and docs; GitHub issue
trackers; Hacker News threads, via the Algolia API; press coverage.

**Rules.**
- Reaction counts, star counts and prices were read on 15 October 2026 unless a row says otherwise.
- Every claim has a source.
- **[unverified]** marks a claim that rests on a single secondary source, or where sources disagree.
- Prices change often. Re-check the vendor's page before quoting a price anywhere.

**Gaps in this pass.**
- **Reddit:** reddit.com and its mirrors were blocked from the research environment, so this pass has no Reddit evidence.
- **X/Twitter:** search surfaced almost nothing.
- **Partial pages:** a few pages returned 403 or 404 and are marked where used.
- **GitHub counts:** issue reaction counts come from GitHub's search API, sorted by 👍 reactions. A highly reacted issue worded differently could be missing.

**Where the features are.** `docs/NEXT_FEATURES.md` turns this analysis into
ranked features and bold bets.

**Vibezeo, for reference.**
- **Build:** 0.7.2 for Linux x86-64, as a .deb and an .rpm (CHANGELOG).
- **Free plan:** no account, nothing sent to our servers.
- **Engines:** hosts 11 coding-agent CLIs on the user's own subscriptions, in terminal panes and git worktrees. Claude Code, Codex and ACP sessions can also be read as threads, with inline approvals.
- **Built since 10 October** (0.7.0): worktree setup, review comments to the agent, the overlap check and GitLab merge requests (F-WT-01…04).
- **Also built:** checkpoints, Best of N, the ports panel, teammates, tasks over MCP, plugins with approvals, Dictate, Conductor, the phone companion and SSH workspaces.

---

## 1. Landscape

### 1.1 Agent workspaces and orchestrators

| Product | What | Platforms | Price (15 Oct) | Signal (15 Oct) | Shipped mid-Aug to mid-Oct |
|---|---|---|---|---|---|
| **Orca** (stablyai/orca) | Cross-platform agent workspace: worktrees, usage meters, automations | macOS, Windows, Linux, iOS, Android | Free (MIT) | 92.9k stars, 4.2k open issues. The largest in the category; 42.9k stars in Aug 2026 [secondary] | Releases every 1–2 days. Rewindable chats, a managed server on SSH hosts, per-run model for automations, Antigravity quota meters (v1.4.220–224, 4–10 Oct) |
| Conductor | Closed-source workspaces for Claude Code, Codex and others | Local on macOS (Apple Silicon only per one review [unverified]); cloud on Linux; iOS | Free local; Pro $50/mo (cloud, multiplayer, mobile); Teams $60/user | No public tracker | Stacks (10 Aug); MCP (20 Aug); auto port forwarding (27 Aug); edit in diffs (2 Sep); routines (9 Sep); Sign in with ChatGPT (29 Sep); iOS app (2 Oct) |
| Superset | Electron, terminal-first, any CLI, worktrees, daemon | macOS; Linux AppImage "experimental"; no Windows | Free (1 user); Pro $20/user ($15 yearly) | 15.2k stars, 459 open issues | Plugin marketplace (7 Sep); Linux packaging (13 Sep); cloud multi-repo workspaces (19 Sep); shared workspaces with presence (30 Sep); PR summary and comment timeline (8–14 Oct) |
| cmux | Native macOS terminal for agents | macOS only; iOS TestFlight | Free (GPL); Pro $40–50/mo with cloud VMs; Team $60/user | 28.2k stars, 1,683 open issues | Session vault with forkable checkpoints (14 Sep); Claude/Codex status over SSH, crash recovery (5 Oct); session search, OSC 7501 status protocol (12 Oct) |
| Paseo | Self-hosted daemon with desktop, mobile, web and CLI clients; cross-provider subagents; voice | Desktop, mobile, web | Free (Apache-2.0) | 21.0k stars | Markets "no telemetry, no forced log-ins". HN: "very pleasant, except sub-agents hang" (HN 49945423) |
| Happy | E2E-encrypted phone/desktop client for Claude, Codex, Grok | All desktops, iOS, Android, web | Free (MIT) | 24.2k stars | Desktop linking (13 Sep), session recovery (22 Sep), Opus 5.5 (9 Oct) |
| Emdash (YC W26) | Electron, many agents, worktrees, SSH | macOS, Windows, Linux x64/arm64 | Free (Apache-2.0); cloud unpriced | 6.0k stars | Project env vars (2 Sep); multiple accounts per integration (18 Sep); Chat/TUI switch (27 Sep); LSP and a Claude/Codex usage page (10 Oct) |
| Orchestrator.inc (ex-Composio agent-orchestrator) | Planner agent, live kanban (Working / Needs you / In review / Ready to merge), CI tracking | — | Free OSS | 13.2k stars | — |
| Nimbalyst | Editors, parallel sessions, kanban | macOS, Windows, Linux, iOS, Android | Free for individuals (MIT) | ~1.9k stars, 700 open issues | ~40 releases. Multi-repo Git panel (Sep), remote sessions in your own Cloudflare sandbox (14 Sep), nested session trees (7 Oct) |
| herdr | tmux-style agent multiplexer with live status; joined YC | Terminal, web UI | — | HN 49201003, 6 Aug: 281 pts / 189 comments, with a VC backlash | — |
| Claude Squad | Go TUI over tmux | Unix | Free (AGPL) | 8.6k stars | v1.0.20 (20 Aug): slow cadence |
| opcode | Tauri GUI for Claude Code | No release binaries | Free (AGPL) | 22.4k stars | Dormant: last code commit Oct 2025 |
| Sculptor (Imbue) | Agents in containers, pairing sync | macOS (Apple Silicon), Linux | Not stated | 239 stars | Auto-naming, "Night Mode" (21 Sep) |
| CCManager, Agent Deck | TUIs for worktree sessions | Terminal | Free (MIT) | 1.3k / 1.1k stars | Agent Deck: worktree hooks approved per exact content; reads model lists from the installed CLIs; anonymous install telemetry from 4 Oct (opt-in unconfirmed [unverified]) |
| Container Use (Dagger) | Containers for agents over MCP | Terminal | Free | 4.1k stars | Nothing in the window: likely stalled |
| BridgeMind | Workspace with up to 16 agents | Desktop | ~$50/mo [unverified] | Product Hunt 1.0 from 2 reviews | Nothing in the window |
| Gone | Vibe Kanban (shut down 10 Apr 2026), Terragon, Crystal (→ Nimbalyst), Omnara (pivoted) | | | | |

**Small launches in the window.** All were small on HN:
- Pinrail, a review inbox: HN 49995778.
- Foremerge, intent conflicts: HN 49789356, 45 pts.
- Agent-Manager: HN 49107749, 98 pts.
- Pocketty, a phone SSH client: HN 50009634.
- Pizza Bot: HN 49713894.
- Supapool: HN 49100518.
- Port Zero: HN 49063648.
- Concord MCP, Riftri and OctoShell.

An awesome-list now counts 100+ orchestrators
(github.com/andyrewlee/awesome-agent-orchestrators). HN commenters call the
category saturated: "basically no moat" (HN 49983772); "every day I see a paid
or OSS version of this" (HN 49023427).

**Sources:**
- github.com/stablyai/orca; moclaw.ai/blog/what-is-orca-ade;
- conductor.build/changelog and /pricing;
- github.com/superset-sh/superset/releases; superset.sh/pricing;
- cmux.com/changelog and /pricing; github.com/manaflow-ai/cmux;
- github.com/getpaseo/paseo; github.com/slopus/happy/releases;
- github.com/generalaction/emdash/releases;
- github.com/OrchestratorInc/agent-orchestrator; nimbalyst.com/changelog;
- github.com/smtg-ai/claude-squad/releases; github.com/winfunc/opcode/commits/main;
- github.com/imbue-ai/sculptor/releases; github.com/kbwo/ccmanager/releases;
- github.com/asheshgoplani/agent-deck/releases; github.com/dagger/container-use/releases;
- producthunt.com/products/bridgespace-3; everydev.ai/developers/bridgemind.

### 1.2 AI-native terminals and IDEs

| Product | Hosts your own CLIs? | Parallel / worktrees | Linux | Pricing (15 Oct) | Shipped mid-Aug to mid-Oct |
|---|---|---|---|---|---|
| Cursor (now SpaceX/xAI) | No (own harness) | Agents Window; cloud agents; Projects (a coordinator over parallel cloud agents, 10 Sep) | .deb, .rpm, AppImage, x64/arm64. Agents Window doesn't work with WSL or remote, no ETA | **"X Subscription" from 12 Oct:** Free; Plus $8 (Grok only); Premium $30; Super $100; Ultra $200; one weekly pool. **Claude and GPT billed at API list price** | Self-hosted machines (2 Sep); Projects (10 Sep); Rollouts and Security Review (23 Sep); iOS remote control of local agents (6 Oct); app marketplace (13 Oct). OpenAI models leave the managed picker on 12 Nov [one source disputes the date] |
| Warp | Yes in the terminal; Factories run Claude Code and Codex as workers | Cloud orchestration ("Warp Agent"); Factories (early access) | Yes | Free; Build $20 ($20 of usage); Max $200; Business $50/user. Factories at API +20% | Factories (18/27 Aug); grok CLI (9 Sep); ChatGPT sign-in (23 Sep); Kiro CLI, "blocked" state for unattended runs (7 Oct); open-sourced |
| Zed | Yes, via ACP and Terminal Threads | Parallel agents with worktree threads | First-class | Personal $0; Pro $10 (+10% on tokens); Business $30/seat | Delta, PR-less review threads, public beta (16 Sep); `spawn_agent` takes a model; `prevent_idle_sleep` (1.19–1.24) |
| VS Code (Agents window) | Claude and Codex harnesses beside Copilot | 2-D session grid, worktrees, cleanup | Yes | Editor free; Copilot separate | Multi-folder sessions (1.140, 30 Sep); continue Codex chats from CLI/app, worktree cleanup, terminal sandboxing on all OSes (1.141, 7 Oct); move a chat to a new worktree; admin policies can disable Claude/Codex (1.142, 14 Oct) |
| Devin Desktop (ex-Windsurf) | ACP clients' MCP; own agent | Local / Worktree / Cloud picker; SSH hosts (10 Sep) | Not confirmed [unverified] | Pro $20, Max $200 (Apr 2026; live page 404) [unverified] | Cascade removed (8 Sep); ChatGPT sign-in (29 Sep) |
| JetBrains Air | Yes via ACP (Claude Agent, Codex, Junie, Copilot, OpenCode) | Worktrees; Air Teams cloud (early access) | Conflicting: help page says "planned for 2026" [unverified] | JetBrains AI credits or BYO | Expanded into IDE plugin, Teams and Governance (22 Sep) |
| Kiro (AWS) | No; works in ACP IDEs | Workflows | .deb, tar.gz, x64/arm64 | Free (50 credits); Pro $20; Pro+ $40; Pro Max $100; Power $200 | Local command sandbox, live status on tabs (IDE 1.3.0, 13 Oct); CLI memory and sandbox (2.29–2.31) |
| Trae, Void | No | — | — | Trae's prices conflict [unverified]; Void paused | — |
| Microsoft Intelligent Terminal | Detects ACP agent CLIs | — | — | Experimental [unverified] | — |

**Sources:**
- cursor.com/changelog and /pricing; forum.cursor.com/t/introducing-the-x-subscription/174450;
- engadget.com/2246969/openai-pull-its-models-from-cursor-due-to-spacexai-acquisition;
- thenextweb.com/news/spacex-cursor-acquisition-official-60-billion;
- forum.cursor.com/t/agents-window-still-fails-on-wsl/160388;
- docs.warp.dev/changelog/2026; warp.dev/pricing; hn.svelte.dev/item/47937349;
- zed.dev/blog/delta-public-beta; zed.dev/releases/stable; zed.dev/pricing;
- code.visualstudio.com/updates/v1_140 and /v1_141 and /updates;
- docs.devin.ai/desktop/changelog; jetbrains.com/air;
- thenewstack.io/jetbrains-air-agents-ide;
- kiro.dev/changelog and /pricing and /downloads.

### 1.3 Vendors' own apps, CLIs and cloud agents

| Product | Shipped mid-Aug to mid-Oct | Linux desktop | Price (15 Oct) | Top open requests (👍, 15 Oct) |
|---|---|---|---|---|
| Claude Code (Anthropic) | About weekly releases (v2.1.284–2.1.300):<br>• "Mods" plugin APIs and marketplace install<br>• blocking hooks<br>• agent view filters, with live `/model` on background sessions<br>• `claude --desktop`<br>• rate-limit options | Desktop beta since 30 Jun 2026: Ubuntu/Debian, x64/arm64. No Fedora, voice or Wayland hotkeys | Pro $20 ($17 yearly); Max 5x from $100 and Max 20x, both with monthly API credits; 5-hour and weekly limits | #45596 Bring Back Buddy 1191 · **#18435 multiple accounts in Desktop 904** · #36151 multi-account on mobile 756 · **#16157 limits with Max 695** · #38335 limits since Mar 23 476 · #1455 XDG dirs 441 |
| Codex (OpenAI) | • Linux desktop preview, `/import` from Claude Code and Cursor (11 Aug)<br>• GitLab in cloud (19 Aug)<br>• experimental worktrees (0.154)<br>• fork with `f` (0.157)<br>• finish sound (22 Sep)<br>• GPT-6 / 6.1 Sol | Preview since 11 Aug: .deb, .rpm, x64/arm64; Wayland/IME issues | Go $8; Plus $20; Pro $100 / $200 / $500. Pro $200 limits reportedly halved from 29 Oct [unverified] | **#8745 LSP 503** · **#9203 bring back /undo 482** · #25719 macOS CPU runaway 399 · #3000 voice 241 · #2998 IDE diff/approval 236 · #11626 rewind 228 · #28969 60-s auto-resolve 215 · #3962 sound on finish 201 · **#48195 daemon opt-in 179** |
| GitHub Copilot (CLI, app, Agent HQ) | • dynamic workflows and `/fleet` (1 Oct)<br>• computer use (1 Oct)<br>• local sandboxing GA, local-model discovery (7 Oct)<br>• Copilot desktop app with a worktree per session (preview May, GA June [secondary]) | App reportedly on Linux [secondary] | AI Credits since 1 Jun. Pro $10 ($15 credits); Pro+ $39 ($70 credits); Claude and Codex agents on Pro+ only | copilot-cli #1613 worktree lifecycle 38 · #3709 `/model` across BYOK and local 34 |
| Google: Antigravity, Antigravity CLI, Gemini CLI, Jules | Consumer Gemini CLI ended 18 Jun. Antigravity CLI (Go, closed source) shares a harness with the Antigravity 2.0 desktop | Desktop as tarballs; apt repo stale at 1.23.2 | Individual $0 with weekly limits; AI Pro / Ultra prices not on the page [conflicting third-party figures] | antigravity-cli **#31 ACP mode 405** · #4 open-source it 194 · #880 auto approval mode 21 |
| OpenCode (now anomalyco/opencode) | v1.19.0 (11 Oct), path to OpenCode 2; Linux desktop .deb/.rpm; ChatGPT sign-in partner [secondary] | Yes | Free; BYO | #11176 VS Code extension 162 · #27167 `/goal` 153 · #6152 context usage 140 · #10288 mobile/web UI 97 · #2242 sandboxing 79 · #38281 quota 63 |
| Others | • Cline 4.1.24 (13 Oct)<br>• Goose moved under the Linux Foundation's AAIF<br>• Aider dormant (last release looks like Aug 2025)<br>• Kimi K3 (16 Jul; new subscriptions paused 19 Jul [secondary])<br>• Grok Build 1.0 (7 Aug [unverified])<br>• Meta Muse Code for macOS and Linux (5 Aug)<br>• Amp "Orbs" remote agents | — | Kimi $19–$199; Droid $20–$200 [single aggregator] | Aider #3314 MCP 196; Kimi #1282 remote control 32 |

**Vendor terms on third-party harnesses.** These matter for Thread mode.
- **Anthropic:**
  - 19–20 Feb 2026: subscription OAuth was barred in third-party tools.
  - 14 May: programmatic use (SDK, `claude -p`, third-party apps) was to move to a separate credit from 15 Jun (theregister.com/ai-ml/2026/05/14/anthropic-tosses-agents-into-the-api-billing-pool/5240748).
  - 15 Jun: that change was paused.
  - 7 Oct: "You can still use the Claude Agent SDK, claude -p, and third-party apps with your subscription limits" (support.claude.com/en/articles/15036540).
  - The legal page allows an end user to sign in to the **unmodified** Claude Code binary with their own subscription. It bars third parties from offering Claude.ai login or intermediating credentials (code.claude.com/docs/en/legal-and-compliance).
  - The two texts don't fully agree on "third-party apps", so this is an owner check.
- **OpenAI:** "Sign in with ChatGPT" opened to third-party tools around 29 Sep, with per-app caps. Launch partners reportedly include Conductor, Warp, OpenCode, Amp and Devin [secondary] (thenewstack.io/sign-in-with-chatgpt). Joining would mean holding OpenAI tokens, which Vibezeo's rules forbid.
- **Google:** a thread on third-party automation of Antigravity risking account suspension (HN 49548452, 3 Sep, 338 pts), with a rebuttal pointing to Google's headless docs (antigravity.google/docs/cli/headless).

**Sources:**
- code.claude.com/docs/en/changelog; claude.com/pricing;
- github.com/anthropics/claude-code/issues/65697 (Linux, 498 👍, closed as completed);
- basic-tutorials.com/news/claude-desktop-for-linux-anthropic-launches-the-official-beta;
- learn.chatgpt.com/docs/changelog and /pricing;
- techrepublic.com/article/news-openai-chatgpt-codex-linux-desktop-preview;
- letsdatascience.com/news/openai-introduces-500-chatgpt-pro-500-tier-329fe640;
- github.blog/changelog; github.blog/2026-04-27-github-copilot-is-moving-to-usage-based-billing;
- github.com/features/copilot/plans;
- developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli;
- antigravity.google/pricing; github.com/anomalyco/opencode/releases;
- github.com/cline/cline/releases;
- aaif.io/blog/how-goose-is-building-a-distributed-ai-teammate-ecosystem;
- 9to5mac.com/2026/08/05/meta-launches-muse-code-ai-coding-agent-for-macos-and-linux;
- the issue trackers named in the table.

---

## 2. Demand signals

### 2.1 Most-reacted requests on the trackers (15 October 2026)

| Theme | Strongest evidence | Vibezeo today |
|---|---|---|
| Multiple accounts per vendor | Claude #18435 (904), #36151 (756), #27302 multiple accounts per connector (414); Emdash v1.2.5 shipped multiple accounts per integration | **Built** (F-AC-01…03) |
| Usage limits and seeing them | Claude #16157 (695), #38335 (476); Codex #24182 (19); OpenCode #38281 (63); meters shipped by Orca, Emdash, Agent Deck | Built (F-AC-04/07). **Forecast and per-session cost not built** → NEXT_FEATURES #4 |
| Linux and Windows for Mac-first tools | cmux #330 Linux (168/220), #1012 Windows (51); Superset #405 (16/23), #2692 (14); Claude Linux #65697 (498, closed with a Debian-only beta) | Linux built. macOS and Windows in progress |
| Code intelligence (LSP) | Codex #8745 (503); Orca #961 (39) | Not built → #19 |
| Undo, rewind, checkpoints | Codex #9203 (482), #11626 (228); cmux's forkable checkpoints | **Built** (F-EX-01) |
| A standard protocol (ACP) | Antigravity CLI #31 (405) | ACP for three engines → #14 generalises it |
| History and resume for external agents | Zed #37074 (260, closed); Warp #14720; Happy #437 (15) | Saved sessions inside Vibezeo only → #5 session library |
| Voice | Codex #3000 (241) | **Built** (Dictate, Conductor) |
| Diff review and approval in a GUI | Codex #2998 (236); Zed #59157 (11); Warp #8542 (44) | Built (F-WT-02). PR comments back → #13 |
| Attention: sound, "needs input", no auto-resolve | Codex #28969 (215), #3962 (201); cmux #1027 (42), #2322 (22) | Built (F-DB-02/03, F-EN-05). A cross-vendor review queue is not → #2 |
| Opt-in background daemons, control of engine updates | Codex #48195 (179); HN 50060694 (606 pts) | Not built → #3 |
| Sandboxing | OpenCode #2242 (79); VS Code, Kiro and Copilot all shipped local sandboxes in Sep–Oct | Engines' own sandboxes only → #11 |
| jj | Orca #1082 (46); container-use #194 (8) | Not built → #21 |
| Multi-repo | Orca #1099 (43/71); VS Code #311148 (34); Superset #5354 (7); Claude Squad #56 (6) | Not built → #18 |
| Remote/SSH with persistence; headless daemon | cmux #1664 (38); opcode #163 (34); Nimbalyst #49 (11), #416 (5); cmux #1663 (26/27) | SSH built (F-RM-01). Local agents don't survive closing the app → #10 |
| New harnesses within days | Happy #265 OpenCode (53), #1213 pi (30), #1313 Antigravity (15); Orca #14323 DeepSeek (31) | 11 engines. No Antigravity or Kiro yet → #9 |
| Use your own subscription in the host | VS Code #314952 (84), #320339 (50), #318578 no GitHub auth (43) | **Built**: the core of the product |
| No forced sign-in, offline | Superset #4894 (11); Paseo's pitch | **Built** |

### 2.2 What developers running several agents struggle with (Hacker News, mid-July to mid-October 2026)

Ranked by how often and how loudly each comes up:

1. **Knowing which agent needs you.**
   - Agent-Manager (HN 49107749, 30 Jul, 98 pts / 80 comments): one agent is always "sitting on a permission prompt I never saw".
   - herdr (HN 49201003, 281 / 189).
   - "Ask HN: how many sessions do you juggle?" (HN 49469723): "8 is my limit".
   - Pocketty (HN 50009634).
   - Vendors solve it only for their own agent (Claude Code agent view; the Codex app).
2. **Review is the bottleneck.**
   - "AI coding agents generate more code, but not more software" (HN 50059523, 12 Oct, 364 / 309).
   - "How do you do PR reviews now?" (HN 49195510): triage by blast radius, a summary with each change.
   - Several agents finishing at once (HN 49049986).
   - Overload: "AI is killing my brain" (HN 49468252, 54 / 29).
   - Cross-model review is a common habit (HN 49348751; Isolade, HN 49168002; Crew, HN 49595212).
3. **Worktrees don't isolate the runtime: ports, databases, Compose, deps, hooks.**
   - HN 49110389 (32 / 35) and HN 49085449.
   - Supapool (HN 49100518) and Port Zero (HN 49063648).
   - 4–5 agents force-quitting an 8 GB machine (HN 49104747).
4. **Conflicts between agents, including intent conflicts git can't see.**
   - Foremerge (HN 49789356).
   - Cursor building its own VCS (HN 48982535, 278 pts).
   - Claude's cross-session messaging (HN 49222824, 173 pts) doesn't cross harnesses.
5. **Limits, cost and runaway spend.**
   - Weekly limits thread (HN 49348751, 295 / 267).
   - Codex resets (HN 48963465, 312 / 201).
   - Hitting limits (HN 49904074).
   - "$78,000 rogue agents" (HN 49861047) [unverified].
   - Claude #75314: 10 background tasks for 34+ hours.
   - Meta and Microsoft capping employee usage (HN 49997161, 377 / 388).
6. **Moving context between vendors.**
   - Skillsync (HN 49743049).
   - AGENTS.md vs CLAUDE.md (HN 49760187, 741 / 285; HN 49814947, 486 / 284).
   - Agent memory as a file format (HN 49508317, 191 pts).
7. **Trust in the harness: sandboxes, secrets, silent defaults.**
   - Auto mode broken (HN 49506819, 399 pts).
   - Auto mode made the default (HN 49239021, 292 / 313).
   - Codex's daemon (HN 50060694, 606 / 292).
   - ZCode uploading git history (HN 49752422, 262 pts).
   - OneCLI's credential gateway (HN 49023427, 49363710).
   - WorkOS on agents babysitting agents (workos.com/blog/agents-babysitting-agents).
8. **Unattended and phone use.**
   - A spare Mac for Claude Code (HN 48959392, 251 / 196).
   - Pizza Bot (HN 49713894).
   - Agents 24/7 (HN 49653369): it works only for compact, verifiable problems.
9. **Local models.**
   - Muse Glimmer (HN 49241679, 1,209 / 638).
   - Ante, offline (HN 49245437, 169 pts).
   - OpenCode with Ollama setup pain (HN 49652122).
10. **Linux.**
    - Codex's Linux preview (HN 49281916, 469 / 315).
    - Explicit "please support Linux" demand shows on the trackers (cmux #330, Superset #405), not on HN. Reddit couldn't be read.

---

## 3. Each competitor's gaps, and why they exist

| Competitor | Gap | Why |
|---|---|---|
| Orca | The broadest free rival, and cross-platform. Fast churn: releases every 1–2 days, 4.2k open issues. Top asks are jj, multi-repo diffs and LSP | Focus on breadth. Its scale makes "free, local, cross-platform" table stakes, not a differentiator |
| Conductor | Local is macOS only; cloud is the paid tier; built on vendor SDKs, so exposed to subscription-terms changes (HN 49996437 [unverified]) | Mac-native team; the business is cloud workspaces |
| Superset | Linux experimental, no Windows, sign-in required (#4894) | Per-seat cloud |
| cmux | macOS only (#330, 220 reactions), flaky status (#1027) | Swift/AppKit on libghostty |
| Emdash, Nimbalyst | Close on features; small trackers; cloud unpriced | Young |
| Paseo, Happy | Remote-first clients; thin on review and git | Focus on remote control |
| Cursor | Now ties included usage to Grok. Claude and GPT at API price; OpenAI leaving the picker | Owner's model strategy (SpaceX/xAI) |
| Warp | Outside agents run as workers in Warp's cloud, at API +20% | Margin on metered inference |
| Zed, VS Code, JetBrains Air | Host many vendors, but through ACP/SDK harnesses that expose only what the adapter does. VS Code admins can turn Claude and Codex off (1.142) | Their own agent and editor come first |
| Vendor apps (Claude, Codex, Copilot, Antigravity) | One vendor each; usage, multi-account and history only for that vendor; Linux builds partial (Claude: no Fedora; Codex: preview) | Each wants to own the harness |

**What the category has learned since 10 October.**
- The "parallel agents in worktrees" workspace is now standard everywhere, in vendors' apps too (VS Code, Copilot app, Codex app, Zed, Cursor, Devin).
- The open ground has moved to:
  - review at volume;
  - runtime isolation;
  - cross-vendor coordination;
  - cost visibility;
  - trust: sandboxes and no silent defaults;
  - persistence;
  - neutrality. Cursor's turn shows a host can stop being neutral overnight.

---

## 4. Where Vibezeo stands

### Ahead
- **Any vendor's CLI, unmodified, on the user's own subscriptions, with no markup.**
  - This is the compliant design under Anthropic's current legal page.
  - It matters more after Cursor's 12 Oct pricing.
  - Warp's Factories charge API +20%.
- **Several accounts per engine across vendors.** Claude's top two requests (#18435, #36151) ask for this inside one vendor.
- **Explicit authority:** inline approvals, risk-class plugin approvals, held actions for unattended runs, trusted worktree commands. Agent Deck only reached content-bound hook approval in October.
- **Teammates with memory, tasks over MCP, Conductor voice, local Dictate on Linux.** No rival has these together.
- **Checkpoints, Best of N, worktree setup, review comments, overlap forecast and GitLab MRs**, all built.

### At parity
- Worktrees, diff and PR, notifications, browser and ports panels, SSH workspaces, phone companion, usage page.
- Free and local are now common: Orca, Emdash, Nimbalyst, Paseo.

### Behind or open
- **Cross-vendor review queue:** none exists anywhere; vendors do it for their own agent.
- **Session library:** cmux has one, Mac-only.
- **Forecast of limits.**
- **Runtime isolation per worktree:** Conductor has auto port forwarding.
- **Agents surviving the window closing:** Superset, Paseo and cmux's restore.
- **Sandbox:** VS Code, Kiro and Copilot shipped one.
- **LSP:** Emdash shipped it.
- **Multi-repo and jj.**
- **Engine catalog:** Antigravity CLI and Kiro CLI are missing.
- **Platforms:** macOS and Windows downloads; Linux ARM64.
- **Free plan caps** (4 panes, 3 workspaces) against free, uncapped rivals.
- **Awareness:** no public launch yet.

---

## 5. Opportunities

Product features are ranked, with evidence and acceptance criteria, in
`docs/NEXT_FEATURES.md`. Its top five:
1. one instructions file for every engine;
2. the review queue;
3. engine versions and updates under the user's control;
4. the usage forecast;
5. the session library.

It also has six bold bets, and the owner-only items below.

Rows from the 10 October list:

| # | Opportunity | Status (15 Oct) |
|---|---|---|
| 1–4 | Worktree setup, review comments, overlap check, GitLab MRs | **Built** (F-WT-01…04, 0.7.0) |
| 5 | Positioning around Linux, any vendor's CLI, no markup, local | Still right. Add **neutrality** after Cursor's 12 Oct change (owner, site session) |
| 6 | Anthropic's terms for Thread mode's `claude -p` | Clarified on 7 Oct: subscription limits still apply. The legal page bars credential intermediation, and Vibezeo never touches tokens. Owner to re-read both |
| 7 | Lift Free's local caps | More pressing: Orca, Emdash, Nimbalyst and Paseo are free and uncapped (owner) |
| 8 | .rpm, Flatpak, AUR | .rpm shipped (0.7.x); store packages built (F-RM-07), submissions are the owner's |
| 9 | Antigravity CLI engine | Not built → NEXT_FEATURES #9 (with Kiro CLI) |
| 10–11 | Community and launches | Still owner's. The category is saturated, so lead with the open, local, no-account stance (herdr's VC backlash) |
| 12 | Diff since the last turn | **Covered** by checkpoints (F-EX-01) |
| 13 | Multi-repo workspaces | Evidence grew (Orca #1099) → NEXT_FEATURES #18 |
| 14 | jj | Evidence grew (Orca #1082) → NEXT_FEATURES #21 |

`docs/GO_TO_MARKET.md` still holds the positioning, channels and launch
plans. Its "0.5.0" references should be updated to 0.7.2 by the site session.

---

## 6. Evidence index for NEXT_FEATURES

`docs/NEXT_FEATURES.md` describes its evidence without naming competitors. These are the full, named sources behind each entry, read on 15 October 2026.

### E-EN-07
- "Claude Code now reads AGENTS.md if there is no CLAUDE.md": HN 49760187, 18 Sep 2026, 741 points / 285 comments.
- "…only when telemetry is on [fixed]": HN 49814947, 486 points / 284 comments.
- Mdmanager, which syncs these files across machines: HN 49587839.
- VS Code 1.140 ships multi-folder sessions for the Copilot, Claude and Codex harnesses (code.visualstudio.com/updates/v1_140). Instructions that differ per harness are a direct cost of running several of them.

### E-DB-04
- "AI coding agents generate more code, but not more software": HN 50059523, 12 Oct 2026, 364 points / 309 comments. Commenters agree review is the bottleneck.
- "Ask HN: How do you do PR reviews now?": HN 49195510, 6 Aug 2026. Teams sort by blast radius and ask for a summary with each change.
- "Ask HN: What do you do when your agents are working?": HN 49049986, 25 Jul 2026. Several agents finish at once and all need review.
- Pinrail, a desktop review inbox: HN 49995778, 7 Oct 2026. A commenter asks for approvals that go stale when the diff changes.
- Agent-Manager: HN 49107749, 30 Jul 2026, 98 points / 80 comments. The author wants a full diff of what each agent changed, one key away.
- One vendor ships a similar view for its own agent only: Claude Code's agent view groups sessions into "Needs input / Ready for review / Working / Completed" (claude.com/blog/agent-view-in-claude-code, 11 May 2026).

### E-EN-08
- Codex CLI's persistent, self-updating daemon: HN 50060694, 12 Oct 2026, 606 points / 292 comments. One user found 12 GB of old versions.
- Codex issue #48195 "make `daemon_auto_start` opt-in": 179 👍, opened 25 Sep 2026.
- CLI updates breaking wrappers: Happy #149 and Vibe Kanban #2794 (old analysis §2, pattern 7).
- VS Code #332295 "auto-pick the latest Claude/Codex SDK": 10 reactions. Users notice when versions drift.

### E-AC-08
- Claude Code #16157 "instantly hitting usage limits with Max": 695 👍. #38335: 476 👍.
- Codex #24182 "show 5-hour and weekly limits persistently": 19 👍.
- OpenCode #38281 opencode-quota: 63 👍.
- Plan churn makes forecasts more valuable:
  - OpenAI's Pro $200 limits are reportedly halved from 29 Oct 2026 **[unverified, secondary]** (letsdatascience.com/news/openai-introduces-500-chatgpt-pro-500-tier-329fe640);
  - Cursor moved to weekly pools on 12 Oct 2026 (forum.cursor.com/t/introducing-the-x-subscription/174450);
  - HN 49348751, 18 Aug 2026, 295 points / 267 comments, on Claude's weekly limits.
- "Ask HN: How do you handle hitting usage limits?": HN 49904074, 30 Sep 2026.

### E-TH-17
- Zed #37074 "support history with external ACP agents": 260 reactions. The largest agent-related signal on that tracker (read 15 Oct 2026).
- Warp #14720 "resume CLI-agent conversations per project after restart".
- VS Code 1.141 (7 Oct 2026) continues a Codex chat started in the ChatGPT app or CLI, and lists Copilot CLI sessions automatically (code.visualstudio.com/updates/v1_141).
- Codex's `/import` from Claude Code and Cursor (11 Aug 2026, learn.chatgpt.com/docs/changelog).
- Skillsync (YC W26), which converts and searches sessions across harnesses: HN 49743049, 17 Sep 2026, 68 points.
- Claude Code #43303 and #23800, resume losing history (old analysis).
- One rival already has this, for macOS only: cmux 0.64.23 (14 Sep 2026) added a searchable vault of sessions with checkpoints you can fork from, and 0.65.1 (12 Oct) search across conversation text (cmux.com/changelog).

### E-WT-05
- Cross-model review is a common habit:
  - HN 49348751 (18 Aug 2026): users keep both Claude and OpenAI plans "because the models catch each other's misses";
  - Isolade (HN 49168002) and Crew (HN 49595212);
  - dev.to/offrun/running-claude-code-and-codex-in-parallel-on-one-repo-git-worktree-gotchas-and-cross-review-41eg.
- Line comments that feed the agent: Zed #59157 (11 reactions); JetBrains Air's diff comments (jetbrains.com/air).
- A survey reports Claude preferred for quality and Codex for cost and limits **[unverified]** (thenote.app/post/en/claude-code-vs-codex-2026-what-500-reddit-developers-really-think-pnatezwsc8).

### E-WT-06
- "Git worktrees are not an isolation boundary for coding agents": HN 49110389, 30 Jul 2026, 32 points / 35 comments. Its top comments name ports, databases, `.env` and Compose names.
- "Setup Script Should Support Git Worktrees": HN 49085449, 28 Jul 2026. The author lists the problems: ports, databases, cache keys, queues and Compose resources.
- Port Zero, which gives each branch its own hostname: HN 49063648, 27 Jul 2026. A frontend talked to the wrong worktree's backend.
- Supapool, a throwaway database per agent: HN 49100518, 29 Jul 2026.
- VS Code 1.140 added `git.worktreeSymlinkFolders` for `node_modules` (code.visualstudio.com/updates/v1_140).
- Superset users report "I've never used lsof so much" (old analysis §2).
- Claude Squad's top issue, #260, asks for worktree setup with port isolation: 8 reactions.
- Conductor 0.83.0 shipped auto port forwarding (27 Aug 2026, conductor.build/changelog).

### E-AC-09
- Claude Code #75314: ten background tasks stuck for 34+ hours, about 1M tokens.
- "Codex agents went rogue, $78,000": HN 49861047, 26 Sep 2026. It claimed 826 child agents **[unverified: flagged post, fresh account]**. The thread also has a separate report of 437 subagents in a review loop.
- Cursor forum, "zombie subagents", 30 Sep 2026 **[single report]** (forum.cursor.com/t/zombie-subagents/173463).
- Microsoft reportedly cut per-employee Claude caps: HN 49997161, 7 Oct 2026, 377 points **[unverified]**.

### E-EN-09
- developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli.
- Warp added the Kiro CLI (7 Oct 2026), the grok CLI (9 Sep) and Antigravity (3 Jul) to its agent toolbar (docs.warp.dev/changelog/2026).
- Kiro CLI 2.31 (kiro.dev/changelog/cli/2-31).
- Meta's Muse Code ships for macOS and Linux (9to5mac.com/2026/08/05/meta-launches-muse-code-ai-coding-agent-for-macos-and-linux) **[secondary]**.
- Requests to support new harnesses within days:
  - Happy #1313 Antigravity CLI (15) and #1213 pi (30);
  - Orca #14323 DeepSeek harness (31).
- Agent Deck #2664 and v1.16.21 read model catalogs from the installed CLIs instead of hard-coding them. Vibezeo should do the same where an engine can list its models.

### E-SH-14
- herdr's comparison page says desktop managers stop agents when the app quits, while multiplexers survive (herdr.dev/compare).
- Superset's daemon keeps sessions alive (old analysis §1.1).
- VS Code 1.140 moved its Copilot harness into a separate agent-host process so windows can attach to one session (code.visualstudio.com/updates/v1_140).
- VS Code #318997 "remote sessions run detached in the background": 23 reactions.
- Zed added `agent.prevent_idle_sleep` (zed.dev/releases/stable).
- cmux #1663, Zellij integration to keep processes alive across relaunch: 26/27 reactions.
- Nimbalyst #416, headless server plus desktop client: 5 reactions.
- Paseo (21k stars) is built around a self-hosted daemon with thin clients (github.com/getpaseo/paseo).
- Today Vibezeo restores agents as paused cards (F-SH-07), so a quit ends their turns.

### E-TL-11
- "Breaking Claude Code Opus 5 Auto Mode": HN 49506819, 31 Aug 2026, 399 points. Auto mode is not a sandbox.
- Auto mode as the default: HN 49239021, 10 Aug 2026, 292 points / 313 comments.
- An agent writing `.git/hooks`: comments on HN 49110389.
- The category is moving to sandboxes:
  - VS Code 1.141 terminal sandboxing on all three OSes (code.visualstudio.com/updates/v1_141);
  - Kiro IDE 1.3.0's local command sandbox (kiro.dev/changelog/ide/1-3);
  - Copilot's local sandboxing GA, 7 Oct 2026 (github.blog/changelog).
- OpenCode #2242 "sandboxing": 79 👍.

### E-WT-07
- A local merge queue for agent branches: HN 49104747, 30 Jul 2026, 42 points / 22 comments. 90 commits a day, a local test suite, rollback.
- Codex users having a "merge-queue thread" land the other threads one at a time (comments on that post).
- Emdash #1269 "local merge without PR" (old analysis).

### E-WT-08
- WorkOS, "agents babysitting agents" (31 Aug 2026, workos.com/blog/agents-babysitting-agents): CI logs and bot comments are untrusted input, so keep a human on merge.
- Claude Code ships cloud auto-fix (code.claude.com/docs/en/changelog).
- Cursor's Bugbot, plus its Rollouts and Security Review bots (23 Sep 2026, cursor.com/changelog).
- Warp #8542 "full git staging/commit in the Changes panel": 44 reactions.

### E-EN-10
- Antigravity CLI #31 "Add ACP stdio JSON-RPC mode": 405 👍, that tracker's top request. Plus #195.
- Goose #12255, launching external ACP clients.
- JetBrains Air runs "any ACP agent" (jetbrains.com/air).
- Zed's agent registry (zed.dev/docs/ai/external-agents).

### E-EN-11
- Muse Glimmer, a 30B model built for always-on local agents: HN 49241679, 10 Aug 2026, 1,209 points / 638 comments.
- OpenCode with Ollama setup pain: HN 49652122, 11 Sep 2026.
- Ante, an offline agent: HN 49245437, 169 points.
- Copilot CLI added local-model discovery on 7 Oct 2026 (github.blog/changelog).
- Copilot CLI #3709, switching to local models in one session: 34 👍.

### E-TK-05
- Foremerge: HN 49789356, 21 Sep 2026, 45 points. Its point is that the worst conflicts are intent conflicts git doesn't flag. A commenter is sceptical that scope is known in advance.
- Claude Code's first-party cross-session messaging: HN 49222824, 8 Aug 2026, 173 points. The thread asks whether it works across harnesses; it doesn't.
- Cursor built its own VCS for agent swarms: HN 48982535, 20 Jul 2026, 278 points.

### E-EX-06
- Four or five agents building and testing at once force-quit an 8 GB machine: HN 49104747, 30 Jul 2026.
- Running 3–4 local database stacks makes a MacBook overheat: Supapool, HN 49100518, 29 Jul 2026.

### E-CD-22
- Orca #1099 "multi-repo diffs": 43/71 reactions (15 Oct 2026).
- Superset Cloud Workspaces v2 (19 Sep 2026) and Nimbalyst 0.76.2–0.76.3 (Sep 2026) ship multi-repo work.
- VS Code #311148 "multi-root workspaces with cross-repo tracking and worktrees": 34 reactions.
- VS Code 1.140's experimental multi-folder sessions.
- Superset #5354 and Claude Squad #56 (old analysis).

### E-CD-23
- Codex #8745 "LSP integration": 503 👍, the top request on that tracker.
- Orca #961 "LSP": 39 reactions.
- Emdash v1.2.8 (10 Oct 2026) shipped editor language intelligence.

### E-EX-07
The ask comes up mainly after incidents:
- HN 49861047, where the poster's records were gone **[unverified post]**;
- a commenter building "session scanners" on the Remote Control thread, HN 49565799 **[thin]**.

### E-CD-24
- Orca #1082 "Jujutsu workspaces": 46 reactions.
- container-use #194: 8 reactions.
- Emdash #1570: 2 reactions.
- jj workspaces came up as an alternative in the merge-queue thread, HN 49104747.

### E-B3
- Cursor Projects (10 Sep 2026): cursor.com/changelog.
- Warp Factories (Aug 2026): docs.warp.dev/factories; eweek.com/news/warp-ai-software-factory.
