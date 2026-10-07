# Tandem: analysis and build plan

This file is the entry point for anyone picking up the build. It explains what is being built, why the repository is shaped the way it is, how far the build has got, and the order of the remaining work. The binding detail lives in `build-kit/` (see "Source documents").

## 1. Source documents

| Document | Role | Size |
|---|---|---|
| `build-kit/DECISIONS.md` | Binding choices: names, stack, repo layout, business model, visual identity, engineering rules | 8 KB |
| `build-kit/PRODUCT_SPEC.md` | What to build. 152 features (`F-AREA-nn`), each with acceptance criteria | 211 KB |
| `build-kit/ARCHITECTURE.md` | How to build it. Crates, IPC commands and events, SQLite DDL, API endpoints, engine adapters, gateway | 261 KB |
| `build-kit/UI_SPEC.md` | Design tokens, components, 83 screens with wireframes and microcopy | 225 KB |
| `build-kit/MILESTONES.md` | Ordered plan M0–M16, with tasks, verification commands and E2E scenarios per milestone | 156 KB |
| `build-kit/BUILD_PROMPT.md` | The operating loop for a coding agent | 7 KB |
| Drive `bridgemind-analysis/BRIDGEMIND_SPEC.md` | Background research on the reference product. Used only to resolve ambiguity. Not committed here | 574 KB |

Precedence when documents disagree: DECISIONS > PRODUCT_SPEC > ARCHITECTURE > UI_SPEC > MILESTONES, except that MILESTONES sets the build order.

## 2. What the reference products are, and what Tandem builds

The research covers one company's product line. The live products, and what replaces each one here:

| Reference product | What it does | Tandem equivalent | Where |
|---|---|---|---|
| Desktop "agent super app", Code mode | Real terminal panes over local folders, hosting the user's own coding CLIs (Claude Code, Codex, Cursor Agent, Gemini, Copilot, Grok, OpenCode, …) | Code mode: workspaces, pane tree, PTYs, xterm.js, presets, worktree lanes, handoff, browser, files, git | M3, M4, M6, M9 |
| Thread mode | A coding session read as a chat, with approvals, plan → implement and a changes panel | Thread mode with Claude stream-JSON, Codex app-server and ACP adapters | M7 |
| Agent mode | Named teammates with memory, skills, routines, plugins and teammate mail | Teammates, Tasks (`tandem-tasks` MCP), Routines, plugin gateway (`tandem-gateway`) | M10–M12 |
| Dashboard, notifications, accounts | Live state of every agent; several subscriptions per engine via isolated config homes | Dashboard, notification center, provider homes, usage and limits | M4–M6 |
| Voice orchestrator | Realtime voice model that drives the app through a closed list of actions | **Conductor** | M16 (P2) |
| Push-to-talk dictation app | Global hotkey, local Whisper/Parakeet, cloud STT, Enhance, dictionary, history | **Dictate** | M8, M13 |
| Tickets MCP helper | Local Linear-style tracker exposed to agents over stdio MCP | `tandem-tasks` | M11 |
| Remote host helper, phone remote | SSH workspaces and an encrypted phone companion | `tandem-host`, WSL, phone PWA | M14, M16 |
| Backend | Accounts, entitlements, credits, billing, relays | `apps/api` (Hono, Drizzle, Postgres, Redis, Stripe) | M1, M13 |
| Website, docs, changelog, account portal | Marketing site with an in-browser demo, docs, `llms.txt` | `apps/web` (Next.js 15, Fumadocs) | M1, M2, M15 |

Products in the research that are **not** rebuilt, and why:

- The earlier "agentic development environment", the swarm feature, the mission runner and the hosted MCP were all absorbed into the desktop app. Their behaviour is already covered by Code mode, presets and lanes, Teammates and Routines, and `tandem-tasks`.
- The terminal coding-agent CLI was a fork of an open-source engine. PRODUCT_SPEC §6 puts reimplementing an engine out of scope: Tandem hosts engines and does not ship one.
- The screenshot tool, the video clipper and the benchmark site are marketing side projects, not part of the paid product. They can be specced separately if wanted.

### Clean-room rule

The goal is feature parity, not a copy. Behaviour and ideas are free to reimplement. Names, logos, artwork, UI copy and code are not, and a near-identical look invites trade-dress claims. So every feature is built from the build-kit specs under Tandem's own names (Conductor, Dictate, Teammate, Routine), its own visual identity (teal accent, initials avatars, waveform ring) and its own copy. The research document is never used as source material. DECISIONS.md and CLAUDE.md enforce this.

