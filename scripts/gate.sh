#!/usr/bin/env bash
# Global gate G (DECISIONS rule 1). Run before every commit. One command per
# line: under `set -e` a failure inside an `a && b` list doesn't stop the
# script, so chaining would let a red step pass the gate.
set -euo pipefail
cd "$(dirname "$0")/.."
export TANDEM_FAKE_KEYCHAIN=1
# The wake phrase links sherpa-onnx (F-CN-07); use a pre-fetched archive when
# there is one (scripts/fetch-sherpa.sh).
if [ -z "${SHERPA_ONNX_ARCHIVE_DIR:-}" ] && ls "$HOME"/.cache/sherpa-onnx/sherpa-onnx-v*.tar.bz2 >/dev/null 2>&1; then
  export SHERPA_ONNX_ARCHIVE_DIR="$HOME/.cache/sherpa-onnx"
fi
pnpm -r lint
pnpm -r test
cargo test --workspace
pnpm --filter desktop build
cargo clippy --workspace --all-targets -- -D warnings
# The Windows code paths, for every crate but the app (D-1700; about 40 s
# warm). The app's MSVC check and the Wine tests are in ci:local.
bash scripts/windows-check.sh --clippy
cargo fmt --all --check
pnpm gen:protocol
git diff --exit-code packages/protocol
# New or deleted generated files don't show in git diff: one merge committed
# the folder empty and still passed (D-836).
if [ -n "$(git status --porcelain packages/protocol)" ]; then
  git status --short packages/protocol
  echo "packages/protocol: generated files are untracked or deleted; stage them" >&2
  exit 1
fi
# The cfg(target_os = "macos") code, both Mac architectures, from Linux
# (D-2105; docs/MACOS.md §2).
bash scripts/macos-check.sh --clippy
