# Competitive analysis (October 2026)

Internal document. Competitors are named here and only here: never on the
site or in the app (CLAUDE.md "Never").

Researched on 10 October 2026 with web search and the products' own pages,
docs, GitHub issue trackers (reaction counts read on that day) and Hacker News
threads. Every claim has a source. **[unverified]** marks a claim that rests on
a single secondary source or where sources disagree. Prices change often:
re-check a vendor's page before quoting it anywhere.

Vibezeo, for reference: 0.5.0 for Linux x86-64 (.deb), Free plan, no account,
nothing sent to our servers. It hosts 11 coding-agent CLIs on the user's own
subscriptions in terminal panes and git worktrees, reads Claude Code, Codex and
ACP sessions as threads with inline approvals, and adds teammates, tasks over
MCP, plugins with risk-class approvals, local Dictate and SSH workspaces.
Pricing: Free / Pro $29 a month / Team $49 a seat (PRODUCT_SPEC §3.1).

---

## 1. Landscape

### 1.1 Agent workspaces and orchestrators

| Product | What | Platforms | Price | Community signal | Status |
|---|---|---|---|---|---|
| Conductor (Melty Labs) | Closed-source app: Claude Code, Codex, Cursor agents in worktree workspaces, review and merge | macOS only (Apple Silicon per a review) | Free local; Pro $50/mo; Teams $60/user | Show HN 228 pts / 115 comments | Active |
| Superset | Electron, terminal-first, any CLI agent, worktrees, daemon keeps sessions alive | macOS; Linux build "not maintained as well"; no Windows | Free; Pro $20/user; source-available (ELv2); sign-in required | 15k stars, 919 open issues | Active |
| cmux | Native macOS terminal for agents: notification rings, browser, SSH beta | macOS only | Free (GPL) + paid edition | 28k stars | Active |
| Emdash (YC W26) | Electron: 25+ agents, worktrees, setup/teardown scripts, SSH remotes, issue intake | macOS, Windows, Linux | Free OSS; cloud unpriced | 6k stars; claims 1M+ downloads | Active |
| Nimbalyst (ex-Crystal) | Electron workspace: editors, parallel sessions, kanban | macOS, Windows, Linux | Free for individuals (MIT) | 1.9k stars | Active; Crystal deprecated Feb 2026 |
| Vibe Kanban (Bloop) | Kanban → agent in a worktree → diff review with inline comments → PR | npx / web | Was free + cloud | 28k stars | **Shut down 2026-04-10** ("couldn't find a business model") |
| Claude Squad | Go TUI over tmux and worktrees | Unix (tmux + gh) | Free (AGPL) | 8.6k stars | Active, small |
| opcode (ex-Claudia) | Tauri GUI for Claude Code with usage dashboard | Listed for all; no release binaries | Free (AGPL) | 22k stars | Little maintenance |
| Happy | E2E-encrypted phone/web client for local Claude Code/Codex | iOS, Android, web, desktop | Free (MIT) | 24k stars | Active |
| Sculptor (Imbue) | Parallel Claude agents, containers then worktrees | macOS (Apple Silicon), Linux | Free (MIT) | 238 stars | Low traction |
| Terragon | Cloud background agents opening PRs | Web | Was paid | — | **Shut down**, code released |
| Omnara | Phone/web remote for Claude Code | Web, mobile | Was $9–20/mo | Show HN 310 pts | Pivoted to agent infrastructure |
| BridgeMind | Agent workspace with kanban, up to 16 agents, voice, MCP | macOS, Windows, Linux (per a directory listing) | ~$50/mo **[unverified]** | Product Hunt 1.0 from 2 reviews **[unverified]** | Active, 5 people |
| CCManager, Agent Deck, Uzi, Container Use | TUIs / CLI / MCP for worktree sessions | Terminal | Free OSS | 0.6k–4k stars | Active |