## 3. Architecture

### 3.1 One repository, several deployables

The repository stays a single monorepo (DECISIONS "Monorepo layout"; BUILD_LOG D-11). The request to split it was considered. Splitting would mean:
- duplicating the wire types that `ts-rs` generates from Rust into `packages/protocol`, which the desktop UI, the API and the web app all consume;
- losing the one global gate that proves the desktop, the API and the web app agree on every commit;
- coordinating releases across repos for changes that are almost always cross-cutting (a new entitlement touches the API, the protocol package and the desktop).

The deployables are still independent. Each has its own build, release cadence and runtime:

| Deployable | Path | Runtime | Ships as |
|---|---|---|---|
| Desktop app | `apps/desktop` (React UI) + `apps/desktop/src-tauri` (Rust) | Tauri 2 on macOS, Windows and Linux | Signed installers + update feed |
| API | `apps/api` | Node 22, Hono, Drizzle, Postgres 16, Redis 7 | Container |
| Web | `apps/web` | Next.js 15 | Container or edge |
| `tandem-hook`, `tandem-tasks` | `crates/tandem-engines`, `crates/tandem-tasks` | Rust | Sidecars bundled with the desktop app |
| `tandem-host` | `crates/tandem-host` | Rust, static musl | Uploaded to SSH hosts on demand |

A deployable moves to its own repository only once it has its own release cadence and on-call rotation, which in practice means the API after launch.

### 3.2 Components

```
Desktop (Tauri 2, one process)
  React UI ── IPC (commands + per-pane channels) ── tandem_app services
    tandem-core     paths, settings, bus, redactor, containment, clock, keychain, entitlements
    tandem-store    SQLite (WAL) + Markdown file stores
    tandem-pty      PTY sessions, scrollback ring, flow control
    tandem-engines  registry, detection, provider homes, status bus, thread adapters
    tandem-git      worktree lanes, status, diff, commit, push
    tandem-gateway  loopback MCP: plugin credentials, grants, approvals
    tandem-routines scheduler + OS launchers (runs with the app closed)
    tandem-voice    capture, hotkeys, local STT, insertion, Conductor client
  children: login shells → engine TUIs, chat engines on stdio, tandem-hook, tandem-tasks
Cloud
  apps/api   auth (better-auth + desktop PKCE), signed entitlements, credits ledger,
             Stripe billing, AI/STT relays, remote config, phone relay
  apps/web   marketing, docs, changelog, account portal, phone PWA
```

Load-bearing rules (DECISIONS "Non-negotiable engineering rules"):
1. Secrets live only in the OS keychain.
2. Nothing leaves the machine unless the user opts in.
3. Entitlements are enforced on the server and cached on the client as a signed token with a 24 h grace.
4. Tandem never reads, stores or proxies an engine vendor's tokens. Each account runs in its own provider home.
5. Logs never contain prompts, file contents or tokens.

## 4. State of the repository when this plan was written

A previous session built part of M0 without the build kit in the repository. Relative to DECISIONS and MILESTONES:

| Area | Present | Missing or different |
|---|---|---|
| Rust | `tandem-core` (redaction, containment, atomic writes, clock, bus, settings, commands, keychain mock), `tandem-store`, `tandem-engine` | Crate is named `tandem-engine`, not `tandem-engines`. No `tandem-pty`, `tandem-git`, `tandem-gateway`, `tandem-tasks`, `tandem-routines`, `tandem-voice`, `tandem-host` or `src-tauri`. No `ts-rs` export |
| Desktop | Vite + React shell frame | No Tauri crate (no WebKitGTK at the time), no i18n catalog, no design tokens from UI_SPEC §1.2 |
| API | Health, Problem Details, a sync-log endpoint | The sync log is not in the ARCHITECTURE §14.6 endpoint catalogue. No better-auth, billing or entitlements yet |
| Tooling | CI file, `scripts/dev.mjs`, compose file | No `check-coverage.sh`, `trace-tests.ts`, `ci-local.sh` or `gen:protocol`. The global gate does not yet include the desktop build or protocol drift |

The environment now supports more than it did: WebKitGTK and `webkit2gtk-driver` install with apt, and the Docker daemon starts, so Postgres, Redis, Mailpit and stripe-mock all run healthy. The two blockers recorded in BUILD_LOG (no Tauri build, no Docker) no longer apply.

