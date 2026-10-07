# BUILD_LOG

## Current state
- Repo: https://github.com/NamanJain-Engineer/tandem (private, owner NamanJain-Engineer).
- Milestone: M0 in progress. Not tagged.
- Done: core, engine registry, local domain store, sync API, desktop shell frame, web health, CI.
- Next: process supervisor for engines, then the Tauri window host once GTK/WebKit is available in CI.
- Gate: `pnpm -r lint`, `pnpm -r test`, `cargo test --workspace`, clippy, rustfmt. Docker is not available here, so compose is not started.

## Decisions
- D-1: F-CD-09 owned by M9. Needs the files editor and browser.
- D-2: F-SH-08 framework slice in M5, accounts step in M6, owned by M8.
- D-3: F-DI-09 owned by M8 so dictionary actions exist before enhance interplay.
- D-4: M7 builds the gateway skeleton early so thread approvals are real.
- D-5: M7 ships a mock auto-title route; the real provider lands in M13.
- D-6: Dashboard approval is tested on the broker first, then again with a real thread.
- D-7: Thread interrupted state is tested on the sessions table in M5 and in the UI in M7.
- D-8: Create-with-AI is tested against an API fixture in M10.
- D-9: Threat-model tests land beside each mechanism; M15 audits completeness.
- D-10: Test-only dependencies do not change the runtime stack.
- Deviation: production OS keychain (`keyring`) is not linked yet. `MemoryKeychain` is selected when `TANDEM_FAKE_KEYCHAIN=1`.
- D-11: One repository. Split a deployable only when it has its own release cadence.
- D-12: Local product data is SQLite. The API sync log is Postgres when `DATABASE_URL` is set, so API nodes stay stateless.
- D-13: Tauri is not in the workspace yet. No GTK/WebKit here, and a crate that cannot compile would fail the gate. The shell UI is `apps/desktop`.

## Log
- M0: F-TL-02 and F-TL-03 core, plus the early slices of settings, commands, and sanitized logs.
- M0: API `GET /v1/health`, web `GET /api/health`, protocol types, Button, compose file, CI.
- M0: engine registry, local domain log, sync API, desktop shell.
- M0: engine spawn plans drop secret environment variables before a child starts.
- Public site: `apps/site` is the Vibezeo page (vibezeo.com). It describes the product without invented traction.
- Public site on Vercel: `apps/web` is now the vibezeo.com site (home, about, privacy, terms, security, robots, sitemap, security.txt), a static export (`pnpm --filter web build:site`) that Vercel deploys from `main` (`apps/web/vercel.json`, `docs/DEPLOY_SITE.md`). It uses the `@tandem/ui` tokens, fonts, Badge and StatusDot. `apps/site` is the earlier one-page draft and isn't deployed. The rest of the M0–M5 work arrives through a pull request.
