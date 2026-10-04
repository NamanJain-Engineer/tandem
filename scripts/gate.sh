#!/usr/bin/env bash
# Global gate G (DECISIONS rule 1). Run before every commit.
set -euo pipefail
cd "$(dirname "$0")/.."
export TANDEM_FAKE_KEYCHAIN=1
pnpm -r lint && pnpm -r test && cargo test --workspace && pnpm --filter desktop build
cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check
pnpm gen:protocol && git diff --exit-code packages/protocol
