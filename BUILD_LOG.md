# BUILD_LOG

## Current state
- Repo: github.com/NamanJain-Engineer/tandem (private). Build kit in `build-kit/`; plan in `docs/PLAN.md`.
- Milestone: **M3 in progress** (M2 done; tags `m0`, `m1`, `m2` are local only: the remote accepts the session branch only). Done in M3: layout engine and pane grid, command registry and shortcuts, rail and tab strip, `tandem-pty` side channel, per-run child registry and orphans, agent handback and Saved sessions, minimal engine registry, WebGL pool, clipboard/paste/drops/keys, find bar. Next: shell integration (task 10), the remaining E2E-M3 specs (1, 2, 3, 9, 10, 11, 13), perf suite, then tags `m3a`/`m3`.
- Layout matches DECISIONS: 10 crates under `crates/`, Tauri crate `apps/desktop/src-tauri` (lib `tandem_app`, bin `tandem`), `apps/{api,web}`, `packages/{ui,protocol}`.
- Environment: WebKitGTK, webkit2gtk-driver, xvfb, tauri-driver, dbus, xclip, openbox, cargo-audit and a MinGW cross compiler (`cargo check --target x86_64-pc-windows-gnu`) are installed; `dockerd` must be started by hand in a fresh container (`dockerd &`), then `pnpm infra:up`. When the disk fills, delete `target/debug/incremental` and the stale `target/debug/deps/{tandem,libtandem}*` artifacts (they rebuild in a couple of minutes).
- Gate: `pnpm gate` (lint, vitest, cargo test, desktop build, clippy, fmt, protocol drift) is green. Stage new files first: the drift check diffs the working tree against the index. `gen:protocol` also writes `packages/protocol/shortcuts.json`.
- Desktop E2E: `dbus-run-session -- xvfb-run -a -s "-screen 0 1920x1080x24" pnpm --filter desktop e2e` runs the real Tauri binary via tauri-driver (starts API + web if needed, openbox for WM behaviour, scripted Chromium for sign-in and checkout). 9/9 spec files pass.
- API tests need `pnpm infra:up` (they create `tandem_test`). Web E2E: `PLAYWRIGHT_CHROMIUM=/opt/pw-browsers/chromium pnpm --filter web test:e2e` after `db:migrate`, `db:seed`, `keys:dev`.
- Release harness check: `bash scripts/check-release-harness.sh` (release build, asserts no `test_*` commands in the binary).
- Open issues: none blocking. Root scripts `gen:shortcuts`, `gen:docs` arrive with M3 and M15. The Linux single-instance plugin needs a DBus session bus (E2E runs under `dbus-run-session`).

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

## M1 checklist (MILESTONES M1)
- [x] 1. Drizzle schema per ARCHITECTURE §14.2 (+ better-auth tables with `role`, `deletionRequestedAt`), `drizzle/0000_init.sql`, `db:migrate`
- [x] 2. Plans module `packages/protocol/src/plans.ts` (prices, limits, credit prices, packs); Free limits pinned by a shared fixture also read by Rust
- [x] 3. better-auth: email+password (≥10 chars, verification), magic link, Google when configured, 30-min resets, breached-password hook (offline list), telemetry off; double-submit CSRF; `DELETE /v1/me`
- [x] 4. Entitlements: `GET /v1/me` with Ed25519 JWS (`ek-1`, 24 h), `pnpm keys:dev`, `requirePlan()` → 403 FORBIDDEN_PLAN, SSE `/v1/me/events` over Redis pub/sub
- [x] 5. Rust verifiers: `tandem-core::entitlement` (grace rule, refresh never downgrades on network error) and `tandem-core::remote_config` (bundled fallback); interop fixture signed by the API's own code
- [x] 6. Remote config: canonical payload signed with `ck-1`, ETag + `max-age=600`, 304, `x-tandem-min-version` on every response
- [x] 7. Billing: `BillingProvider` (Stripe + Mock), checkout (Pro 7-day / Team 14-day trial, promos), portal, credit-pack checkout, idempotent webhooks, monthly grant with reset, dunning email, lapse rules in `effectivePlan`, mock checkout and mock portal through the same handler
- [x] 8. Edge: Redis token bucket 300/60 s per IP (health, webhooks, test routes excluded), per-user feature helper, Turnstile on web sign-up/sign-in only (bearer bypass), webhooks by signature, `alert=auth_spike`
- [x] 9. Desktop auth server side: allowlisted redirect URIs, authorize (5-min single-use code bound to challenge + fingerprint), exchange (wrong verifier 400), rotating refresh with family revocation, logout, `/v1/me/sessions` list + revoke
- [x] 10. Web pages: /signup, /signin (+ magic link), /verify, /reset, /desktop-login, /account, /billing, /billing/success, /billing/mock, /billing/mock-portal, /devices, /credits; `/v1/*` proxied first-party
- [x] 11. Test routes `/__test/{clock,plan,mail/latest,user/delete,subscriptions}` (flag only, never production), `scripts/stripe-fixture.sh`, `pnpm db:seed`, `apps/api/openapi.json` generated from zod
- [x] Verification commands: all pass (401 problem+json, 304, 400 bad signature, invoice.paid ×2 → 10,000 once, min-version header)
- [x] E2E-M1-1…5 (Playwright, `apps/web/e2e/m1-account.spec.ts`): all pass; trace-tests M1: 16/16 criteria covered

