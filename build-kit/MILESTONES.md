# MILESTONES: the ordered build plan for Tandem (M0–M16)

> **Audience:** the autonomous coding agent running the loop in `BUILD_PROMPT.md` §2.
> **Precedence:** DECISIONS > PRODUCT_SPEC > ARCHITECTURE > UI_SPEC > MILESTONES, except that this file's milestone order and §2 deviations are the binding build *order* (PRODUCT_SPEC §7.2). If this file conflicts with a higher document on anything else, follow the higher one and log it in `BUILD_LOG.md` under "Decisions".
> **Names:** crates, binaries and paths follow `DECISIONS.md`. IPC command and event names used here are *descriptive*. The binding names are the ones in `ARCHITECTURE.md`; add any that are missing there first (CLAUDE.md, IPC rule).
> **UI_SPEC** means `build-kit/UI_SPEC.md` (it was assembled from four drafts; mentions of "ui_part1…4" or "this part" inside it refer to its own sections, and the `_drafts/` folder is superseded and must not be used). Screens are cited by id (`UI-42`) and sections by number (§4.9).

---

## 0. How to use this plan

For each milestone, in order:
1. Copy its **Ordered tasks** into `BUILD_LOG.md` as a checklist, and map every task to feature IDs.
2. Implement the tasks **in the listed order**. The order respects the dependency levels (§2). Write tests first, from the acceptance criteria.
3. Run the milestone's **Verification** commands and the **global gate G** after every slice. Commit as `M<n>: F-XX-nn <what>`.
4. When every item on the **Exit checklist** is ticked, mark the milestone DONE in `BUILD_LOG.md`, append its **HUMAN_TODO** items to `HUMAN_TODO.md`, and run `git tag m<n>`.

**Terms used in every milestone**

| Term | Meaning |
|---|---|
| **Owns** | The features this milestone *closes*. At exit, every acceptance criterion of an owned feature has a passing automated test, or it is `[manual]` with a recorded proxy test. Each P0 and P1 ID is owned by exactly one milestone (see the §20 coverage table). |
| **Completes** | Behaviour bullets of features owned *earlier* that could only be finished once this milestone's feature existed. An example is the Title bar's Plan-usage chip, which lands with F-AC-04. The earlier feature's criteria are already green; this milestone adds the remaining behaviour and extends its tests. |
| **▸slice F-XX-nn → Mk** | A minimal, real and tested part of a feature owned by a *later* milestone `Mk`. It is built now because something here needs it. It is never a stub or a mock, and the owning milestone closes the rest. |
| **Verification** | Shell commands that must exit 0. They are run from the repo root with infra up (`pnpm infra:up`) unless noted. |
| **E2E-M&lt;n&gt;-&lt;k&gt;** | A named end-to-end scenario. It is implemented in the spec file named in the milestone. Every step and every expected result is an assertion. |
| **Size** | Agent effort. **S** ≤ 2 sessions, **M** 3–5, **L** 6–10, **XL** > 10 (a session is roughly one context window of work). |

**Global gate G** (root script `pnpm gate`; run before every commit; required at every milestone exit; identical to DECISIONS rule 1):
```bash
pnpm -r lint && pnpm -r test && cargo test --workspace && pnpm --filter desktop build
cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check
pnpm gen:protocol && git diff --exit-code packages/protocol
```

**Milestone close** (in addition to G):
```bash
pnpm tsx scripts/trace-tests.ts --milestone M<n> --fail-on-missing   # every owned P0/P1 criterion has a test
bash scripts/check-coverage.sh                                      # plan ↔ spec ID coverage (M0 creates it)
git tag m<n>
```

---

## 1. Shared conventions (created in M0, used everywhere)

**Package names (pnpm filters):** `desktop` (apps/desktop), `api` (apps/api), `web` (apps/web), `@tandem/ui` (packages/ui), `@tandem/protocol` (packages/protocol). The Tauri crate is `tandem-desktop` (`apps/desktop/src-tauri`).

