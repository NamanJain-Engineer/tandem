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
| `apps/web` | Website, docs and account portal: Next.js 15 |
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

## Tests

```bash
pnpm gate                                   # the global gate: lint, tests, desktop build, clippy, fmt, protocol drift
cargo test --workspace                      # Rust
pnpm -r test                                # TypeScript (vitest)
xvfb-run -a pnpm --filter desktop e2e       # desktop E2E (WebdriverIO + tauri-driver)
bash scripts/smoke-dev.sh                   # `pnpm dev` orchestration smoke test
bash scripts/check-coverage.sh              # every spec feature is owned by one milestone
pnpm tsx scripts/trace-tests.ts --milestone M0   # acceptance criteria without a test
pnpm ci:local                               # the CI jobs, locally
```

Tests set `TANDEM_HOME` to a temp directory and `TANDEM_FAKE_KEYCHAIN=1`.

## Release

Tag `v*` triggers `.github/workflows/release.yml` (added in M5): signed installers when signing secrets are present, unsigned artifacts with a summary note when they aren't.
