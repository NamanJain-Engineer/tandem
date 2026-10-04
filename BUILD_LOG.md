# BUILD_LOG

## Current state
- Repo: github.com/NamanJain-Engineer/tandem (private). Build kit in `build-kit/`; plan in `docs/PLAN.md`.
- Milestone: **M0 DONE** (tag `m0`). Next: **M1** backend (accounts, entitlements, remote config, mock billing).
- Layout matches DECISIONS: 10 crates under `crates/`, Tauri crate `apps/desktop/src-tauri` (lib `tandem_app`, bin `tandem`), `apps/{api,web}`, `packages/{ui,protocol}`.
- Environment: WebKitGTK, webkit2gtk-driver, xvfb, tauri-driver and cargo-audit are installed; `dockerd` must be started by hand in a fresh container (`dockerd &`), then `pnpm infra:up`.
- Gate: `pnpm gate` (lint, vitest, cargo test, desktop build, clippy, fmt, protocol drift) is green.
- Desktop E2E: `xvfb-run -a pnpm --filter desktop e2e` runs the real Tauri binary via tauri-driver.
- Open issues: none blocking. Root scripts `keys:dev`, `db:*`, `gen:shortcuts`, `gen:docs` arrive with M1, M3 and M15.

## M0 checklist (MILESTONES M0)
- [x] 1. Repo files: CLAUDE.md (from build-kit), BUILD_LOG, HUMAN_TODO, README, .editorconfig, .gitignore, rust-toolchain, .nvmrc
- [x] 2. Workspace wiring: pnpm workspace, root scripts, Cargo workspace with all ten crates + `tandem-desktop`, `unwrap_used = deny`, rustfmt
- [x] 3. `scripts/check-env.sh` (+ `bootstrap-linux.sh`)
- [x] 4. `tandem-core`: paths (DataDir), error (ErrorCode/IpcError/CoreError with stage + next step), atomic, clock (System/Mock), bus, settings schema, commands registry, keychain (OS via keyring + MemoryKeychain), ids, status store, supervisor
- [x] 5. Redactor F-TL-02: vendor formats, JWT, PEM, assignments, CLI flags, URL passwords, high-entropy near key words, registered-secret bloom; StreamRedactor 512-byte window. Corpus: 78 secrets, 1,248 real code lines
- [x] 6. Containment F-TL-03: canonicalize, `..`, symlinks, `%2e`, fullwidth and other look-alike dots/slashes, case-fold on case-insensitive FS, private-file policy; 51-line escape corpus
- [x] 7. Logging slice F-SH-11: tracing → size-rotating redacting file (10 MB × 5); diagnostics bundle with `~`
- [x] 8. `tandem-store`: WAL, one writer thread + 4 readers, migrations with backups, full ARCHITECTURE §4.2 DDL in `0001_init.sql`, settings repo, fractional sort keys
- [x] 9. Protocol generation: ts-rs → `packages/protocol/src/generated` via `pnpm gen:protocol`; typed API client skeleton
- [x] 10. `@tandem/ui`: tokens.css verbatim from UI_SPEC §1.2, self-hosted Inter + JetBrains Mono, Button with per-state snapshots, vitest + axe
- [x] 11. `apps/desktop`: Tauri 2 + React 19 + Vite + strict TS, bundle id `app.tandem.desktop`, CSP, least-privilege capability, ICU i18n catalog + `react/jsx-no-literals`, `gen:pseudo`, UI-02 splash
- [x] 12. `apps/api`: Hono + zod, `GET /v1/health`, RFC 9457 problems, request-id/client-header middleware, pino with redaction
- [x] 13. `apps/web`: Next.js 15 + Tailwind tokens, `GET /api/health`
- [x] 14. `infra/docker-compose.yml`: postgres:16, redis:7, mailpit, stripe-mock with healthchecks
- [x] 15. `pnpm dev` (`scripts/dev.ts`): compose up --wait, api/web/desktop with prefixed logs, `--no-desktop`, `--api-only`, Ctrl-C leaves infra
- [x] 16. CI: `.github/workflows/ci.yml` (lint, test-rust matrix, test-ts, desktop-build matrix, integration, audit), `security/waivers.toml`, `scripts/check-waivers.ts`, `scripts/ci-local.sh`
- [x] 17. Traceability: `scripts/check-coverage.sh` (152 IDs owned once), `scripts/trace-tests.ts`
- [x] E2E-M0-1 boot smoke (`apps/desktop/e2e/specs/m0-boot.e2e.ts`): title, splash from catalog, axe, clean logs
- [x] E2E-M0-2 dev orchestration (`scripts/smoke-dev.sh`): health endpoints in 7 s, SIGINT stops api/web, compose stays up