Sources: conductor.build and /pricing; news.ycombinator.com/item?id=44594584;
superset.sh/pricing; news.ycombinator.com/item?id=48236770;
github.com/superset-sh/superset; cmux.com; github.com/manaflow-ai/cmux;
emdash.com; github.com/generalaction/emdash; nimbalyst.com;
github.com/stravu/crystal; vibekanban.com/blog/shutdown;
github.com/BloopAI/vibe-kanban; github.com/smtg-ai/claude-squad;
github.com/winfunc/opcode; github.com/slopus/happy;
imbue.com/product/sculptor; github.com/terragon-labs/terragon-oss;
news.ycombinator.com/item?id=46991591; aichief.com/ai-development-tools/bridgemind;
producthunt.com/products/bridgespace-3; everydev.ai/developers/bridgemind;
github.com/kbwo/ccmanager; github.com/asheshgoplani/agent-deck;
github.com/dagger/container-use.

### 1.2 AI-native terminals and IDEs

| Product | Hosts your own CLIs? | Parallel / worktrees | Linux | Pricing model | Main complaints |
|---|---|---|---|---|---|
| Warp | Yes in the terminal, with notifications and review comments for Claude Code, Codex, OpenCode; they don't appear in its agent panels | Cloud orchestration (Oz) | Yes (.deb, .rpm, AUR, AppImage) | Free; Build $20 = $20 usage at API rates; pay-as-you-go API +20% | Pricing churn (three tiers scrapped Oct 2025), bloat, daily updates, telemetry history, "hard to tell what warp is anymore" |
| Cursor 3 | No (own harness and models) | Agents Window: local, cloud, worktree, SSH | Yes, with AppImage problems | Pro from $20, usage pools, on-demand billing | Opaque limits (July 2025 apology and refunds), credit burn, Linux crashes, lock-in |
| Devin Desktop (ex-Windsurf) | Reportedly yes **[unverified]** | Local + cloud command center | Yes | Quotas; Pro $20, Max $200 **[unverified]** | Three owners in a year, Cascade retired, quota opacity |
| Zed | Yes, via ACP (adapter-limited) | Worktree threads, terminal threads | Yes | Free host; hosted tokens +10% | Claude Code hooks, some slash commands, resume missing via ACP |
| JetBrains Air (preview) | Yes via ACP, **except Anthropic subscriptions** | Worktree / Docker | Launched macOS only | JetBrains AI or BYO | Mac-only preview |
| Kiro (AWS) | No | Spec-driven | Yes **[unverified]** | Credits | "Wallet-wrecking" repricing (Aug 2025) |
| Trae (ByteDance) | No | Cloud tasks | — | Token billing | Telemetry after opt-out (26 MB in 7 min) |
| Void | No | No | Yes | Free | Development paused |

Sources: docs.warp.dev/agent-platform/cli-agents/claude-code;
warp.dev/pricing; warp.dev/blog/warp-new-pricing-flexibility-byok;
news.ycombinator.com/item?id=45772558; cursor.com/pricing;
cursor.com/en/blog/june-2025-pricing;
techcrunch.com/2025/07/07/cursor-apologizes-for-unclear-pricing-changes-that-upset-users;
news.ycombinator.com/item?id=47618084;
forum.cursor.com/t/appimage-run-fails-without-no-sandbox-on-linux/168960;
zed.dev/docs/ai/external-agents; zed.dev/docs/ai/parallel-agents;
zed.dev/pricing; blog.jetbrains.com/air/2026/03/12/air-launches-as-public-preview;
theregister.com/2025/08/18/aws_updated_kiro_pricing;
theregister.com/2025/07/28/bytedance_trae_telemetry;
noqta.tn/en/blog/devin-desktop-windsurf-ai-agent-fleet-management-2026 **[secondary]**.

### 1.3 Vendors' own apps and cloud agents

