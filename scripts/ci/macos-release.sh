#!/usr/bin/env bash
# The whole macOS release on one Mac, for build hosts without
# .github/workflows/release.yml's job graph (codemagic.yaml, a self-hosted
# Mac; D-2104). Same steps and checks as release.yml's macOS path:
#   notices → tandem-host for all three host platforms (the macOS one signed)
#   → its SHA-256 manifest → sidecars, signed → universal .app and .dmg,
#   signed by Tauri with the entitlements, notarized and stapled → the dmg
#   notarized → every Mach-O verified → out/ with SHA256SUMS.
# Without APPLE_CERTIFICATE everything is ad-hoc signed and named -unsigned.
#
#   bash scripts/ci/macos-release.sh [out-dir]
# Needs: Xcode command line tools, rustup with aarch64-apple-darwin and
# x86_64-apple-darwin, Node with pnpm, cargo-zigbuild and zig (the Linux
# helpers), cargo-about (notices). TANDEM_HOSTED=0 for early access.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
out="${1:-$root/out}"
target=universal-apple-darwin
sign="$root/scripts/ci/macos-sign.sh"
tauri_dir="$root/apps/desktop/src-tauri"
cd "$root"
mkdir -p "$out"

echo "== UI and notices"
pnpm --filter desktop build
pnpm tsx scripts/gen-notices.ts --out dist --resources
cp dist/THIRD_PARTY_NOTICES.md "$tauri_dir/resources/THIRD_PARTY_NOTICES.md"

identity="$(bash "$sign" identity)"
notarize=false
if [ "$identity" != "-" ]; then
  if [ -n "${APPLE_API_KEY_P8:-}" ] && [ -n "${APPLE_API_KEY:-}" ] && [ -n "${APPLE_API_ISSUER:-}" ]; then
    export APPLE_API_KEY_PATH="${TMPDIR:-/tmp}/AuthKey_$APPLE_API_KEY.p8"
    printf '%s\n' "$APPLE_API_KEY_P8" >"$APPLE_API_KEY_PATH"
    notarize=true
  elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
    notarize=true
  fi
fi
echo "identity: $identity · notarize: $notarize"

echo "== tandem-host (SSH helpers)"
for p in linux-x86_64 linux-aarch64 macos-universal; do
  bash scripts/ci/build-tandem-host.sh "$p"
done
bash "$sign" sign "$identity" "$tauri_dir/resources/tandem-host/macos-universal/tandem-host"
TANDEM_HOST_PREBUILT=1 sh scripts/build-tandem-host.sh
cargo run -q -p tandem-host --example manifest -- --check "$tauri_dir/resources/tandem-host" \
  --require linux-x86_64,linux-aarch64,macos-universal

echo "== sidecars"
sh scripts/build-sidecars.sh "$target"
bash "$sign" sign "$identity" "$tauri_dir/binaries/tandem-tasks-$target" "$tauri_dir/binaries/tandem-hook-$target" "$tauri_dir/binaries/tandem-wake-$target"

echo "== app"
configs=(--config src-tauri/tauri.sidecars.conf.json)
if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  configs+=(--config '{"bundle":{"createUpdaterArtifacts":true}}')
fi
if [ "$identity" = "-" ]; then
  configs+=(--config '{"bundle":{"macOS":{"signingIdentity":"-"}}}')
  unset APPLE_SIGNING_IDENTITY
else
  export APPLE_SIGNING_IDENTITY="$identity"
fi
# Tauri imports a certificate itself when APPLE_CERTIFICATE is set; it is
# already in the keychain, so it mustn't.
if [ "$notarize" != true ]; then
  unset APPLE_ID APPLE_PASSWORD APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH
fi
(cd apps/desktop && env -u APPLE_CERTIFICATE -u APPLE_CERTIFICATE_PASSWORD \
  pnpm tauri build --target "$target" --bundles app,dmg "${configs[@]}")

bundle="$root/target/$target/release/bundle"
app="$(find "$bundle/macos" -maxdepth 1 -name '*.app' | head -1)"
dmg="$(find "$bundle/dmg" -maxdepth 1 -name '*.dmg' | head -1)"
if [ "$notarize" = true ]; then
  codesign --force --timestamp --sign "$identity" "$dmg"
  bash "$sign" notarize "$dmg"
  bash "$sign" verify --notarized "$app" "$dmg" | tee "$bundle/macos-signature.txt"
else
  bash "$sign" verify "$app" | tee "$bundle/macos-signature.txt"
fi
sh scripts/check-host-bundle.sh "$app" linux-x86_64,linux-aarch64,macos-universal

echo "== collect"
suffix=""
[ "$notarize" = true ] || suffix="-unsigned"
for f in "$dmg" "$bundle"/macos/*.app.tar.gz* "$bundle/macos-signature.txt"; do
  [ -e "$f" ] || continue
  name="$(basename "$f")"
  case "$name" in
    # The updater's archive keeps its name: its .sig refers to it.
    *.sig | *.tar.gz) cp "$f" "$out/" ;;
    *) cp "$f" "$out/${name%.*}$suffix.${name##*.}" ;;
  esac
done
(cd "$out" && shasum -a 256 -- * | grep -v ' SHA256SUMS$' >SHA256SUMS)
ls -la "$out"