## Decisions
- D-1: F-CD-09 owned by M9. It needs the Files editor and the Tandem browser.
- D-2: F-SH-08 framework slice in M5, Accounts step in M6, owned by M8.
- D-3: F-DI-09 owned by M8 so dictionary actions exist before the Enhance interplay.
- D-4: M7 builds the `tandem-gateway` skeleton (loopback MCP, leases, approvals) as a slice of F-PL-01.
- D-5: M7 ships `/v1/ai/title` with a mock provider; the real provider lands in M13.
- D-6: Dashboard approvals are tested on the core ApprovalBroker in M5, again with a real thread in M7.
- D-7: Thread Running → Interrupted is tested on the sessions table in M5, in the UI in M7.
- D-8: "Create with AI" is tested against an API fixture in M10; the real route lands in M13.
- D-9: Threat-model rule tests land beside each mechanism; M15 owns F-TL-06 and audits completeness.
- D-10: Test-only dependencies (wiremock, fast-check, vt100, …), the `sshd` compose profile and the `tandem-invariants` crate don't change the runtime stack.
- D-11: One repository with independent deployables (desktop, API, web, sidecars). The user asked whether to split into several repos; DECISIONS fixes a monorepo, and the generated protocol types tie the desktop, API and web to one commit. Revisit only when a deployable gets its own release cadence.
- D-12 (consistency pass, CONSISTENCY_REPORT "Decisions made in this pass"): composer dictation uses the global Dictate key (⌥Space also on macOS; Alt+Space unbound on Windows/Linux); light accent teal-700 #0F766E; meter thresholds 80/95%; Free = 1 teammate besides Lead; thread modes stored `plan`/`act`, access `read`/`edits`/`full`; one main window; Plan/Act `Mod+Shift+P`, model picker `Mod+Shift+O`; global gate G is the 3-line MILESTONES gate.
- D-13: The earlier session's cloud sync log (`/v1/sync/events`, local `events` table) was removed. It has no feature ID, it isn't in the ARCHITECTURE §14.6 endpoint catalogue, and uploading local events by default would break DECISIONS rule 4.
- D-14: Unit-only Rust enums serialize as plain snake_case strings (`"forbidden_plan"`), not `{type: …}`. ARCHITECTURE §3.1's `tag = "type"` rule is applied to enums that carry data.
- D-15: The whole ARCHITECTURE §4.2 DDL ships as `0001_init.sql` now (MILESTONES M0 lists only four tables). No user data exists yet, and one shipped schema avoids migration churn across milestones.
- D-16: Redaction corpus lines mark each fake secret with «…» and split it with `{{}}` so repository secret scanners don't flag fixtures; the test strips the marker. The benign corpus is 1,248 real lines sampled from Python stdlib, C headers, npm/eslint sources and this repo. Two sampled lines that held a real-format token and a password-bearing URL were removed: they aren't benign.
- D-17: The desktop E2E injects axe-core into the webview with `browser.execute` because `@axe-core/webdriverio` opens helper windows, which a Tauri webview cannot. WebdriverIO runs with `wdio:enforceWebDriverClassic` (WebKitWebDriver has no BiDi).
- D-18: Vite 7 across the desktop app so vitest and the app share one Vite major.
- Deviation: the `keyring` crate is linked (Secret Service on Linux). `MemoryKeychain` is selected only with `TANDEM_FAKE_KEYCHAIN=1`.

## Log
- M0 (earlier session): first slices of core, engine registry, local store, desktop shell frame, API health, CI.
- M0: build kit and plan committed (`build-kit/`, `docs/PLAN.md`).
- M0: realigned to DECISIONS: crate rename to `tandem-engines`, seven new crates, Tauri crate with capabilities and CSP, ts-rs protocol generation, UI_SPEC tokens, ICU i18n, rewritten core (redactor, containment, settings schema, keychain, logging, store with writer thread and full schema), API restructured to ARCHITECTURE §14.1, web on the shared tokens, dev/gate/coverage/trace/CI scripts, desktop E2E harness. Both M0 E2E scenarios pass. M0 closed.
