# Tandem

A cross-platform desktop workspace for building software with the coding-agent CLIs you already pay for: terminal panes (Code mode), coding sessions read as conversations (Thread mode), and persistent teammates (Agent mode). Tandem hosts those engines. It never reads their tokens, and nothing leaves your machine unless you turn it on.

Tandem is a clean-room product. It does not use another product's names, copy, artwork or code.

- What and why: [`docs/PLAN.md`](docs/PLAN.md) (analysis, architecture, milestone plan)
- Binding specs: [`build-kit/`](build-kit/) (DECISIONS, PRODUCT_SPEC, ARCHITECTURE, UI_SPEC, MILESTONES)
- Progress: [`BUILD_LOG.md`](BUILD_LOG.md). Human-only steps: [`HUMAN_TODO.md`](HUMAN_TODO.md)

## Repository layout

| Path | What |
|---|---|
| `apps/desktop` | Tauri 2 app: React UI in `src/`, Rust crate `tandem-desktop` (lib `tandem_app`, bin `tandem`) in `src-tauri/` |
| `apps/api` | Backend: Hono, Drizzle, Postgres, Redis |
| `apps/web` | Website ([vibezeo.com](https://vibezeo.com), static export on Vercel: [docs/DEPLOY_SITE.md](docs/DEPLOY_SITE.md)), docs and account portal: Next.js 15 |
| `apps/site` | An earlier one-page draft of the public page (not deployed) |
| `crates/tandem-*` | Rust libraries: core, store, pty, engines, git, gateway, tasks, routines, voice, host |
| `packages/ui` | React components and design tokens |
| `packages/protocol` | Wire types (generated from Rust with ts-rs) and the API client |
| `infra/` | docker compose: Postgres 16, Redis 7, Mailpit, stripe-mock |

## Setup

Requirements: Rust stable, Node 22, pnpm 9, Docker with compose v2. On Linux, `bash scripts/bootstrap-linux.sh` installs WebKitGTK and the other system packages; `bash scripts/check-env.sh` lists anything missing.

```bash
pnpm install
pnpm dev                 # infra + API (:8787) + web (:3000) + desktop app
pnpm dev --no-desktop    # infra + API + web
pnpm dev --api-only      # just the API
```

Ctrl-C stops the apps and leaves the infra running (`pnpm infra:down` stops it). If `api.tandem.local` doesn't resolve, the dev script prints the `/etc/hosts` line and uses `127.0.0.1` for that run.

First run:

```bash
pnpm infra:up
pnpm keys:dev       # Ed25519 dev keys → .dev-keys/, public keys → the desktop's resources/keys.json
pnpm db:migrate
pnpm db:seed
```

Seeded accounts (all verified, password `Tandem-dev-pass-1`): `free@tandem.test` (Free), `pro@tandem.test` (Pro, 10,000 monthly credits), `pastdue@tandem.test` (payment failed today), `lapsed@tandem.test` (cancelled, period ended). Email goes to Mailpit at http://localhost:8025. Billing runs on a mock provider: Upgrade completes instantly through `/billing/mock`. `bash scripts/stripe-fixture.sh <event> <email>` posts a signed webhook fixture.

## Sign in locally

With `pnpm dev` running (API on :8787, web on :3000):

1. Create an account at http://localhost:3000/signup (or use a seeded one, e.g. `free@tandem.test` / `Tandem-dev-pass-1`). The verification email lands in Mailpit at http://localhost:8025.
2. In the desktop app, click **Continue with email**. Your browser opens `http://localhost:3000/desktop-login`; sign in there.
3. The browser tab says "You can return to Tandem." and the app shows your plan in the rail footer ("FREE", or "PRO · 10,000 credits").

The app listens for the callback on an ephemeral `127.0.0.1` port and also accepts `tandem-dev://auth/callback` (dev builds use the `tandem-dev` scheme). Point the app elsewhere with `TANDEM_API_URL` and `TANDEM_WEB_URL`. **Use Tandem offline** skips the account: Free limits, sign-in needed within 14 days. Tokens are stored AES-GCM encrypted in `$TANDEM_HOME/auth/credentials.enc`, with the key in the OS keychain (`app.tandem.dev.auth` in dev builds).

To try the paid path, click **See plans** (account menu) and start the Pro trial: the mock checkout opens in your browser, and the app switches to Pro within a few seconds of completing it.

## Tests

```bash
pnpm gate                                   # the global gate: lint, tests, desktop build, clippy, fmt, protocol drift
cargo test --workspace                      # Rust
pnpm -r test                                # TypeScript (vitest)
dbus-run-session -- xvfb-run -a -s "-screen 0 1920x1080x24" pnpm --filter desktop e2e   # desktop E2E (WebdriverIO + tauri-driver)
bash scripts/smoke-dev.sh                   # `pnpm dev` orchestration smoke test
bash scripts/check-coverage.sh              # every spec feature is owned by one milestone
pnpm tsx scripts/trace-tests.ts --milestone M0   # acceptance criteria without a test
pnpm ci:local                               # the CI jobs, locally
```

Tests set `TANDEM_HOME` to a temp directory and `TANDEM_FAKE_KEYCHAIN=1`. The desktop E2E runner starts the API (test routes, mock billing) and the web app if they aren't up, drives sign-in and checkout through a scripted headless Chromium (`e2e/helpers/browser-signin.ts`, wired as `TANDEM_OPEN_URL_CMD`), starts openbox for window-manager behaviour, and fails the run if the logs contain secret patterns or the network log shows analytics or crash hosts. It prints `E2E_LAST_HOME` (also saved in `test-results/e2e-last-home`). API tests and the web E2E use the compose Postgres and Redis, so run `pnpm infra:up` first (API tests create their own `tandem_test` database).

```bash
pnpm --filter web test:e2e      # Playwright; set PLAYWRIGHT_CHROMIUM to a Chromium binary if Playwright's own isn't installed
pnpm --filter api openapi       # regenerate apps/api/openapi.json
```

## Release

Tag `v*` triggers `.github/workflows/release.yml` (added in M5): signed installers when signing secrets are present, unsigned artifacts with a summary note when they aren't.
