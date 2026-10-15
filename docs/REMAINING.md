# What's left to build

The work remaining between today's product and the finished Vibezeo, in
build order. Generated from `build-kit/MILESTONES.md` (feature ownership and
tasks) and `build-kit/PRODUCT_SPEC.md` (feature titles). Every feature below
has its behaviour and acceptance criteria in PRODUCT_SPEC under its ID, and
every milestone has its tasks, exit checks and E2E list in MILESTONES. Tick
items off here and in `BUILD_LOG.md` as they're done. `docs/FEATURES_TO_BUILD.md`
has every feature below with its full behaviour and acceptance tests.

**Where it stands (12 October 2026, `product-m0-m5`):** M0–M14 are done, and
so are M15's six features and its non-functional checks (pull request #38; the
performance budgets still need the reference machines, `HUMAN_TODO.md`). All
152 of the original features are done: all 65 P0s, every P1 and all 20 of M16's
P2s; the macOS Simulator pane (F-CD-21) came with the `chain5` batch. The spec
has 165 IDs now, and the 13 added since are done: M17's five extras (F-EX-01
to F-EX-05) and worktree workflow (F-WT-01 to F-WT-04), and M16's phone apps
and Linux store packages (F-RM-04 to F-RM-07), so all 165 are. Conductor's
last five (F-CN-01 to F-CN-04 and F-CN-08) pass every acceptance criterion
against the dev mock, and the vendor's realtime client is built against its
published events (D-1120, on `m16a-conductor`); checking it against a recorded
session waits on the key (`HUMAN_TODO.md`). F-CN-06 and F-CN-07 (persona,
memory and wake phrase) are done too (D-1119). vibezeo.com serves 0.7.2 (15 October). Next is M18 (below): the top picks of the 15 October research, `docs/NEXT_FEATURES.md`, built by four sessions. The website's elite pass (D-1819, D-1830 to D-1844) and its blog (F-WB-06) are live on vibezeo.com (`chain22`). Routines and running
tasks are built and switch on with Pro; WSL workspaces wait for the Windows
download.

## 0. First, before new features

