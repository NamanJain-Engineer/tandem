#!/usr/bin/env bash
# Global gate G (DECISIONS rule 1). Run before every commit. One command per
# line: under `set -e` a failure inside an `a && b` list doesn't stop the
# script, so chaining would let a red step pass the gate.
set -euo pipefail
cd "$(dirname "$0")/.."
export TANDEM_FAKE_KEYCHAIN=1
pnpm -r lint
pnpm -r test
cargo test --workspace
pnpm --filter desktop build
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
pnpm gen:protocol
git diff --exit-code packages/protocol
# The cfg(target_os = "macos") code, both Mac architectures, from Linux
# (D-2105; docs/MACOS.md §2).
bash scripts/macos-check.sh --clippy