| Product | Surfaces | Linux desktop | Price | Main complaints |
|---|---|---|---|---|
| Claude Code (Anthropic) | CLI, IDE, Desktop Code tab (parallel worktree sessions, terminal, diff review, SSH/WSL/cloud, finish notification, usage ring, Remote Control), web, mobile | Beta, Debian/Ubuntu only, no dictation | Pro $20, Max $100/$200; 5-hour and weekly limits, figures unpublished | Limits hit "in 45 minutes" (#16157, open since Jan 2026); #38335 (~870 comments); no multi-account (#18435); resume loses history (#43303, #23800); Desktop doesn't see CLI sessions |
| Codex (OpenAI) | CLI (OSS), IDE, cloud, ChatGPT desktop app with worktrees | Preview since 2026-08-11 (.deb, .rpm; Wayland experimental) | Plus $20, Pro $100–500 | Token cost jump (#28879), questions auto-resolve after 60 s (#28969), sound on finish (#3962), rewind (#11626) |
| GitHub Copilot: coding agent, Agent HQ, CLI | Cloud PR agents; Agent HQ runs Copilot, Claude, Codex side by side | — | Pro $10, Pro+ $39 **[third-party]**; AI Credits since 2026-06-01 | Billing shock after the switch to credits |
| Google: Gemini CLI → Antigravity CLI, Jules | Consumer Gemini CLI stopped 2026-06-18; Antigravity CLI is reportedly closed-source | — | Jules free 15 tasks/day **[unverified]** | Pushback on closed source; stranded users |
| Devin, Factory, Amp | Cloud and CLI agents | — | $20–$200 tiers **[unverified]** | Pricing changes; Amp's free tier closed to new users **[unverified]** |
| OpenCode, Cline, Goose, Aider, Kilo | OSS agents; OpenCode and Goose ship Linux desktops | Yes | Free; BYO keys | Cut off from Claude subscriptions (Jan 2026); Roo Code shut down 2026-05-15 |

Sources: code.claude.com/docs/en/desktop; code.claude.com/docs/en/desktop-linux;
claude.com/pricing; github.com/anthropics/claude-code/issues/16157, /38335,
/18435, /43303, /17118;
community.openai.com/t/codex-in-chatgpt-desktop-app-for-linux-is-now-in-preview/1390027;
learn.chatgpt.com/docs/pricing; github.com/openai/codex/issues/28879, /28969,
/3962, /11626; thenewstack.io/github-agent-hq;
visualstudiomagazine.com/articles/2026/06/04/copilot-billing-shock-hits-developers.aspx;
developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli;
thenewstack.io/gemini-cli-antigravity-replacement; github.com/sst/opencode;
github.com/block/goose; github.com/RooCodeInc/Roo-Code;
falcao.org/posts/anthropic-claude-access-crackdown-ecosystem-fallout.

---

## 2. Each competitor's gaps, and why they exist

| Competitor | Gap | Why (technical, business model, focus) |
|---|---|---|
| Conductor | Mac only; required broad GitHub OAuth at launch; built on the Agent SDK, which Anthropic now meters separately **[unverified]** | Focus: a Mac-native team. Business: cloud workspaces and multiplayer are the paid tier, so accounts come first. Technical: an SDK-driven GUI loses the CLI's own features and inherits SDK billing |
| Superset | Linux second-class, no Windows, sign-in required, no usage screen (#5733), ports and env per worktree unsolved ("I've never used lsof so much") | Focus: "nobody on the team daily-drives Linux or Windows" (HN 48251026). Business: per-seat cloud, so sign-in |
| cmux | Mac only (Linux issue #330: 220 reactions, the strongest signal found), flaky "needs input" status (#1027), no session restore (#2086) | Technical: native Swift/AppKit on libghostty doesn't port. Detects agent state from the terminal, not from hooks |
| Emdash | Closest feature set (cross-platform, setup scripts, SSH). Gaps: diff since last turn (#1635), local merge without PR (#1269) | Young; cloud plans unpriced |
| Vibe Kanban, Terragon, Crystal, Omnara | Gone or pivoted | No business model: free local orchestration doesn't pay, and cloud margins on others' models are thin |
| BridgeMind | Reviews call it buggy; $50/mo with credits **[unverified]** | Small team, broad suite |
| Warp | Outside agents are second-class (not in its agent panels); pricing changed three times in a year | Business: margin on metered inference (API +20%); the company's focus moved to cloud orchestration |
| Cursor | Lock-in to its harness and models; unpredictable usage; Linux AppImage crashes and broken updater | Business: reselling inference and pushing its own model (Composer) for margin. Technical: Electron/VS Code fork, Mac-first team |
| Zed | Claude Code via ACP lacks hooks, some slash commands and resume | Technical: ACP shows only what the adapter or SDK exposes. Its own agent comes first |
| JetBrains Air | Mac-only preview; no Anthropic subscriptions | Anthropic's terms on the Agent SDK |
| Claude, Codex desktop apps | One vendor's agent only; usage only for that vendor; Claude Desktop can't see CLI sessions; Debian-only Linux beta; no Linux dictation | Business: subscriptions are subsidized funnels, and each vendor wants to own the harness |
| Copilot Agent HQ | The only first-party multi-vendor runner, but cloud, PR-based and paid through Copilot, not the user's own plans | Business: sells Copilot seats and credits |
| Gemini CLI | Consumer users moved to a closed-source CLI | Vendor strategy |

**Patterns users complain about everywhere** (ranked by how often and how loud):
1. Linux and Windows missing or second-class (cmux #330, Superset #405/#2692, opcode with no binaries, JetBrains Air Mac-only, Cursor's AppImage).
2. Usage limits nobody can predict, and pricing that keeps changing (Claude #16157/#38335, Codex #28879, Copilot credits, Cursor, Warp, Kiro, Windsurf).
3. Remote/SSH work (cmux #1664, opcode #163, Nimbalyst #49, Conductor and Sculptor threads).
4. Sessions lost on quit, update or reboot (cmux #2086, Superset #3496, Claude #23800).
5. Not knowing an agent is waiting (cmux #1027/#2322, Codex/Claude auto-continuing after 60 s, Conductor thread "forget an agent paused").
6. Worktree environment setup: `.env` and other ignored files, setup scripts, ports (Claude Squad #260, its top issue; Vibe Kanban #1947; Conductor HN; a standalone "sync env files to worktrees" Show HN in July 2026).
7. Lock-in and harness churn (Anthropic's subscription rules; Gemini CLI; Roo Code; Windsurf; CLI updates breaking wrappers: Happy #149, Vibe Kanban #2794).
8. Privacy: forced sign-in (Superset #4894), default-on analytics (Vibe Kanban), broad OAuth scopes (Conductor), Trae's telemetry.
9. Review and merge: merge conflicts between parallel agents, diff since last turn, local merge, GitLab (Vibe Kanban's top issue #1697), stacked PRs, multi-repo.

---

## 3. Where Vibezeo stands

### Ahead
- **Linux as a first-class platform, today**, with the same features promised for macOS and Windows. Every Mac-only rival's top request.
- **Any vendor's CLI, unmodified, on the user's own subscriptions.** 11 engines in terminal panes; Claude Code, Codex and ACP engines as threads. No vendor app hosts a rival's agent.
- **No account, nothing leaves the machine by default**, no inference markup. Rivals that resell inference keep repricing.
- **Accounts across vendors:** several accounts per engine in separate provider homes, usage polling and a usage page across engines (F-AC-01…07). Claude's own apps lack multi-account (#18435).
- **Explicit authority:** inline approvals, risk-class plugin approvals, held actions for unattended runs, forced worktree removal only with a local confirm.
- **SSH workspaces** with session survival for 10 minutes and host-key pinning; restore after crash (F-SH-07).
- **Local Dictate on Linux** (neither vendor app has dictation on Linux).
- Teammates with memory and mail, a local task tracker over MCP: no orchestrator has these.

### At parity
- Worktree per agent with diff, merge and PR (F-CD-15); handoff between engines; notifications when an agent waits or finishes (F-DB-02/03); browser pane and element picker; files and git panels.

### Behind (before this branch)
- **Worktree setup:** no way to copy `.env` and other ignored files or run a setup command when a worktree is made. Conductor, Emdash, CCManager and Superset all have one.
- **Review loop:** no inline comments on a diff that go back to the agent (Warp, Claude Desktop, Vibe Kanban have it).
- **Merge conflicts between parallel worktrees** are found only at merge time.
- **GitLab:** Create PR is GitHub-only (`gh`).
- **macOS and Windows downloads** (in progress elsewhere), and only a `.deb` on Linux (no .rpm, Flatpak or AUR).
- **Free plan caps** (4 live panes, 3 workspaces) where Conductor, Superset, Emdash and Nimbalyst are free and uncapped locally.
- **Awareness:** no public community, issue tracker or launch yet.

---

## 4. Ranked opportunities

Scores: impact (I) and effort (E), each 1–5; rank by I − E, then I. "Owner"
says who should do it.

| # | Opportunity | Area | I | E | Owner | Why |
|---|---|---|---|---|---|---|
| 1 | **Worktree setup**: copy ignored files, run setup and teardown commands, explicit trust for repo-committed config (F-WT-01) | Product | 5 | 2 | this branch | Most-requested worktree fix across the category; rivals auto-run repo scripts without asking |
| 2 | **Review → agent**: line comments on a worktree's diff, sent as one prompt to its agent (F-WT-02) | Product | 4 | 2 | this branch | Parity with Warp, Claude Desktop, Vibe Kanban; closes the review loop |
| 3 | **Overlap check**: which worktrees touch the same files, and a conflict forecast before merge (F-WT-03) | Product | 4 | 2 | this branch | Nobody has it; answers "merge conflicts persist" |
| 4 | **GitLab merge requests** from a worktree (F-WT-04) | Product | 3 | 1 | this branch | Vibe Kanban's top issue |
| 5 | Positioning around the four true differentiators (Linux, any vendor's CLI, no markup, nothing leaves the machine) | Marketing | 5 | 1 | site session (via coordinator) | Each maps to a top complaint about rivals |
| 6 | Check Thread mode's use of `claude -p` against Anthropic's current terms and billing | Risk | 5 | 1 | owner (HUMAN_TODO) | Reported separate metering of `-p`/SDK use since June 2026 **[unverified]** |
| 7 | Lift the Free plan's local caps; charge for hosted, unattended and team features | Pricing | 4 | 1 | owner | Free local orchestration is table stakes; Vibe Kanban died charging for nothing hosted |
| 8 | .rpm, Flatpak and AUR packages | Distribution | 4 | 2 | packaging owner | Fedora/Arch users have no Claude desktop; ChatGPT's has no Flatpak/AppImage |
| 9 | Antigravity CLI engine (keep Gemini CLI for API keys) | Product | 3 | 1 | engines owner | Consumer Gemini CLI users were moved (verify the binary and flags first) |
| 10 | Public community: issues/discussions repo, changelog posts, a Linux launch | Community | 4 | 2 | owner | cmux #330's 220 Linux voters are an audience no rival serves |
| 11 | Show HN when macOS ships; r/linux, r/ClaudeAI, r/LocalLLaMA posts for the Linux build now | Marketing | 4 | 2 | owner | HN is where every rival launched |
| 12 | Diff since the last turn in the Changes panel | Product | 3 | 2 | later | Emdash #1635; checkpoints (M17) may cover it |
| 13 | Multi-repo workspaces | Product | 3 | 4 | later | Superset #5354, Claude Squad #56 |
| 14 | jj (Jujutsu) support | Product | 2 | 4 | later | Small but vocal |

Onboarding opportunities (all small): the first run already needs no account;
show "Detected: claude, codex…" and offer a worktree with setup in the first
launch; explain plan limits before the user hits them.

`docs/GO_TO_MARKET.md` turns rows 5, 7, 8, 10 and 11 into plans.
