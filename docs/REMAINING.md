# What's left to build

The work remaining between today's product and the finished Vibezeo, in
build order. Generated from `build-kit/MILESTONES.md` (feature ownership and
tasks) and `build-kit/PRODUCT_SPEC.md` (feature titles). Every feature below
has its behaviour and acceptance criteria in PRODUCT_SPEC under its ID, and
every milestone has its tasks, exit checks and E2E list in MILESTONES. Tick
items off here and in `BUILD_LOG.md` as they're done. `docs/FEATURES_TO_BUILD.md`
has every feature below with its full behaviour and acceptance tests.

**Where it stands (9 October 2026, `product-m0-m5`):** M0–M7 and M9 are done
(75 of 152 features, 54 of 65 P0). Left: 77 features in eight milestones
(M8, M10–M16), of which 11 are P0. M15 makes the P0/P1 product launchable;
M16 adds the P2 extras and the release candidate.

## 0. First, before new features

1. **Release 0.3.0 and deploy the site.** Vibezeo 0.2.0 (M6 + M7) and the
   browser demo are committed but were never deployed: vibezeo.com (`main`)
   still serves 0.1.0. M9 has landed since, and the demo already shows its Files
   tab, so ship them together: 0.3.0 = M6 + M7 + M9. Bump the version, add the
   CHANGELOG section, update the site copy (features, changelog M9, progress
   figures from the gate and the spec files), build and smoke-test the .deb,
   run the full E2E regression and the gate, then merge `product-m0-m5` into
   `main` and check the live site (`docs/OPERATING.md` §6–7).
2. **Close known gaps:**
   - The Free plan's `chatEngines` limit (Claude Code and Codex in Thread mode,
     PRODUCT_SPEC §3.1) is defined but not enforced; Free threads also run the
     ACP engines. Enforce it with the entitlement checks, with a test, and keep
     the site's plan copy matching.
   - Perf items marked [manual] (threads scroll fps, the 2 MB fence, the
     16-pane and startup suites) need the reference machines (`HUMAN_TODO.md`).

## 1. Features to build, by milestone

Build in this order; each milestone ends with its E2E specs, the full
regression, the gate, BUILD_LOG and a tag. Prerequisites are as MILESTONES
lists them (M9 was built ahead of M8, D-200).

### M8 · Dictate (local) and onboarding (XL) · 11 features, 9 of them P0
Push-to-talk anywhere: persistent audio capture, local Whisper/Parakeet, safe
insertion into the focused app, the widget, history and the dictionary; plus
the complete first-run onboarding.
- [ ] F-DI-01 Global push-to-talk (P0)
- [ ] F-DI-02 Hotkey engine per OS (P0)
- [ ] F-DI-03 Audio capture and microphones (P0)
- [ ] F-DI-04 Local speech models (P0)
- [ ] F-DI-06 Text insertion pipeline (P0)
- [ ] F-DI-07 Dictate widget (P0)
- [ ] F-DI-11 Onboarding and permissions (P0)
- [ ] F-DI-12 In-app composer dictation (P0)
- [ ] F-SH-08 First-run onboarding (P0; M5 built the framework, M6 the Accounts step)
- [ ] F-DI-08 History and stats (P1)
- [ ] F-DI-09 Dictionary (P1)

Tasks: capture; local models; dictionary engine; takes and insertion; hotkeys;
widget (UI-60); composer dictation (UI-68); setup and onboarding; history and
stats (UI-65); dictionary UI (UI-64); Settings › Dictate (UI-62/108) and the tray.
E2E-M8-1…8: push-to-talk, taps and cancel, guards, widget, composer, onboarding,
history and dictionary, model download.

