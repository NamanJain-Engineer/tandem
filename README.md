# Tandem

Cross-platform desktop workspace for building software with the coding-agent CLIs you already pay for. Terminal panes, readable threads, and named teammates. Tandem hosts those engines. It does not replace them, and it never reads their tokens.

This repository is a clean-room product. It does not use another product’s names, copy, or code.

## Status

Milestone M0 is in progress. Shipped so far:

- `tandem-core` and `tandem-store`
- `api`: `GET /v1/health`, problem details, redacted request logs
- `web`: `GET /api/health`
- `@tandem/ui`: `Button`
- `@tandem/protocol` shared types

The desktop shell is next.

## Develop

Requirements: Rust stable, Node 22, pnpm 9. Docker is optional and starts Postgres, Redis, Mailpit, and stripe-mock.

```bash
pnpm install
pnpm -r lint
pnpm -r test
cargo test --workspace
pnpm dev          # API on http://127.0.0.1:8787
pnpm --filter web dev
```

`GET /v1/health` and `GET /api/health` return `{ "ok": true, "version": "0.1.0" }`.

Tests set `TANDEM_HOME` to a temp directory and `TANDEM_FAKE_KEYCHAIN=1`.
