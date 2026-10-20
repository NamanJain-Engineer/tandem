# DECISIONS: fixed choices every build-kit document must follow

These are final. Don't re-debate them during the build; if one proves impossible, log it in `BUILD_LOG.md` with the reason and the smallest deviation.

## Identity
- Working product name: **Tandem** (placeholder; rename before launch after a trademark check). Package prefix `tandem`, bundle id `com.vibezeo.tandem` (as shipped in `tauri.conf.json`; bundle ids are permanent once published, D-2100), API host `api.tandem.local` (dev).
- This is a **clean-room** product in the same category as BridgeMind. Never use BridgeMind names ("BridgeMind", "BridgeVoice", "Bridge", "BridgeSpace", "Hey Bridge"), logos, orb artwork, copy, or code. Own names:

  | Concept | Tandem name |
  |---|---|
  | Voice assistant | **Conductor** |
  | Wake word | "Hey Conductor" |
  | Dictation | **Dictate** |
  | Agent teammate | **Teammate** |
  | Scheduled job | **Routine** |
  | Remote helper | `tandem-host` |
  | Task MCP | `tandem-tasks` |
  | Plugin gateway | `tandem-gateway` |

## Platforms
- One codebase: **Tauri 2** on macOS 14+ (universal), Windows 10/11 x64 and Linux x86_64 (AppImage + deb).
- Dev and CI must work on Linux first. macOS and Windows builds happen in CI.

## Monorepo layout (pnpm workspaces + Cargo workspace)
```
tandem/
  CLAUDE.md                 # agent rules (copied from build-kit)
  BUILD_LOG.md              # progress log, decisions, deviations
  HUMAN_TODO.md             # things only the human can do
  apps/
    desktop/                # Tauri app
      src-tauri/            # Rust crate `tandem-desktop`: lib `tandem_app` (services: teammates, mail, Auto, task execution, restore, …) + bin `tandem` (thin Tauri shell + commands); depends on crates/*
      src/                  # React UI
    api/                    # backend (Hono + Drizzle + Postgres)
    web/                    # marketing site + docs + account portal (Next.js)
  crates/
    tandem-core/            # shared types, config, paths, errors, event bus
    tandem-store/           # SQLite (rusqlite) + file stores (memory/skills Markdown)
    tandem-pty/             # portable-pty sessions, scrollback, resize, restore
    tandem-engines/         # CLI detection/install, launch, provider homes, status hooks, thread adapters (claude stream-json, codex app-server, ACP)
                            #   bins: `tandem-hook` (status-hook helper, bundled sidecar) and `fake-engine` (test-only fixture CLI)
    tandem-git/             # worktrees, status, diff, commit, push (git2 + git CLI fallback)
    tandem-gateway/         # local MCP gateway: plugin registry, OAuth/API keys via keyring, credential injection, approvals
    tandem-tasks/           # stdio MCP server binary (bundled sidecar) + lib: local task tracker
    tandem-routines/        # scheduler (cron + interval), runner, OS launcher integration
    tandem-voice/           # audio capture (cpal), hotkeys, local STT (whisper-rs), paste/insert, realtime voice client
    tandem-host/            # SSH remote helper binary
  tests/
    invariants/             # `tandem-invariants` test-only crate (threat-model rule tests; added in M15)
  packages/
    ui/                     # React component library + design tokens (Tailwind preset)
    protocol/               # TS types generated from Rust (ts-rs) for IPC + API client
  infra/
    docker-compose.yml      # postgres 16, redis 7, mailpit, stripe-mock
  .github/workflows/        # ci.yml, release.yml
```