### M10 · Teammates and gateway core (XL) · 12 features (P1)
Agent mode: persistent teammates with a brief, memory, skills, learning,
session search and teammate-to-teammate mail, all over the single local gateway
with leases.
- [ ] F-PL-01 Gateway process (M7 built its skeleton, D-4; in progress in pull request #5)
- [ ] F-AG-01 Teammate record
- [ ] F-AG-02 Agent rail
- [ ] F-AG-03 Teammate page and chats
- [ ] F-AG-04 Create teammate
- [ ] F-AG-05 Teammate settings
- [ ] F-AG-06 Briefing assembly
- [ ] F-AG-07 Memory
- [ ] F-AG-08 Learn after a turn
- [ ] F-AG-09 Skills
- [ ] F-AG-11 Session search tool
- [ ] F-AG-12 Teammate mail

Tasks: gateway; teammate record; briefing; memory (UI-73); skills (UI-74);
session search; Agent rail (UI-70); teammate page and chats (UI-71); settings
and creation (UI-72/75/76/107); learn after a turn; mail.
E2E-M10-1…8: teammates, briefing and learning, memory and skills, session
search, mail, settings, create with AI, gateway.

### M11 · Tasks and routines (L) · 8 features (P1)
A local task tracker agents read and write over MCP, with Pro execution in
lanes; scheduled routines that run in-app and in the background.
- [ ] F-TK-01 Task store
- [ ] F-TK-02 `tandem-tasks` MCP server
- [ ] F-TK-03 Tasks UI
- [ ] F-TK-04 Task execution (Pro)
- [ ] F-RT-01 Routine model (Pro)
- [ ] F-RT-02 In-app scheduler and runs
- [ ] F-RT-03 Background runner (Pro)
- [ ] F-RT-04 Routines UI

Tasks: task store; MCP server; Tasks UI (UI-80/81); task execution
(UI-82/83/84); routine model and schedules; scheduler; background runner;
Routines UI (UI-50…53).
E2E-M11-1…6: agents edit tasks, execute (Pro), routine, missed run, background, Free.

### M12 · Plugins and approvals (L) · 8 features (P1)
Connect external services once, grant them per teammate or thread, and enforce
risk-class approvals and held drafts. Engines never see a token.
- [ ] F-PL-02 Vendor modules
- [ ] F-PL-03 Connecting a plugin
- [ ] F-PL-04 Credential storage and injection
- [ ] F-PL-05 Grants
- [ ] F-PL-06 Risk classes and approval policy
- [ ] F-PL-07 Approval card
- [ ] F-PL-08 Held drafts queue
- [ ] F-PL-09 Initial catalog

Tasks: vendor modules; credentials; connect flows (UI-90/91/92); grants;
policy engine; approval card (UI-93); held drafts; catalog.
E2E-M12-1…6: OAuth, OAuth failures, grants, interactive approval, held draft, preview.

### M13 · Hosted services, credits and Team (L) · 10 features (P1)
The metered hosted features (credits ledger, AI and STT relays, cloud
Dictate, Enhance and Polish, credit packs), refunds, the Team plan with admin
policy and audit, and the transparency page.
- [ ] F-BE-05 Credits ledger
- [ ] F-BE-06 Hosted AI relay (titles and Enhance/Polish routes exist, D-103/D-106)
- [ ] F-BE-07 STT relay
- [ ] F-DI-05 Cloud speech-to-text (Pro, credits)
- [ ] F-DI-10 Enhance and Polish (Pro, credits)
- [ ] F-BL-03 Credit packs
- [ ] F-BL-05 Refunds
- [ ] F-BL-06 Team plan (P1 core; SSO is P2, M16)
- [ ] F-TL-07 Transparency page
- [ ] F-TL-10 Audit log (Team)

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
- [ ] F-RM-01 SSH workspaces and `tandem-host`
- [ ] F-RM-02 WSL workspaces (Windows)

Tasks: `tandem-host`; SSH transport; remote workspaces; WSL; infra (the
compose `sshd` profile).
E2E-M14-1…5: connect, host key, containment, re-attach, remote engines. WSL
needs a Windows machine (manual QA in `HUMAN_TODO.md` until CI runs Windows).

### M15 · Website, docs, data controls and launch hardening (L) · 6 features (2 P0)
Close every remaining P0 and P1 and every non-functional requirement. After
this milestone the P0/P1 product is launchable.
- [ ] F-WB-01 Marketing site (P0; the site exists, M15 completes it and checks it against the API's plans, AC3)
- [ ] F-TL-06 Threat-model rules (P0; invariant tests for every rule)
- [ ] F-WB-02 Docs
- [ ] F-WB-03 Changelog (in-app "What's new" too)
- [ ] F-WB-04 Downloads and machine-readable discovery
- [ ] F-TL-05 Local data controls (export and delete)

Tasks: marketing site; docs; changelog; downloads and discovery; data
controls; invariants; performance (§5.1); reliability (§5.2); security (§5.3);
accessibility (§5.4); i18n and observability; cleanup and docs.
E2E-M15-1…5: web, what's new, data controls, axe sweep, quality-bar run.
**Launch also needs** signed macOS and Windows builds (Apple Developer ID,
Azure Trusted Signing, GPG for Linux, updater keys, release hosting; see
`HUMAN_TODO.md`) and GitHub Actions minutes for the macOS/Windows runners.

### M16 · P2 differentiators and release candidate (XL) · 20 features (P2)
In independent sub-phases, each green and tagged (`m16a` … `m16d`), then the
release candidate. Stop after any sub-phase if the budget runs out.
- 16A Conductor (voice): [ ] F-CN-01 Call lifecycle · [ ] F-CN-02 Talker/doer
  architecture and tool catalog · [ ] F-CN-03 Delivery rules and safety ·
  [ ] F-CN-04 Conductor UI · [ ] F-CN-05 Find work by meaning · [ ] F-CN-06
  Persona and memory · [ ] F-CN-07 Wake phrase · [ ] F-CN-08 Audio routing for
  calls · [ ] F-BE-08 Voice session minting
- 16B Auto: [ ] F-AU-01 Auto (Pro, credits)
- 16C Phone companion: [ ] F-RM-03 Phone companion · [ ] F-BE-11 Remote relay
- 16D Extras: [x] F-SH-12 Scratchpad · [ ] F-CD-21 Simulator pane (macOS) ·
  [ ] F-AC-07 Usage dashboard · [ ] F-AG-10 Skill sync to engines ·
  [ ] F-AG-13 Teammate in a terminal · [ ] F-RT-05 Schedule tool for teammates ·
  [x] F-BE-03 CLI device-code sign-in · [ ] F-BE-12 Admin
- Also: app-shortcut rebinding (F-SH-09 P2 part, Settings › Shortcuts UI-113),
  Team SSO (F-BL-06 P2 part), the remaining AI routes (`/auto/next`,
  `/voice/act`, `/voice/resolve`).
E2E-M16-1…5: Conductor call, find work, Auto, phone, extras.

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