## M2 checklist (MILESTONES M2)
- [x] 1. `@tandem/ui`: UI_SPEC §2.1–§2.19, §2.22 CommandPalette shell, §2.31 composites (ListRow, PageHeader, SettingsRow/Group, Banner, Callout, KeyValue, EmptyState, Stepper); one gallery list drives `/__gallery` (debug builds) and a snapshot + axe check per state in both themes (204 checks); `ThemeProvider` (dark/light/system live, derived high contrast, reduce motion → 0.01 ms), §1.9 terminal palettes
- [x] 2. Frame F-SH-01: decorations off on Windows/Linux, overlay title bar config for macOS; UI-01 title bar (drag regions, min/max-restore/close with the label toggle, logo + wordmark + DEV pill, rail toggle, mode switch, Workspace menu only in Code, omnibox, credits, bell, sidebar toggle, account menu); window-state in `$DATA/window-state.json`, saved 400 ms after any move/resize and re-centred when not fully on a display; single-instance focuses the first window, forwards links, exits 0; §3.6 status bar
- [x] 3. Theme F-SH-05: native background from the persisted theme before first paint, system theme live, high contrast, reduce motion; OSC 10/11/12 answered in Rust with the theme colours (`term_set_theme_colors`)
- [x] 4. Settings F-SH-04: UI-100 shell (rail becomes nav, "‹ Back (Esc)", Esc restores mode and focus, reopens on last page, filter narrows pages and highlights rows for 2 s), pages UI-101/102/104/112/114/116, writes apply immediately and revert with Retry on failure; table-driven round trip over all keys (store test)
- [x] 5. Logging F-SH-11: tracing in the app, UI errors through `app_log` by type only, app menu Help items Copy diagnostics (also clipboard) and Open logs folder
- [x] 6. Desktop sign-in F-BE-02 / UI-03: PKCE, state, device fingerprint, ephemeral loopback + `tandem(-dev)://auth/callback`, `TANDEM_OPEN_URL_CMD` or the opener plugin, AES-256-GCM `auth/credentials.enc` with the key in `NS.auth/credentials-key` (session-only key without a keyring), refresh at 80% / on wake / on 401 (single-flight), four failure categories with Copy details, mismatched state ignored, offline mode
- [x] 7. Entitlements in the app: `/v1/me` verified with the bundled keys and cached, SSE `/v1/me/events` refetch, UI-133 banners (offline, grace over, offline-mode deadline, payment failed), Account & Plan page, Manage billing / Buy credits through the system browser
- [x] 8. F-WB-05: revoking a device on `/devices` revokes its family; the desktop's next refresh fails and it shows sign-in (E2E-M2-6, web `@m2`)
- [x] 9. Telemetry F-TL-01: `tandem-core::telemetry` (allowlist in `packages/protocol/telemetry-allowlist.json`, analytics id file deleted on opt-out, crash scrubbing), uploads through `Net` so `TANDEM_NET_LOG` sees them; off by default; two toggles on the Privacy page
- [x] 10. Deep links F-SH-13: known routes only, 4 KB limit, "Unrecognized link"; `auth/callback` and `open?path=`; links queue until the UI is ready; launch at login (autostart) on General
- [x] 11. Skeleton slices: Code rail with add (non-blocking picker), dedupe by canonical path, tabs, Free limit of 3 → UI-120 sheet → UI-121 checkout handoff; one plain shell per tab over a binary `Channel` per pane; mode switch with interim Agent/Thread surfaces; plan/credits footer
- [x] Verification commands: `f_be_02` (6), `f_tl_01` (5), `f_sh_11` (2), UI 228 tests, desktop `-t "F-SH-0[145]|F-TL-01|F-SH-13"` (13), web `@m2`, desktop E2E m2-*, log and net.log greps clean
- [x] E2E-M2-1…8 (`apps/desktop/e2e/specs/m2-*.e2e.ts`): all pass against the real app; axe passes in both themes on sign-in, the shell, the upgrade sheet and every Settings page
- [x] Exit: README "Sign in locally"; release builds have no test harness (unit test + `scripts/check-release-harness.sh`); HUMAN_TODO `SENTRY_DSN`, `POSTHOG_KEY`, production keys, manual QA items

