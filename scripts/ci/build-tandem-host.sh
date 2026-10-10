#!/bin/sh
# CI: builds tandem-host for one host platform into
# apps/desktop/src-tauri/resources/tandem-host/<platform>/tandem-host
# (.github/workflows/ci.yml and release.yml; ARCHITECTURE §2.11).
#   linux-x86_64     x86_64-unknown-linux-musl, static (cargo-zigbuild)
#   linux-aarch64    aarch64-unknown-linux-musl, static (cargo-zigbuild)
#   macos-universal  aarch64 + x86_64 Apple, joined with lipo
set -e
PLATFORM="$1"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/apps/desktop/src-tauri/resources/tandem-host/$PLATFORM"
cd "$ROOT"
mkdir -p "$OUT"
case "$PLATFORM" in
  linux-x86_64|linux-aarch64)
    arch="${PLATFORM#linux-}"
    target="$arch-unknown-linux-musl"
    cargo zigbuild -p tandem-host --release --target "$target"
    cp "target/$target/release/tandem-host" "$OUT/tandem-host"
    file "$OUT/tandem-host" | grep -q "statically linked\|static-pie" || { echo "not static"; exit 1; }
    ;;
  macos-universal)
    cargo build -p tandem-host --release --target aarch64-apple-darwin
    cargo build -p tandem-host --release --target x86_64-apple-darwin
    lipo -create -output "$OUT/tandem-host" \
      target/aarch64-apple-darwin/release/tandem-host \
      target/x86_64-apple-darwin/release/tandem-host
    lipo -info "$OUT/tandem-host"
    ;;
  *) echo "unknown platform $PLATFORM"; exit 2 ;;
esac
