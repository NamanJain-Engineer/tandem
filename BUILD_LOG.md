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
- Public site for the Claude startup program: a Built with Claude section (Claude Code hooks and `--resume`, built with Claude Code, hosted AI features planned on the Claude API), real app screenshots (`apps/desktop/e2e/screens/site.screens.ts`), real progress figures and a changelog, a Request early access email, founder and company facts. Removed a claim the app doesn't support yet: approving tool requests from Dashboard cards (no real approval source until Thread mode). Then, after looking at a selected applicant's site (onemana.dev): `/pricing/` with the launch plans from PRODUCT_SPEC §3.1 (marked "Pricing at launch", planned features badged), and a 50 s unedited screen recording of the real app (`apps/desktop/e2e/screens/demo.screens.ts`: Claude Code detected, four terminals, kill -9, restore), served as VP9 WebM and H.264 MP4. On 7 October 2026 the site renamed the product to Vibezeo (the company's name), says 0.1.0 is released for Linux (macOS and Windows next), and serves the .deb and its SHA256SUMS from `public/downloads/` with no links to a code repository; the desktop app, its screenshots and the demo video still say Tandem until the app is renamed.
- Search and sharing: the founder's name links to their X profile, and every page carries an Open Graph and X card preview (the real app screenshot, `twitter:creator` from that profile). The public `vibezeo` repo no longer deploys a competing GitHub Pages site for vibezeo.com: its README points at the site and its page redirects there.