## Stack
| Layer | Choice |
|---|---|
| Desktop UI | React 19 + TypeScript + Vite + Tailwind 4 + Zustand + TanStack Query + react-resizable-panels + dnd-kit + cmdk |
| Terminal | xterm.js 5 + addon-fit + addon-webgl + addon-search + addon-web-links. **WebGL context pool (max 8 live); off-screen or overflow panes fall back to the DOM renderer.** |
| Rust | tokio, serde, portable-pty, rusqlite (bundled), git2, keyring, notify, cpal, whisper-rs, enigo/arboard (paste), global-hotkey, rmcp (MCP SDK), reqwest, tokio-tungstenite, ts-rs, tracing |
| Tauri plugins | updater, deep-link, single-instance, notification, shell, dialog, fs, os, global-shortcut, autostart, window-state |
| Backend | Node 22 + **Hono** + Drizzle ORM + Postgres 16 + Redis 7 + zod; auth via **better-auth** (email+password, magic link, Google OAuth, device-code for CLI, desktop PKCE); **Stripe** Billing (stripe-mock in dev); email via SMTP (mailpit in dev) |
| Web | Next.js 15 (App Router) + Tailwind + Fumadocs for docs |
| Tests | cargo test (unit + integration), vitest + Testing Library (UI), Playwright (web), WebdriverIO + tauri-driver (desktop E2E on Linux, via xvfb) |
| Observability | tracing to rotating local logs; Sentry and PostHog behind an **opt-in** toggle (off by default) |
| CI/CD | GitHub Actions matrix (ubuntu, macos, windows): lint, test, build; tag `v*` triggers a signed release (signing steps skip gracefully when secrets are absent) |

## Engine support (detect on the user's login-shell PATH)
- **Terminal launch** (all): claude, codex, cursor-agent, gemini, copilot, grok, opencode, droid, amp, aider, kimi.
- **Thread adapters:**
  - Claude Code: `claude -p --output-format stream-json --input-format stream-json --verbose`.
  - Codex: `codex app-server` (JSON-RPC over stdio).
  - Cursor, Grok, OpenCode: Agent Client Protocol (ACP).
- Tandem never reads, stores or proxies vendor tokens. Each account lives in its own **provider home** directory (e.g. `CLAUDE_CONFIG_DIR`, `CODEX_HOME`) under `~/.tandem/providers/<engine>/<account-id>/`.

## Business model (defaults)
- Free: 4 live panes, 3 workspaces, local Dictate, 1 Teammate besides the built-in Lead (the full matrix is PRODUCT_SPEC §3.2).
- Pro: $29/mo or $290/yr (7-day trial), unlimited, 10,000 credits a month.
- Team: $49/seat/mo or $490/seat/yr (min 2 seats, 14-day trial), 10,000 credits per seat a month pooled per org, SSO later.
- Credit packs (Pro/Team, web only): 5,000 for $5, 12,000 for $10, 30,000 for $20. 1 credit = US$0.001 of list value; per-feature prices are PRODUCT_SPEC §3.3.
- Credits meter only hosted features: cloud STT, prompt Enhance, the Conductor voice agent, the Auto supervisor.
- Entitlements are **always checked server-side** and cached in the app with a signed token (24h offline grace).

## Visual identity (Tandem's own)
- Dark and light themes from day one.
- Accent **#2DD4BF** (teal-400) in the dark theme; **#0F766E** (teal-700) in the light theme, because #2DD4BF and teal-600 fail WCAG AA on white. Status colours and exact values are the UI_SPEC §1.2 tokens (working sky, waiting amber, done green, idle grey, error rose). Neutrals: the near-black `ink` scale with gentle elevation (D-2600, at the owner's request on 20 October 2026; it replaced the slate scale). Radius 10px on panels and 8px on controls. Soft 1px borders. Inter plus JetBrains Mono.
- Teammate avatars: deterministic initials in a rounded square with a generated gradient (not geometric "faces").
- Conductor visual: a waveform ring (not an orb).

## Non-negotiable engineering rules
1. Every milestone ends green on the **global gate G** (root script `pnpm gate`; identical in BUILD_PROMPT, CLAUDE.md and MILESTONES §0):
   ```bash
   pnpm -r lint && pnpm -r test && cargo test --workspace && pnpm --filter desktop build
   cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check
   pnpm gen:protocol && git diff --exit-code packages/protocol
   ```
2. Secrets only ever go in the OS keychain (`keyring`), never in SQLite, logs or prompts.
3. Anything that needs human-owned accounts gets a **mock/dev provider** behind an interface, plus an entry in `HUMAN_TODO.md`, so the build never blocks on it.
4. Nothing leaves the machine by default. Opt-in only for telemetry and Auto scrollback.
5. No telemetry, crash report or log line contains prompt text, file contents or tokens.