1. [x] **Release 0.3.0 with the interactive demo on the home page. And deploy the site.** Done: vibezeo.com serves 0.3.0 since 10 October 2026 (pull requests #18, #20 and #16; D-128, D-810). The demo moves from its own page into the home page, explorable full screen and on phones, with message limits and a reset (full spec: "The interactive demo on the website" in `docs/FEATURES_TO_BUILD.md`). Vibezeo 0.2.0 (M6 + M7) and the
   browser demo are committed but were never deployed: vibezeo.com (`main`)
   still serves 0.1.0. M9 has landed since, and the demo already shows its Files
   tab, so ship them together: 0.3.0 = M6 + M7 + M9. Bump the version, add the
   CHANGELOG section, update the site copy (features, changelog M9, progress
   figures from the gate and the spec files), build and smoke-test the .deb,
   run the full E2E regression and the gate, then merge `product-m0-m5` into
   `main` and check the live site (`docs/OPERATING.md` §6–7).
2. **Keep the live demo in step with every release.** vibezeo.com/demo (the page) opens /demo/app/: the real desktop interface running in the browser against a simulated core, built with the site (D-126; the Files tab, D-204). Its code is `apps/desktop/src/demo/`: `backend.ts` answers the app's commands, `agent.ts` scripts the agent's turns, `shell.ts` the terminal, `files.ts` the Files tab, `DemoBar.tsx` the strip that says it's a demo. When a release ships something visible in the interface, make it work in the demo too wherever it can be simulated honestly (sample data, no network, no microphone), add a test to `demo.test.ts`, and walk it with `scripts/agent/demo-walkthrough.mjs` before deploying. The demo shows only what the released download does, never more, and always says its agents are simulated.
3. **Close known gaps:**
   - [x] The Free plan's `chatEngines` limit (Claude Code and Codex in Thread
     mode, PRODUCT_SPEC §3.2) is enforced from the entitlement (D-127), and the
     site's plan copy matches.
   - Perf items marked [manual] (threads scroll fps, the 2 MB fence, the
     16-pane and startup suites) need the reference machines (`HUMAN_TODO.md`).
   - Six agents' first frames: the bisect blamed D-139's frame holding, fixed
     in 0.7.1 (~745 ms, as before 0.6.0). The ≤ 300 ms target is still
     missed: the spawn takes ~38 ms, and the rest is the webview opening six
     terminals at once (the performance session's open-path work, F-SH-07.7).
   - Memory (§5.1, D-138): hidden panes now hand their xterm to the core
     after 30 s and replay when shown (D-139; filled while hidden, 3.0 MB a
     pane, met). Still missed: panes filled on screen and then moved to a
     background tab (~60 MB each, budget 3.9 MB) and the base app (~617 MB
     PSS on 039dec5, budget 350). Measure again with `--suite memory`.

## 1. Features to build, by milestone

Build in this order; each milestone ends with its E2E specs, the full
regression, the gate, BUILD_LOG and a tag. Prerequisites are as MILESTONES
lists them (M9 was built ahead of M8, D-200).

### M8 · Dictate (local) and onboarding (XL) · 11 features, 9 of them P0 · done
Push-to-talk anywhere: persistent audio capture, local Whisper/Parakeet, safe
insertion into the focused app, the widget, history and the dictionary; plus
the complete first-run onboarding. **Done** (branch `m8-dictate-onboarding`,
merged 10 October 2026). F-DI-05 and F-DI-10, listed under M13, are done too.
- [x] F-DI-01 Global push-to-talk (P0)
- [x] F-DI-02 Hotkey engine per OS (P0)
- [x] F-DI-03 Audio capture and microphones (P0)
- [x] F-DI-04 Local speech models (P0)
- [x] F-DI-06 Text insertion pipeline (P0)
- [x] F-DI-07 Dictate widget (P0)
- [x] F-DI-11 Onboarding and permissions (P0)
- [x] F-DI-12 In-app composer dictation (P0)
- [x] F-SH-08 First-run onboarding (P0; M5 built the framework, M6 the Accounts step)
- [x] F-DI-08 History and stats (P1)
- [x] F-DI-09 Dictionary (P1)

The 14 October audit (D-1121's session) found spec behaviours under these ticks not built yet, and they are all built now: the bug fixes (D-520, D-521), F-DI-09's dictionary sync (D-523), and on macOS the automatic Fn → Control+Option fallback (F-DI-02) and F-DI-11's "Press 🌐 key to" Do Nothing with consent and undo (D-524, D-848). Checking the macOS pair on a real Mac is in `HUMAN_TODO.md`.

Tasks: capture; local models; dictionary engine; takes and insertion; hotkeys;
widget (UI-60); composer dictation (UI-68); setup and onboarding; history and
stats (UI-65); dictionary UI (UI-64); Settings › Dictate (UI-62/108) and the tray.
E2E-M8-1…8: push-to-talk, taps and cancel, guards, widget, composer, onboarding,
history and dictionary, model download.

### M10 · Teammates and gateway core (XL) · 12 features (P1)
Agent mode: persistent teammates with a brief, memory, skills, learning,
session search and teammate-to-teammate mail, all over the single local gateway
with leases. **Done in pull request #5** (branch `m10-teammates-gateway`); F-DI-12's mic in "Create with AI" followed once M8 merged.
- [x] F-PL-01 Gateway process
  - Not built: the behaviour's "terminal TUI opted into plugins". No acceptance criterion covers it, and terminal agents, local and on SSH hosts, get the tasks server only (D-1606).
- [x] F-AG-01 Teammate record
- [x] F-AG-02 Agent rail
- [x] F-AG-03 Teammate page and chats
- [x] F-AG-04 Create teammate
- [x] F-AG-05 Teammate settings
- [x] F-AG-06 Briefing assembly
- [x] F-AG-07 Memory
- [x] F-AG-08 Learn after a turn
- [x] F-AG-09 Skills
- [x] F-AG-11 Session search tool
- [x] F-AG-12 Teammate mail

Tasks: gateway; teammate record; briefing; memory (UI-73); skills (UI-74);
session search; Agent rail (UI-70); teammate page and chats (UI-71); settings
and creation (UI-72/75/76/107); learn after a turn; mail.
E2E-M10-1…8: teammates, briefing and learning, memory and skills, session
search, mail, settings, create with AI, gateway.

### M11 · Tasks and routines (L) · 8 features (P1)
A local task tracker agents read and write over MCP, with Pro execution in
lanes; scheduled routines that run in-app and in the background. **Done in
pull request #36** (branch `m11-tasks-routines`), with M16's F-RT-05.
- [x] F-TK-01 Task store
- [x] F-TK-02 `tandem-tasks` MCP server
- [x] F-TK-03 Tasks UI
- [x] F-TK-04 Task execution (Pro)
- [x] F-RT-01 Routine model (Pro)
- [x] F-RT-02 In-app scheduler and runs
- [x] F-RT-03 Background runner (Pro)
- [x] F-RT-04 Routines UI

Tasks: task store; MCP server; Tasks UI (UI-80/81); task execution
(UI-82/83/84); routine model and schedules; scheduler; background runner;
Routines UI (UI-50…53).
E2E-M11-1…6: agents edit tasks, execute (Pro), routine, missed run, background, Free.

### M12 · Plugins and approvals (L) · 8 features (P1)
Connect external services once, grant them per teammate or thread, and enforce
risk-class approvals and held drafts. Engines never see a token. **Done in
pull request #34** (branch `m12-plugins-approvals`). M11's runner names its
runs through `set_unattended_origin` (BUILD_LOG M12 item 7).
- [x] F-PL-02 Vendor modules
- [x] F-PL-03 Connecting a plugin
- [x] F-PL-04 Credential storage and injection
- [x] F-PL-05 Grants
- [x] F-PL-06 Risk classes and approval policy
- [x] F-PL-07 Approval card
- [x] F-PL-08 Held drafts queue
- [x] F-PL-09 Initial catalog

Tasks: vendor modules; credentials; connect flows (UI-90/91/92); grants;
policy engine; approval card (UI-93); held drafts; catalog.
E2E-M12-1…6: OAuth, OAuth failures, grants, interactive approval, held draft, preview.

### M13 · Hosted services, credits and Team (L) · 10 features (P1)
The metered hosted features (credits ledger, AI and STT relays, cloud
Dictate, Enhance and Polish, credit packs), refunds, the Team plan with admin
policy and audit, and the transparency page.
- [x] F-BE-05 Credits ledger
- [x] F-BE-06 Hosted AI relay (titles, Enhance/Polish and teammate drafts; `/auto/next` and the voice routes come with their M16 features)
- [x] F-BE-07 STT relay
- [x] F-DI-05 Cloud speech-to-text (Pro, credits): built on `m8-dictate-onboarding` over M13's relay client (D-516)
- [x] F-DI-10 Enhance and Polish (Pro, credits): built on `m8-dictate-onboarding` over M13's rewrite client (D-517)
- [x] F-BL-03 Credit packs
- [x] F-BL-05 Refunds
- [x] F-BL-06 Team plan (P1 core; the P2 SSO part is built on `m16-team-sso`)
- [x] F-TL-07 Transparency page
- [x] F-TL-10 Audit log (Team)

Tasks: ledger; AI relay; STT relay; cloud Dictate; Enhance and Polish; credit
packs; refunds; Team core; audit; transparency.
E2E-M13-1…7: cloud Dictate, Enhance, packs, Team, audit, transparency, refund.
**Going live needs the owner's accounts** (Stripe products and prices, SMTP,
the Claude API key, managed Postgres and Redis, hosting for `api.` and `app.`,
production signing keys; see `HUMAN_TODO.md`). Build and test against the mocks
and test modes until they exist.

### M14 · Remote: SSH and WSL (L) · 2 features (P1)
Terminals, agents, threads, files and git on SSH hosts through `tandem-host`,
and in WSL distros, never forwarding local credentials.
- [x] F-RM-01 SSH workspaces and `tandem-host`
- [x] F-RM-02 WSL workspaces (Windows)

Tasks: `tandem-host`; SSH transport; remote workspaces; WSL; infra (the
compose `sshd` profile).
E2E-M14-1…5: connect, host key, containment, re-attach, remote engines. WSL
needs a Windows machine (manual QA in `HUMAN_TODO.md` until CI runs Windows).
Done on `m14-remote-ssh-wsl` (PR #6): every task, E2E-M14-1…5 green; WSL is
unit-tested with fakes, and its Windows pass, real macOS/aarch64 hosts and the
CI-built binaries for all three targets wait in `HUMAN_TODO.md`.

### M15 · Website, docs, data controls and launch hardening (L) · 6 features (2 P0)
Close every remaining P0 and P1 and every non-functional requirement. After
this milestone the P0/P1 product is launchable.
- [x] F-WB-01 Marketing site (P0; D-128, D-132)
- [x] F-TL-06 Threat-model rules (P0; `tests/invariants`, one file per rule, D-131; rules 3, 4, 5, 7 and 8 gain cases as M8 and M10–M13, M16 build their paths)
- [x] F-WB-02 Docs (Fumadocs at /docs, tables generated from code; D-134)
- [x] F-WB-03 Changelog (in-app "What's new" too; D-133)
- [x] F-WB-04 Downloads and machine-readable discovery (D-135)
- [x] F-TL-05 Local data controls (export and delete: the desktop's export and delete, D-812; the web's Delete account and the server's 30-day purge, D-813)

Tasks: marketing site; docs; changelog; downloads and discovery; data
controls; invariants; performance (§5.1); reliability (§5.2); security (§5.3);
accessibility (§5.4); i18n and observability; cleanup and docs.
E2E-M15-1…5: web, what's new, data controls, axe sweep, quality-bar run.
**Launch also needs** signed macOS and Windows builds (Apple Developer ID,
Azure Trusted Signing, GPG for Linux, updater keys, release hosting; see
`HUMAN_TODO.md`) and GitHub Actions minutes for the macOS/Windows runners.

### M16 · P2 differentiators and release candidate (XL) · 24 features (P2)
In independent sub-phases, each green and tagged (`m16a` … `m16d`), then the
release candidate. Stop after any sub-phase if the budget runs out.
- 16A Conductor (voice), all 47 tools, the floating ring, the call-log setting, talk faster/slower and the vendor realtime client built (D-1114 to D-1120; the vendor client waits on a recorded session with the key, `HUMAN_TODO.md`): [x] F-CN-01 Call lifecycle · [x] F-CN-02 Talker/doer
  architecture and tool catalog · [x] F-CN-03 Delivery rules and safety ·
  [x] F-CN-04 Conductor UI · [x] F-CN-05 Find work by meaning (local search and the palette, D-814; the API's ranking, #30; teammates and the app's opt-in to the hosted ranking, D-1302; the Conductor's spoken answer comes with F-CN-02) · [x] F-CN-06
  Persona and memory (D-1119 wires it into calls; D-1121 the voice memory commands, reflection and journal) · [x] F-CN-07 Wake phrase (D-1119) · [x] F-CN-08 Audio routing for
  calls · [x] F-BE-08 Voice session minting (pull request #30, D-1000–D-1008)
- 16B Auto: [x] F-AU-01 Auto (Pro, credits)
- 16C Phone companion: [x] F-RM-03 Phone companion · [x] F-BE-11 Remote relay
- Phone apps and Linux stores (added on `mobile-apps`, D-2000 to D-2040): [x] F-RM-04 LAN pairing ·
  [x] F-RM-05 Vibezeo Remote app · [x] F-RM-06 Encrypted push notifications · [x] F-RM-07 Linux store
  packages (the owner's signing keys, store submissions, the iOS build and real-device checks: `HUMAN_TODO.md`)
- 16D Extras: [x] F-SH-12 Scratchpad · [x] F-CD-21 Simulator pane (macOS; manual QA on a Mac pending) ·
  [x] F-AC-07 Usage dashboard · [x] F-AG-10 Skill sync to engines ·
  [x] F-AG-13 Teammate in a terminal · [x] F-RT-05 Schedule tool for teammates ·
  [x] F-BE-03 CLI device-code sign-in · [x] F-BE-12 Admin
- Also: [x] app-shortcut rebinding (F-SH-09 P2 part, Settings › Shortcuts UI-113),
  [x] Team SSO (F-BL-06 P2 part), [x] the remaining AI routes (`/auto/next`,
  `/voice/act`, `/voice/resolve`; pull request #30).
E2E-M16-1…5: Conductor call, find work, Auto, phone, extras.

### M17 · Worktree workflow (M) · 4 features (P2)
From the competitive analysis (`docs/COMPETITIVE_ANALYSIS.md`): the worktree
gaps against the category. Built on branch `competitive-gaps`.
- [x] F-WT-01 Worktree setup (copy ignored files, setup and teardown, explicit trust)
- [x] F-WT-02 Review comments to the agent
- [x] F-WT-03 Overlap check and merge forecast
- [x] F-WT-04 GitLab merge requests
E2E-M17-WT-1…3: setup, review, overlap.

### M17 · Blog (S) · 1 feature (P2)
The owner's request of 15 October, built on `site-blog` with the elite pass's design (D-853 gives it this milestone).
- [x] F-WB-06 Blog (`/blog/`, posts as files in `apps/web/content/blog`, tag pages and an Atom feed; D-1850 to D-1864, `docs/BLOG.md`)

### M18 · Next features (in progress) · from `docs/NEXT_FEATURES.md`
The 15 October research ranked 21 features and six bold bets beyond the 165 (evidence in `docs/COMPETITIVE_ANALYSIS.md`). Its top picks are being built, each with its PRODUCT_SPEC entry first, tests from the criteria, an E2E spec (`m18-*.e2e.ts`) and its docs page:
- Engines (`m16d-teammate-extras`, D-2400 on): [x] F-EN-07 One instructions file for every engine (chain21, D-2400–D-2403) · [ ] F-EN-09 Engine catalog refresh · [ ] F-EN-08 Engine versions and updates under your control · [ ] F-TH-17 Session library across engines
- Review loop (`m16a-conductor`, D-2420 on): [x] F-WT-05 Second opinion from another engine · [x] F-DB-04 Review queue · [ ] F-CN-09 Walk-and-review (bold bet B4)
- Worktrees (`competitive-gaps`, D-2440 on): [x] F-WT-06 Worktree runtime (ports, env, Compose, `<slug>.localhost`; chain21, D-2440–D-2445, D-2448) · [ ] F-WT-09 Previews per worktree (bold bet B2; slice 1 of 3 in chain21, D-2446–D-2447: the preview command, Preview and Compare's Previews)
- Usage (`product-research`, D-2460 on): [x] F-AC-08 Usage forecast (chain21, D-2460–D-2462) · [ ] F-AC-09 Runaway guard · [ ] F-RT-06 The Night Shift (bold bet B1)
- Reliability (`perf-open`, D-2480 on, after §0.3's memory and first-frame work): [ ] F-SH-14 Keep agents running when the window closes

Later, in the research's order: F-TL-11 agent sandbox, F-WT-07 local merge queue, F-WT-08 CI results back to the agent, F-EN-10 any ACP agent as a thread engine, F-EN-11 local models, F-TK-05 claims, F-EX-06 resource governor, F-CD-22 multi-repo workspaces, F-CD-23 code intelligence, F-EX-07 activity ledger, F-CD-24 jj workspaces, and the bold bets B3 (the Foreman), B5 (agent-proof secrets) and B6 (intent radar). Its owner-only items (§4: terms, pricing, Linux-first exceptions, ARM64, launch) are the owner's.

## 2. What only a person can do

Code is never blocked on these (stub, list in `HUMAN_TODO.md`, continue), but
the product can't go live without them. `HUMAN_TODO.md` has the details:
- Accounts and keys: Stripe products and prices, SMTP and a sender, the Claude
  API key, Google OAuth, Cloudflare Turnstile, managed Postgres and Redis,
  `api.vibezeo.com` and `app.vibezeo.com` hosting, production Ed25519 keys,
  telemetry keys (opt-in only).
- Signing and distribution: updater (minisign) keys, Apple Developer ID and
  notarization, Azure Trusted Signing, a GPG key, release hosting, Actions minutes.
- Legal: trademark search, engine marks, reviewed UI labels.
- Manual QA on real machines (macOS, Windows, real engine installs, perf
  reference machines).
