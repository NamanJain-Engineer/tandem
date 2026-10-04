# Tandem

Cross-platform desktop workspace for building software with the coding-agent CLIs you already pay for. Terminal panes, readable threads, and named teammates. Tandem hosts those engines. It does not replace them, and it never reads their tokens.

This repository is a clean-room product. It does not use another product’s names, copy, or code.

## Status

Milestone M0 is in progress. Shipped in this repo so far:

- `tandem-core`: paths, errors, atomic writes, clock, topic bus, settings, command registry, in-memory keychain, secret redaction, path containment, sanitized logs
- `tandem-store`: SQLite store (WAL) with the first migration

Desktop, API, and web apps land in the next slices of M0.

## Develop

Requirements: Rust stable, Node 22.

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

Tests set `TANDEM_HOME` to a temp directory and `TANDEM_FAKE_KEYCHAIN=1`.