**Dev ports:** API `127.0.0.1:8787`, web `localhost:3000`, Vite `1420`, Postgres `5432`, Redis `6379`, mailpit SMTP `1025` / UI+API `8025`, stripe-mock `12111`, mock update feed `8790`, fixture pages `5173`, sshd (profile `remote`) `2222`. `api.tandem.local` is used when `/etc/hosts` maps it. Otherwise every tool reads `TANDEM_API_URL` (default `http://api.tandem.local:8787`, falling back to `http://127.0.0.1:8787` for the run when the name doesn't resolve; ARCHITECTURE §17.5). Curl checks use `--resolve api.tandem.local:8787:127.0.0.1`, or plain `127.0.0.1`.

**Environment variables the code must honour**

| Variable | Effect |
|---|---|
| `TANDEM_HOME` | Data dir. Every test sets it to a temp dir. |
| `TANDEM_API_URL`, `TANDEM_WEB_URL` | Backend and web base URLs. |
| `TANDEM_FAKE_KEYCHAIN=1` | In-memory keychain backend (`MemoryKeychain`; tests and CI; ARCHITECTURE §2.2, §4.4). Production always uses `keyring`. |
| `TANDEM_E2E=1` | Enables the **test harness** (`window.__tandemTest` plus test-only IPC: counters for PTY resizes, WebGL contexts, reflows, spawn log, fake clock). It exists only in debug builds, and a release-build test asserts it is absent. |
| `TANDEM_SPAWN_LOG=<file>` | Every engine and PTY spawn appends a JSON line: argv, env *keys*, provider-home path, cwd. Values of secret-shaped env vars are replaced with `[REDACTED]`. Debug builds only. |
| `TANDEM_FAKE_CLOCK=<rfc3339>` | Starts the injected `Clock` at a fixed time. The test harness can advance it. |
| `TANDEM_OPEN_URL_CMD=<path>` | Replaces "open system browser" (E2E sign-in and OAuth use a scripted Playwright helper). |
| `TANDEM_NET_LOG=<file>` | Logs every outbound HTTP request from Rust (host, path, no bodies). Used for "nothing leaves the machine" tests. |
| `TANDEM_REAL_ENGINES=1` | Enables gated integration tests against real CLIs on PATH. |
| `TANDEM_API_TEST_ROUTES=1` | API mounts `/__test/*` (clock, plan, credits, mail helpers). A test asserts they return 404 without the flag or with `NODE_ENV=production`. |
| `BILLING_PROVIDER=mock\|stripe`, `AI_PROVIDER=mock\|claude`, `STT_PROVIDER=mock\|…` | Dev and test run on mocks. Mocks are chosen automatically when keys are absent. |

**Fixtures and fakes (locations are fixed):**
- `crates/tandem-engines/tests/fixtures/bin/`: one fake per engine command (`claude`, `codex`, `cursor-agent`, `gemini`, `copilot`, `grok`, `opencode`, `droid`, `amp`, `aider`, `kimi`). Each is a symlink to the `fake-engine` binary (ARCHITECTURE §6.12: a test-only `[[bin]]` of `tandem-engines`), which dispatches on `argv[0]` (or `TANDEM_FAKE_ENGINE`), plays the script in `TANDEM_FAKE_SCRIPT` and records to `TANDEM_FAKE_RECORD`. It supports three modes:
  - **TUI mode** (`TANDEM_FAKE_MODE=tui`): alt-screen, bracketed paste, capability queries, and logs received bytes to the record file.
  - **stream mode**: replays `fixtures/<engine>/<scenario>.script.jsonl`.
  - **login and install modes**.
- `crates/tandem-engines/tests/fixtures/<engine>/<scenario>.script.jsonl` plus `<scenario>.expected.json` (the goldens): recorded adapter streams and their expected normalized events (ARCHITECTURE §6.12).
- `crates/tandem-core/tests/redaction/{secrets.txt,benign.txt}`, and `crates/tandem-core/tests/containment/escapes.txt`.
- `crates/tandem-voice/tests/fixtures/*.wav`: generated once by `scripts/gen-voice-fixtures.sh` (espeak-ng + sox) and committed.
- `apps/api/test/fixtures/stripe/*.json`: webhook bodies, signed at test time with `whsec_test`.
- `apps/desktop/e2e/fixtures/pages/`: static test pages, served by `scripts/serve-fixtures.ts`.
- **Seeds** (`pnpm db:seed`): `free@tandem.test`, `pro@tandem.test` (active, 10,000 monthly credits), `pastdue@tandem.test`, `lapsed@tandem.test`, `admin@tandem.test` (role admin; added in M13), plus `owner@` and `member@` (added in M13). All use the password `Tandem-dev-pass-1` and are verified.

**Test naming:** Rust tests are named `f_cd_03_4_close_kills_tree` (feature, criterion, words). TS tests are named `"F-CD-03.4 close kills tree"`. `scripts/trace-tests.ts` parses these names and cross-checks them against the criteria in PRODUCT_SPEC.

**Test runners**
- **Desktop E2E:** `xvfb-run -a pnpm --filter desktop e2e -- --spec 'e2e/specs/m<n>-*.e2e.ts'` (WebdriverIO + tauri-driver).
  - The runner (`apps/desktop/e2e/run.ts`) does the setup: it creates a temp `TANDEM_HOME`, sets `TANDEM_FAKE_KEYCHAIN=1` and `TANDEM_E2E=1`, and prepends the fake-engine dir to `PATH`.
  - It starts the API with `scripts/dev.ts --api-only` if the API isn't already up.
  - It runs axe-core on every screen it visits.
- **Performance:** `xvfb-run -a pnpm --filter desktop perf -- --suite <name>`. Results go to `perf/results/<milestone>.json`, with thresholds from PRODUCT_SPEC §5.1.
- **Slow Rust tests:** marked `#[ignore = "slow"]`. They run explicitly with `-- --include-ignored` in milestone verification and in nightly CI. Marking a test slow is not skipping it.

---

## 2. Ordering rules and recorded deviations

**Rules**
1. Milestone order follows PRODUCT_SPEC §7.2. The level rule is read as: **P1 work of level N starts only after every P0 and P1 criterion of levels < N is green.** P0 work of a later level may be pulled forward when its technical dependencies exist. This is how M1–M2 bring backend and sign-in (level 4, all P0) forward to give a walking skeleton by M2.
2. Within a milestone, tasks are ordered so that lower-level items go green first.
3. P2 features come last (M16), after all P0 and P1 are green. If the budget runs out, the unfinished P2 items go into the final `BUILD_LOG.md` entry as next steps.
4. **Interim surfaces.** Until a mode's milestone lands, that mode's surface renders a development-only `ModeUnavailable` state behind the flag `interim.<mode>`. This exists only in debug builds. M15 deletes every `interim.*` flag, and `scripts/check-no-interim.sh` fails the build if any remain. Settings pages, sidebar tabs, menu items and title-bar controls for features that aren't built yet are **absent**, not disabled.

**Deviations from the spec's level table.** Copy these into `BUILD_LOG.md` → Decisions at M0.

| # | Item | Resolution | Why |
|---|---|---|---|
| D-1 | F-CD-09 (level 2) | Owned by **M9** (level 6). | Its acceptance needs the Files editor (F-CD-19) and the Tandem browser (F-CD-17). |
| D-2 | F-SH-08 (level 4) | Framework ▸slice in M5, Accounts step in M6; owned by **M8**. | Steps 3–4 need F-AC-02 (level 5) and F-DI-11 (level 6). |
| D-3 | F-DI-09 (level 8) | Owned by **M8** (level 6). | F-DI-06 post-processing and F-DI-08's "Add to dictionary"/"Correct" (level 6) need it. The Enhance interplay is tested with a fake rewrite client. |
| D-4 | F-TH-04 needs `tandem_approvals` on the gateway (level 7) | M7 builds the `tandem-gateway` skeleton (loopback MCP, leases, approvals built-in) as ▸slice F-PL-01 → M10. | Chat approvals are P0. |
| D-5 | F-TH-15 auto-title uses `/v1/ai/title` (F-BE-06, level 8) | M7 ships that route as ▸slice F-BE-06 → M13 (mock provider, free, rate-limited). | The title feature is free and P0. |
| D-6 | F-DB-01.3 and F-DB-02.1 (approval from the Dashboard) | Tested in M5 against the core `ApprovalBroker` with a fixture source; re-run with a real thread in M7. | Threads are level 5. |
| D-7 | F-SH-07.6 (thread Running → Interrupted) | M5 tests settlement on the `sessions` table; M7 adds the UI assertion. | Same as D-6. |
| D-8 | F-AG-04 "Create with AI" | Tested against an API fixture in M10 (as the spec's criterion says); the real route lands in M13. | Credits are level 8. |
| D-9 | F-TL-06 invariants | Each rule's test is written where its mechanism lands (rule 2: M3/M4; rules 1 and 4: M7; rule 6: M6/M9; rule 7: M2/M13; rules 3 and 8: M10–M12; rule 5: M6 for the local part, M16 for voice and phone). **M15** owns F-TL-06 and audits completeness. | The rules span areas. |
| D-10 | Dev-only additions | Test dev-dependencies (`wiremock`, `fast-check`, `vt100`, `hmac`, `axum` for loopback servers, `chrono-tz`, `ignore`), an `sshd` compose service (profile `remote`, M14), and a `tandem-invariants` test crate at `tests/invariants/` (M15). | They are needed for fixtures and tests, and don't change the runtime stack. |

---

## 3. Overview

| M | Title | Owns (count) | Size | Runnable result at exit |
|---|---|---|---|---|
| M0 | Foundations: repo, tooling, CI, infra, core libs | TL-02, TL-03 (2) | L | `pnpm dev` boots infra, API, web and an empty desktop window; the gate is green |
| M1 | Backend: accounts, entitlements, config, billing (mock) | BE-01, BE-04, BE-09, BE-10, BL-01, BL-02, BL-04 (7) | L | Sign up on the web, verify via mailpit, upgrade in mock checkout |
| M2 | Desktop shell, design system, sign-in (**walking skeleton**) | SH-01, SH-04, SH-05, SH-11, SH-13, BE-02, WB-05, TL-01 (8) | L | Sign up → desktop sign-in → open a folder → a live shell → the Free gate works |
| M3 | Code mode core: workspaces, panes, PTY, terminal fidelity | SH-02, SH-03, SH-09, CD-01…08, CD-10 (12) | XL | 16 real shells, split, drag, find, paste and drop, at target performance |
| M4 | Engines, provider homes, status bus | EN-01…06, AC-01, CD-11, CD-13 (9) | L | Launch real or fake engines; live working/waiting/done status |
| M5 | Dashboard, notifications, sidebar, restore, updater, release | DB-01…03, SH-10, CD-12, SH-07, SH-06, TL-04 (8) | L | Crash-safe restore, a Dashboard wall, signed updates, an unsigned release dry-run |
| M6 | Lanes, presets, handoff, accounts and usage | CD-14, CD-15, CD-16, AC-02…06 (8) | L | Launch a Fleet in worktrees, handoff, multi-account, limit cards |
| M7 | Thread mode | TH-01…16 (16) | XL | Conversations with Claude/Codex/ACP, approvals, plan → implement |
| M8 | Dictate (local) and onboarding | DI-01…04, DI-06…09, DI-11, DI-12, SH-08 (11) | XL | Push-to-talk anywhere with a local model; full first-run |
| M9 | Files, git, browser, links | CD-09, CD-17…20 (5) | L | Edit, diff, commit, browse, pick elements, click paths |
| M10 | Teammates and gateway core | PL-01, AG-01…09, AG-11, AG-12 (12) | XL | Persistent teammates with memory, skills, search and mail |
| M11 | Tasks and routines | TK-01…04, RT-01…04 (8) | L | Agent-editable tasks, scheduled routines, background runs |
| M12 | Plugins and approvals | PL-02…09 (8) | L | Connect services, scoped grants, risk-class approvals, held drafts |
| M13 | Hosted services, credits, Team | BE-05…07, DI-05, DI-10, BL-03, BL-05, BL-06, TL-07, TL-10 (10) | L | Cloud STT, Enhance, credit packs, Team policy and audit |
| M14 | Remote: SSH and WSL | RM-01, RM-02 (2) | L | Terminals, threads and files on SSH hosts and WSL |
| M15 | Website, docs, data controls, launch hardening | WB-01…04, TL-05, TL-06 (6) + all NFRs | L | A launchable P0/P1 product: every screen passes axe, perf and invariants |
| M16 | P2 differentiators and release candidate | all P2 (20) + BL-06 SSO extension | XL | Conductor, Auto, phone companion, extras; RC tagged |
| M17 | Extras | EX-01…05 (5) | L | Checkpoints and rewind, command recall, ports, Best of N, prompt library |

The quality-bar walk-through (BUILD_PROMPT §4) becomes possible in stages: sign-up, open a folder and the Free gate in **M2**; 16 panes in **M3**; an engine thread in **M7**; dictation in **M8**; a routine in **M11**. The full walk-through is automated as E2E-M15-5.

---
## M0 · Foundations: repo, tooling, CI, infra, core libs (L)

**Goal.** Go from an empty directory to the monorepo in DECISIONS. At exit, `pnpm dev` brings up the infra, the API, the web app and a desktop window; CI files exist; the gate is green; and the level-0 core libraries (redaction, containment, atomic writes, clock, event bus, settings and command-registry cores) are built and tested.

**Owns:** F-TL-02 (P0), F-TL-03 (P0)
**Early slices:**
- ▸slice F-SH-11 → M2: rotating logger, redaction layer, diagnostics bundle builder.
- ▸slice F-SH-04 → M2: typed settings schema and store.
- ▸slice F-SH-09 → M3: command registry data model.

**Prerequisites.** Node 22, pnpm 9, Rust stable, Docker with compose v2, and the Linux build dependencies. `scripts/bootstrap-linux.sh` lists them: webkit2gtk-4.1, webkit2gtk-driver, xvfb, librsvg2, libayatana-appindicator3, libasound2, libxdo, clang/cmake, espeak-ng, sox, vim, htop and less.

**Ordered tasks**
1. **Repo.** Run `git init tandem`. Copy `build-kit/CLAUDE.md` to the root. Create:
   - `BUILD_LOG.md`, with a "Current state" block of 40 lines or fewer, plus Decisions (containing D-1…D-10) and Log sections;
   - `HUMAN_TODO.md`, with the four groups from BUILD_PROMPT §6;
   - `README.md`, `.editorconfig`, `.gitignore` (which must include `.dev-keys/`, `target/`, `perf/results/*.tmp`), `rust-toolchain.toml` and `.nvmrc`.
2. **Workspace wiring.**
   - pnpm: `pnpm-workspace.yaml` (apps/*, packages/*).
   - Root `package.json` scripts: `dev`, `gate`, `lint`, `test`, `e2e`, `ci:local`, `infra:up`, `infra:down`, `db:migrate`, `db:seed`, `keys:dev`, `gen:protocol`, `gen:shortcuts`, `gen:docs`.
   - Cargo: a workspace with all ten crates from DECISIONS (`tandem-tasks` and `tandem-host` are bins) plus `apps/desktop/src-tauri`. Set workspace lints (`clippy::unwrap_used = "deny"` outside tests) and add `rustfmt.toml`.
3. **`scripts/check-env.sh`.** Check for node, pnpm, cargo, docker, xvfb-run, WebKitWebDriver and tauri-driver, and print what is missing. If it can't install something (no sudo), add a HUMAN_TODO entry.
4. **`tandem-core` modules**, each with unit tests:
   - `paths`: resolves `$DATA` and `TANDEM_HOME`, `$LOGS`, `$PROVIDERS`, `run/`.
   - `error`: `thiserror` errors that carry `stage` and `next_step`.
   - `atomic`: temp write, fsync, rename, directory fsync.
   - `clock`: a `Clock` trait with `SystemClock` and `FakeClock`.
   - `bus`: a typed tokio broadcast bus with per-topic channels.
   - `settings`: the typed key schema with defaults (the SH-04 core).
   - `commands`: the registry model (id, title key, category, default shortcut, enabled predicate id; the SH-09 core).
   - `secrets`: wraps `keyring` and provides an in-memory mock selected by `TANDEM_FAKE_KEYCHAIN=1`.
5. **Redactor (F-TL-02)** in `tandem-core::redact`.
   - Patterns: OpenAI, Anthropic, GitHub, Stripe, AWS, Slack and Google keys; JWTs; PEM private-key blocks; `password=` and `token=` assignments; high-entropy strings next to key words.
   - A `StreamRedactor` with a 512-byte sliding window.
   - A hook for a bloom filter of known credentials, filled in M12.
   - Corpus files: at least 60 secrets and 1,000 benign code lines.
   - Tests:
     - `f_tl_02_1_corpus_redacted_no_false_positives`
     - `f_tl_02_2_split_secret_every_offset`
6. **Containment (F-TL-03)** in `tandem-core::contain(root, path, policy)`.
   - Canonicalize the path. Reject `..` escapes, symlink escapes, `%2e%2e`, fullwidth dots, and NFC or case-fold lookalikes on case-insensitive filesystems.
   - Private-file policy: `.env*`, `*.pem`, `id_*`, and `.git/` internals for writes.
   - Test: `f_tl_03_1_escape_corpus_rejected_nested_accepted`, with symlink fixtures created in a temp dir.
7. **Logging core** (▸slice F-SH-11): `tracing-subscriber` with a rolling file appender (10 MB × 5) in `$LOGS`, plus a redaction layer that runs every formatted field through `redact`.
   - `diagnostics::bundle()` collects version, OS, flags and the last 200 sanitized lines, and replaces the home path with `~`.
   - Tests: `f_sh_11_1_secret_patterns_absent_from_log_file`, `f_sh_11_2_bundle_has_no_home_paths`.
8. **`tandem-store`.**
   - rusqlite (bundled) in WAL mode, with a migration runner and `migrations/0001_init.sql` (`settings`, `rail_items`, `workspaces`, `tabs`).
   - `Store::open(&Paths)` and helpers for atomic file stores.
   - A round-trip test for settings.
9. **Protocol generation.** `ts-rs` exports from `tandem-core` go to `packages/protocol/src/generated/`, driven by `pnpm gen:protocol`. `@tandem/protocol` re-exports them together with a typed API client skeleton.
10. **`@tandem/ui`.**
    - Tailwind 4 preset, with `src/styles/tokens.css` copied verbatim from UI_SPEC §1.2.
    - Self-hosted Inter and JetBrains Mono.
    - vitest, Testing Library and vitest-axe.
    - The first primitive, `Button` (UI_SPEC §2.1), with a snapshot per state.
11. **`apps/desktop`.**
    - Tauri 2, React 19, Vite and strict TS. Bundle id `app.tandem.desktop`; dev deep-link scheme `tandem-dev`.
    - The CSP from PRODUCT_SPEC §5.3, and a least-privilege `capabilities/main.json`.
    - i18n: `src/i18n/en.json` with ICU MessageFormat, an ESLint rule that bans literal JSX strings, and `scripts/gen-pseudo.ts` for `en-XA`.
    - At first launch the window shows only the UI-02 splash.
12. **`apps/api`.**
    - Hono, zod and Drizzle (with `drizzle.config.ts`).
    - `GET /v1/health` returns `{ok, version}`.
    - RFC 9457 Problem Details helpers.
    - Middleware that reads the `x-tandem-app/version/platform` and `x-request-id` headers.
    - A pino logger with body and header redaction.
    - vitest.
13. **`apps/web`.** A Next.js 15 App Router app with the Tailwind preset and `GET /api/health` only. Pages arrive in M1.
14. **`infra/docker-compose.yml`.** postgres:16, redis:7, mailpit and stripe-mock, each with a healthcheck.
15. **`pnpm dev`** (`scripts/dev.ts`).
    - Runs `compose up -d --wait`, `db:migrate`, then api (tsx watch), web (next dev) and desktop (tauri dev) with prefixed logs.
    - Flags: `--no-desktop` and `--api-only`.
    - Ctrl-C stops the child processes and leaves infra running.
16. **CI (`.github/workflows/ci.yml`).**
    - A matrix (ubuntu, macos, windows) runs install, lint, `cargo fmt --check`, clippy, `cargo test --workspace`, `pnpm -r test` and the desktop build.
    - An ubuntu-only `integration` job runs compose services, API tests, web Playwright, and desktop E2E under xvfb.
    - An `audit` job runs `cargo audit` and `pnpm audit --audit-level high`, with `scripts/check-waivers.ts` reading `security/waivers.toml` (entries expire).
    - Add `scripts/ci-local.sh` (`pnpm ci:local`), which runs the same steps without GitHub.
17. **Traceability tools.**
    - `scripts/check-coverage.sh`: every `F-XX-nn` in PRODUCT_SPEC appears on exactly one `**Owns:**` line in this file, and P2 IDs appear only in M16.
    - `scripts/trace-tests.ts`: lists the acceptance criteria of owned IDs that lack a test named after them.

**Mocks and fakes.** Mock keyring, `FakeClock`, and the redaction and containment corpora.

**Verification**
```bash
bash scripts/check-env.sh
pnpm install --frozen-lockfile
pnpm infra:up && docker compose -f infra/docker-compose.yml ps --format '{{.Service}} {{.Health}}' | grep -c healthy   # → 4
cargo test -p tandem-core f_tl_02 && cargo test -p tandem-core f_tl_03 && cargo test -p tandem-core f_sh_11
cargo test -p tandem-store
pnpm --filter @tandem/ui test
pnpm dev --no-desktop & sleep 20
curl -sf http://127.0.0.1:8787/v1/health | jq -e '.ok == true'
curl -sf http://localhost:3000/api/health
curl -sf http://127.0.0.1:8025/api/v1/info >/dev/null
curl -sf -u sk_test_123: http://127.0.0.1:12111/v1/customers >/dev/null
xvfb-run -a pnpm --filter desktop e2e -- --spec 'e2e/specs/m0-*.e2e.ts'
bash scripts/check-coverage.sh && pnpm ci:local
```
**E2E**
- **E2E-M0-1 Boot smoke** (`m0-boot.e2e.ts`). Launch with a fresh `TANDEM_HOME`. Expected: the window title is "Tandem"; the UI-02 splash renders from the i18n catalog; axe reports zero serious violations; `$TANDEM_HOME/logs/` contains a log file with no secret patterns.
- **E2E-M0-2 Dev orchestration** (`scripts/smoke-dev.sh`). Start `pnpm dev --no-desktop`. Expected: all health endpoints answer within 60 s, and SIGINT stops the api and web processes while the compose services stay up.

**Exit checklist:** G green · all four compose services healthy · `pnpm dev` works · CI files present and `pnpm ci:local` green · TL-02 and TL-03 criteria green · D-1…D-10 logged · README has setup and the `pnpm dev` section.
**HUMAN_TODO:**
- *Accounts & keys:* create the GitHub repository and enable Actions (the agent never pushes).
- *Manual QA:* install any system packages `check-env.sh` reported missing, if sudo wasn't available.

---

## M1 · Backend: accounts, entitlements, remote config, billing (mock) (L)

**Goal.** Make the server side of accounts and plans real:
- sign-up with email verification through mailpit;
- signed entitlement tokens that the client libs verify;
- signed remote config;
- mock Stripe checkout and webhooks, with lapse rules;
- rate limiting.

Also build the server half of desktop sign-in and the account portal pages that the desktop needs in M2.

**Owns:** F-BE-01, F-BE-04, F-BE-09, F-BE-10, F-BL-01, F-BL-02, F-BL-04 (all P0)
**Early slices:**
- ▸slice F-BE-02 → M2: server endpoints and the `/desktop-login` page.
- ▸slice F-WB-05 → M2: portal pages.
- ▸slice F-BE-05 → M13: `credit_buckets` and an append-only `credit_ledger` with the monthly grant only.

**Prerequisites.** M0.

**Ordered tasks**
1. **Drizzle schema and migrations** (names and columns per ARCHITECTURE §14.2):
   - better-auth tables (with `user.deletionRequestedAt` as the deletion-request marker);
   - `desktop_login_codes` (code hash, challenge, state, redirect_uri, expires_at, used_at);
   - `refresh_tokens` (family_id, parent_id, rotated_at, revoked_at, device fingerprint/name/platform; this is the device list behind `/v1/me/sessions`);
   - `subscriptions` (plan, status, period_end, cancel_at_period_end, past_due_since, trial_end);
   - `webhook_events` (event id primary key);
   - `credit_buckets`, `credit_ledger` and `credit_reservations`;
   - the remaining §14.2 tables as their milestones need them.
2. **Plans module** `packages/protocol/src/plans.ts`. It is the single source for the §3.1 prices, the §3.2 limits matrix and the §3.3 credit prices. The API, and later the web pricing page, llms.txt and docs, import it.
3. **better-auth (F-BE-01).**
   - Email and password: at least 10 characters, and a breached-password check. The check is `BREACH_CHECK=offline` in dev and tests, using `apps/api/src/auth/breached.txt`, which contains `password123`. HIBP k-anonymity is used in production.
   - Email verification sent over nodemailer SMTP to mailpit.
   - Magic link.
   - Google OAuth, enabled only when `GOOGLE_CLIENT_ID` is set.
   - Password reset links with a 30-min TTL.
   - Cookies are httpOnly, Secure and SameSite=Lax. Double-submit CSRF middleware protects cookie-authenticated state-changing routes.
   - `DELETE /v1/me` (`{confirm:'DELETE'}`), which creates a 30-day deletion request.
4. **Entitlements (F-BE-04).**
   - `GET /v1/me` returns the user, plan, status, limits, credits {monthly, purchased, resets_at}, flags and `entitlement_token`. The token is an Ed25519 JWS with a `kid`, holding `{sub, plan, limits, exp: now+24h}`.
   - `pnpm keys:dev` (`scripts/dev-keys.ts`) generates the keys into `.dev-keys/` and copies the public keys to `apps/desktop/src-tauri/resources/keys.json` (ARCHITECTURE §14.4, §17.5).
   - `requirePlan()` middleware returns `403 FORBIDDEN_PLAN`. Its table-driven test grows as each hosted route lands (M7, M8, M13, M16).
   - SSE on `/v1/me/events`, backed by Redis pub/sub for plan changes.
5. **Client verifiers in Rust** (`tandem-core::entitlements`, `tandem-core::remote_config`).
   - JWS verification against the bundled public keys, with `kid` rotation.
   - Offline grace: a token is accepted up to 24 h past `exp`; after that, Free limits apply.
   - Only an explicit inactive status downgrades the client. A network error never does.
   - The config verifier falls back to the bundled copy.
6. **Remote config (F-BE-09).** `GET /v1/config` returns a signed envelope `{payload, kid, sig}`. The payload holds the model catalog (`config/models.json`), engine registry overrides, price table, flags, `plugin_oauth_clients` and `min_version`. The response carries an ETag and `Cache-Control: max-age=600`, and returns 304 on a matching `If-None-Match`. Add middleware that sets `x-tandem-min-version` on every response.
7. **Billing (F-BL-01/02/04).**
   - A `BillingProvider` interface with `StripeProvider` and `MockProvider`.
   - `POST /v1/billing/checkout` for pro_monthly, pro_annual and team: 7-day trial with card required, promo codes allowed.
   - `POST /v1/billing/portal`.
   - `POST /v1/webhooks/stripe`: verifies the signature, stores the event id for idempotency, and handles `checkout.session.completed`, `customer.subscription.*`, `invoice.paid` (grant 10,000 monthly credits and zero the previous monthly balance with a ledger entry), `invoice.payment_failed` (sets `past_due_since` and sends the dunning email "Payment failed — update your card") and `trial_will_end`.
   - Lapse rules in `computePlan()`: past_due for more than 7 days, or canceled after `period_end`, means Free. Data is never touched.
   - Mock mode completes checkout instantly by feeding a synthetic event through the same webhook handler.
8. **Edge (F-BE-10).**
   - Redis token buckets: 300 req/60 s per IP, plus a per-user-per-feature helper.
   - A Turnstile-compatible verifier on the web sign-up and login routes only (an always-pass dev verifier when `TURNSTILE_SECRET` is unset). A valid bearer token bypasses it.
   - Stripe webhooks are allowed by signature, not by IP.
   - 401/403/429 counters per authenticated client. A threshold emits the structured log line `alert=auth_spike`.
9. **Desktop auth server side** (▸slice F-BE-02).
   - Allowlisted redirect URIs: `tandem://auth/callback`, `tandem-dev://auth/callback` and `http://127.0.0.1:<port>/callback`.
   - `POST /v1/auth/desktop/authorize`, called by the web page: returns a one-time `{code}` with a 5-min lifetime, bound to the challenge.
   - `POST /v1/auth/desktop/exchange {code, verifier}` returns an access JWT (1 h) and a rotating refresh token (30 d). A wrong verifier returns 400.
   - `POST /v1/auth/desktop/refresh`: reuse of an old refresh token revokes the whole family.
   - `POST /v1/auth/desktop/logout`, plus `GET /v1/me/sessions` and `DELETE /v1/me/sessions/:id` (device list and revoke).
10. **Web pages** (▸slice F-WB-05), built from `@tandem/ui`:
    - `/signup`, `/signin`, `/verify`, `/reset`;
    - `/desktop-login`, which shows "You can return to Tandem." after the redirect;
    - `/account` (email, password, delete);
    - `/billing` (plan, status, renewal, portal link);
    - `/billing/success` ("You're on Pro", with an "Open Tandem" button);
    - `/devices` (list with Revoke);
    - `/credits` (balance only for now).
11. **Test-only routes and tooling.** `/__test/clock`, `/__test/plan` and `/__test/mail/latest` (behind `TANDEM_API_TEST_ROUTES`). `scripts/stripe-fixture.sh <event> <email>` posts a signed fixture. Add `pnpm db:seed`. Generate the OpenAPI document (`apps/api/openapi.json`) from the zod routes.

**Mocks and fakes.** `MockBillingProvider`, signed Stripe fixtures, the offline breached list, the Turnstile dev verifier, mailpit, and the API clock via `/__test/clock`.

**Verification**
```bash
pnpm infra:up && pnpm db:migrate && pnpm db:seed && pnpm keys:dev
pnpm --filter api test                                          # includes F-BE-01/04/09/10, F-BL-01/02/04 suites
cargo test -p tandem-core f_be_04 && cargo test -p tandem-core f_be_09
pnpm --filter web test:e2e -- --grep @m1
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8787/v1/me                          # → 401
curl -sI http://127.0.0.1:8787/v1/me | grep -qi 'content-type: application/problem+json'
ETAG=$(curl -sI http://127.0.0.1:8787/v1/config | awk -F': ' 'tolower($1)=="etag"{print $2}' | tr -d '\r')
curl -s -o /dev/null -w '%{http_code}\n' -H "If-None-Match: $ETAG" http://127.0.0.1:8787/v1/config   # → 304
curl -s -o /dev/null -w '%{http_code}\n' -X POST -H 'stripe-signature: t=1,v1=bad' http://127.0.0.1:8787/v1/webhooks/stripe   # → 400
bash scripts/stripe-fixture.sh invoice.paid pro@tandem.test && bash scripts/stripe-fixture.sh invoice.paid pro@tandem.test
# then, signed in as pro@: GET /v1/me → .credits.monthly == 10000 (granted once)
curl -sI http://127.0.0.1:8787/v1/health | grep -qi 'x-tandem-min-version'
```
**E2E** (`apps/web/e2e/m1-*.spec.ts`, Playwright)
- **E2E-M1-1 Sign-up and verify.**
  1. Sign up as `new@tandem.test`. Expected: "check your email".
  2. Fetch the mailpit message and follow the link. Expected: verified.
  3. Sign in. Expected: `/account` shows the email.
  4. Try the password "password123". Expected: rejected as breached.
  5. POST `/v1/auth/change-password` with the session cookie but without the CSRF header. Expected: 403.
- **E2E-M1-2 Mock upgrade.** Sign in as free@ and subscribe to an SSE listener. Click Upgrade → mock checkout. Expected: `/billing/success` shows "You're on Pro"; the SSE invalidation arrives within 5 s; `/me` reports plan pro.
- **E2E-M1-3 Cancel at period end.** Cancel pro@ in the portal (mock). Expected: `/billing` shows "Pro until <date>". Advance the clock past `period_end`. Expected: `/me` reports Free limits.
- **E2E-M1-4 Dunning.** Send `invoice.payment_failed` for pastdue@. Expected: the dunning email is in mailpit. Advance the clock 8 days. Expected: Free limits, with the user's data rows unchanged.
- **E2E-M1-5 Webhook idempotency.** Replay `checkout.session.completed` twice. Expected: one subscription row and one ledger grant.

**Exit checklist:** G green · every criterion of the owned IDs is tested · rate-limit and bearer-bypass tests green · the OpenAPI document is generated · seeds documented in README.
**HUMAN_TODO:**
- *Accounts & keys:*
  - Google OAuth client (`GOOGLE_CLIENT_ID` / `GOOGLE_CLIENT_SECRET`, redirect `https://app.tandem.dev/api/auth/callback/google`).
  - Stripe account: products and prices for Pro $29/mo and $290/yr (7-day trial) and Team $49/seat/mo and $490/seat/yr (14-day trial); the customer portal configuration; dunning set to cancel after 14 days; the webhook endpoint and `STRIPE_WEBHOOK_SECRET`; `STRIPE_SECRET_KEY`.
  - SMTP provider credentials (`SMTP_URL`).
  - Turnstile site and secret keys.
  - Production Ed25519 keys for entitlements and config (`ENTITLEMENT_SIGNING_KEYS`, `CONFIG_SIGNING_KEYS`) with a `kid` rotation plan.
  - Managed Postgres and Redis.
  - Domains `api.tandem.dev` and `app.tandem.dev`.

---

## M2 · Desktop shell, design system and sign-in: the walking skeleton (L)

**Goal.** A signed-in desktop app with the real frame, design system, theming, settings, logging, deep links and opt-in telemetry. At exit you can sign up on the web, sign in from the desktop, see your plan, open a folder, type in a live shell, and hit the Free workspace limit.

**Owns:** F-SH-01, F-SH-04, F-SH-05, F-SH-11, F-SH-13, F-BE-02, F-WB-05, F-TL-01 (all P0)
**Early slices:**
- ▸slice F-SH-02 → M3: the segmented mode switch with mounted surfaces; Agent and Thread show interim surfaces.
- ▸slice F-SH-03 → M3: the Code rail list, add/select, and the footer with plan and credits.
- ▸slice F-CD-01 → M3: add a workspace via the picker or a deep link, dedupe, tabs, the Free limit of 3.
- ▸slice F-CD-03 → M3: one plain login shell per tab, over a per-pane channel.

**Prerequisites.** M1 (auth endpoints, `/me`, config, client verifiers).

**Ordered tasks**
1. **`@tandem/ui` primitives.** Build UI_SPEC §2.1–§2.19, §2.22 (CommandPalette shell) and the §2.31 composites (ListRow, PageHeader, SettingsRow, Banner, Callout, KeyValue, EmptyState, Stepper).
   - Add a `/__gallery` route (debug builds only) that shows every state.
   - Each state gets a vitest snapshot and a vitest-axe check.
   - Add a `ThemeProvider` with dark, light and system modes, derived high-contrast variants and reduce motion.
2. **Frame (F-SH-01).**
   - Window config: native traffic lights on mac; `decorations: false` on Windows and Linux.
   - The UI-01 title bar:
     - drag regions, with double-click to maximize;
     - custom min, max/restore and close buttons, whose label toggles between "Maximize" and "Restore";
     - logo and wordmark, with the amber DEV pill on dev builds;
     - left-sidebar toggle (`Mod+\`);
     - mode-switch slot, omnibox slot, account avatar menu;
     - the Workspace menu, rendered only when mode = Code.
   - Plugins: window-state (persisted in `$DATA/window-state.json`, re-centred when off-screen) and single-instance (focus the first instance, forward any deep link, and exit 0).
   - The UI §3.6 status bar: connection state and version.
3. **Theme (F-SH-05).**
   - Set the native window background from the persisted theme before the webview is shown, and inline the critical CSS.
   - System theme follows the OS live. Derive high-contrast variants. Reduce Motion forces `animation-duration` ≤ 0.01 ms.
   - Add the UI §1.9 terminal `ITheme`s. OSC 10/11 queries are answered with the theme colours through the xterm OSC handler.
4. **Settings (F-SH-04).**
   - The UI-100 shell: the rail becomes the nav with "‹ Back (Esc)"; Esc restores the previous mode and focus; Settings reopens on the last page viewed.
   - The filter search narrows pages and highlights matching controls.
   - IPC `settings_get_all`, `settings_set`, `settings_reset` (ARCHITECTURE §3.2), with values applied immediately.
   - Pages shipped now: General (UI-101), Appearance (UI-102), Terminal (UI-104; the keys exist and are consumed in M3), Privacy & Telemetry (UI-112), Account & Plan (UI-114), About / Advanced & Diagnostics (UI-116). The pages for the other areas arrive with their milestones.
   - A table-driven round-trip test over **all** schema keys runs automatically as keys are added.
5. **Logging and diagnostics (F-SH-11).**
   - Initialize tracing in the desktop app. Route UI console output through the `log_ui` command.
   - The app menu ("Tandem ▾" on Windows and Linux; the native menu on mac) and the Help items **Copy diagnostics** and **Open logs folder**.
6. **Desktop sign-in (F-BE-02)** and the UI-03 screen.
   - Generate PKCE, `state` and the device fingerprint. Listen on both an ephemeral 127.0.0.1 port and the `tandem-dev://auth/callback` deep link.
   - Open the system browser through `TANDEM_OPEN_URL_CMD` or the shell plugin, then call exchange.
   - Store tokens AES-GCM-encrypted in `$DATA/auth/credentials.enc`, with the key in the keychain (service `app.tandem.auth`, or `app.tandem.dev.auth` in dev builds; account `credentials-key`; ARCHITECTURE §4.3–4.4).
   - Refresh at 80% of the access token's lifetime, on OS resume, and on a 401.
   - Failures fall into four categories, each with Copy details. A mismatched `state` is ignored, and a later valid callback still succeeds.
7. **Entitlements and config in the app.**
   - Wire the M1 verifiers into a `useEntitlements()` hook.
   - Subscribe to `/me/events` over SSE.
   - Show the UI-133 banners (offline, signed out, past_due "Payment failed — update your card"). This completes the F-BL-04 banner.
   - The Account page shows plan, renewal, status and credits (completes F-BL-02 in-app), and a "Manage billing" link that opens the portal.
8. **Portal completion (F-WB-05).** Revoking a device on `/devices` revokes its refresh family. The desktop's next refresh fails and it shows the sign-in prompt.
9. **Telemetry (F-TL-01).**
   - `tandem-core::telemetry`: PostHog capture and the `sentry` crate, both in Rust so every request shows up in `TANDEM_NET_LOG`. Both are off by default.
   - An allowlist in `packages/protocol/telemetry-allowlist.json`. The wrapper drops unlisted properties.
   - Crash reports scrub `~` paths and every env value. Turning stats off deletes the analytics id.
   - Two separate toggles on the Privacy page. The onboarding question arrives in M5 (▸slice SH-08).
10. **Deep links (F-SH-13).**
    - A router with known routes only and a 4 KB length limit; anything else shows "Unrecognized link" and does nothing.
    - Routes now: `auth/callback` and `open?path=`. `thread/`, `teammate/`, `dictate/toggle` and `plugin/callback` are added in M7, M10, M8 and M12.
    - Launch at login (autostart, off) on the General page. The tray arrives with Dictate (M8) and background routines (M11).
11. **Skeleton slices.**
    - Code rail: Open workspace with a non-blocking picker, dedupe by canonical path, tabs.
    - Free plan: a 4th workspace opens the UI-120 upgrade sheet; Upgrade opens web checkout.
    - One plain-shell `TerminalView` per tab, using `tandem-pty` minimal spawn/read/write/resize/kill with the login-shell env and a Tauri `Channel<Vec<u8>>` per pane.
    - The mode switch with interim surfaces.

**Mocks and fakes.**
- `apps/desktop/e2e/helpers/browser-signin.ts`: the `TANDEM_OPEN_URL_CMD` target. It drives headless Chromium through `/desktop-login` and follows the redirect to the loopback callback.
- The mock keyring.
- The API in test mode.

**Verification**
```bash
cargo test -p tandem-desktop f_be_02 && cargo test -p tandem-core f_tl_01 && cargo test -p tandem-core f_sh_11
pnpm --filter @tandem/ui test && pnpm --filter desktop test -- -t "F-SH-0[145]|F-TL-01|F-SH-13"
pnpm --filter web test:e2e -- --grep @m2
xvfb-run -a pnpm --filter desktop e2e -- --spec 'e2e/specs/m2-*.e2e.ts'
grep -rEq 'sk-[A-Za-z0-9]{20}|ghp_[A-Za-z0-9]{20}' "$E2E_LAST_HOME/logs" && exit 1 || true   # runner exports E2E_LAST_HOME
grep -Eq 'posthog|sentry' "$E2E_LAST_HOME/net.log" && exit 1 || true
```
**E2E** (`apps/desktop/e2e/specs/m2-*.e2e.ts`)
- **E2E-M2-1 Walking skeleton.**
  1. Sign up on the web and verify through mailpit.
  2. Launch the desktop fresh and click **Sign in**. The scripted browser signs in. Expected: the tab shows "You can return to Tandem."; the rail footer shows "FREE".
  3. Open workspace `/tmp/ws1`. Expected: a tab with a live shell.
  4. Type `echo hi`. Expected: the output shows `hi`.
- **E2E-M2-2 Free gate.** Add 3 workspaces. The 4th opens the upgrade sheet. Mock checkout. Expected: within 5 s the footer reads "PRO · 10,000 credits" and the 4th workspace can be added.
- **E2E-M2-3 Deep links.** With the app closed, run the binary with `tandem-dev://open?path=/tmp/x`. Expected: `/tmp/x` is added as a workspace. Then try `tandem-dev://../../etc` and a 5 KB link. Expected: "Unrecognized link" and no state change.
- **E2E-M2-4 Single instance and window state.** A second launch exits 0 in under 1 s and the first window is focused. Maximize, quit and relaunch. Expected: maximized. A saved off-screen position: expected fully on-screen. On Linux, the maximize label toggles "Maximize"/"Restore". Tabbing through the title bar follows visual order, with every control named.
- **E2E-M2-5 Settings and theme.** Press `Mod+,`. Expected: Settings opens. Type "scroll". Expected: the Terminal page, with the scrollback control highlighted. Press Esc. Expected: the previous mode and focus return. Switch to the Light theme. Expected: the terminal palette changes within 200 ms, with the same PID. OSC 11 returns the background token. Set each key, relaunch: every value persists.
- **E2E-M2-6 Revoked device.** Revoke the device on the web and trigger a refresh through the test harness. Expected: the desktop shows the sign-in prompt.
- **E2E-M2-7 Nothing leaves by default.** Run a scripted session with telemetry off. Expected: `net.log` has no analytics or crash hosts. Turn stats on then off. Expected: the analytics id file is deleted.
- **E2E-M2-8 Diagnostics.** Help → Copy diagnostics. Expected: the clipboard holds version, OS and the last lines, with no `/home/` paths.

**Exit checklist:** G green · walking skeleton demoed (E2E-M2-1) · every M2 screen passes axe in both themes · release build has no test harness (test) · README "Sign in locally" section.
**HUMAN_TODO:**
- *Accounts & keys:* `SENTRY_DSN`, `POSTHOG_KEY` (opt-in only); production entitlement and config public keys, bundled at build into `apps/desktop/src-tauri/resources/keys.json` (ARCHITECTURE §14.4).
- *Manual QA:* the `tandem://` scheme on mac and Windows; mac overlay title bar with traffic lights; Windows snap layouts on the custom maximize button.

---
## M3 · Code mode core: workspaces, panes, PTY, terminal fidelity (XL)

**Goal.** Real terminal panes that behave like a native emulator at the §5.1 targets. Covers the full workspace, rail, mode-switch and command-palette behaviour.

**Owns:**
- P0: F-SH-02, F-SH-03, F-CD-01, F-CD-02, F-CD-03, F-CD-04, F-CD-05, F-CD-06, F-CD-07
- P1: F-SH-09, F-CD-08, F-CD-10

**Prerequisites.** M2.

**Order.** Tasks 1–7 finish level 1 and level-2 P0 before the P1 tasks 8–10, which is required by §2 rule 1. Tag `m3a` after task 7 as a checkpoint.

**Ordered tasks**
1. **`tandem-pty` sessions (F-CD-03).**
   - Spawn asynchronously, in parallel, over portable-pty (ConPTY on Windows).
   - Capture the login-shell env once with `$SHELL -lic env` (3 s timeout, cached). Set `TERM`, `COLORTERM`, `TERM_PROGRAM*`, `FORCE_COLOR`, `CLICOLOR` and `TANDEM_PANE_ID`, and never pass `TERM=dumb`.
   - One reader task per pane, coalescing output for 16 ms onto a per-pane channel.
   - A line-bounded ring buffer, plus a `vt100` shadow screen. The shadow screen answers DA1/DA2, DSR/CPR, XTVERSION and OSC 10/11 while the pane is hidden; xterm answers while it is visible. The switch between the two is atomic.
   - Kill the whole process tree: process group with SIGHUP → SIGTERM → SIGKILL at 2 s on Unix, a job object with `CREATE_NO_WINDOW` on Windows.
   - On Windows, resolve commands from PATH only (the cwd is excluded).
   - Orphans: keep a PID file with process start times. On the next launch, show the UI-132 "Stop them / Keep them" dialog.
   - Agent-terminal wrapper `$SHELL -lic '<cmd>; exec $SHELL -l'` (PowerShell equivalent on Windows). When the agent exits, the pane hands back to the shell and a Saved sessions row is written to `sessions`.
   - Flush the final output chunk before showing "Process exited (code N)". Plain shells get Restart / Close.
   - Enforce the live-PTY entitlement limit: the pane picker shows the upgrade card and no process is created.
   - On quit, write a scrollback snapshot (last 2,000 lines) to `$DATA/scrollback/<pane>.bin`.
2. **Layout engine (F-CD-02)** in `src/features/code/layout/`.
   - A pure-TS binary tree: split, remove, swap, move, maximize, serialize.
   - Property tests with fast-check prove the serialized layout round-trips.
   - Render with react-resizable-panels. The PTY resizes once, on pointer-up. Window resize is throttled to 50 ms. Minimum pane size is 240×120.
   - Drag and drop with dnd-kit: 4 px activation, drop zones computed on enter/leave, and a ghost that names the outcome ("Swap", "Split left/right/up/down", "New tab"). Esc cancels.
   - Maximize hides siblings without unmounting them. Focus ring, and `Mod+Alt+Arrow` to move focus.
   - Pane picker: Terminal now; engines join in M4 and Browser in M9.
3. **Workspaces and tabs (F-CD-01).**
   - `workspaces` gets a `parent_id` column for lanes.
   - Add a workspace by picker, by dropping a folder on the rail, or by deep link. Dragging a pane onto the rail creates a workspace for its cwd and keeps the same PID.
   - Missing folders show the warning state and block launches.
   - Tab strip: rename, close, reorder and `+`. Closing the last tab leaves an empty tab.
   - Layout persistence is debounced; M5 restore reads it.
4. **Rail (F-SH-03).**
   - A generic `RailList` used by all modes.
   - Pinned band by drag, reorder, and a context menu with every action in the spec. Removing an item needs confirmation and touches nothing on disk.
   - Missing folders offer Locate… / Remove.
   - Resizable from 180 to 360 px, and the width persists.
   - Footer: initials, plan tag with credits, appearance cycle, settings.
   - Status slots (Working n / Waiting on you / Failed / age / unread dot) are fed by M4.
   - Expanding a workspace lists its session rows and "Saved sessions (n)".
5. **Mode switch (F-SH-02).**
   - `radiogroup` "Work mode" with arrow keys and `Mod+1/2/3`, which also work from a focused terminal.
   - Surfaces stay mounted and are hidden with `display:none`.
   - The last mode is restored on launch. Tooltips and screen-reader announcements.
6. **Terminal rendering (F-CD-04).**
   - `TerminalView` (UI §2.27).
   - A WebGL context pool (max 8, held by the focused pane and the most recently visible ones). Other panes use the DOM renderer; hidden panes release their context after 8 s.
   - Frame pacing: the focused pane at 60 fps; unfocused panes coalesce to ≤ 30 fps; hidden panes don't render.
   - A "Jump to bottom ↓" pill instead of yanking a scrolled-up view.
   - DECSCUSR cursor shapes, the Unicode 11 addon, CJK and emoji fallback fonts, and scoped terminal CSS.
7. **Clipboard, drops and keys (F-CD-05, F-CD-06, F-CD-07).**
   - **Selection and copy:** a selection stays anchored while output streams. Shift+drag makes a local selection when the TUI captures the mouse. Ctrl+C copies if there is a selection, otherwise sends 0x03. Ctrl+Shift+C always copies. Copy-on-select is optional.
   - **Paste:** bracketed paste, with "Paste N lines?" for shells. Image paste into agent panes stages a PNG in `$DATA/staging/<pane>/` (swept after 7 days) and types its quoted path. "Copy last 100 / 500 / all lines" reads the ANSI-stripped shadow buffer.
   - **Drops:** every pane is a drop target, including a background tab after 600 ms of hover. `tandem-core::shellquote` quotes for POSIX, PowerShell and cmd, and refuses unsafe names. `tandem-core::wslpath` translates Windows paths. A drop never sends Enter.
   - **Keys:** the passthrough rule, and Escape always reaches TUIs. Shift+Enter sends the per-pane `newline_sequence` (the registry sets it in M4; default `ESC CR` for agents). Option-as-Meta is a setting. IME composition commits on Enter without sending CR.
8. **Command registry, palette and shortcuts (F-SH-09).**
   - Registry metadata lives in `tandem-core`; handlers are bound in TS.
   - cmdk palette (`Mod+K`) with fuzzy matching and recents first.
   - A shortcut dispatcher that implements the passthrough table.
   - Suppress webview shortcuts (F5, Ctrl+R/P/F/S, F3, devtools) in release builds.
   - An IME guard.
   - `pnpm gen:shortcuts` writes `packages/protocol/shortcuts.json`, which the docs use in M15.
9. **Find (F-CD-08).** A find bar using the search addon: case and regex toggles, "n of m", and next/previous with `Mod+G` / `Mod+Shift+G` or Enter / Shift+Enter.
10. **Shell integration (F-CD-10).**
    - Scripts in `crates/tandem-pty/shell-integration/{bash,zsh,fish,pwsh}`, injected via a ZDOTDIR shim, `--rcfile` or `-NoExit`.
    - OSC 7 and OSC 133 A–D marks carry a per-launch token; the Rust parser drops marks without it.
    - Uses: cwd tracking, "Copy last command output", ✓/✗ gutter decorations, idle detection for plain shells, and a per-shell disable setting.

**Mocks and fakes.** `fake-engine` in TUI mode (logs received bytes, enables bracketed paste, sends DA queries), synthetic DnD and IME events injected through WebDriver `execute`, `yes | head -c 50M` for streaming load, and CI-installed vim, htop and less.

**Verification**
```bash
cargo test -p tandem-pty && cargo test -p tandem-pty -- --include-ignored f_cd_04_5     # 1 GB memory bound
cargo test -p tandem-core shellquote && cargo test -p tandem-core wslpath
pnpm --filter desktop test -- -t "F-CD-0[2-8]|F-SH-0[239]|F-CD-10"
xvfb-run -a pnpm --filter desktop e2e -- --spec 'e2e/specs/m3-*.e2e.ts'
xvfb-run -a pnpm --filter desktop perf -- --suite terminals   # P95 key→echo ≤50 ms @16 streaming; focused ≥50 fps;
                                                              # 6 spawns framed ≤300 ms; mode switch ≤100 ms; DOM vs WebGL diff ≤1%
```
**E2E** (`m3-*.e2e.ts`)
- **E2E-M3-1 Sixteen shells.** Split to 16 panes. Expected: 16 distinct `$TANDEM_PANE_ID`s, at most 8 live WebGL contexts, and no blank or unstyled pane.
- **E2E-M3-2 TUIs.**
  - vim: `:e` a file, resize, `:q`.
  - htop: resize.
  - less: open a file and page through it.
  - Expected: the serialized screens match the goldens.
- **E2E-M3-3 Divider drag.** Drag a divider with 8 panes. Expected: exactly one `resize` per affected PTY, after pointer-up.
- **E2E-M3-4 Swap, maximize, cancel.**
  - Swap two panes. Expected: PIDs and scrollback preserved.
  - Maximize, then restore. Expected: no resize.
  - Press Esc mid-drag. Expected: the layout is back to its pre-drag state.
- **E2E-M3-5 Handback.** Run fake `claude` in an agent pane, then `/exit`. Expected: a shell prompt in the same cwd, and "Saved sessions (1)" in the rail.
- **E2E-M3-6 Tree kill.** Run `bash -c 'sleep 1000 & sleep 1000'` and close the pane. Expected: `pgrep -f 'sleep 1000'` is empty within 3 s.
- **E2E-M3-7 Free pane limit.** Open 4 panes, then try a 5th. Expected: the upgrade card, and the spawn log shows no new process.
- **E2E-M3-8 Orphans.** kill -9 the app with a live child, then relaunch. Expected: the dialog. Choose Stop. Expected: the PID is gone.
- **E2E-M3-9 Keyboard.**
  - Ctrl+W/T/D/[/] in a terminal. Expected: bytes 0x17, 0x14, 0x04, 0x1b, 0x1d reach the PTY.
  - Ctrl+Shift+D. Expected: the pane splits and nothing is sent.
  - Ctrl+C with and without a selection. Expected: copy vs 0x03.
  - F5 and Ctrl+R in the release variant. Expected: no reload.
  - `Mod+1` during `compositionstart`. Expected: no mode change.
  - Every registry command runs from the palette, through the same handler (spy).
- **E2E-M3-10 Paste and drop.**
  - Paste "a\nb" and confirm. Expected: `ESC[200~a\nbESC[201~`.
  - Paste an image into an agent pane. Expected: a staged PNG that decodes, and its quoted path typed.
  - Drop onto the 3rd of 4 panes. Expected: text lands in the 3rd pane.
  - Drop `a b'c`. Expected: `'a b'\''c'`.
  - Drop a filename containing a newline. Expected: refused with the toast.
- **E2E-M3-11 IME.** Compose "にほんご" and press Enter. Expected: the UTF-8 bytes once, with no CR during composition.
- **E2E-M3-12 Find.** Print "needle" at lines 10, 500 and 900. Expected: "1 of 3", and next/previous cycle through them.
- **E2E-M3-13 Shell integration.**
  - `cd /tmp` in bash and zsh. Expected: the stored cwd is `/tmp` within 200 ms.
  - A program prints `printf '\e]133;A\a'`. Expected: ignored.
  - "Copy last command output" after `ls`. Expected: exactly ls's output.
- **E2E-M3-14 Rail and modes.**
  - Add `/a/b` twice. Expected: one row.
  - Reorder tabs, pin by drag, relaunch. Expected: both persist.
  - Delete a folder on disk. Expected: warning style; Remove removes only the rail row.
  - Re-add the folder. Expected: its tabs and saved sessions return.
  - Opening a non-first workspace. Expected: the order doesn't change.
  - Switch Code → Thread → Code. Expected: PIDs and scrollback identical.
  - Press `Mod+2` from a terminal. Expected: Code mode.
  - Screen reader. Expected: "Work mode, Code, selected, 2 of 3".
- **E2E-M3-15 Hidden DA.** A program in a hidden pane writes `ESC[c`. Expected: a response within 100 ms.
- **E2E-M3-16 Picker non-blocking.** Run a palette command while the folder picker is open. Expected: the command completes.

Windows CI additionally runs `cargo test -p tandem-pty f_cd_03_6` (the malicious `claude.cmd` fixture) and `f_cd_06_4` (WSL drop translation).

**Exit checklist:** G green · `perf/results/m3.json` meets every threshold (or records the reference-machine gap as `[manual]`) · tree-kill verified on Linux and Windows CI · TL-06 rule 2 (shell marks) test `invariant_rule_2_marks_need_token` added in `tandem-pty` (M15 gathers every rule test into `tests/invariants/`, see D-9).
**HUMAN_TODO:** *Manual QA:* real Japanese IME on mac and Windows, Option-as-Meta, ConPTY TUIs (vim, htop), and the 16-pane benchmark on the reference machines.

---

## M4 · Engines, provider homes and status bus (L)

**Goal.** Detect and launch the 11 engines in isolated provider homes, and know every agent's state from hooks and titles rather than scraping. Adds the launch grid, install sheet, readiness and typing, and Arrange/Card view.

**Owns:**
- P0: F-EN-01, F-EN-02, F-EN-04, F-EN-05, F-AC-01, F-CD-13
- P1: F-EN-03, F-EN-06, F-CD-11

**Completes:** F-CD-03 (Saved sessions get the engine session id from hooks); F-SH-03 (live status slots and unread dots).

**Prerequisites.** M3.

**Ordered tasks**
1. **Registry (F-EN-02).** `crates/tandem-engines/registry.json` (ARCHITECTURE §6.1) covers all 11 engines.
   - Fields from the spec: `id`, `label`, `vendor`, `commands`, `install{mac,win,lin}`, `login_cmd`, `logout_cmd`, `version_cmd`, `chat_adapter`, `acp_cmd`, `provider_home_env`, `resume_cmd_template`, `newline_sequence`, `hooks_support`, `usage_log_globs`, `skills_dir`, `logo`.
   - Added fields (already in the ARCHITECTURE §6.1 schema): `status_cmd` (auth status probe), `cheap_model`, `title_patterns`, `ready_patterns`, `waiting_patterns`, `limit_patterns` and `keychain_isolation`, plus `login_mode`, `home_strategy`, `supports_resume`, `onboarding_seed`, `mcp_native`, `adapter_versions`, `version_regex`.
   - JSON-schema validation, and signed remote overrides merged in from `/v1/config`.
   - Logos are original monogram glyphs (see HUMAN_TODO about vendor marks).
2. **Detection (F-EN-01)** per host (local now; SSH and WSL hosts in M14).
   - Probe with `$SHELL -lic` `command -v` (PowerShell `Get-Command` or `where` on Windows).
   - Check the known locations: `~/.local/bin`, `~/.claude/local`, the npm global prefix, nvm, bun, mise, volta, Homebrew, `%APPDATA%\npm`, `%LOCALAPPDATA%\Programs`.
   - `--version` with a 2 s timeout, and an 8 s overall deadline. A slow shell gets its own diagnostic.
   - Cache in `engine_installs`. Detect at startup, on focus (throttled to 60 s) and on Check again, always off the UI thread.
3. **Provider homes (F-AC-01).**
   - `$PROVIDERS/<engine>/<account-id>/` with mode 0700, and an `accounts` table.
   - A default "Default" account per engine is created on first use.
   - Merge writers that preserve user keys: `serde_json` for Claude `settings.json`, `toml_edit` for Codex `config.toml`.
   - One-off homes are deleted on pane close unless the user picks Keep.
   - Engines with `keychain_isolation: "unsupported_mac"` are limited to one account on macOS, with the explanatory copy.
   - The spawn builder refuses to pass a local home to a remote host kind (tested with the host-kind enum; exercised for real in M14).
4. **Launch environment (F-EN-04).**
   - A `LaunchSpec` builder: login env, `TANDEM_*`, provider-home env and status variables.
   - Argv is scanned with the redactor's patterns; a match refuses the spawn.
   - Claude: hooks are written into the provider home's user settings, and onboarding is pre-seeded. The trust answer is pre-seeded only if "Trust workspaces I add" is on.
   - Codex: `CODEX_HOME`, plus `notify` pointing to `tandem-hook`.
   - A file/stdin helper for large inputs.
   - Every spawn goes to `TANDEM_SPAWN_LOG`.
5. **Status bus (F-EN-05).**
   - A UDS server at `$DATA/run/status.sock` (loopback on a random port on Windows) with HMAC-SHA256 verification. The secret is rotated per launch.
   - `tandem-hook`, a `[[bin]]` in `tandem-engines`, reads the hook JSON from stdin, strips transcript content, and posts only event, tool and session id.
   - Mappings: Claude hooks as in the spec; Codex `notify`; OSC 0/2 title patterns; fallback heuristics (2 s activity → working, prompt pattern → waiting, 30 s quiet → done, idle after 10 min).
   - Idle stability: output within 500 ms of a resize or repaint doesn't count as activity.
   - Events are ordered by ts per pane, and stale ones are suppressed.
   - A snapshot table and a watchdog heartbeat.
   - The rail and the basic `PaneChrome` status dot subscribe to it.
6. **Launch grid (F-CD-13, UI-10).**
   - Installed engines first, as 2-column tiles; "N more not installed ▾" with Install buttons; the Terminal tile.
   - "Launch several…" is flagged on in M6.
   - With nothing detected: the diagnostics (shells and folders probed, the likely fix) and **Check again**.
7. **Install (F-EN-03, UI-33/34, Settings → Engines UI-105).**
   - The sheet shows the registry's exact command, with Copy and **Run in terminal**. Run opens a visible pane and never adds sudo.
   - When the command exits, re-detect and show "Installed ✓ — Sign in next".
   - The Engines page: path, version, Check again, and extra launch args per engine.
8. **Titles and readiness (F-EN-06).**
   - Title priority: hook > OSC (spinner glyphs stripped) > first prompt (8 words) > cwd basename.
   - A readiness detector (first OSC title, a ready pattern, or 1.5 s quiet after the first output).
   - `type_prompt(pane, text)`: strip control characters except `\t`; send as one bracketed paste; send Enter as a separate write 50 ms later. A pane that is waiting refuses with "<name> is waiting for your approval".
9. **Arrange, Card view and collapsed sessions (F-CD-11).**
   - Arrange is a pure grid function (cols = ceil(√n)) and resizes each pane once.
   - Card view shows title cards with live status and no PTY resize.
   - "Show terminal sessions" collapses panes, persisted per workspace.
   - These are items in the title-bar Workspace menu. "Usage…" is added in M16.

**Mocks and fakes.**
- Fake binaries for all 11 engines.
- `tests/fixtures/slowshell/.bashrc`, which sleeps 20 s.
- An nvm-layout fixture tree.
- A fake installer script that creates a fake binary.
- Hook stdin fixtures that include transcript text.
- A remote-config fixture adding the engine `zed-agent`.

**Verification**
```bash
cargo test -p tandem-engines && cargo test -p tandem-engines --features fixtures -- --include-ignored f_en_01_2
cargo test -p tandem-core f_en_05
S="$TANDEM_HOME/run/status.sock"; curl -s --unix-socket "$S" -o /dev/null -w '%{http_code}\n' \
  -H 'x-tandem-signature: deadbeef' -d '{"pane_id":"p1","event":"done","ts":1}' http://localhost/v1/status   # → 401
pnpm --filter desktop test -- -t "F-CD-11|F-CD-13|F-EN-06"
xvfb-run -a pnpm --filter desktop e2e -- --spec 'e2e/specs/m4-*.e2e.ts'
TANDEM_REAL_ENGINES=1 cargo test -p tandem-engines real_ || true   # only when real CLIs exist; result logged, never gating
```
**E2E** (`m4-*.e2e.ts`)
- **E2E-M4-1 Launch grid.**
  - PATH contains only fake `claude`, inside an nvm-style dir that isn't on the GUI PATH. Expected: Claude Code is first; the others show as "10 more not installed".
  - With no engines: expected the list of probed shells; Check again re-runs detection.
  - With a slow rc: expected the "slow shell" diagnostic at 8 s, with known-location results still returned.
- **E2E-M4-2 Install.** Install `gemini` (fake installer via a registry override). Expected: the sheet shows the exact command; Run opens a visible pane; then "Installed ✓ — Sign in next".
- **E2E-M4-3 Status lifecycle.** A fake Claude emits UserPromptSubmit → Notification → Stop. Expected: the rail shows Working 1 → Waiting on you → an unread dot; the header dot goes `status-working` → `status-waiting` → `status-done` (UI_SPEC §1.2 tokens: sky → amber → green). Resize the idle pane 20 times. Expected: it stays idle. Check `tandem-hook`'s post body. Expected: no transcript text.
- **E2E-M4-4 Provider homes.** Launch on two Claude accounts. Expected: the spawn log shows distinct `CLAUDE_CONFIG_DIR`s, mode 0700, no token-shaped env values, and a pre-existing `settings.json` key preserved.
- **E2E-M4-5 Typing.** `type_prompt("hello\x03world")`. Expected: the fake TUI log shows "helloworld", then CR, as two writes. Type into a waiting pane. Expected: refused with the message.
- **E2E-M4-6 Arrange and Card view.** Arrange 5 panes. Expected: a 3+2 grid with equal widths in each row. Card view. Expected: cards. Change a status to waiting. Expected: the card updates. Unflip. Expected: the same PIDs and no resizes.
- **E2E-M4-7 Remote registry.** A config fixture adds `zed-agent`. Expected: it is detected and launches in a terminal with no code change.

**Exit checklist:** G green · TL-06 rule 2 status-bus half added as `invariant_rule_2_status_needs_hmac` (HMAC rejection) · registry documented in ARCHITECTURE.
**HUMAN_TODO:**
- *Legal:* permission, or trademark guidance, before shipping any vendor logos (monograms ship by default).
- *Manual QA:* real engines per OS; macOS GUI-PATH detection with a zsh `~/.zprofile`.

---

## M5 · Dashboard, notifications, sidebar, restore, updater, release pipeline (L)

**Goal.** The shared awareness layer (Dashboard, notification center, OS notifications, right sidebar), crash-safe session restore, signed auto-update, and a release workflow that produces unsigned artifacts without secrets.

**Owns:** F-DB-01, F-DB-02, F-DB-03, F-SH-10, F-CD-12, F-SH-07, F-SH-06, F-TL-04 (all P0)
**Early slices:** ▸slice F-SH-08 → M8: the onboarding step framework, with Welcome, Engines, the Privacy question (completes F-TL-01's "asks once") and First workspace.

**Prerequisites.** M4.

**Ordered tasks**
1. **`ApprovalBroker`** in `tandem-core`.
   - Pending approvals, keyed by id and source.
   - Resolve, deny with a reason, and expiry after 30 min.
   - An event stream.
   - Threads, the gateway and plugins all use this one broker.
2. **Right sidebar (F-SH-10).**
   - Resizable 320–900 px, default 470 (UI_SPEC §1.2 and §3.5 agree).
   - Full-screen overlays the main area without a reflow or PTY resize.
   - Below 1,100 px of window width it floats instead of docking.
   - Pills are closable and drag-reorderable. The **+** menu focuses a singleton if it's already open.
   - Tab state (order, active, width, full-screen) persists.
   - The test harness exposes reflow counters.
3. **Dashboard (F-DB-01, UI-42/43).**
   - Header count and subtitle, a 4-segment bar, Group by Status | Project (lanes nest in M6), and filter chips.
   - Cards: glyph or avatar, human name plus task title, meta line, elapsed time, and a 2-line redacted preview.
   - Waiting cards have inline Allow once / Deny through the broker.
   - `⋯` menu: Stop, Close (confirmed when working) and Handoff (M6).
   - "Close idle" closes visible, matching idle cards only. Search is AND over the listed fields; Esc clears it.
   - Empty state, and a 4-column full-screen wall.
   - Clicking a card jumps to its source across modes and flashes it.
4. **Pane header (F-CD-12, UI §2.20).**
   - Status palette.
   - Title rules: never truncated below 12 characters; the full title in the tooltip.
   - Human names: an original 200-name list in `src/data/names.json`, unique per workspace, persisted in `panes`, renameable.
   - The metadata slot.
   - Right-side slots: the account chip and context meter (M6), the Auto chip (M16), and the `⋯` menu, whose items appear as their features land.
   - Close confirm "Stop {name}? It's working."
   - A 1.2 s flash ring (steady under Reduce Motion).
5. **Notification center (F-DB-02, UI-40).**
   - `notifications` table with 7-day retention.
   - Bell badge showing `99+` above 99; the "Waiting on you" popover.
   - Item types from the spec. Click marks the item read and jumps to its source, switching modes if needed.
   - Mark all read. Items auto-resolve from their source's state.
   - The clicked item opens reliably even when several arrive in a burst.
6. **OS notifications (F-DB-03, UI-41/103).**
   - A `Notifier` trait, with the tauri notification plugin in production and a mock in tests.
   - Sent only when the app is unfocused or the source isn't visible. Per-type toggles.
   - A non-blocking soft bell (original asset), dock bounce / taskbar flash, and click-to-focus.
   - The Windows AppUserModelID is registered on the Start Menu shortcut.
7. **Session restore (F-SH-07).**
   - Atomic persistence, debounced at 500 ms and again at quit, into `layout_*`, `panes` and `sessions`. Launch configs are stored as JSON.
   - Splash sequence: "Restoring your session…", then "Checking your plan…" (bounded to 3 s). Any key or click skips it; Reduce Motion replaces it with a fade.
   - Shells reopen at their OSC 7 cwd; if it's gone they open at the workspace root with a notice.
   - Agents come back as UI-132 **paused cards**. "Resume agents automatically" (off) resumes newest first with at most 2 spawns at once.
   - Resume uses `resume_cmd_template` with the same provider home. A missing config shows "Launch configuration unavailable" and never falls back to defaults.
   - Ctrl+C on a card cancels that pane's restore. Closing a pane cancels its queued launch.
   - `settle_sessions()` marks Running/Waiting sessions Interrupted.
   - Quit is bounded to 5 s, then child trees are force-killed.
   - `scripts/killfuzz.ts`.
8. **Updater (F-SH-06, UI-115/130/131).**
   - tauri-plugin-updater with a minisign public key from `.dev-keys` in dev.
   - Checks at launch + 60 s, then every 4 h, and on Check now. Stable and Beta channels. Background download.
   - Update pill: "Restart to update", and "Update failed — Retry" with the failing stage in the tooltip. The restart dialog lists live agents.
   - Anti-rollback.
   - The `x-tandem-min-version` overlay allows a 72 h grace in which terminals keep working, then becomes modal.
   - Updates never require a plan. A deb install shows "Updates are managed by your package manager".
   - State lives in `$DATA/update-state.json`.
   - `scripts/mock-update-feed.ts --scenario valid|badsig|lower` serves the test feeds.
9. **Release pipeline (F-TL-04).**
   - `.github/workflows/release.yml`, triggered by `v*` tags, builds with tauri-action: universal dmg, msi/nsis, AppImage and deb.
   - Each signing step runs only when its secrets exist. Otherwise the artifact is marked "unsigned" and the job summary lists what is missing.
   - Also produced: the AppImage detached signature and deb signing (when keys exist), `latest.json` signed with minisign, a CycloneDX SBOM (cargo-cyclonedx + @cyclonedx/cyclonedx-npm), and `THIRD_PARTY_NOTICES.md` from `scripts/gen-notices.ts` (cargo-about + license-checker + `licenses/extra-attributions.toml`, which lists Parakeet CC-BY-4.0 and the OFL fonts). The notices are shown in About → Licenses.
   - `scripts/release-local.sh --dry-run` reproduces the no-secrets path locally.
   - `crates/tandem-voice/models/catalog.json` (Dictate model catalog data) is created now, so the attributions have a source of truth.
10. **Onboarding framework** (▸slice F-SH-08): an `OnboardingState` step machine (`onboarding_get` / `onboarding_set`) with Skip and Back, re-runnable from Settings › General → "Run setup again".

**Mocks and fakes.** Mock notifier, mock feed server with dev minisign keys, an approval fixture source, and FakeClock.

**Verification**
```bash
cargo test -p tandem-core approvals && cargo test -p tandem-store f_sh_07
pnpm --filter desktop test -- -t "F-DB-0[123]|F-SH-10|F-CD-12"
(pnpm tsx scripts/mock-update-feed.ts --port 8790 &) && xvfb-run -a pnpm --filter desktop e2e -- --spec 'e2e/specs/m5-*.e2e.ts'
pnpm tsx scripts/killfuzz.ts --iterations 20
xvfb-run -a pnpm --filter desktop perf -- --suite startup      # cold start P50 ≤2.0 s (16 shells); dashboard ≤500 ms
bash scripts/release-local.sh --dry-run --target x86_64-unknown-linux-gnu
grep -q 'Parakeet' dist/THIRD_PARTY_NOTICES.md && grep -q 'LGPL' dist/THIRD_PARTY_NOTICES.md && ls dist/*.cdx.json
command -v act && act -W .github/workflows/release.yml -j linux --dryrun || true
```
**E2E** (`m5-*.e2e.ts`)
- **E2E-M5-1 Crash restore.**
  1. Open 4 shells (with distinct cwds) and 4 fake-Claude panes, then kill -9 the app.
  2. Relaunch. Expected: an identical layout, 4 live shells at their cwds, and 4 paused cards.
  3. Click Resume on one card. Expected: the spawn log shows exactly one process, with the saved `CLAUDE_CONFIG_DIR` and `--resume <id>`.
- **E2E-M5-2 Auto-resume.** Turn on "Resume agents automatically". Expected: never more than 2 engine processes spawning at once (instrumented).
- **E2E-M5-3 Restore edge cases.**
  - A missing launch config. Expected: "Launch configuration unavailable", with no spawn.
  - Ctrl+C on a card. Expected: it leaves the queue and its scrollback is still viewable.
  - A session persisted as Running. Expected: settled to Interrupted in the store.
- **E2E-M5-4 Dashboard.**
  - 3 working, 1 waiting (fixture source) and 2 idle. Expected: "6 agents" with matching subtitle counts.
  - Search "api", then Close idle. Expected: only matching idle agents close.
  - Allow once on the waiting card. Expected: the broker resolves and the source proceeds.
  - Click a card. Expected: focus moves to the right workspace and tab, the mode switches if needed, and the flash class lasts 1.2 s.
  - Expected: updates land within 500 ms.
- **E2E-M5-5 Notifications.**
  - Unfocus the app and let an agent finish. Expected: one mock-notifier call; clicking it focuses the source.
  - With the "done" toggle off. Expected: no call.
  - Answer an approval on the Dashboard. Expected: its notification resolves.
  - From Thread mode, click the "done" item of a Code pane. Expected: the mode switches.
- **E2E-M5-6 Sidebar.**
  - Toggle the sidebar with 16 panes open. Expected: at most 1 reflow per pane.
  - Enter full-screen. Expected: 0 PTY resizes.
  - Relaunch. Expected: order and active tab persist.
  - Open Dashboard twice. Expected: one tab.
- **E2E-M5-7 Updater.**
  - Valid feed. Expected: "Restart to update"; the dialog lists live agents; the install succeeds.
  - Bad signature. Expected: "Update can't be verified", at the verify stage.
  - Lower version. Expected: nothing offered.
  - Min-version header. Expected: the overlay; the terminal still takes input at +71 h; modal at +73 h.
  - Lapsed account. Expected: the update still installs.
- **E2E-M5-8 Pane names.** Expected: two agent panes never share a name, and names persist across relaunch. Close a working pane. Expected: "Stop {name}? It's working."
- **E2E-M5-9 Onboarding framework.** A fresh profile shows Welcome. Continue offline → Engines (skip) → Privacy (both toggles off) → pick a folder. Expected: Code mode with the launch grid.

**Exit checklist:** G green · the release dry-run artifacts exist and are marked unsigned · the kill-fuzz run is clean.
**HUMAN_TODO:**
- *Signing & distribution:*
  - Minisign updater keypair (`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`; the public key goes in `tauri.conf.json`).
  - Apple Developer ID and notarization (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`).
  - Azure Trusted Signing (`AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, endpoint, profile).
  - A GPG key for the deb and AppImage.
  - Release hosting at `https://releases.tandem.dev` (bucket + CDN) for `latest.json`.
- *Manual QA:* Windows toast AUMID and click-through; mac dock bounce.

---
## M6 · Lanes, presets, handoff, accounts and usage (L)

**Goal.** Launch several agents at once, each with a role and optionally its own worktree. Hand work between engines. Run multiple accounts per engine, with visible usage and explicit limit handling.

**Owns:**
- P1: F-CD-14, F-CD-15, F-CD-16, F-AC-04, F-AC-06
- P0: F-AC-02, F-AC-03, F-AC-05

**Early slices:** ▸slice F-SH-08 → M8: onboarding step 3, Accounts.
**Completes:** F-DB-01 (project grouping nests lanes); F-CD-12 (account chip and context meter slots); F-SH-01 (Plan-usage chip).

**Prerequisites.** M5.

**Order.** CD-15, CD-14 and CD-16 are level 3 P1, so they go green before AC-04 and AC-06 (level 5 P1).

**Ordered tasks**
1. **`tandem-git` base.**
   - A git CLI runner. Read operations always use `-c core.fsmonitor= -c core.hooksPath=<empty dir> -c credential.helper=`.
   - git2 fallback for status.
   - Worktree add, remove and prune; `merge --no-ff` with a conflict report.
   - An idempotent append to `.git/info/exclude`.
   - A slug function (kebab-case, ≤ 40 characters, `-2`/`-3` dedupe).
   - Add `invariant_rule_6_read_ops_no_repo_helpers`.
2. **Lanes (F-CD-15).**
   - Each lane is a child workspace, nested in the rail and the Dashboard.
   - The lane briefing text is sent before the task.
   - Lane menu: Diff vs base (DiffView), Merge into base, Create PR (runs `gh pr create` in a shell pane when `gh` is present), Remove lane. "Open in Files" arrives in M9.
   - A forced remove needs a local confirm dialog. An `Origin` of Conductor or Phone is rejected (`invariant_rule_5_local_review_origin`).
   - A sweep at startup and every 6 h, which raises a "Clean up lanes" notification and never deletes anything.
3. **Launch wizard and presets (F-CD-14, UI-12).**
   - Open with `Mod+Shift+L`, from the empty state, or from `+`.
   - Built-in presets Solo, Pair, Bench and Fleet. The seat editor, How many (1–8), a shared task, isolation, and the Will launch preview.
   - An ICU launch label (e.g. "Launch 4 agents + 1 shell"); `Mod+Enter` launches.
   - Saved presets in the `presets` table.
   - Shell seats type their command at OSC 133 A or after 1.5 s of quiet.
   - On partial failure, show a summary with Retry and never roll back the seats that launched.
   - Browser seats arrive in M9.
4. **Handoff (F-CD-16, UI-13).**
   - `tandem-engines::brief` has three strategies:
     1. Ask the source, if it's a live chat-engine TUI: type the fixed template request and capture the reply between markers.
     2. Run the target engine headless over the last 400 redacted lines, passing input on stdin.
     3. Fall back to the raw last 200 lines plus the user's ask.
   - The target opens split right, with the brief typed via `type_prompt`.
   - The source pane gets the note "Handed off to …".
   - Thread targets are added in M7.
5. **Accounts (F-AC-02, UI-35/106).**
   - Rows grouped by engine, with label, plan, usage summary, Default badge, and Set default / Sign out / Remove.
   - Add account: a dialog with a mini terminal running `login_cmd` in a new home, with success detected via `status_cmd`.
   - Cancel kills the login process and deletes the new home.
   - Free plan: 1 account per engine (upgrade sheet).
6. **Account resolution (F-AC-03).** The resolver order is explicit > teammate > workspace override > default. There is a workspace override setting, and the AccountChip switches the *next* session only.
7. **Limit card (F-AC-05, UI-36).**
   - Terminal limits come from `limit_patterns` with a parsed `resets_at`.
   - The card copy is verbatim from the spec.
   - Resume at reset types "continue" at the reset time (FakeClock).
   - Choose account… shows the summary confirm and continues through the `brief` fallback.
   - Nothing launches until the user clicks.
8. **Usage (F-AC-04).**
   - A `UsageSource` per engine: status or usage command, stream data (added in M7), or a log estimate marked "estimated".
   - Refresh every 5 min and after each turn.
   - The title-bar chip shows the window closest to exhaustion, e.g. "Claude · 82% · resets 4:10 PM". It turns `warning` at 80% and `danger` at 95% (UI_SPEC §1.3 meter thresholds), and the popover lists every account.
   - With no data it shows "Usage unavailable", with the source in the tooltip.
9. **Context meter (F-AC-06).** A ring plus text (e.g. "114K / 200K") in the pane header, amber at 80%, with the tooltip copy.

**Mocks and fakes.** Fake engine `login` and `auth status` modes, limit-message fixtures, usage-command fixtures, and temp repos with a local bare remote.

**Verification**
```bash
cargo test -p tandem-git && cargo test -p tandem-engines brief && cargo test -p tandem-engines f_ac_0
pnpm --filter desktop test -- -t "F-CD-1[456]|F-AC-0[2-6]"
xvfb-run -a pnpm --filter desktop e2e -- --spec 'e2e/specs/m6-*.e2e.ts'
git -C "$E2E_LAST_REPO" branch --list 'tandem/*'           # tandem/fix-auth, tandem/fix-auth-2
test "$(grep -c '^\.tandem/$' "$E2E_LAST_REPO/.git/info/exclude")" = 1
```
**E2E** (`m6-*.e2e.ts`)
- **E2E-M6-1 Fleet.** Launch the Fleet preset with the task "fix flaky tests". Expected: 4 fake agents, each receiving its role-prefixed task as its first input after readiness.
- **E2E-M6-2 Worktree per agent.** 3 agent seats plus 1 shell. Expected: 3 lanes; the shell's cwd is the repo root; the lanes nest in the rail and Dashboard. Two lanes for "fix auth". Expected: `tandem/fix-auth` and `tandem/fix-auth-2`.
- **E2E-M6-3 Partial failure.** One lane creation fails. Expected: 3 agents launch and the summary lists 1 failure with Retry. Save a preset and relaunch. Expected: the same seat list.
- **E2E-M6-4 Lane safety.**
  - Remove a dirty lane without force. Expected: a prompt.
  - A Conductor-origin forced removal. Expected: rejected.
  - Merge with a conflict. Expected: a conflicted state is reported and no commit is made.
- **E2E-M6-5 Handoff.** Fake Codex pane → Handoff → Claude. Expected: a pane opens to the right, and its first input has the template headings. With an empty brief: expected the fallback, and the target still launches.
- **E2E-M6-6 Accounts.**
  - Cancel mid-login. Expected: no process and no account.
  - Add a second account and Set default. Expected: the next pane uses it; existing panes keep theirs.
  - On Free: expected the 2nd account is blocked.
  - Resolver unit test: override = B plus teammate preference = C gives the expected result.
- **E2E-M6-7 Limit.** Show the limit fixture. Expected: the card appears and no other account is used until Continue. Resume at reset (fake clock). Expected: "continue" is typed.
- **E2E-M6-8 Usage chip and meter.**
  - Windows at 40% and 85%. Expected: 85% in `warning`.
  - No data. Expected: "Usage unavailable".
  - 150k of 200k. Expected: 75%, neutral colour. At 170k: expected amber.

**Exit checklist:** G green · lane and account criteria tested · onboarding Accounts step present.
**HUMAN_TODO:** *Manual QA:* real multi-account sign-in for Claude Code and Codex on each OS; the single-account message for mac Keychain engines; `gh pr create`.

---

## M7 · Thread mode (XL)

**Goal.** Read and drive coding sessions as conversations: Claude Code, Codex and ACP adapters on one normalized event stream, with inline approvals, the composer, plan → implement, the changes panel, the grid and engine switching.

**Owns:**
- P0: F-TH-01, F-TH-02, F-TH-03, F-TH-04, F-TH-05, F-TH-07, F-TH-08, F-TH-09, F-TH-11, F-TH-12, F-TH-15
- P1: F-TH-06, F-TH-10, F-TH-13, F-TH-14, F-TH-16

**Early slices:**
- ▸slice F-PL-01 → M10: the gateway skeleton (D-4).
- ▸slice F-BE-06 → M13: `/v1/ai/title` (D-5).

**Completes:**
- F-SH-02: the Thread surface replaces its interim one.
- F-SH-13: the `thread/<id>` route.
- F-CD-16: thread targets.
- F-AC-04: stream usage data. F-AC-05: the thread limit card. F-AC-06: the thread header meter.
- F-SH-07: the "Interrupted — Resume?" UI.
- F-DB-01 and F-DB-02: threads as items (D-6).

**Prerequisites.** M6.

**Order.** The P0 tasks (1–9) come first, then the P1 tasks (10–14).

**Ordered tasks**
1. **Normalized events and adapter trait (F-TH-03).**
   - `NormalizedEvent` (ts-rs), with every event listed in the spec.
   - A `ThreadAdapter` trait: start, send_turn, interrupt, resume.
   - Turn ids; stale events are dropped.
   - One turn at a time, plus one queued follow-up.
   - Stop sends the engine's interrupt; after 3 s it kills the tree and marks the turn cancelled (not failed).
   - If the engine exits mid-send: an error with Retry.
   - Persistence: `threads`, `thread_messages` (appended as events arrive), `attachments`. Tool outputs over 64 KB go to a file. Assistant deltas are compacted into the final message at turn end.
2. **Gateway skeleton** (▸slice F-PL-01).
   - `tandem-gateway` serves rmcp Streamable HTTP on 127.0.0.1 at a random port.
   - 32-byte lease tokens with Bearer auth; no lease → 401.
   - The built-in `tandem_approvals__request` (Claude sees `mcp__tandem__tandem_approvals__request`), backed by the ApprovalBroker.
3. **Claude adapter (F-TH-04).**
   - Flags: the exact flag set from the spec, `--mcp-config <tmp> --strict-mcp-config`, `--permission-prompt-tool mcp__tandem__tandem_approvals__request`, `--resume`, and `CLAUDE_CONFIG_DIR`.
   - Parse `system/init`, the assistant, user and result messages, and `stream_event` deltas.
   - Map usage. Detect limits with `resets_at`.
   - `prompt_too_long` becomes an error with the hint and the "Summarize & continue" action.
   - Attachments: images as base64 blocks, files as @paths.
   - Add `invariant_rule_1_repo_settings_cannot_grant`.
4. **Codex adapter (F-TH-05).**
   - `codex/protocol.rs` is pinned, and a version check fails closed with the spec's message.
   - JSON-RPC client over stdio.
   - Access mapping; MCP servers via `-c mcp_servers.*`.
   - Approvals map to `approval_request`, token counts to `usage`, rate limits to `limit`.
   - If app-server exits: restart it, resume the conversation, show "Reconnecting…", and mark any in-flight turn error + Retry.
   - Long commands are middle-truncated for display only.
5. **Access (F-TH-09).**
   - An `AccessResolver` over (level, mode, origin). Plan forces Read.
   - Unattended origins never get Full unless the routine or task explicitly grants it (`invariant_rule_4_unattended_never_full`).
   - Pending approvals expire after 30 min and count as a deny. One-time approvals never widen access.
6. **Inline approvals (F-TH-11, UI-23).**
   - Stacked cards above the composer, with a detail expander.
   - Allow once / Allow for this thread / Deny (with an optional reason). Keys: `Mod+Enter`, `Mod+Shift+Enter`, and Esc to focus Deny.
   - "Allow for this thread" rules are exact prefix or tool-name matches, and are cleared when the engine changes.
   - Denied tool rows use the spec's copy. A turn that had denials ends as "Completed with blocked actions".
   - A pending approval shows Waiting for you everywhere.
7. **Transcript (F-TH-12, UI-21).**
   - A virtualized list.
   - Renderers per event type. 3 or more consecutive tool rows fold into "+N tool calls".
   - `MarkdownView` parses in a worker. Only the last block re-parses while streaming, with a 50 ms budget per frame; oversized or malformed input degrades to plain text.
   - "Working for 5s".
8. **Composer (F-TH-07, UI-22, UI §2.23, in `@tandem/ui`).**
   - Every toolbar control from the spec (mode & access, attach, engine, model, effort, Dictate slot for M8, Send/Stop, token readout).
   - IME-safe Enter.
   - Remembered defaults per engine.
   - `@`-picker using the `ignore` crate.
   - Attachments: up to 20 files and 20 MB; drop overlay; pasted images become PNG chips in `$DATA/attachments/<thread>/`.
   - The draft persists every 300 ms. A failed send puts the text back *ahead of* anything typed since.
   - The toolbar collapses as the width shrinks.
9. **Model catalog, threads, folderless threads and titles.**
   - **Catalog (F-TH-08):** runtime > remote (10-min TTL) > bundled; Automatic always first; a notice when a saved model is missing.
   - **Rail and header (F-TH-01, UI-20):** folder groups, Pinned, No folder, Other; unread dot; header chips; archive/restore; delete (from archived only) keeps a 7-day recovery copy with mode 0600.
   - **Folderless (F-TH-02):** a scratch cwd with Read access by default. Add a folder keeps the draft and attachments.
   - **Titles and errors (F-TH-15):** the title comes from `/v1/ai/title` (with a mock AI provider and a 60/h Redis bucket), with the local 8-word fallback within 2 s. User renames are never overwritten. Stage-specific error copy. "No output for 10 minutes" after 10 min of silence.
10. **ACP adapter (F-TH-06).**
    - initialize → session/new (cwd, mcpServers) → prompt → update → cancel. `request_permission` maps to `approval_request`.
    - Launch commands come from the registry. Automatic until the agent reports models; a withdrawn model falls back with the note.
    - Auth and model errors have distinct stages; Sign in opens a terminal login pane.
    - Newly saved credentials are used on the next turn.
11. **Plan → implement (F-TH-10).** Plan revisions v1, v2, …; a Plan card with View, Copy and **Implement**. Implement preserves any unsent draft as a separate queued message. Revisions are listed in the inspector.
12. **Changes and Files panel (F-TH-13, UI-24).**
    - A diff against the thread's starting commit plus the working tree, or snapshot hashes when the folder isn't a git repo.
    - Scoped to the workspace subfolder. Refreshes at every turn end. Empty-state copy.
13. **Grid (F-TH-14).** Up to 6 cells (Free gets 1, from the entitlements), each with its own composer. "Grid is full (6)". The layout persists. `Mod+Alt+Arrow` moves between cells.
14. **Switch engine mid-thread (F-TH-16).** The new session is seeded with a summary (last 20 messages plus completed tool action names). A divider marks the switch, and usage accumulates per engine.

**Mocks and fakes.** Fixture scenarios in `fixtures/{claude,codex,acp}/`: init, turn, tools, approval, limit, prompt_too_long, ignore-stdin, reconnect, unsupported version, ACP conformance, models withdrawn, auth error. Each has a `*.golden.json`. Also the mock AI provider.

**Verification**
```bash
cargo test -p tandem-engines f_th_03 && cargo test -p tandem-engines f_th_04 && cargo test -p tandem-engines f_th_05
cargo test -p tandem-engines f_th_06 && cargo test -p tandem-engines f_th_09 && cargo test -p tandem-gateway lease
pnpm --filter desktop test -- -t "F-TH-"
pnpm --filter api test -- -t "ai/title"
curl -s -X POST http://127.0.0.1:8787/v1/ai/title -H "authorization: Bearer $PRO_TOKEN" \
  -H 'content-type: application/json' -d '{"text":"fix the login bug"}' | jq -e '.title | length <= 60'
xvfb-run -a pnpm --filter desktop e2e -- --spec 'e2e/specs/m7-*.e2e.ts'
xvfb-run -a pnpm --filter desktop perf -- --suite threads    # 5,000 events: open ≤500 ms, ≥55 fps, page <100 ms; 2 MB fence ≤50 ms block
```
**E2E** (`m7-*.e2e.ts`)
- **E2E-M7-1 Folderless.** A fresh profile in Thread mode shows the composer. Send "hello". Expected: a thread under No folder, a streamed reply, and a title. Offline: expected the fallback title within 2 s. Rename the thread and send more turns. Expected: the name sticks.
- **E2E-M7-2 Approvals.**
  - Fake Claude asks to run `npm test`. Expected: a card, and Waiting on the Dashboard within 500 ms.
  - Allow for this thread. Expected: a second `npm test` is auto-allowed; `npm install` still asks.
  - Deny with a reason. Expected: a denied row and "Completed with blocked actions".
  - Allow from the Dashboard card. Expected: the thread proceeds (D-6).
- **E2E-M7-3 Stop and crash.** Stop the ignore-stdin scenario. Expected: cancelled within 3.5 s, with the tree gone. Kill the engine mid-turn. Expected: an error with Retry, and the app stays up.
- **E2E-M7-4 Composer.** Expected:
  - Shift+Enter inserts a newline; Enter sends.
  - Enter during IME composition doesn't send.
  - A failed send restores the text first.
  - A pasted screenshot becomes a PNG chip, and the file exists.
  - `@src/` lists only non-ignored files.
  - A new thread uses the remembered defaults.
- **E2E-M7-5 Codex.** Kill app-server while idle, then send. Expected: it restarts, the history stays, and "Reconnecting…" is shown. An unsupported version: expected the fail-closed message.
- **E2E-M7-6 Plan → implement.** Two plan turns. Implement v1. Expected: v1's text is sent and the composer mode becomes Act.
- **E2E-M7-7 Grid.** Drag 7 threads in. Expected: 6 cells and "Grid is full (6)"; the layout persists across relaunch; a Free account gets 1 cell.
- **E2E-M7-8 Switch engine.** Claude → Codex after 3 turns. Expected: the first Codex input contains the summary, and the divider is shown.
- **E2E-M7-9 Changes.** A turn edits 2 files. Expected: both listed with correct diffs. In a subfolder workspace: expected only that subfolder's changes.
- **E2E-M7-10 Interrupted.** kill -9 during a Running thread. Expected: after relaunch it shows "Interrupted — Resume?" and is not marked done.
- **E2E-M7-11 Handoff.** Pane → Handoff → Codex. Expected: a Thread opens in the same workspace.
- **E2E-M7-12 ACP.** Run the conformance and models-withdrawn fixtures. Expected: the goldens match, and "Model no longer offered by <engine>" appears.

**Exit checklist:** G green · every adapter's golden contract tests pass · the thread perf suite meets its thresholds · the Thread surface's interim flag is removed.
**HUMAN_TODO:** *Manual QA:* real Claude Code, Codex, Cursor Agent, Grok and OpenCode sessions per OS (`TANDEM_REAL_ENGINES=1`); record the supported version ranges in the registry.

---

## M8 · Dictate (local) and onboarding (XL)

**Goal.** Push-to-talk anywhere: persistent audio capture, local Whisper/Parakeet, safe insertion into the focused app, the widget, history and the dictionary. Plus the complete first-run onboarding.

**Owns:**
- P0: F-DI-01, F-DI-02, F-DI-03, F-DI-04, F-DI-06, F-DI-07, F-DI-11, F-DI-12, F-SH-08
- P1: F-DI-08, F-DI-09 (D-3)

**Completes:** F-SH-13 (the `dictate/toggle` route and the tray icon); the status bar's "Dictate: Fn"; F-TH-07 (the composer mic).

**Prerequisites.** M7.

**Ordered tasks**
1. **Capture (F-DI-03)** in `tandem-voice::capture`.
   - A persistent cpal stream that is paused while idle, feeding an `rtrb` ring. Downmix to mono and resample to 16 kHz with rubato. Gain steps Off, 2×, 4×, 6×, 8×.
   - Device policy: never auto-select loopback or virtual devices; apply the Bluetooth rule; support a backup mic.
   - Silent-stream recovery: no samples within 1 s → rebuild the stream, then fail over to the backup, then show the error.
   - Permission checks run off the main thread. "Pause other audio" keeps a crash marker. Chimes are synthesized in Rust.
   - An `AudioDevice` trait, with `FakeDevice` for tests.
2. **Local models (F-DI-04)**, as a `SpeechEngine` trait.
   - whisper-rs variants: base.en, large-v3-turbo q5 and distil-large-v3.5.
   - Parakeet via transcribe-rs (feature `parakeet`, not built for Intel mac).
   - The downloader reads `models/catalog.json`: resumable ranges, `Retry-After`, SHA-256 and size checks, symlinks rejected. A failed check deletes the file and offers Retry.
   - Stages: Verifying → Loading → Ready. A model switch waits for the current take to end.
   - Fall back to the best downloaded model, and say so.
   - Guards: silence gate, trailing trim, too short/quiet, hallucination suppression, and a 10-min take cap.
   - The model manager (UI-63).
3. **Dictionary engine (F-DI-09).**
   - Aho-Corasick matching over rules with match case, protect links and code, spacing, enabled, and scope by language or app profile.
   - Protected-term markers (wrap before a rewrite, verify after it).
4. **Takes and insertion (F-DI-06).**
   - A take state machine with ids: late transcripts are discarded, a new take cancels older pending insertions, and everything fails closed.
   - A `Platform` trait per OS: capture the target, restore focus (the Windows sequence includes a benign Alt tap and a readback), paste via the clipboard, then restore the clipboard after 500 ms on mac and Linux only.
   - Fallback: "Copied — press {Mod}+V to paste".
   - Terminal targets have control characters stripped and never get Enter.
   - Tandem's own windows are never external targets. Elevated targets get a warning. "Copy instead of typing" is available.
   - A Linux banner for missing tools, with apt, dnf and pacman commands.
   - Post-processing: dictionary replacements, then filler and ellipsis stripping.
5. **Hotkeys (F-DI-01, F-DI-02).**
   - macOS: a CGEventTap thread that handles the Fn flag, self-heals, and pauses for secure input.
   - Windows: a listen-only `WH_KEYBOARD_LL` hook, with a masking key on release. `scripts/guards/no-consuming-hook.sh` rejects any consuming return.
   - Linux: XInput2 on X11; the ashpd GlobalShortcuts portal on Wayland, falling back to instructions for `xdg-open tandem://dictate/toggle`.
   - Modes: hold, toggle (no modifier-only bindings) and latch. Taps under 150 ms are ignored. A real key-up is required. A Windows watchdog. A quick tap during mic start still counts. Esc cancels.
   - Status chips, and the press-and-release verification.
6. **Widget (F-DI-07, UI-60).**
   - A separate transparent always-on-top window (`dictate-widget`) with minimal capabilities.
   - States with 30 fps bars; visibility and position modes; one-line toasts with the full diagnostic in the tooltip; styles.
   - The Enhance and Polish quick actions arrive in M13.
7. **Composer dictation (F-DI-12, UI-68).** The mic button, or holding the global Dictate key (or ⌥Space on macOS) while the composer has focus, inserts directly at the caret, with no clipboard. `Alt+Space` is not bound on Windows/Linux (UI_SPEC §5). The mic-owner registry blocks it while Conductor holds the mic (used in M16).
8. **Setup and onboarding (F-DI-11, F-SH-08).**
   - Dictate setup steps: mac permission statuses with Quit & Reopen, shortcut choice, Try it (press and release), engine (Local only until M13 adds Cloud), where text goes, and the optional mac 🌐 preference change after consent.
   - Onboarding complete: Welcome → Engines → Accounts → Dictate → Privacy → Workspace. It resumes at the saved step, and skipping every step lands in the empty Code state.
9. **History and stats (F-DI-08, UI-65).**
   - `dictate_takes` with FTS5 and paging; actions Copy, Delete, Add to dictionary, Correct.
   - CSV export with the exact header. Clear history keeps the aggregates in `dictate_stats`.
   - Home card, and a share-card PNG (1080×810).
10. **Dictionary UI (F-DI-09, UI-64).**
    - Words, Corrections and Review tabs. Mined suggestions expire after 90 days, and only Remember creates a rule.
    - A test box, JSON/CSV import and export with a preview, and a Deleted view kept for 30 days.
    - Pro sync through a new API route, `POST /v1/dictionary/sync`: last writer wins per rule, a conflict review, `requirePlan('pro')`.
11. **Settings → Dictate (UI-62/108) and the tray** (Open Tandem, Dictate status, Quit).

**Mocks and fakes.**
- `FakeDevice`: zeros, a WAV file, a disconnect.
- `FakePlatform`: records focus, paste and clipboard calls.
- A fake hotkey event source.
- A corrupting download server (`scripts/serve-models.ts --corrupt-byte 1000`).
- Committed WAV fixtures: `hello_world_16k.wav`, `silence_2s.wav`.

**Verification**
```bash
bash scripts/fetch-models.sh base.en                  # SHA-pinned, cached in ~/.cache/tandem-models
cargo test -p tandem-voice && cargo test -p tandem-voice --features whisper -- --include-ignored f_di_04_2
bash scripts/guards/no-consuming-hook.sh
pnpm --filter desktop test -- -t "F-DI-|F-SH-08"
pnpm --filter api test -- -t "dictionary/sync"
xvfb-run -a pnpm --filter desktop e2e -- --spec 'e2e/specs/m8-*.e2e.ts'
xvfb-run -a pnpm --filter desktop perf -- --suite dictate   # record start ≤50 ms (fake device)
```
**E2E** (`m8-*.e2e.ts`)
- **E2E-M8-1 Push-to-talk.** Fake hold → WAV → release. Expected: FakePlatform receives the paste of text containing "hello world", and the previous clipboard is restored.
- **E2E-M8-2 Taps and cancel.** A 100 ms tap. Expected: no recording. Esc while recording. Expected: no transcription call and no insertion. A toggle binding set to a bare modifier. Expected: a validation error.
- **E2E-M8-3 Guards.** 2 s of silence. Expected: "No speech detected". A focus-restore failure. Expected: the text is on the clipboard and the toast shows. `\x1b[` sent to a terminal target. Expected: stripped, with no CR.
- **E2E-M8-4 Widget.** "While recording" mode. Expected: shown when recording starts, hidden 1 s after insertion. An error toast. Expected: the full diagnostic in its tooltip.
- **E2E-M8-5 Composer.** Dictate into a thread composer with the caret mid-text. Expected: the text is inserted at the caret, and the clipboard is unchanged.
- **E2E-M8-6 Onboarding.**
  - A fresh profile, all steps. Expected: Code mode with the chosen workspace.
  - Quit at step 3 and relaunch. Expected: step 3.
  - Skip everything. Expected: the empty state.
  - Try it. Expected: passes only after both press and release.
- **E2E-M8-7 History and dictionary.**
  - Search for a word from a stored take. Expected: found. CSV export. Expected: the exact header. Clear history. Expected: the total word count is unchanged.
  - The rule "tandem"→"Tandem" applied to "use tandem, https://tandem.dev". Expected: "use Tandem, https://tandem.dev".
  - App-profile scoping. Expected: the rule applies only when the target app matches.
- **E2E-M8-8 Model download.** Corrupt byte. Expected: SHA verification fails, the file is deleted, and Retry is offered. Switching model mid-take. Expected: the switch applies after the take ends.

**Exit checklist:** G green · the base.en transcription test passes in CI (model cache) · the Windows hook guard is in CI · TL-01 and the Dictate privacy rows are correct.
**HUMAN_TODO:** *Manual QA:*
- The default hotkey in TextEdit, Notepad and gedit.
- The macOS permissions flow.
- Wayland on GNOME and KDE.
- Parakeet latency on an M1 (P50 ≤ 700 ms).
- A Bluetooth headset with a built-in mic.
- Windows Start-menu suppression.
- Review the Parakeet CC-BY-4.0 terms.

---
## M9 · Files, git, browser and links (L)

**Goal.** The Files sidebar with an editor, the git panel, the embedded browser with element picker, and clickable terminal links.

**Owns:** F-CD-09 (D-1), F-CD-17, F-CD-18, F-CD-19, F-CD-20 (all P1)
**Completes:**
- F-SH-10: Files and Browser tabs.
- F-CD-06: drops from the Files tree.
- F-CD-14: browser seats.
- F-CD-15: the lane "Open in Files" item.
- F-TH-07: dragging files into a composer.
- The status bar branch segment.

**Prerequisites.** M8.

**Ordered tasks**
1. **Files backend.**
   - A lazy tree that honours `.gitignore` via `ignore`, with a "Show ignored" toggle.
   - Writes go to a symlink's target. Conflicts are detected by comparing mtime and hash against when the file was opened.
   - Files over 5 MB open read-only; binary and image files are detected.
   - Paths outside every workspace are read-only. Every non-UI origin goes through `contain()`.
   - A debounced `notify` watcher.
2. **Files sidebar and editor (F-CD-19).**
   - FileTree (UI §2.26) with git badges, following the active context (Code workspace or Thread folder; the teammate Home is added in M10).
   - CodeMirror 6 tabs. Closing a tab selects its left neighbour.
   - Auto-save after 800 ms idle, and `Mod+S`.
   - The conflict dialog "File changed on disk — Reload / Overwrite / Compare", with Compare opening a DiffView.
   - Drag out to panes and composers.
   - Wide and narrow layouts. The divider position and open tabs persist per context.
3. **Git panel (F-CD-20).**
   - `tandem-git` gains status, diff, stage, unstage, discard (with confirm), commit, push, pull, publish, switch and new branch.
   - A branch chip with ahead/behind counts.
   - Works in a repo subfolder, before the first commit, and in lanes.
   - Status refreshes on watch (500 ms debounce) and on agent `done`.
   - Read operations always use the hardened flags.
4. **Browser (F-CD-17).**
   - A child webview with **no** IPC capability (a zero-permission capability file).
   - Toolbar: Back, Forward, Reload, and the address field "Search or enter address" (DuckDuckGo by default; configurable).
   - The address bar shows the committed URL. Only http, https and file (inside workspaces) are allowed; bare hosts become `http://`.
   - Sidebar tabs. A pane can move to the sidebar and back.
   - Device preview (Phone 390×844 and Tablet 820×1180, with rotate and fit).
   - Profile at `$DATA/browser-profile`, with Clear data.
   - On Linux, fall back to the sidebar only when child webviews aren't available.
5. **Element picker (F-CD-18).**
   - An injected picker script: hover highlights, click selects, Shift+click adds, Alt+click selects the parent. A note field.
   - Results come back through an intercepted navigation to a nonce-guarded reserved scheme, so the page never gets IPC.
   - Copy brief produces the spec's Markdown: outerHTML ≤ 4 KB, a unique selector, React/Vue names, size and position, key computed styles, and the note.
   - Send to… pastes into a pane or composer without submitting.
6. **Links (F-CD-09).**
   - The web-links addon for URLs, plus a path link provider for absolute and cwd-relative paths with `:line[:col]`, re-resolved after OSC 7 changes.
   - `Mod+Click` opens URLs in the Tandem browser (setting "Open links in Tandem", on) or the system browser.
   - Files open in the editor at the line. Files outside every workspace open read-only with a banner.
   - Clicks are ignored while the pane is hidden or mid-drag.

**Mocks and fakes.** `scripts/serve-fixtures.ts` on :5173, serving the fixture pages including a React page; an fsmonitor trap repo; a local bare remote.

**Verification**
```bash
cargo test -p tandem-git f_cd_20 && rm -f /tmp/pwned && cargo test -p tandem-git f_cd_20_1 && test ! -e /tmp/pwned
cargo test -p tandem-core files
pnpm --filter desktop test -- -t "F-CD-09|F-CD-1[789]|F-CD-20"
(pnpm tsx scripts/serve-fixtures.ts &) && xvfb-run -a pnpm --filter desktop e2e -- --spec 'e2e/specs/m9-*.e2e.ts'
```
**E2E** (`m9-*.e2e.ts`)
- **E2E-M9-1 Editor.**
  - An agent changes a file on disk, then the user saves an edit. Expected: the conflict prompt.
  - Expected: auto-save lands within 1 s.
  - A gitignored `node_modules`. Expected: hidden until "Show ignored".
  - Relaunch. Expected: the same tabs reopen.
- **E2E-M9-2 Git.** Stage, commit and push to a bare remote. Expected: ahead/behind shows 0/0. An empty repo. Expected: the first commit works. The fsmonitor trap. Expected: no `/tmp/pwned`.
- **E2E-M9-3 Browser.** Type `localhost:5173`. Expected: `http://localhost:5173` loads. A page probing `window.__TAURI__`. Expected: it reports undefined. `javascript:` and `tandem:` URLs. Expected: blocked.
- **E2E-M9-4 Picker.** Select two elements and copy the brief. Expected: two selectors, each matching exactly one element. Send to a pane. Expected: no trailing newline.
- **E2E-M9-5 Links.** Output `src/main.rs:42:7`, then `Mod+Click`. Expected: the editor opens at line 42. `/etc/hosts`. Expected: read-only. After `cd sub`: expected relative paths resolve against `sub`.

**Exit checklist:** G green · `invariant_rule_6` extended to cover status polling.
**HUMAN_TODO:** *Manual QA:* browser panes on WebView2 and WKWebView; the minimum WebKitGTK version for Linux child webviews.

---

## M10 · Teammates and gateway core (XL)

**Goal.** Agent mode: persistent teammates with a brief, memory, skills, learning, session search and teammate-to-teammate mail. All of it runs over the single local gateway with leases.

**Owns:** F-PL-01, F-AG-01, F-AG-02, F-AG-03, F-AG-04, F-AG-05, F-AG-06, F-AG-07, F-AG-08, F-AG-09, F-AG-11, F-AG-12 (all P1)
**Completes:**
- F-SH-02: the Agent surface replaces its interim one.
- F-SH-13: the `teammate/<id>` route.
- F-AC-03: the teammate's preferred account.
- F-CD-16: a teammate as handoff target.
- F-CD-19: the teammate Home context.
- F-DB-01: teammate cards.
- F-DI-12: the mic in "Create with AI".

**Prerequisites.** M9.

**Ordered tasks**
1. **Gateway (F-PL-01).**
   - One in-process gateway listener, with watchdog and memory metrics.
   - `prepare(session)` issues a lease recording the caller, allowed plugins and access level.
   - MCP injection per engine: Claude via `--mcp-config` with a Bearer header; Codex via `-c`; ACP via `mcpServers`.
   - Tools are named `<plugin>__<tool>`. The default listing is `tandem__search_tools` + `tandem__call`; the setting "Expose all plugin tools directly" lists every granted tool.
   - An unavailable plugin doesn't break the others.
   - A built-ins registry.
   - Test vendor module `vendors/test_echo`, behind feature `test-vendors`.
2. **Teammate record (F-AG-01).**
   - `teammates` table and the Home directory.
   - Name validation: 1–40 characters, unique case-insensitively, "Name already used".
   - Avatar: initials on a 12-hue gradient hashed from the id; hue or image override.
   - Lead is auto-created and pinned, with the default brief.
   - Terminal-only engines run the teammate as a terminal session, with the "Only chat engines carry memory and skills." note.
3. **Briefing (F-AG-06).** `tandem-engines::briefing` assembles, in order: preamble, brief, fenced memory, skills index, roster, tool guidance.
   - Delivery: Claude `--append-system-prompt-file`, Codex `developer_instructions`, ACP first prompt. Always through a file or stdin, never argv.
   - Scan for injection markers and redact credentials.
   - Add `invariant_rule_8_untrusted_fencing`.
4. **Memory (F-AG-07, UI-73).**
   - The `tandem__memory_write` built-in enforces the cap and rejects credentials ("Memory can't store credentials").
   - List and Raw views, with a byte-exact round trip. Add note, delete note, budget meter.
5. **Skills (F-AG-09, UI-74).**
   - Front-matter validation: name kebab-case ≤ 48, description ≤ 280.
   - The 6 original starter skills live in `crates/tandem-store/assets/skills/`.
   - The `tandem__read_skill` built-in. Agent-authored skills show "New" until viewed.
   - The "Allow teammates to write skills" setting.
6. **Session search (F-AG-11).** FTS5 over teammate-chat user and assistant prose only, scoped per teammate. Exposed as `tandem__session_search(query, limit≤10)`, scoped by the lease's caller.
7. **Agent rail (F-AG-02, UI-70).** The Lead hero tile, then rows with status pip and last line. Hover: Pin, New chat. Context menu: Rename, Duplicate, Delete (files go to trash).
8. **Teammate page and chats (F-AG-03, UI-71).**
   - The header.
   - Chat tabs: status dots, `+`, drag to reorder (persisted), Rename / Close / Delete, arrow keys.
   - The shared Transcript and Composer, with the placeholder "Message {name}…".
   - The plugin nudge strip.
   - Each chat's workspace is Home or a Place.
   - Empty state: 3 suggestion chips generated locally from the brief.
9. **Settings and creation (F-AG-05, F-AG-04; UI-72/75/76/107).**
   - Settings: the full table from the spec, with a Save/Revert bar and an unsaved-changes guard.
   - Create: Create with AI (Pro, online) against `/v1/ai/teammate-draft`. Test it with a desktop API fixture; the real route lands in M13 (D-8).
   - Offline or Free: the local templates instead.
   - The engine grid with Install links and Recheck.
10. **Learn after a turn (F-AG-08).**
    - A headless follow-up on the registry's `cheap_model`, with **no MCP servers**, returning JSON `{add, remove, rationale}`.
    - Ask mode: suggestion cards with Keep / Discard. Auto mode: apply, with Undo. Off: no follow-up.
    - Never runs for unattended turns unless the mode is Auto.
11. **Mail (F-AG-12).**
    - `tandem__send_message` and `tandem__list_teammates` built-ins, with permission checks.
    - Delivery: append to an idle chat with the same `conversation_ref`, or wake the recipient in a fresh chat titled "{sender}: {first line}".
    - At-most-once delivery (`mail` table), and a row in both transcripts that opens the full message in a modal.
    - Refusal reasons; a queued Retry that never auto-loops; the global pause in Settings › Teammates (UI-107).
    - Mail-triggered runs are unattended, capped at Edits.
    - Loop guard: at most 20 per hour, then "Rate limit reached".
    - `@name` in the composer sends mail.

**Mocks and fakes.** Teammate-turn scenarios for the fake engines (memory edits, mail tool calls, learn JSON), a teammate-draft API fixture, and `test_echo`.

**Verification**
```bash
cargo test -p tandem-gateway && cargo test -p tandem-engines f_ag_06 && cargo test -p tandem-engines f_ag_08
cargo test -p tandem-store f_ag_0 && cargo test -p tandem-store f_ag_11
PORT=$(jq -r .port "$TANDEM_HOME/run/gateway.json"); curl -s -o /dev/null -w '%{http_code}\n' \
  -X POST "http://127.0.0.1:$PORT/mcp" -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'     # → 401
pnpm --filter desktop test -- -t "F-AG-|F-PL-01"
xvfb-run -a pnpm --filter desktop e2e -- --spec 'e2e/specs/m10-*.e2e.ts'
```
**E2E** (`m10-*.e2e.ts`)
- **E2E-M10-1 Teammates.** A fresh profile. Expected: Lead is pinned. Create "Scout", then "scout". Expected: "Name already used". An uninstalled engine. Expected: Create is disabled; after Recheck finds the fake engine, it enables. Expected: the avatar snapshot is stable across launches.
- **E2E-M10-2 Briefing and learning.**
  - A Scout turn. Expected: the spawn uses a briefing file with fenced memory and a 3-skill index; a 100 KB briefing never appears in argv.
  - Ask mode. Expected: suggestions render; Keep appends to `NOTES.md`; the follow-up spawn had no MCP config.
  - Off mode. Expected: no follow-up.
- **E2E-M10-3 Memory and skills.**
  - A write over the cap. Expected: rejected, with the file unchanged. "token=ghp_…" is rejected.
  - List → Raw → List. Expected: byte-identical.
  - Add `release-notes` from the library. Expected: a valid folder. Invalid front matter. Expected: an error, and the skill is excluded from the index.
- **E2E-M10-4 Session search.** A word only in tool output. Expected: no results. A word in an assistant message. Expected: that chat. Teammate A searching. Expected: none of B's chats.
- **E2E-M10-5 Mail.**
  - A → B. Expected: a fresh chat for B, and rows in both transcripts.
  - Global pause on. Expected: queued. Resume. Expected: delivered once.
  - The 21st mail in an hour. Expected: queued.
  - B's default access is Full. Expected: the mail run spawns at Edits.
- **E2E-M10-6 Settings.**
  - Change the engine and save. Expected: the next chat uses the new engine, with memory unchanged.
  - May message off. Expected: `tandem__send_message` is absent from the next turn's `tools/list`.
  - Expected: tab order persists. The nudge Enable. Expected: it grants `test_echo`.
- **E2E-M10-7 Create with AI.** The fixture prefills the form. Expected: nothing is saved until Create.
- **E2E-M10-8 Gateway.** No lease. Expected: 401. Lease A calling a plugin granted only to B. Expected: refused. 10 sessions. Expected: one listener. With 3 plugins granted, `tools/list`. Expected: the search and call tools plus the built-ins.

**Exit checklist:** G green · the Agent surface's interim flag is removed · the gateway memory stays ≤ 80 MB in the metric test.
**HUMAN_TODO:** *Manual QA:* teammate chats on real engines; *Legal:* review the starter skill content.

---

## M11 · Tasks and routines (L)

**Goal.** A local task tracker that agents read and write over MCP, with Pro execution in lanes. Scheduled routines that run in-app and in the background.

**Owns:** F-TK-01, F-TK-02, F-TK-03, F-TK-04, F-RT-01, F-RT-02, F-RT-03, F-RT-04 (all P1)
**Completes:**
- F-SH-10: the Tasks tab.
- F-SH-13: the tray's "Pause routines".
- F-DB-02: "Ready for review" and task-review items.
- F-BL-04: routines pause on lapse.
- F-AG-05: the Routines link.

**Prerequisites.** M10.

**Ordered tasks**
1. **Task store (F-TK-01)** in the `tandem-tasks` lib.
   - `ISSUE-<n>.json` files with revisions, plus `meta.json` and an append-only `history.jsonl`.
   - `TAN-n` ids.
   - `expected_revision` checks (failing with `REVISION_CONFLICT`), 24 h idempotency keys, per-issue lock files, atomic writes.
2. **MCP server (F-TK-02).**
   - `tandem-tasks mcp --dir --connection` over rmcp stdio, supporting protocol versions 2025-11-25, 2025-06-18 and 2025-03-26.
   - The instructions text and all 17 tools.
   - Cursor pagination, 50 per page.
   - Revoking a connection makes further calls fail with `CONNECTION_REVOKED`.
   - Injected into chat sessions, and into terminal engines' native config when "Expose tasks to terminal agents" is on.
3. **Tasks UI (F-TK-03, UI-80/81).**
   - The sidebar tab and a full page. List grouped by status, quick add, archive and restore.
   - Detail: inline property edits, a description with explicit Save/Cancel, threaded comments, and a history timeline.
   - Field-level merge: unsaved local edits survive other writers' changes to other fields; a same-field change shows the conflict banner "Keep mine / Take theirs".
   - A watcher refreshes within 500 ms.
4. **Task execution (F-TK-04, UI-82/83/84).**
   - The Execute dialog: default isolation is a new lane, default access is Edits, and Full needs an explicit choice.
   - An attempt is a lane plus a session. The briefing fences issue content as untrusted.
   - Progress arrives as comments. When the agent is done, the status goes to In review with the result saved.
   - Independent review (a different engine when possible) posts a verdict.
   - Stop, Resume and Retry. At most 4 active attempts, with "Queued (position n)" beyond that.
   - Assigning an issue never starts work. Execution needs sign-in and Pro.
   - Stopping during lane creation rolls back the empty lane.
5. **Routine model and schedules (F-RT-01)** in `tandem-routines`.
   - A 5-field cron parser (ranges, steps, lists, SUN as 0 or 7). Oversized steps are rejected.
   - DST handling with chrono-tz. The rule is documented: a nonexistent local time shifts forward by the gap; an ambiguous time uses the first occurrence.
   - Schedules in words through i18n, and the next 3 runs.
   - Full access needs an explicit choice, with the warning "This routine will act without asking."
6. **Scheduler (F-RT-02).**
   - A 30 s tick against the Clock.
   - Each run creates a fresh chat named after the routine (auto-title never renames it) and runs unattended.
   - When the turn ends: set `last_status`, send "Ready for review — {routine}", set `last_run_chat_id`.
   - Missed runs: one "late" catch-up for a miss within the last 24 h.
   - Failures set a reason on the row, with one notification per error type per day.
   - Overlap is skipped with "previous run still in progress".
7. **Background runner (F-RT-03).**
   - `tandem --run-routines`: a headless mode that shares the GUI's lock at `$DATA/run/app.lock` (+ `app.lock.json`).
   - A `Launcher` trait: a LaunchAgent plist on mac, `schtasks` on Windows, a systemd `--user` timer with a crontab fallback on Linux. `TANDEM_LAUNCHER_ROOT` overrides the location for tests.
   - Toggling the option installs or removes the entry. Headless mode sends OS notifications.
8. **Routines UI (F-RT-04, UI-50..53).**
   - The 24-hour timeline strip with a now line, and routine cards with their actions.
   - The New routine flow, including the guidance tip. The CronInput builder, run history, and the Esc guard.
   - Free accounts see the upsell.

**Mocks and fakes.**
- FakeClock.
- Recording shims for `systemctl`, `launchctl` and `schtasks`.
- MCP request fixtures `crates/tandem-tasks/tests/fixtures/*.jsonl`.
- Fake engine scenarios for attempts and reviews.

**Verification**
```bash
cargo test -p tandem-tasks && cargo test -p tandem-tasks f_tk_02          # conformance: 3 versions, 17 tools
cargo test -p tandem-routines                                            # cron, DST, fake-clock scheduler, launchers
H=$(mktemp -d) && TANDEM_HOME=$H pnpm tsx scripts/seed-routine.ts --due-now && \
  TANDEM_HOME=$H ./target/debug/tandem-desktop --run-routines; test $? -eq 0
pnpm --filter desktop test -- -t "F-TK-|F-RT-"
xvfb-run -a pnpm --filter desktop e2e -- --spec 'e2e/specs/m11-*.e2e.ts'
```
**E2E** (`m11-*.e2e.ts`)
- **E2E-M11-1 Agents edit tasks.** A thread calls `save_issue`. Expected: the UI shows it within 500 ms. The agent changes the status while the user edits the title. Expected: both kept. A same-field conflict. Expected: the banner.
- **E2E-M11-2 Execute (Pro).**
  - Execute TAN-1. Expected: a lane, progress comments, then In review with a result, then a review verdict comment.
  - A 5th attempt. Expected: "Queued (position 1)", and it starts when one finishes.
  - Stop during lane creation. Expected: no process and no lane.
  - Assign an issue. Expected: nothing spawns.
- **E2E-M11-3 Routine.**
  - Cron `0 9 * * 1-5`. Expected: "Weekdays at 9:00 AM", and the next runs skip the weekend. `0 10 * * 6-7`. Expected: Saturday and Sunday.
  - Advance the clock to 09:00. Expected: one fire, a chat named after the routine, and the notification.
  - Run now. Expected: Running, then Ready for review.
  - A paused routine. Expected: no fire. A deleted teammate. Expected: an error on the row and no spawn.
- **E2E-M11-4 Missed run.** Closed across the due time and reopened within 24 h. Expected: exactly one late run.
- **E2E-M11-5 Background.** Enable. Expected: the unit files exist. Disable. Expected: removed. With the GUI running, `--run-routines`. Expected: exits without running anything.
- **E2E-M11-6 Free.** Expected: the Routines upsell, and Execute disabled with the plan copy.

**Exit checklist:** G green · DST and cron tables fully covered · the headless mode's single-instance lock is tested.
**HUMAN_TODO:** *Manual QA:* launchd on macOS, Task Scheduler on Windows, systemd user timers (with lingering) on Ubuntu and Fedora, and behaviour across sleep and wake.

---

## M12 · Plugins and approvals (L)

**Goal.** Connect external services once, grant them per teammate or thread, and enforce risk-class approvals and held drafts. Engines never see a token.

**Owns:** F-PL-02, F-PL-03, F-PL-04, F-PL-05, F-PL-06, F-PL-07, F-PL-08, F-PL-09 (all P1)
**Completes:**
- F-SH-13: `plugin/callback`.
- F-AG-03: the nudge, and F-AG-05: per-plugin toggles, both with real plugins.
- F-TH-07: Attach ▾ → "Plugins for this thread" (UI-94).
- F-DB-01: plugin approvals on cards.
- F-TL-02: the bloom filter of the user's credentials.

**Prerequisites.** M11.

**Ordered tasks**
1. **Vendor modules (F-PL-02).**
   - `vendors/<id>/` declares id, label, category, monogram logo, description, auth, upstream, pinned hosts, tools with risk classes, and status.
   - Discovery rules map remote-MCP tools to risk classes; a tool with no rule is `unclassified`.
   - A declaration test covers every module.
2. **Credentials (F-PL-04).**
   - Keyring service `app.tandem.plugins`, account `<plugin>:<connection-id>`. Credentials over 16 KB are rejected.
   - Injection only on an exact pinned scheme, host, port and path-prefix match. A redirect drops the credentials.
   - Echoed tokens in responses are redacted.
   - Refresh failure sets Needs attention and sends one notification.
   - Add `invariant_rule_3_pinned_hosts_only`.
3. **Connect flows (F-PL-03, UI-90/91/92).**
   - A searchable card grid.
   - OAuth DCR: RFC 8414 discovery, RFC 7591 registration, a public client with PKCE (confidential when the metadata requires it).
   - The redirect goes to loopback or `tandem://plugin/callback`. Only pinned auth hosts are allowed; private addresses are rejected.
   - API keys are masked and verified before saving. Local-port plugins are probed.
   - Errors name the failing step and include the provider's text untruncated.
   - Credential ordering: save before showing success; reject late callbacks; leave the old credential in place on a failed verification.
   - Disconnect revokes upstream, deletes the credential and invalidates leases.
4. **Grants (F-PL-05).** Separate grants per teammate, thread and workspace, plus the "Grant these plugins to all threads" setting. The Team allowlist hook reads the entitlement claims; it is tested with a fixture here, and M13 feeds it real policy.
5. **Policy engine (F-PL-06).**
   - The decision table: risk class × {interactive Full, interactive Edits, unattended} gives allow, ask or hold.
   - Allow-once vs conversation scopes, reset on quit, sign-out or reconnect.
   - Spend approvals are bound to a hash of the amount and budget, and re-verified at execution.
   - Grant, lease and connection are rechecked at execution.
5a. Add `invariant_rule_4_unattended_never_approves` (with the M7 half).
6. **Approval card (F-PL-07, UI-93).** Every field from the spec, the full request with credentials masked, buttons per risk class, and 30-min expiry that denies and notifies the agent. Also shown on Dashboard cards.
7. **Held drafts (F-PL-08).** A `held_drafts` table and the Held actions list. Approve & run executes exactly once. Edit & run uses a schema-validated JSON editor. Discard, and bulk actions. The agent gets "Held for review (draft id D-n)".
8. **Catalog (F-PL-09).**
   - 11 ready modules: GitHub, Linear, Notion, Slack, Sentry, Vercel, Cloudflare, Supabase, Stripe, PostHog, Gmail.
   - 9 preview modules.
   - The built-in Web fetch, which respects robots.txt and blocks private ranges.
   - A fixture contract test per ready module against wiremock.
   - Client ids come from `plugin_oauth_clients` in the remote config.

**Mocks and fakes.** A mock OAuth authorization server (`tests/support/mock_as.rs`: DCR, PKCE, refresh, redirect and failure switches), wiremock upstreams per vendor, and the mock keyring.

**Verification**
```bash
cargo test -p tandem-gateway && cargo test -p tandem-gateway f_pl_06_1 && cargo test -p tandem-gateway --test vendors
pnpm --filter desktop test -- -t "F-PL-0[2-9]"
xvfb-run -a pnpm --filter desktop e2e -- --spec 'e2e/specs/m12-*.e2e.ts'
grep -rq "$E2E_FAKE_TOKEN" "$E2E_LAST_HOME" && exit 1 || true    # token only in (mock) keychain
```
**E2E** (`m12-*.e2e.ts`)
- **E2E-M12-1 OAuth.** GitHub through the mock AS. Expected: Connected, and the token exists only in the mock keyring.
- **E2E-M12-2 OAuth failures.** A redirect to `169.254.169.254`. Expected: rejected. A late callback after cancel. Expected: ignored. A failed reconnect verification. Expected: the old credential is kept.
- **E2E-M12-3 Grants.** A new teammate. Expected: no grants. The nudge Enable. Expected: GitHub granted. Expected: "Plugins for this thread" works.
- **E2E-M12-4 Interactive approval.**
  - A thread at Edits calls `github__create_issue`. Expected: the card with all its fields. Allow once. Expected: exactly 1 upstream request.
  - A `destructive` tool. Expected: no "Allow for this conversation".
  - A `spend` whose amount changed after approval. Expected: refused.
  - Advance 30 min. Expected: deny.
- **E2E-M12-5 Held draft.** A routine calls a write tool. Expected: a draft, and 0 upstream requests. Approve & run. Expected: exactly 1 request.
- **E2E-M12-6 Preview.** Expected: Connect disabled. A Team allowlist fixture. Expected: Connect disabled, with the explanation.

**Exit checklist:** G green · every ready module has a fixture test · the policy table has 21 cases.
**HUMAN_TODO:**
- *Accounts & keys:* OAuth app registrations for GitHub, Linear, Notion, Slack (bot), Sentry, Vercel, Cloudflare, Supabase, Stripe, PostHog and Gmail (Google OAuth consent screen, plus restricted-scope verification for `gmail.send`). Put the client ids in the remote config `plugin_oauth_clients.<id>`.
- *Manual QA:* a smoke test of each ready module with a real account.

---
## M13 · Hosted services, credits and Team (L)

**Goal.** The metered hosted features: the credits ledger, the AI and STT relays, cloud Dictate, Enhance and Polish, and credit packs. Plus refunds, the Team plan with admin policy and audit, and the "What leaves this machine" page.

**Owns:** F-BE-05, F-BE-06, F-BE-07, F-DI-05, F-DI-10, F-BL-03, F-BL-05, F-BL-06 (P1 core), F-TL-07, F-TL-10 (all P1)
**Completes:**
- F-AG-04: the real `/v1/ai/teammate-draft`.
- F-DI-06: formatting instructions and Auto-Enhance.
- F-DI-07: the Enhance and Polish quick actions.
- F-DI-11: the Cloud engine option.
- F-DI-09: the live protected-term path.
- F-WB-05: the Credits ledger and packs, and the Team page.
- F-DB-02: the credit-low item.
- F-SH-01: the ◈ credits display with its warning and danger states.

**Prerequisites.** M12.

**Ordered tasks**
1. **Ledger (F-BE-05).**
   - `reserve` holds for 15 min; `settle` and `refund` complete it.
   - Serializable transactions or `FOR UPDATE`. Monthly credits are spent before purchased ones. A balance never goes negative.
   - A sweeper job with a Redis lock refunds expired reservations.
   - Monthly reset on `invoice.paid`, with a ledger entry.
   - `GET /v1/credits`. A failed reserve returns `402 CREDITS_EXHAUSTED`.
2. **AI relay (F-BE-06).**
   - An `AiProvider` interface: the Claude API (D-103), plus a deterministic mock used when there is no key.
   - Per-feature model ids come from the config. 30 s timeout, 1 retry on 5xx. Metering maps actual tokens to credits through the price table.
   - No request or response bodies are logged (a log-capture test).
   - Redis buckets per user and feature return 429 with `Retry-After`.
   - Routes: `/ai/rewrite` (3 credits), `/ai/teammate-draft` (10), and `/ai/title` (free; the 61st in an hour returns 429). `/auto/next`, `/voice/act` and `/voice/resolve` land with their P2 features in M16.
3. **STT relay (F-BE-07).**
   - Multipart upload: 10 min and 25 MB at most, otherwise 413.
   - A provider interface (mock in dev). Streams to the provider and never writes audio to disk.
   - Meters per started 15 s. The 120 min/day cap returns `DAILY_LIMIT`.
4. **Cloud Dictate (F-DI-05).**
   - The Local | Cloud choice. Cloud needs Pro; otherwise "Cloud transcription needs Tandem Pro".
   - Opus encoding, a list of 30+ languages, and the dictionary-hints toggle.
   - Errors name the limit that was hit. There is never a silent fallback; the error offers "Use local for this take".
5. **Enhance and Polish (F-DI-10).**
   - Inputs over 5,000 characters are rejected client-side, with no call.
   - Protected terms go through the DI-09 markers.
   - Auto-Enhance, and formatting instructions (up to 2,000 characters, optionally per app profile).
   - A one-time disclosure on first use. On any failure the raw transcript is inserted, with a toast.
   - Widget quick actions: undo + paste while the target still has focus, otherwise the clipboard.
6. **Credit packs (F-BL-03).** The web `/credits` page. A one-time Checkout with mode=payment credits the purchased bucket exactly once. The in-app **Buy credits** opens that page.
7. **Refunds (F-BL-05).** `POST /v1/admin/users/:id/refund` (role admin, seeded `admin@`): refund via the provider, end paid access, zero the monthly bucket. Unused packs are refundable. The admin UI is F-BE-12 in M16.
8. **Team core (F-BL-06).**
   - Orgs, email invites, roles (Owner / Admin / Member), and a seat quantity synced to Stripe.
   - Pooled credits with per-member caps.
   - Admin policy: allowed engines and plugins, max access, force telemetry off, disable Auto and Conductor, phone off. It travels in the entitlement claims plus the config. Clients enforce it (the Full option disabled, Connect disabled with the explanation, telemetry toggles locked), and the server rejects any claim outside it.
   - The Postgres `audit_events` table (ARCHITECTURE §14.2), and the web Team page.
9. **Audit (F-TL-10).** Metadata-only events, e.g. "Full access used" and "Plugin write approved", sent only for Team members whose org enables it. A notice in Settings → Privacy.
10. **Transparency (F-TL-07).** `packages/protocol/hosted-features.json` (feature, what it sends, destination) drives Settings → Privacy → "What leaves this machine" with live On/Off states. It is also exported for the web `/privacy` page (M15). Add the subprocessor data file.
11. Add `invariant_rule_7_no_egress_without_optin`: a full scripted session with every opt-in off, checked against `TANDEM_NET_LOG`.

**Mocks and fakes.** Mock AI and STT providers (deterministic fixtures), Stripe fixtures for payment and seat changes, the `/__test/clock` route, and the new seeds (owner@, member@, admin@).

**Verification**
```bash
pnpm --filter api test -- -t "F-BE-05|F-BE-06|F-BE-07|F-BL-03|F-BL-05|F-BL-06"   # includes 100×reserve(10) vs 500 → 50 ok
curl -s -o /dev/null -w '%{http_code}\n' -X POST http://127.0.0.1:8787/v1/ai/rewrite \
  -H "authorization: Bearer $FREE_TOKEN" -H 'x-tandem-plan: pro' -H 'content-type: application/json' -d '{"mode":"polish","text":"hi"}'  # → 403
curl -s -o /dev/null -w '%{http_code}\n' -X POST http://127.0.0.1:8787/v1/stt/transcribe \
  -H "authorization: Bearer $PRO_TOKEN" -F audio=@crates/tandem-voice/tests/fixtures/eleven_minutes.opus                         # → 413
pnpm --filter web test:e2e -- --grep @m13
xvfb-run -a pnpm --filter desktop e2e -- --spec 'e2e/specs/m13-*.e2e.ts'
cargo test -p tandem-voice f_di_05 && cargo test --workspace invariant_rule_7
```
**E2E** (`m13-*.e2e.ts`, `apps/web/e2e/m13-*.spec.ts`)
- **E2E-M13-1 Cloud Dictate.**
  - Pro take. Expected: the mock text is inserted, and the ledger shows the reserve and settle.
  - A Free account. Expected: the Pro message.
  - A 402. Expected: the out-of-credits message, and nothing inserted.
  - A cloud error. Expected: no local transcription until the user clicks "Use local for this take".
- **E2E-M13-2 Enhance.** More than 5,000 characters. Expected: blocked with no call. A server failure. Expected: the raw text and a toast. The mock alters a protected term. Expected: the corrected, un-enhanced text is used.
- **E2E-M13-3 Packs.** Buy 5,000 and replay the webhook. Expected: +5,000 exactly once, and the desktop footer updates within 5 s.
- **E2E-M13-4 Team.**
  - Invite a member (via mailpit) and set the policy to max access = Edits. Expected: Full is disabled for the member, and a forged claim is rejected.
  - A plugin that isn't allowlisted. Expected: blocked.
  - A 1,000 cap. Expected: `CREDITS_EXHAUSTED` at 1,000.
  - A seat change. Expected: the Stripe quantity is updated.
- **E2E-M13-5 Audit.** A Team member starts a Full session. Expected: an audit row. A Pro user. Expected: none.
- **E2E-M13-6 Transparency.** Enable cloud STT. Expected: its row reads "On", with the destination.
- **E2E-M13-7 Refund.** An admin refund. Expected: Free limits, and the monthly bucket at 0.

**Exit checklist:** G green · every hosted route is in the `requirePlan` table test · no request bodies in the API logs.
**HUMAN_TODO:**
- *Accounts & keys:* `ANTHROPIC_API_KEY` and per-feature model ids; STT provider key; enable no-training and no-retention provider settings; Stripe one-time prices for the packs ($5/5,000, $10/12,000, $20/30,000) and Team seat prices.
- *Legal:* refund policy, support mailbox, DPAs with providers, subprocessor list review.

---

## M14 · Remote: SSH and WSL (L)

**Goal.** Terminals, agents, threads, files and git on SSH hosts through `tandem-host`, and in WSL distros, without ever forwarding local credentials.

**Owns:** F-RM-01, F-RM-02 (P1)
**Completes:**
- F-CD-01: SSH and WSL workspace kinds.
- F-EN-01: per-host detection.
- F-AC-01.3: remote spawns get no provider homes, now exercised for real.
- F-CD-06: WSL drops.
- Settings → Remote (UI-111) and UI-14.

**Prerequisites.** M13.

**Ordered tasks**
1. **`tandem-host`.**
   - JSON-RPC over stdio: PTY spawn, resize, kill and attach; engine detection; fs list, read and write through `contain()`; git status; the status relay.
   - A session daemon that keeps PTYs alive for 10 min after a disconnect.
   - A protocol version.
   - CI builds static binaries for linux-x86_64 (musl), linux-aarch64 (musl) and macOS universal, and bundles a SHA-256 manifest into the desktop resources.
2. **SSH transport.**
   - System `ssh` with ControlMaster (`ControlPath=$DATA/run/ssh-%C`, known hosts in `$DATA/ssh/known_hosts`; ARCHITECTURE §13.1), using aliases from `~/.ssh/config` or `user@host:port`, and ssh-agent or a key file.
   - An askpass helper dialog.
   - Host keys: Tandem manages its own `known_hosts`. An unknown host shows the fingerprint for approval; a changed key is refused.
   - Upload `tandem-host` to `~/.tandem-host/<version>/` and verify its SHA-256 remotely.
   - The error taxonomy from the spec, and "Reconnecting…" followed by re-attach.
3. **Remote workspaces.**
   - Remote folder picker.
   - Panes, agent terminals and Claude/Codex threads (the adapter processes run through `tandem-host`).
   - Remote Files and git.
   - The `+` menu lists the host's engines. Attachments are stored under the remote workspace.
   - Provider homes and plugin credentials are never forwarded.
4. **WSL (F-RM-02).**
   - `wsl -l -q` (UTF-16 output).
   - `wsl -d <distro> --cd <path> -- <cmd>` spawns.
   - Detection inside each distro, labelled "(in Ubuntu)".
   - Path translation in both directions (`C:\x y\z` → `/mnt/c/x y/z`, `\\wsl$\Ubuntu\home\a` → `/home/a`).
   - A default-distro setting.
5. **Infra.** Add an `sshd` service under the compose profile `remote` (port 2222; the key comes from `scripts/gen-ssh-fixture.sh`) (D-10).

**Verification**
```bash
bash scripts/gen-ssh-fixture.sh && docker compose -f infra/docker-compose.yml --profile remote up -d --wait sshd
cargo build -p tandem-host --release --target x86_64-unknown-linux-musl && cargo test -p tandem-host
cargo test -p tandem-host --features e2e-ssh -- --include-ignored f_rm_01
cargo test -p tandem-core wslpath && cargo test -p tandem-core f_rm_02_2
xvfb-run -a pnpm --filter desktop e2e -- --spec 'e2e/specs/m14-*.e2e.ts'
ssh -p 2222 -i infra/ssh/id_ed25519 -o StrictHostKeyChecking=no tandem@127.0.0.1 'ls ~/.tandem-host'   # version dir present
```
**E2E** (`m14-*.e2e.ts`)
- **E2E-M14-1 Connect.** Connect to 127.0.0.1:2222, approve the fingerprint, and pick `/home/tandem/project`. Run `echo hi`. Expected: `hi`. Expected: the remote spawn env contains no `.tandem/providers`.
- **E2E-M14-2 Host key.** Regenerate the container's host key and reconnect. Expected: refused, with the warning.
- **E2E-M14-3 Containment.** Read `../../etc/passwd` remotely. Expected: refused.
- **E2E-M14-4 Re-attach.** Run `ssh -O exit`. Expected: "Reconnecting…", then the same remote PID re-attaches.
- **E2E-M14-5 Remote engines.** Put a fake `claude` in the container. Expected: the `+` menu lists it, and a remote thread runs through `tandem-host`.

**Exit checklist:** G green · `tandem-host` binaries are produced for all 3 targets in CI · the WSL translation tests pass on Windows CI.
**HUMAN_TODO:**
- *Manual QA (win):* a WSL workspace with a WSL-only `claude`; SSH to real macOS and aarch64 hosts.
- *Signing:* codesign the macOS `tandem-host` binary.

---

## M15 · Website, docs, data controls and launch hardening (L)

**Goal.** Close every remaining P0 and P1: the website, docs, changelog and downloads; local data export and delete; the threat-model invariants; and every non-functional requirement. After this milestone, the P0/P1 product is launchable.

**Owns:**
- P0: F-WB-01, F-TL-06
- P1: F-WB-02, F-WB-03, F-WB-04, F-TL-05

Also closes PRODUCT_SPEC §5.1–§5.7.

**Prerequisites.** M14.

**Ordered tasks**
1. **Marketing site (F-WB-01).**
   - Pages: `/`, `/pricing` (generated from `plans.ts`, with a build check against `/v1/config`), `/download` (OS-detected), `/changelog`, `/docs`, `/privacy` (the M13 transparency table plus subprocessors), `/terms`, `/security`.
   - The interactive demo, built from `@tandem/ui` with sample data and no network or mic, labelled "Interactive demo · sample data".
   - A consent banner before any non-essential script, and no analytics by default.
   - `@lhci/cli` thresholds of 90 or higher.
2. **Docs (F-WB-02).** Fumadocs with every page the spec lists. `pnpm gen:docs` writes the shortcuts, settings keys, engines, plugin catalog and platform matrix from code. Build checks confirm the content matches, and that every Settings page has an anchor.
3. **Changelog (F-WB-03).**
   - The `CHANGELOG.md` format, with platform tags and categories, and its parser.
   - It generates the web index and per-version pages, the in-app "What's new" modal (shown once per version) and the feed notes.
   - `scripts/check-changelog.ts` fails the release when the version has no section.
4. **Downloads and discovery (F-WB-04).** Download links with SHA-256 from the release manifest, `/llms.txt`, `/llms-full.txt`, `GET /api/v1/site-info`, and the published OpenAPI 3.1 document.
5. **Data controls (F-TL-05).**
   - Export: a zip holding SQLite data as JSON per table, plus memory, skills, tasks, routines and settings, with no keychain values.
   - Delete local data: keeps provider homes unless "Also remove engine sign-ins" is ticked.
   - The server's 30-day account deletion job, which also cancels the subscription.
6. **Invariants (F-TL-06).**
   - Create the `tests/invariants/` crate `tandem-invariants` (D-10).
   - Move or re-export `invariant_rule_1…8` into it, one file per rule.
   - Rule 5 covers the local-review Origin checks for Conductor and Phone origins; M16 extends it with the live voice and phone paths.
7. **Performance (§5.1).** The full suite and memory budgets, written to `perf/results/m15.json`.
8. **Reliability (§5.2).**
   - 200 kill-fuzz iterations.
   - Watchdog tests for the PTY reader, gateway, scheduler and status bus: up to 3 restarts per 10 min, then a surfaced error.
   - A 10 s IPC timeout with a cancellation test.
9. **Security (§5.3).** `cargo audit` and `pnpm audit` with waivers; a CSP test; a capabilities review test (the browser webview has zero commands).
10. **Accessibility (§5.4).**
    - The axe sweep over the UI registry: every UI-xx screen × both themes × {empty, loading, error, populated}.
    - A keyboard-only traversal test, a WCAG contrast test over token pairs, a live-region throttle test (1 per 2 s), and the xterm screen-reader setting.
11. **i18n and observability.** An `en-XA` pseudo-locale E2E (no truncated primary buttons, §5.5). The Help → Diagnostics live panel (§5.7).
12. **Cleanup and docs.** Remove every `interim.*` flag. Update the README: setup, `pnpm dev`, the test commands, the release process.

**Verification**
```bash
pnpm --filter web build && pnpm --filter web test:e2e && pnpm --filter web lhci
curl -s http://localhost:3000/llms.txt | grep -q '\$29' && curl -s http://localhost:3000/api/v1/site-info | jq -e '.pricing and .platforms'
pnpm gen:docs && git diff --exit-code apps/web/content/docs/generated
pnpm tsx scripts/check-changelog.ts --version "$(jq -r .version apps/desktop/package.json)"
cargo test -p tandem-invariants          # 8 rule tests
cargo audit && pnpm audit --audit-level high && pnpm tsx scripts/check-waivers.ts
xvfb-run -a pnpm --filter desktop perf -- --suite all && pnpm tsx scripts/killfuzz.ts --iterations 200
xvfb-run -a pnpm --filter desktop e2e -- --spec 'e2e/specs/m15-*.e2e.ts'
bash scripts/check-no-interim.sh && pnpm tsx scripts/trace-tests.ts --all --fail-on-missing && pnpm ci:local
```
**E2E** (`m15-*.e2e.ts`, `apps/web/e2e/m15-*.spec.ts`)
- **E2E-M15-1 Web.** Expected: no analytics request before consent; pricing equals the config; the download button matches the OS; `/llms.txt` prices equal the config.
- **E2E-M15-2 What's new.** After a version bump, the modal shows once. Expected: it doesn't show a second time.
- **E2E-M15-3 Data controls.** Export. Expected: the zip has `teammates.json`, `threads.json` and `memory/`, and no keychain value. Delete local data with the box unticked. Expected: `$PROVIDERS` is kept.
- **E2E-M15-4 Axe sweep.** Expected: zero serious or critical violations across every screen and state.
- **E2E-M15-5 Quality-bar run** (BUILD_PROMPT §4). Expected, in one scripted pass:
  1. `pnpm dev` comes up.
  2. Sign up.
  3. Open a folder.
  4. Launch 16 shells.
  5. Start an engine thread (fixture).
  6. Schedule a routine.
  7. Dictate (fake device).
  8. The Free gate blocks the 5th pane.

**Exit checklist:** G green · every P0 and P1 criterion is traced to a test or a `[manual]` entry · no interim flags remain · the README is complete · `HUMAN_TODO.md` has the full *Manual QA checklist*.
**HUMAN_TODO:**
- *Legal:* privacy policy, terms and cookie policy text; a trademark check and the final product name.
- *Accounts & keys:* domains and DNS for `tandem.dev`, `app.`, `api.` and `releases.`; web hosting.
- *Manual QA:* Lighthouse against production.

---

## M16 · P2 differentiators and release candidate (XL)

**Goal.** Ship the P2 set in independent sub-phases, each ending green and tagged (`m16a` … `m16d`). Then cut the release candidate. If the budget runs out, stop after any sub-phase and list what remains as next steps in `BUILD_LOG.md`.

**Owns (P2):** F-CN-01, F-CN-02, F-CN-03, F-CN-04, F-CN-05, F-CN-06, F-CN-07, F-CN-08, F-BE-08, F-AU-01, F-RM-03, F-BE-11, F-SH-12, F-CD-21, F-AC-07, F-AG-10, F-AG-13, F-RT-05, F-BE-03, F-BE-12, F-RM-04, F-RM-05, F-RM-06, F-RM-07
**Extends:** the P2 app-shortcut rebinding portion of F-SH-09 (Settings › Shortcuts, UI-113: edit, conflict detection, reset to defaults; persisted in `settings`); the SSO portion of F-BL-06 (P2; the P1 core is owned by M13); the remaining F-BE-06 routes (`/auto/next`, `/voice/act`, `/voice/resolve`); F-TL-06 rule 5 (live voice and phone paths).

**16A: Conductor (CN-01…08, BE-08).**
- API: `POST /v1/voice/session` reserves 10 min of credits and requires Pro. `/settle` is idempotent. A sweeper handles unsettled sessions. `/voice/act` runs the doer with a closed tool schema generated from the command registry, over a workspace snapshot. `/voice/resolve` costs 1 credit.
- `tandem-voice::realtime`: a tokio-tungstenite client with a `RealtimeProvider` trait and a mock provider (`scripts/mock-realtime.ts`), plus a 3 s pre-connect buffer.
- The local tool executor mirrors the registry, including confirmations, the closed `set_setting` list, redacted `read_pane`, and no typing into a waiting pane.
- UI-61 HUD and WaveformRing, including pausing requestAnimationFrame when hidden. UI-66 call log (7 days). UI-67/109 persona settings. Opt-in memory at `$DATA/conductor/memory.md`.
- Wake phrase: a sherpa-onnx KWS model, bundled. It ships disabled if the benchmark fails.
- Audio routing reuses the DI-03 device policy.

**16B: Auto (AU-01).**
- `/auto/next`, the consent dialog, the local turn ceiling and every end reason.
- The redacted 200-line window, capped at 16 KB.
- The header chip "⚡ Finish · 3/20". It pauses while an approval is pending.

**16C: Phone companion (RM-03, BE-11).**
- `/relay/{pairing_id}` WebSocket: opaque frames of up to 64 KB and no crypto imports.
- QR pairing with X25519 and HKDF, then XChaCha20-Poly1305 frames, monotonic counters and rekeying.
- A PWA at `apps/web/app/remote`, with per-capability grants.
- Phone approvals need local arming for step-up. Revoke on unpair or sign-out.

**16D: Extras.**
- SH-12 Scratchpad.
- AC-07 Usage page (UI-37), plus the "Usage…" item in the Workspace menu.
- AG-10 skill sync.
- AG-13 teammate in a terminal.
- RT-05 schedule tools.
- BE-03 device code, with `/device` on the web.
- BE-12 admin page.
- CD-21 Simulator: mac only; absent elsewhere.
- BL-06 SSO through better-auth OIDC/SAML or WorkOS behind an interface, with a mock IdP.

**16E: Release candidate.**
- The full gate and every milestone's verification re-run, then `pnpm tsx scripts/killfuzz.ts --iterations 200` and the perf suite.
- `bash scripts/release-local.sh --dry-run`, then `git tag m16` and `v1.0.0-rc.1` (local only).
- The final `BUILD_LOG.md` summary per BUILD_PROMPT §6.

**Verification**
```bash
pnpm --filter api test -- -t "F-BE-03|F-BE-08|F-BE-11|F-BE-12"
cargo test -p tandem-voice realtime && cargo test -p tandem-voice --features kws -- --include-ignored f_cn_07_1
pnpm --filter desktop test -- -t "F-CN-|F-AU-01|F-SH-12|F-AC-07|F-AG-1[03]|F-RT-05|F-CD-21"
(pnpm tsx scripts/mock-realtime.ts &) && xvfb-run -a pnpm --filter desktop e2e -- --spec 'e2e/specs/m16-*.e2e.ts'
pnpm --filter web test:e2e -- --grep @m16
grep -rq "$E2E_KNOWN_SENTENCE" "$E2E_RELAY_LOG" && exit 1 || true      # relay sees no plaintext
cargo test -p tandem-invariants && bash scripts/release-local.sh --dry-run
```
**E2E** (`m16-*.e2e.ts`)
- **E2E-M16-1 Conductor call.**
  - A 65 s mock call. Expected: it settles to 140 credits, with the rest of the reservation refunded.
  - Hang up during a pending `prompt_pane`. Expected: no input reaches the pane.
  - The 30-min cap. Expected: the call ends with a notice.
  - Speech in the first 2 s, before connect. Expected: delivered after connect.
  - After 5 actions. Expected: 3 chips and "+2".
  - A `set_setting` key outside the closed list. Expected: rejected.
  - `run_command_in_shell`. Expected: nothing runs without the on-screen confirm click.
  - A voice "allow" for approval A. Expected: it can't resolve approval B.
- **E2E-M16-2 Find work.** "login testing". Expected: resolves to the pane "auth tests"; the local fuzzy fallback returns the same result when the server is down.
- **E2E-M16-3 Auto.**
  - The mock always returns `prompt`. Expected: it stops at exactly 20 turns, with end reason turn ceiling.
  - The engine exits mid-loop. Expected: end reason engine exited, and no further PTY writes.
  - A planted `sk-` token. Expected: `[REDACTED]` in the sent scrollback.
  - 0 credits. Expected: end reason out of credits.
  - Waiting for approval. Expected: Auto pauses.
- **E2E-M16-4 Phone.** A replayed frame. Expected: rejected. A phone approval without local arming. Expected: queued. After Revoke. Expected: the next command is rejected. A 65 KB frame. Expected: rejected.
- **E2E-M16-5 Extras.**
  - Scratchpad: kill within 1 s of typing. Expected: at most the last 300 ms is lost. `Mod+B`. Expected: `**` around the selection.
  - Usage fixtures. Expected: correct daily totals, and "N lines skipped".
  - Skill sync. Expected: it creates and removes the synced copy, and a same-name conflict is left untouched.
  - A teammate launched in a terminal. Expected: its first input starts with the brief.
  - A teammate without permission. Expected: no schedule tools. `tandem__schedule_create` with Full. Expected: rejected.
  - Device code: start → approve → token; polling too fast gives `slow_down`; an expired code gives `expired_token`.
  - An admin credit grant. Expected: a ledger entry with the admin id and reason. A non-admin. Expected: 403.

**Exit checklist:** G green · every sub-phase tagged · `HUMAN_TODO.md` final grouping · final `BUILD_LOG.md` entry (features by ID, known limitations, P2 next steps) · `v1.0.0-rc.1` tag.
**HUMAN_TODO:**
- *Accounts & keys:* realtime voice provider key and voice list (`REALTIME_API_KEY`); SSO provider (WorkOS) keys.
- *Legal:* a license review of the wake-word model.
- *Hosting:* the phone PWA at `app.tandem.dev/remote`, and the relay's WebSocket ingress.
- *Manual QA:* Simulator on a Mac with Xcode; wake-phrase false-accept benchmark (≤ 1 per 10 h); phone pairing on iOS Safari and Android Chrome.

---
## M17 · Extras (L)

**Goal.** Local features beyond the original spec that make working with agents faster, safer and more pleasant: checkpoints before every turn, command recall, a ports panel, Best of N, and a prompt library. Built in parallel with M8 and M10–M15 on branch `m17-extras` (decisions D-700–D-799); each feature lands on its own and can ship in any release after it's green.

**Owns:** F-EX-01, F-EX-02, F-EX-03, F-EX-04, F-EX-05 (all P2)
**Prerequisites.** M9 (Files editor, browser, git panel). Nothing in M17 needs M8 or M10–M16.

**Ordered tasks**
1. **Checkpoints (F-EX-01).**
   - `tandem_git::checkpoint`: a bare object store per folder at `$DATA/checkpoints/<key>/` with its own index (stat cache), so a checkpoint rehashes only changed files. The file list comes from the folder's repository (`ls-files --cached --others --exclude-standard`, read-only) or, outside a repository, an `ignore` walk capped at 20,000 files. Hashing runs with `--attr-source` at the empty tree and `core.autocrlf=false`, so bytes are stored as they are. A tree equal to the last one makes no checkpoint.
   - Restore in Rust: blobs written atomically with their mode, symlinks as links, deletions only of files the "now" snapshot lists, all through `contain()`.
   - Migration `0002_extras.sql` (`checkpoints`, `command_history`, `bestof_runs`, `bestof_lanes`); `CheckpointService` with one queue per folder; hooks in `ThreadService::send` (before `send_turn`), `pty_type_prompt` and `pty_write` (an agent pane's Enter waits up to 2 s); pruning; settings.
   - The Checkpoints sidebar tab (UI-140).
2. **Command recall (F-EX-02).**
   - The shell scripts add the command line to `133;C` as `cmdline_url=<percent-encoded>` (bash from its history, zsh and fish from preexec, PowerShell from its last history entry, sent with the `D` mark). `MarkFilter` parses it only with the token and never puts it back into the stream.
   - The side channel times C → D; `HistoryService` redacts and stores; retention at startup and hourly.
   - The recall palette (UI-141): a `QuickPicker` with the command palette's shell (D-706), Insert / Run / Delete.
3. **Ports panel (F-EX-03).**
   - `tandem_pty::ports`: a `PortProbe` trait with Linux (`/proc/net/tcp{,6}` + `/proc/<pid>/fd`), macOS (`lsof`) and Windows (`netstat -ano`) implementations, each unit-tested on recorded output; a pane's process tree from the PTY's pid.
   - `ports_list`, `ports_stop` (rechecks pane ownership and start time); the Ports sidebar tab (UI-142).
4. **Best of N (F-EX-04).**
   - `BestOfService` over the launch service (worktree isolation, a lane per agent) and lanes; `bestof_*` commands; checks run with the login environment, a 10-minute limit and a 200-line tail.
   - The Best of N sheet (UI-143) and the Compare dialog (UI-144) (D-710).
5. **Prompt library (F-EX-05).**
   - `tandem_core::prompts`: list (front matter, 64 KB limit, `contain()`), variables, render; `prompts_*` commands; the exclude-line narrowing.
   - The prompt picker (UI-145) as a `QuickPicker`, its variables form, and insertion into composers and panes without submitting.

**Verification**
```bash
cargo test -p tandem-git checkpoint && cargo test -p tandem-pty f_ex_0 && cargo test -p tandem-core prompts
cargo test -p tandem-desktop --lib f_ex_0
pnpm --filter desktop test -- -t "F-EX-"
dbus-run-session -- xvfb-run -a pnpm --filter desktop e2e -- --spec 'e2e/specs/m17-*.e2e.ts'
pnpm tsx scripts/trace-tests.ts --milestone M17 --fail-on-missing
```
**E2E** (`m17-*.e2e.ts`)
- **E2E-M17-1 Checkpoints.**
  - A thread turn in a repository whose fake engine edits, deletes and creates files. Expected: one checkpoint before the turn; the repository's HEAD, index and refs unchanged; Restore all brings the files back, deletes the new one and adds "Before restore".
  - Restore one chosen file. Expected: only that file changes.
  - Enter typed into an agent pane. Expected: a checkpoint before the engine sees the line.
  - A folder that isn't a repository. Expected: the honest note; restore works.
- **E2E-M17-2 Command recall.**
  - `echo one`, `false` and ` echo hidden` in a bash pane. Expected: two entries with ✓ and ✗ 1.
  - Insert. Expected: the text at the prompt, nothing run. Run. Expected: it runs once.
  - A command with a token. Expected: `[REDACTED]`, no Run.
- **E2E-M17-3 Ports.** A pane runs a small HTTP server on a free port. Expected: listed with the pane's name; Open loads it in a sidebar browser tab; Stop asks, Cancel keeps it, confirming ends it and the pane stays.
- **E2E-M17-4 Best of N.** Three fake-engine seats with a check command. Expected: three worktrees and panes with the same prompt; Compare shows three diffs; Run check shows each exit code; Keep this one merges the winner and removes the other worktrees; branches stay.
- **E2E-M17-5 Prompts.** A repository prompt with `{{branch}}` and `{{ticket}}`, and a profile prompt. Expected: both listed with their source; `{{ticket}}` asks; the text lands in the composer unsent and in a pane with no newline.

**Exit checklist:** G green · `trace-tests --milestone M17 --fail-on-missing` · the full E2E regression on a tree with `product-m0-m5` merged in.
**HUMAN_TODO:** *Manual QA:* the ports probe on macOS and Windows; checkpoints on a large repository (100k files) on the reference machines.

---

## M17 · Worktree workflow (M)

**Goal.** The worktree gaps `docs/COMPETITIVE_ANALYSIS.md` found against the category: setup for new worktrees with explicit trust, review comments sent to the agent, an overlap check and conflict forecast across parallel worktrees, and GitLab merge requests. Built on branch `competitive-gaps` (decisions D-2200–D-2299); each feature lands on its own and can ship in any release after it's green.

**Owns:** F-WT-01, F-WT-02, F-WT-03, F-WT-04 (all P2)
**Prerequisites.** M6 (worktree lanes, F-CD-15).

**Ordered tasks**
1. **Worktree setup (F-WT-01).** `tandem_git::setup`: parse `.vibezeo/worktree.json`, the contained copy walk, the commands' digest. `services::worktree_setup`: the trust file, the copy after every `lanes::create`, the setup pane (typed when ready, like `lane_create_pr`), the teardown before `lane_remove`. Commands `worktree_setup_get`, `worktree_setup_save`, `worktree_setup_trust`, `lane_setup`, `lane_setup_run`. UI-150 and UI-151; the New worktree dialog and the launch sheet call `lane_setup` and run or review.
2. **Review comments (F-WT-02).** The prompt builder (TypeScript, pure), comment affordances on the diff (UI-152), the agent picker, `pty_type_prompt`.
3. **Overlap check (F-WT-03).** `tandem_git::overlap` (changed-file sets, `merge-tree --write-tree --name-only`), `lane_overlap`, UI-153 and the merge confirm's verdict.
4. **GitLab (F-WT-04).** Remote host detection; `glab mr create`; the menu label.

**Verification**
```bash
cargo test -p tandem-git f_wt_0 && cargo test -p tandem-desktop --lib f_wt_0
pnpm --filter desktop exec vitest run -t "F-WT-"
dbus-run-session -- xvfb-run -a -s "-screen 0 1920x1080x24" pnpm --filter desktop e2e -- --spec 'e2e/specs/m17-wt-*.e2e.ts'
```

**E2E**
- **E2E-M17-WT-1 Setup.** A repository with `.env`, an ignored `node_modules/` and a `.vibezeo/worktree.json` whose setup writes a marker file. Expected: a new worktree has `.env`; the review dialog shows the command; Trust and run starts a pane in the worktree that writes the marker; saving from the dialog and making another worktree runs setup without asking; discarding runs the teardown.
- **E2E-M17-WT-2 Review.** A worktree with a fake-engine agent pane and a changed file. Expected: two comments, Send to agent, the agent echoes the prompt's header.
- **E2E-M17-WT-3 Overlap.** Two worktrees changing the same file and a conflicting base commit. Expected: Check overlap lists the sibling and the file; the merge confirm says it will conflict.

**Exit checklist:** G green · `trace-tests --milestone M17 --fail-on-missing` · the full E2E regression on a tree with `product-m0-m5` merged in.

---

## M17 · Blog (S)

**Goal.** A blog at vibezeo.com/blog/ that the owner writes in as files, with no code change (the owner's request, 15 October; D-1820). Built on branch `site-blog` (decisions D-1850–D-1864) with the elite pass's design (`site-elite`, D-1830–D-1844); `docs/BLOG.md` records the research and the writing guide.

**Owns:** F-WB-06 (P2)
**Prerequisites.** M15 (the site, F-WB-01's runtime strip and Lighthouse budgets).

**Ordered tasks**
1. **Posts as files.** The `blog` collection in `source.config.ts`, frontmatter validated at build (`lib/blog/schema.ts`), drafts only in `next dev`.
2. **Pages.** `/blog/`, `/blog/<slug>/`, `/blog/tags/<tag>/`, each post's share image, and the Atom feed; the header, footer, sitemap and `/llms.txt` link the blog.
3. **The runtime strip.** Post pages keep their JSON-LD and `/blog/blog.js` (`scripts/script-allowlist.mjs`); every other blog page ships no script.
4. **The first posts.** "Introducing Vibezeo" and "What's new in 0.7.2", each claim checked against the 0.7.2 build.

**Verification**
```bash
pnpm --filter web exec vitest run app/blog
pnpm --filter web build:site && PLAYWRIGHT_CHROMIUM=/opt/pw-browsers/chromium pnpm --filter web test:site
CHROME_PATH=/opt/pw-browsers/chromium pnpm --filter web lhci
```

**Exit checklist:** G green · the site's Playwright suite (`e2e-site/blog.spec.ts`, and the blog pages in `wb01` and `site-quality`) · Lighthouse CI on `/blog/` and a post within the site's budgets.

---

## M18 · Next features: worktree runtime and previews (M)

**Goal.** Make parallel worktrees runnable side by side: a port block, environment and Compose project per worktree (F-WT-06), then a preview per worktree (F-WT-09, next). From `docs/NEXT_FEATURES.md` (§1 #7, §3 B2). Built on branch `competitive-gaps`; decisions D-2440–D-2459.

**Owns:** F-WT-06, F-WT-09 (P2)
**Prerequisites.** M17 · Worktree workflow (F-WT-01), M17's ports panel (F-EX-03) and Best of N (F-EX-04).

**Ordered tasks**
1. **Worktree runtime (F-WT-06).** `tandem_git::setup` gains `ports` and `env` (validation, placeholders, reserved names, the digest including `env`). `services::worktree_runtime`: port blocks in `$DATA/worktree-ports.json` (allocation, the busy check, freeing on removal), the variables for a worktree, `*.localhost` resolution. The launch environment adds them to every pane in a worktree's tabs; the setup pane, teardown and Best of N's check get them. The ports panel groups by worktree. UI-150's two new fields and the secret warning.

2. **Previews per worktree (F-WT-09), three slices.** (a) `preview` in the file (trusted with the commands), `lane_preview_start`, `lane_previews` (a connect check on 127.0.0.1), the rail's Preview button, setup and preview in one pane, Compare's Previews. (b) Copy-on-write clones in F-WT-01's copy behind a `Cloner` trait (`FICLONE` on Linux, an ordinary copy everywhere else and as the fallback). (c) Side by side: a tab with two browser panes.

**Verification**
```bash
cargo test -p tandem-git f_wt_0 && cargo test -p tandem-desktop --lib f_wt_0
pnpm --filter desktop exec vitest run -t "F-WT-0"
dbus-run-session -- xvfb-run -a -s "-screen 0 1920x1080x24" pnpm --filter desktop e2e -- --spec 'e2e/specs/m18-wt*.e2e.ts'
```

**E2E**
- **E2E-M18-WT06-1 Runtime.** A repository whose `.vibezeo/worktree.json` has `ports` and `env`, trusted. Expected: two worktrees' shells print different `VIBEZEO_PORT` values and the filled-in variables; a server in one is grouped under its worktree in the ports panel; a relaunch keeps the blocks; the teardown sees `COMPOSE_PROJECT_NAME`.
- **E2E-M18-WT09-1 Previews.** A repository whose trusted file has `ports` and a `preview` running a small HTTP server. Expected: Start preview opens a Preview pane; the row's Preview button appears and opens the address in a browser tab; two lanes side by side open two browser panes.

**Exit checklist:** G green · `trace-tests --milestone M18 --fail-on-missing` · the full E2E regression.

---

## M18 · Next features: engines (M)

**Goal.** The engine work `docs/NEXT_FEATURES.md` ranks first (§1 #1, #3, #5, #9): one instructions file for every engine, an engine catalog that keeps up with the vendors, engine versions under the user's control, and one library of every engine's local sessions. Built on its own branch (decisions D-2400–D-2419); each feature lands on its own and can ship in any release after it's green.

**Owns:** F-EN-07, F-EN-09 (P2)
**Prerequisites.** M4 (engine registry and detection), M17's F-WT-01 (worktree copy patterns).

**Ordered tasks**
1. **One instructions file (F-EN-07).** The registry's `instructions` table (D-2400); `tandem_git::instructions` (inspect, plan, apply with backups, undo, copy into worktrees); commands `instructions_get`, `instructions_plan`, `instructions_apply`, `instructions_undo`, `instructions_copy_to_worktrees`; UI-160 from a local folder's rail menu.
2. **Engine catalog (F-EN-09).** Registry rows for Antigravity CLI and Kiro CLI from the vendors' docs (D-2404), the `note` field (Gemini CLI's), shown in the launch grid's not-installed list and Settings › Engines.

**Verification**
```bash
cargo test -p tandem-engines f_en_0 && cargo test -p tandem-git f_en_07 && cargo test -p tandem-desktop --lib f_en_0
pnpm --filter desktop exec vitest run -t "F-EN-0"
dbus-run-session -- xvfb-run -a -s "-screen 0 1920x1080x24" pnpm --filter desktop e2e -- --spec 'e2e/specs/m18-*.e2e.ts'
```

**E2E**
- **E2E-M18-EN07 Instructions.** A repository with a `CLAUDE.md` and an `AGENTS.md` that differ, and an ignored `CLAUDE.local.md`. Expected: the panel says Claude Code reads `CLAUDE.md`, not `AGENTS.md`, and shows the difference; "Use one file for all" writes nothing until confirmed, then `CLAUDE.md` is `@AGENTS.md`; Undo restores it byte for byte; "Copy into worktrees" adds `CLAUDE.local.md` to `.vibezeo/worktree.json`.
- **E2E-M18-EN09 Engine catalog.** Fake `agy` and `kiro-cli` on the login PATH. Expected: both detected and launched in panes, Kiro with `KIRO_HOME`; Gemini CLI keeps its note; neither new engine is chat-capable.

---

## M18 · Next features: usage (M)

**Goal.** The usage work `docs/NEXT_FEATURES.md` ranks (§1 #4, #8 and §3 B1): a forecast of when each account runs out before its window resets, a guard against runaway agents, and the Night Shift, work that waits for a reset. Built on branch `product-research`; decisions D-2460–D-2479.

**Owns:** F-AC-08, F-AC-09 (P2)
**Prerequisites.** M6 and M16 usage polling and the usage dashboard (F-AC-04, F-AC-07).

**Ordered tasks**
1. **Usage forecast (F-AC-08).** Per-window run-out at the current pace in the usage chip, "Who's using it" from the engines' own local logs (numbers only, F-AC-07's parser), the opt-in `usage_forecast` notification, and the launch sheet's near-limit note naming an account with more room; never switches accounts.
2. **Runaway guard (F-AC-09).** `tandem_engines::guard` (limits and per-session state), `tandem_pty::suspend` (`ProcessControl`: SIGSTOP/SIGCONT on Unix, unavailable on Windows) and `PtyManager::foreground_group`; the registry's `interrupt` and `subagent_tools`; `services::runaway` over the status snapshot and the tool sink; UI-170 on the pane, the `runaway` notification, Settings › Teammates › Runaway guard (`runaway_guard`, `runaway_action`); commands `runaway_list`, `runaway_dismiss`, `runaway_pause`, `runaway_resume`.

**Verification**
```bash
cargo test -p tandem-engines f_ac_0 && cargo test -p tandem-pty f_ac_09 && cargo test -p tandem-desktop --lib f_ac_0
pnpm --filter desktop exec vitest run -t "F-AC-0"
dbus-run-session -- xvfb-run -a -s "-screen 0 1920x1080x24" pnpm --filter desktop e2e -- --spec 'e2e/specs/m18-ac*.e2e.ts'
```

**E2E**
- **E2E-M18-AC08 Forecast** (`m18-ac08-forecast.e2e.ts`). A usage window that will run out before it resets: the chip's popover says when; the notification fires once per window when turned on.
- **E2E-M18-AC09 Runaway guard** (`m18-ac09-runaway.e2e.ts`). The fake Claude Code starts subagents past the limit: the card and the notification; Dismiss keeps it quiet for the turn; with "Pause the agent", an agent that ignores the interrupt key is stopped, and Resume continues it.

---

## 20. Coverage table (feature ID → owning milestone)

Every feature in PRODUCT_SPEC §4 appears here exactly once: **172 features** (65 P0, 67 P1, 40 P2). All 132 P0 and P1 features are owned by M0–M15. M16 owns the 20 original P2 features and 4 added since (F-RM-04 to F-RM-07: LAN pairing, the phone app, push and the Linux store packages), and M17 owns 10: the 5 extras (F-EX, §4.19), the 4 worktree-workflow features (F-WT, §4.20) and the blog (F-WB-06). M18 owns the next features as each is specified: 6 so far (F-AC-08, F-AC-09, F-EN-07, F-EN-09, F-WT-06 and F-WT-09). F-BL-06 is listed as P1: its P1 core is owned by M13, and M16 adds the P2 SSO extension.

| ID | P | M | ID | P | M | ID | P | M | ID | P | M |
|---|---|---|---|---|---|---|---|---|---|---|---|
| F-SH-01 | P0 | M2 | F-SH-02 | P0 | M3 | F-SH-03 | P0 | M3 | F-SH-04 | P0 | M2 |
| F-SH-05 | P0 | M2 | F-SH-06 | P0 | M5 | F-SH-07 | P0 | M5 | F-SH-08 | P0 | M8 |
| F-SH-09 | P1 | M3 | F-SH-10 | P0 | M5 | F-SH-11 | P0 | M2 | F-SH-12 | P2 | M16 |
| F-SH-13 | P0 | M2 | F-CD-01 | P0 | M3 | F-CD-02 | P0 | M3 | F-CD-03 | P0 | M3 |
| F-CD-04 | P0 | M3 | F-CD-05 | P0 | M3 | F-CD-06 | P0 | M3 | F-CD-07 | P0 | M3 |
| F-CD-08 | P1 | M3 | F-CD-09 | P1 | M9 | F-CD-10 | P1 | M3 | F-CD-11 | P1 | M4 |
| F-CD-12 | P0 | M5 | F-CD-13 | P0 | M4 | F-CD-14 | P1 | M6 | F-CD-15 | P1 | M6 |
| F-CD-16 | P1 | M6 | F-CD-17 | P1 | M9 | F-CD-18 | P1 | M9 | F-CD-19 | P1 | M9 |
| F-CD-20 | P1 | M9 | F-CD-21 | P2 | M16 | F-TH-01 | P0 | M7 | F-TH-02 | P0 | M7 |
| F-TH-03 | P0 | M7 | F-TH-04 | P0 | M7 | F-TH-05 | P0 | M7 | F-TH-06 | P1 | M7 |
| F-TH-07 | P0 | M7 | F-TH-08 | P0 | M7 | F-TH-09 | P0 | M7 | F-TH-10 | P1 | M7 |
| F-TH-11 | P0 | M7 | F-TH-12 | P0 | M7 | F-TH-13 | P1 | M7 | F-TH-14 | P1 | M7 |
| F-TH-15 | P0 | M7 | F-TH-16 | P1 | M7 | F-EN-01 | P0 | M4 | F-EN-02 | P0 | M4 |
| F-EN-03 | P1 | M4 | F-EN-04 | P0 | M4 | F-EN-05 | P0 | M4 | F-EN-06 | P1 | M4 |
| F-AC-08 | P2 | M18 | F-AC-09 | P2 | M18 | F-EN-07 | P2 | M18 | F-EN-09 | P2 | M18 |
| F-WT-06 | P2 | M18 | F-WT-09 | P2 | M18 | | | | | | |
| F-AC-01 | P0 | M4 | F-AC-02 | P0 | M6 | F-AC-03 | P0 | M6 | F-AC-04 | P1 | M6 |
| F-AC-05 | P0 | M6 | F-AC-06 | P1 | M6 | F-AC-07 | P2 | M16 | F-DB-01 | P0 | M5 |
| F-DB-02 | P0 | M5 | F-DB-03 | P0 | M5 | F-AG-01 | P1 | M10 | F-AG-02 | P1 | M10 |
| F-AG-03 | P1 | M10 | F-AG-04 | P1 | M10 | F-AG-05 | P1 | M10 | F-AG-06 | P1 | M10 |
| F-AG-07 | P1 | M10 | F-AG-08 | P1 | M10 | F-AG-09 | P1 | M10 | F-AG-10 | P2 | M16 |
| F-AG-11 | P1 | M10 | F-AG-12 | P1 | M10 | F-AG-13 | P2 | M16 | F-TK-01 | P1 | M11 |
| F-TK-02 | P1 | M11 | F-TK-03 | P1 | M11 | F-TK-04 | P1 | M11 | F-RT-01 | P1 | M11 |
| F-RT-02 | P1 | M11 | F-RT-03 | P1 | M11 | F-RT-04 | P1 | M11 | F-RT-05 | P2 | M16 |
| F-PL-01 | P1 | M10 | F-PL-02 | P1 | M12 | F-PL-03 | P1 | M12 | F-PL-04 | P1 | M12 |
| F-PL-05 | P1 | M12 | F-PL-06 | P1 | M12 | F-PL-07 | P1 | M12 | F-PL-08 | P1 | M12 |
| F-PL-09 | P1 | M12 | F-AU-01 | P2 | M16 | F-DI-01 | P0 | M8 | F-DI-02 | P0 | M8 |
| F-DI-03 | P0 | M8 | F-DI-04 | P0 | M8 | F-DI-05 | P1 | M13 | F-DI-06 | P0 | M8 |
| F-DI-07 | P0 | M8 | F-DI-08 | P1 | M8 | F-DI-09 | P1 | M8 | F-DI-10 | P1 | M13 |
| F-DI-11 | P0 | M8 | F-DI-12 | P0 | M8 | F-CN-01 | P2 | M16 | F-CN-02 | P2 | M16 |
| F-CN-03 | P2 | M16 | F-CN-04 | P2 | M16 | F-CN-05 | P2 | M16 | F-CN-06 | P2 | M16 |
| F-CN-07 | P2 | M16 | F-CN-08 | P2 | M16 | F-RM-01 | P1 | M14 | F-RM-02 | P1 | M14 |
| F-RM-03 | P2 | M16 | F-BE-01 | P0 | M1 | F-BE-02 | P0 | M2 | F-BE-03 | P2 | M16 |
| F-BE-04 | P0 | M1 | F-BE-05 | P1 | M13 | F-BE-06 | P1 | M13 | F-BE-07 | P1 | M13 |
| F-BE-08 | P2 | M16 | F-BE-09 | P0 | M1 | F-BE-10 | P0 | M1 | F-BE-11 | P2 | M16 |
| F-BE-12 | P2 | M16 | F-BL-01 | P0 | M1 | F-BL-02 | P0 | M1 | F-BL-03 | P1 | M13 |
| F-BL-04 | P0 | M1 | F-BL-05 | P1 | M13 | F-BL-06 | P1 | M13 | F-WB-01 | P0 | M15 |
| F-WB-02 | P1 | M15 | F-WB-03 | P1 | M15 | F-WB-04 | P1 | M15 | F-WB-05 | P0 | M2 |
| F-TL-01 | P0 | M2 | F-TL-02 | P0 | M0 | F-TL-03 | P0 | M0 | F-TL-04 | P0 | M5 |
| F-TL-05 | P1 | M15 | F-TL-06 | P0 | M15 | F-TL-07 | P1 | M13 | F-TL-10 | P1 | M13 |
| F-RM-04 | P2 | M16 | F-RM-05 | P2 | M16 | F-RM-06 | P2 | M16 | F-RM-07 | P2 | M16 |

**Per-milestone totals:** M0 2 · M1 7 · M2 8 · M3 12 · M4 9 · M5 8 · M6 8 · M7 16 · M8 11 · M9 5 · M10 12 · M11 8 · M12 8 · M13 10 · M14 2 · M15 6 · M16 24 · M17 10 · M18 6 = 172 (F-RM-04 to F-RM-07 added on `mobile-apps`, F-WT-01 to F-WT-04 on `competitive-gaps`, F-WB-06 on `site-blog`, and M18's on its sessions' branches).

| ID | P | M | ID | P | M | ID | P | M | ID | P | M |
|---|---|---|---|---|---|---|---|---|---|---|---|
| F-EX-01 | P2 | M17 | F-EX-02 | P2 | M17 | F-EX-03 | P2 | M17 | F-EX-04 | P2 | M17 |
| F-EX-05 | P2 | M17 | F-WT-01 | P2 | M17 | F-WT-02 | P2 | M17 | F-WT-03 | P2 | M17 |
| F-WT-04 | P2 | M17 | F-WB-06 | P2 | M17 | | | | | | |

### 20.1 Completeness check (run in M0, and at every milestone close)
`scripts/check-coverage.sh` implements the check below. It passed when this plan was written: 152 spec IDs, 152 owned, 0 missing, 0 duplicates, 0 unknown, and no P2 outside M16 and no P0/P1 inside it. Since M17 (D-700) the extras' milestone M17 also owns P2 features only, and the script accepts P2 in M16 or M17.
```bash
# 1) every feature heading in the spec, with its priority
grep -E '^#### F-[A-Z]{2}-[0-9]{2} ' build-kit/PRODUCT_SPEC.md \
  | sed -E 's/^#### (F-[A-Z]{2}-[0-9]{2}) [^(]*\([^P]*(P[0-9]).*/\1 \2/' | sort > /tmp/spec_ids.txt   # first P-tag wins
# 2) every ID on an **Owns:** line (continuation lines included), tagged with its milestone
awk '/^## M[0-9]+ ·/{m=$2} /^\*\*Owns/{o=1} o&&/^\*\*(Early|Completes|Extends|Prerequisites)/{o=0}
     o{while(match($0,/F-[A-Z][A-Z]-[0-9][0-9]/)){print substr($0,RSTART,RLENGTH), m; $0=substr($0,RSTART+RLENGTH)}}' \
  build-kit/MILESTONES.md | sort > /tmp/owned.txt
# 3) assertions
test "$(cut -d' ' -f1 /tmp/owned.txt | uniq -d | wc -l)" -eq 0                                 # no duplicates
diff <(cut -d' ' -f1 /tmp/spec_ids.txt) <(cut -d' ' -f1 /tmp/owned.txt)                        # none missing / unknown
join /tmp/spec_ids.txt /tmp/owned.txt | awk '($2=="P2")!=($3=="M16"||$3=="M17"){print "misplaced:",$0; bad=1} END{exit bad}'
```
The same script also checks that this §20 table and the `**Owns:**` lines agree.
