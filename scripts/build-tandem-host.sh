#!/bin/sh
# Builds tandem-host for this machine's targets into the desktop's resources
# and writes their SHA-256 manifest (ARCHITECTURE §13.1, MILESTONES M14):
#   apps/desktop/src-tauri/resources/tandem-host/<platform>/tandem-host
#   apps/desktop/src-tauri/resources/tandem-host/manifest.json
# Linux x86_64 (musl) builds anywhere with musl-tools; linux-aarch64 needs
# its cross linker and macos-universal a Mac (CI builds all three, then
# runs this with TANDEM_HOST_PREBUILT=1 to write the manifest only).
set -e
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/apps/desktop/src-tauri/resources/tandem-host"
mkdir -p "$OUT"
cd "$ROOT"
if [ -z "$TANDEM_HOST_PREBUILT" ]; then
  for pair in ${TANDEM_HOST_TARGETS:-x86_64-unknown-linux-musl:linux-x86_64}; do
    target="${pair%%:*}"; platform="${pair##*:}"
    cargo build -p tandem-host --release --target "$target"
    mkdir -p "$OUT/$platform"
    cp "target/$target/release/tandem-host" "$OUT/$platform/tandem-host"
  done
fi
cargo run -q -p tandem-host --example manifest -- "$OUT"
cat "$OUT/manifest.json"