## M3 checklist (MILESTONES M3)
- [x] 1. `tandem-pty` sessions: side channel (vt100 shadow screen, CPR from its cursor, modes and title for `pty:state`, 100 ms debounce); every query answered in Rust with xterm's replies off; live-pane limit at `pane_create`; per-run child registry `run/children-<pid>.json` (owner-stamped, scanned in the background after the previous instance exits), orphan scan/stop and UI-132 with the policy setting; agent panes under the login shell with a tokenised handback mark → Saved sessions (`sessions_saved_list`, `sessions:saved`); scrollback snapshots at quit and every 5 min; PATH-only engine resolution; Windows Job Object (cross-checked with `--target x86_64-pc-windows-gnu`; behaviour on Windows CI and HUMAN_TODO)
- [x] 2. Layout engine F-CD-02: pure tree (fast-check round trip), PaneGrid on react-resizable-panels with reparented pane hosts, PTY resize after pointer-up / 50 ms throttle, 240×120 minimums, dnd-kit drag (4 px, five zones, ghost, new-tab drop, Esc), maximize without unmounting, focus ring and `Mod+Alt+Arrow`, pane picker with the Free card, `layout_get` / `layout_save` / `pane_move`
- [x] 3. Workspaces and tabs F-CD-01: tab rename (double-click, F2), drag reorder, close, `+`; closing the last tab leaves the empty state; missing folders block launches and offer Locate… / Remove (`workspace_locate`); debounced layout saves. Folder drop on the rail and pane-to-rail adoption wait for cwd tracking (task 10)
- [x] 4. Rail F-SH-03: generic `RailList` (Pinned band by drag, reorder, context menu with Pin/Rename/Move up/down/Reveal/Copy path/Remove, inline rename, keyboard, expand to open panes), resize 180–360 persisted, footer from M2. Status slots and Saved sessions arrive with M4 and task 1
- [x] 5. Mode switch F-SH-02: radiogroup and `Mod+1/2/3` (also from a terminal) via the registry; every surface stays mounted
- [x] 6. Terminal rendering F-CD-04: WebGL pool of 8 (focused first, then most recently shown; hidden panes release after 8 s; context-loss retry; "Lite" badge), Unicode 11, plain-name font stack re-measured after the web font loads, unfocused output coalesced to ~30 fps, Jump-to-bottom pill (`terminal.jump_bottom`)
- [x] 7. Clipboard, drops and keys F-CD-05/06/07: copy/paste rules per platform, "Paste N lines?" for shells, bracketed paste via xterm, clipboard images staged as PNG for agent panes (`term_stage_clipboard_image`, 7-day sweep), OS file drops hit-tested to the pane under the pointer with per-dialect quoting (`term_quote_paths`), 600 ms tab hover, Shift+Enter newline sequence from the registry
- [x] 8. Command registry F-SH-09: `tandem-core::commands` with macOS / Windows-Linux / terminal-focused bindings, `pnpm gen:shortcuts`, registry-driven dispatcher (passthrough rule, IME guard, release webview-key suppression), palette on the same handlers
- [x] 9. Find F-CD-08: search addon bar (case/regex toggles, "n of m", Enter/Shift+Enter and `Mod+G`/`Mod+Shift+G`, Esc), highlight colours from the terminal theme, `terminal.find*` registry commands
- [ ] 10. Shell integration F-CD-10
- [ ] E2E-M3-1…16, perf suite, exit checklist

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
- D-19: `GET /v1/config` sends `payload` as the RFC 8785 canonical JSON *text* and signs its UTF-8 bytes, so the desktop verifies without re-canonicalizing. The bundled fallback is `resources/config.json`, a copy of `apps/api/config/remote-config.json` (a Rust test keeps them equal).
- D-20: API tests run against the compose Postgres and Redis (a fresh `tandem_test` database per run) instead of fakes; `pnpm gate` therefore needs `pnpm infra:up`. CI's test-ts job starts the compose services.
- D-21: Added the dev-only `POST /v1/billing/mock/portal` (declared in ARCHITECTURE §14.6) so the mock portal can cancel at period end through the same webhook handler, and test-only `/__test/user/delete` and `/__test/subscriptions` for re-runnable E2E.
- D-22: The API serves better-auth with `baseURL = WEB_PUBLIC_URL`; browsers reach it through the web app's `/v1/*` rewrite so session cookies are first-party. SSE responses set `Cache-Control: no-cache, no-transform` so the proxy doesn't gzip-buffer the stream.
- D-23: The rate limiter's refill clamps negative elapsed time to zero (found when the test clock moved backwards and locked a client out).
- D-24: Stripe fixture event ids derive from the user, so replays are no-ops (M1 verification); E2E passes a nonce for fresh events.
- D-25: The analytics id lives in its own file, `$DATA/telemetry/analytics-id` (MILESTONES E2E-M2-7 checks "the analytics id file"), instead of a settings row. Opting out deletes the file.
- D-26: Crash reports use a minimal Sentry envelope client over `Net` instead of the `sentry` crate, so every request shows in `TANDEM_NET_LOG` and nothing else can send; the panic hook only writes scrubbed reports under `$DATA/crashes/`.
- D-27: `TANDEM_FAKE_PICK_FOLDER` (debug only, declared in ARCHITECTURE §17.5) makes `workspace_pick_folder` return a path, or the contents of a file it names, because WebDriver can't drive the native dialog. `pane_list` added to §3.2 until `layout_get` lands in M3.
- D-28: ts-rs 12 with `TS_RS_LARGE_INT=number`: JSON over IPC carries i64/u64 as numbers, so the generated TypeScript says `number`.
- D-29: Each PTY IPC call gets its own JS `Channel`: when Rust drops a channel (a failed `pty_attach`) the JS side closes it for good, so reusing it for `pty_spawn` lost every frame.
- D-30: Status badges keep their §1.3 `*-text` colour on the surface with a tinted border instead of a soft tinted fill: the text tokens pass AA on surfaces (UI_SPEC §7.3) but the light amber/green/rose text fell to ~4.2:1 on their own tints. Found by the both-themes axe pass.
- D-31: The E2E keychain is in memory (`TANDEM_FAKE_KEYCHAIN=1`), so a relaunched app is signed out by design; specs that relaunch use "Use Tandem offline" (persisted without secrets) instead of a file-backed fake keychain, which would put secrets on disk.
- D-32: Signing out also leaves offline mode (back to UI-03). The window state is also saved 400 ms after each change so a crash or kill keeps it, and a restored window that isn't fully on one display is re-centred (the plugin alone only checks overlap).
- D-33: Panes render into host elements the grid moves between layout slots (`paneHosts.ts`), so swap, move and maximize never remount xterm: the WebGL context and scrollback stay (F-CD-02 AC2, AC3).
- D-34: A fresh tab starts a Shell (E2E-M2-1 expects "a tab with a live shell"); a tab whose last pane closed, or that replaced the last closed tab, shows UI-10's launch tiles instead.
- D-35: `pane_move {paneId, tabId}` (declared in ARCHITECTURE §3.2) moves a pane to another tab of its workspace with the same process: the tab-strip drop and "Move to new tab".
- D-36: The command registry carries the three binding columns of UI_SPEC §5 (`mac`, `pc`, `pcTerminal`); titles are `cmd.<id>`. It lists only commands that have a handler and grows each milestone; a test checks every `pcTerminal` binding against the terminal intercept list.
- D-37: Code mode's rail order is `workspaces.sort_key` / `pinned` (`rail_move` with `mode: "code"`); `rail_items` holds the Agent and Thread rails when they land. `rail_width` defaults to 248 within 180–360.
- D-38: Terminal queries are answered in Rust for every pane, visible or hidden, and xterm's own replies are switched off with parser hooks (ARCHITECTURE §5.7). MILESTONES M3 describes xterm answering while visible; ARCHITECTURE outranks it, and both give exactly one reply. CPR uses the vt100 shadow cursor as of the query.
- D-39: Orphans are found with `sysinfo`: a record matches when its PID is alive with the same start time, and on Unix any process in the pane's session (the session id is the recorded PID) also counts, so a `nohup` child of a shell that died with the app is found. Windows relies on the Job Object.
- D-40: `scripts/gate.sh` runs one command per line. Under `set -e` a failure inside an `a && b` list doesn't stop the script, so a red `pnpm -r test` once let the gate exit 0 (found when Postgres was down after a container restart; the earlier gated commits had run every step).
- D-41: The E2E axe helper waits for finite animations to finish before running, so a dialog mid-fade isn't measured as low contrast.
- D-42: A minimal engine registry lands in M3 (`crates/tandem-engines/registry.json`: id, label, vendor, commands, provider home variable, resume, newline sequence) with PATH-only detection behind `engines_list`, because E2E-M3-5 runs an agent pane. M4 adds the rest of §6.1, the detection report and install commands; `EngineView` keeps its shape.
- D-43: The handback mark (`OSC 7770;handback;<token>;<code>`) carries a per-launch token passed as `TANDEM_SHELL_TOKEN`; the wrapper copies it into an unexported shell variable and unsets it before the engine starts, so it's in neither argv (`ps`) nor the engine's environment (§5.8's rule for shell marks).
- D-44: Engines resolve on the login PATH, then the app's own PATH: Linux login profiles reset PATH (`/etc/profile`), which would hide installs the app can see. The working directory is never searched.
- D-45: Each run records its children in `run/children-<app pid>.json` stamped with the app's own PID and start time, and deletes it on a clean exit. The startup scan runs in the background, collects only files whose owner has exited and waits up to 8 s for one still shutting down, so a quick relaunch doesn't report children the old instance is stopping, and two overlapping instances don't overwrite each other's records (both were seen in E2E). `orphans_list` waits for the scan.
- D-46: Find highlights use two new terminal theme tokens (`searchMatch`, `searchActive`, `#RRGGBB` as the search addon requires) so the bar adds no ad-hoc colours. A first search starts from the top of the scrollback, so the first match reads "1 of n" (F-CD-08 AC1).
- D-47: `terminalHandles` and the WebGL pool only drop an entry its owner registered, so a late unmount of an earlier view can't remove the live one.
- D-48: E2E specs share one `TANDEM_HOME`, so each launch restores the previous spec's workspace. The reset removes workspaces through IPC and then reloads the webview, so the UI never shows a pane the backend has closed; failures also save `__tandemTest.debug()` (stores, panes, focus, dialogs) next to the screenshot.
- Deviation: the `keyring` crate is linked (Secret Service on Linux). `MemoryKeychain` is selected only with `TANDEM_FAKE_KEYCHAIN=1`.

## Log
- M0 (earlier session): first slices of core, engine registry, local store, desktop shell frame, API health, CI.
- M0: build kit and plan committed (`build-kit/`, `docs/PLAN.md`).
- M0: realigned to DECISIONS: crate rename to `tandem-engines`, seven new crates, Tauri crate with capabilities and CSP, ts-rs protocol generation, UI_SPEC tokens, ICU i18n, rewritten core (redactor, containment, settings schema, keychain, logging, store with writer thread and full schema), API restructured to ARCHITECTURE §14.1, web on the shared tokens, dev/gate/coverage/trace/CI scripts, desktop E2E harness. Both M0 E2E scenarios pass. M0 closed.
- M1: backend accounts, entitlements, remote config, mock billing, edge limits, desktop auth server side, account portal pages. 37 API tests, 6 Playwright scenarios, Rust verifiers with an interop fixture. M1 closed.
- M2: desktop shell, design system and sign-in. `tandem-pty` (reader/batcher/flow control/ring/query responder/tree kill/login env), core `net` + `telemetry`, store repos for workspaces/tabs/panes, desktop services (api, auth, entitlement, deeplink, workspaces, terminal, telemetry, events), the M2 command set and plugins, `@tandem/ui` kit with gallery, and the desktop screens. Fixed on the way: boot waited on an animation frame a hidden window never gets; channel reuse lost PTY output; Settings Esc could miss a non-element target; the terminal stole focus back from Settings' opener. M0's boot spec now asserts the first real screen (the splash is transient since M2). All M2 E2E scenarios pass. M2 closed.
- M3 (in progress): pane grid, picker and drag and drop; command registry, dispatcher and palette; rail, tab strip and missing-folder flow; `tandem-pty` side channel and child registry. Fixed on the way: a new pane took focus twice and could steal it back from Settings' opener; Escape in an inline rename still committed through the blur; the rail tree owned list markup (axe); `gate.sh` could pass with a failed step.
- M3 (cont.): handback and Saved sessions, engine registry slice, Windows Job Object, WebGL pool and rendering options, clipboard/paste/drops/keys, find bar, per-run orphan registry, E2E-M3-4…8, 12, 14…16 specs. Fixed on the way: a quick relaunch reported the old instance's dying children as orphans and could overwrite the new run's registry; the E2E reset left the UI showing the previous spec's closed pane; zombies were counted as orphans; a closed pane could still be spawned.
