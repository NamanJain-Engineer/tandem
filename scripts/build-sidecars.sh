#!/bin/sh
# Builds the app's sidecar binaries for one Tauri target and puts them where
# `bundle.externalBin` (apps/desktop/src-tauri/tauri.sidecars.conf.json)
# looks: src-tauri/binaries/<name>-<target-triple>[.exe]. Tauri installs each
# beside the app's executable without the suffix, which is where the app
# looks for it: tandem-tasks (services::tasks::tasks_bin; TANDEM_TASKS_BIN in
# debug builds) and tandem-hook, the engines' status hook
# (services::engines::hook_bin; TANDEM_HOOK_BIN in debug builds, D-828).
#
#   sh scripts/build-sidecars.sh x86_64-unknown-linux-gnu
#   sh scripts/build-sidecars.sh universal-apple-darwin   # both arches, lipo'd
#   sh scripts/build-sidecars.sh x86_64-pc-windows-msvc   # from Linux: cargo-xwin
set -eu
target="${1:?usage: build-sidecars.sh <target-triple>}"
root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/apps/desktop/src-tauri/binaries"
mkdir -p "$out"

build() {
  case "$1-$(uname -s)" in
    # Cross-building the MSVC target from Linux or macOS (D-1703).
    *-windows-msvc-Linux | *-windows-msvc-Darwin) xwin="xwin" ;;
    *) xwin="" ;;
  esac
  (cd "$root" && cargo $xwin build --release -p tandem-tasks --bin tandem-tasks --target "$1")
  (cd "$root" && cargo $xwin build --release -p tandem-engines --bin tandem-hook --target "$1")
}

case "$target" in
  universal-apple-darwin)
    build aarch64-apple-darwin
    build x86_64-apple-darwin
    for bin in tandem-tasks tandem-hook; do
      lipo -create \
        "$root/target/aarch64-apple-darwin/release/$bin" \
        "$root/target/x86_64-apple-darwin/release/$bin" \
        -output "$out/$bin-$target"
    done
    ;;
  *windows*)
    build "$target"
    for bin in tandem-tasks tandem-hook; do
      cp "$root/target/$target/release/$bin.exe" "$out/$bin-$target.exe"
    done
    ;;
  *)
    build "$target"
    for bin in tandem-tasks tandem-hook; do
      cp "$root/target/$target/release/$bin" "$out/$bin-$target"
    done
    ;;
esac
echo "sidecars: $out"
