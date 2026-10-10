#!/bin/sh
# Checks that a built Vibezeo bundle carries tandem-host for SSH hosts
# (F-RM-01): its host-binaries/manifest.json is this version's, every
# platform in REQUIRE is there, and every binary matches its SHA-256. Without
# it, every connection fails at "Installing helper…".
#   sh scripts/check-host-bundle.sh <file.deb | file.AppImage | folder> [platform,…]
# The default REQUIRE is linux-x86_64,linux-aarch64 (what release-local.sh builds).
set -e
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="$1"
REQUIRE="${2:-linux-x86_64,linux-aarch64}"
[ -n "$TARGET" ] || { echo "usage: check-host-bundle.sh <bundle> [platform,…]" >&2; exit 2; }
TARGET="$(cd "$(dirname "$TARGET")" && pwd)/$(basename "$TARGET")"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
case "$TARGET" in
  *.deb) dpkg-deb -x "$TARGET" "$TMP/root"; SEARCH="$TMP/root" ;;
  *.AppImage) (cd "$TMP" && APPIMAGE_EXTRACT_AND_RUN=1 "$TARGET" --appimage-extract >/dev/null); SEARCH="$TMP/squashfs-root" ;;
  *) SEARCH="$TARGET" ;;
esac
MANIFEST="$(find "$SEARCH" -path '*host-binaries/manifest.json' | head -1)"
if [ -z "$MANIFEST" ]; then
  echo "tandem-host bundle: no host-binaries/manifest.json in $(basename "$TARGET")" >&2
  exit 1
fi
cd "$ROOT"
cargo run -q -p tandem-host --example manifest -- --check "$(dirname "$MANIFEST")" --require "$REQUIRE"
