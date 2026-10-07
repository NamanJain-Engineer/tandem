# Tandem

Cross-platform desktop workspace for building software with the coding-agent CLIs you already pay for. Terminal panes, readable threads, and named teammates. Tandem hosts those engines. It does not replace them, and it never reads their tokens.

This repository is a clean-room product. It does not use another product’s names, copy, or code.

## Status

Milestone M0 is in progress. Shipped so far:

- `tandem-core`, `tandem-engine`, `tandem-store` (workspaces, sessions, panes, agents, event log)
- `api`: health, and an idempotent sync log (`/v1/sync/events`)
- `desktop`: shell frame on port 1420
- `web`: the public site for [vibezeo.com](https://vibezeo.com) (static export on Vercel, see [docs/DEPLOY_SITE.md](docs/DEPLOY_SITE.md)) and `GET /api/health`
- `site`: an earlier one-page draft of the public page (not deployed)
- `@tandem/ui` Button, Badge, StatusDot and the design tokens, and `@tandem/protocol` wire types

See [ARCHITECTURE.md](ARCHITECTURE.md) for why this stays one repository.

## Develop

Requirements: Rust stable, Node 22, pnpm 9. Docker is optional and starts Postgres, Redis, Mailpit, and stripe-mock.

```bash
pnpm install
pnpm -r lint
pnpm -r test
cargo test --workspace
pnpm dev          # API on http://127.0.0.1:8787 and the shell on http://127.0.0.1:1420
pnpm --filter web dev
```

`GET /v1/health` returns `{ "ok": true, "version": "0.1.0" }`.
`POST /v1/sync/events` appends client-authored events. Replays of the same id do not duplicate.
Set `DATABASE_URL` to store that log in Postgres. Without it, the API keeps the log in memory for one process.

Tests set `TANDEM_HOME` to a temp directory and `TANDEM_FAKE_KEYCHAIN=1`.