## 5. Plan

The work follows MILESTONES.md in order. Each milestone ends with: the global gate green, every owned P0/P1 acceptance criterion covered by a test whose name carries its ID, BUILD_LOG updated, and a `m<n>` tag.

| M | Scope (owned feature IDs) | Runnable result at exit |
|---|---|---|
| **M0** Foundations | TL-02 redaction, TL-03 containment; slices of SH-11 logs, SH-04 settings, SH-09 commands | `pnpm dev` boots infra, API, web and an empty desktop window; gate green |
| **M1** Backend | BE-01 auth, BE-04 entitlements, BE-09 remote config, BE-10 edge, BL-01 checkout, BL-02 portal, BL-04 dunning | Sign up on the web, verify via Mailpit, upgrade in mock checkout |
| **M2** Walking skeleton | SH-01 window, SH-04 settings, SH-05 themes, SH-11 diagnostics, SH-13 deep links, BE-02 desktop sign-in, WB-05 portal, TL-01 telemetry | Sign in from the desktop, open a folder, type in a live shell, hit the Free limit |
| **M3** Code mode core | SH-02, SH-03, SH-09, CD-01…08, CD-10 | 16 real shells, split, drag, find, paste and drop at target performance |
| **M4** Engines | EN-01…06, AC-01, CD-11, CD-13 | Launch real or fake engines with live working/waiting/done status |
| **M5** Dashboard and restore | DB-01…03, SH-06, SH-07, SH-10, CD-12, TL-04 | Crash-safe restore, Dashboard wall, signed updates |
| **M6** Lanes and accounts | CD-14…16, AC-02…06 | Presets in worktrees, handoff, multi-account, limit cards |
| **M7** Thread mode | TH-01…16 | Conversations with Claude, Codex and ACP engines, approvals, plan → implement |
| **M8** Dictate | DI-01…04, DI-06…09, DI-11, DI-12, SH-08 | Push-to-talk anywhere with a local model; full first run |
| **M9** Files, git, browser | CD-09, CD-17…20 | Edit, diff, commit, browse, pick elements, click paths |
| **M10** Teammates | PL-01, AG-01…09, AG-11, AG-12 | Teammates with memory, skills, search and mail |
| **M11** Tasks and routines | TK-01…04, RT-01…04 | Agent-editable tasks; routines that run with the app closed |
| **M12** Plugins | PL-02…09 | Connected services, scoped grants, risk-class approvals, held drafts |
| **M13** Hosted services and Team | BE-05…07, DI-05, DI-10, BL-03, BL-05, BL-06, TL-07, TL-10 | Cloud STT, Enhance, credit packs, Team policy and audit log |
| **M14** Remote | RM-01, RM-02 | Terminals, threads and files on SSH hosts and WSL |
| **M15** Launch hardening | WB-01…04, TL-05, TL-06, all NFRs | Every screen passes axe, perf and invariant checks |
| **M16** P2 and RC | All 20 P2 features (Conductor, Auto, phone companion, …) | Release candidate |

The quality-bar walk-through from BUILD_PROMPT §4 becomes possible in stages: sign-up, open a folder and the Free gate in M2; 16 panes in M3; an engine thread in M7; dictation in M8; a routine in M11.

### 5.1 Execution order in this session

1. Bring the repository into line with DECISIONS (finish M0): rename `tandem-engine` to `tandem-engines`, add the remaining crates and the Tauri crate, generate protocol types with `ts-rs`, add the UI_SPEC tokens, the i18n catalog and the traceability scripts, and extend the gate.
2. M1, then M2, which gives the walking skeleton.
3. M3 onward, in order, as far as the session allows. `BUILD_LOG.md` records exactly where work stopped, so the next session resumes from there.

The research estimates parity at 5–7 months for 2–3 engineers. One session will not finish all 17 milestones. The aim is that every milestone that is closed is closed properly: tested, gated and tagged.

### 5.2 Things only a human can do

These are tracked in `HUMAN_TODO.md`. The build never blocks on them, because each one sits behind a mock provider:
- Stripe products, prices and webhook secret; Google OAuth client; SMTP provider; Turnstile keys; OpenRouter, OpenAI and STT keys.
- Apple Developer ID and notarization; Windows code signing; production Ed25519 signing keys; domains.
- Trademark search for the name "Tandem"; terms, privacy policy and subprocessor list.
- Physical checks that need hardware: microphone, global hotkeys, macOS permission prompts.
