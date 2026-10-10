#!/bin/sh
# Builds the app's sidecar binaries for one Tauri target and puts them where
# `bundle.externalBin` (apps/desktop/src-tauri/tauri.sidecars.conf.json)
# looks: src-tauri/binaries/<name>-<target-triple>[.exe]. Tauri installs each
# beside the app's executable without the suffix, which is where the app
# looks for it (services::tasks::tasks_bin; TANDEM_TASKS_BIN overrides it in
# debug builds).
#
#   sh scripts/build-sidecars.sh x86_64-unknown-linux-gnu
#   sh scripts/build-sidecars.sh universal-apple-darwin   # both arches, lipo'd
set -eu
target="${1:?usage: build-sidecars.sh <target-triple>}"
root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/apps/desktop/src-tauri/binaries"
mkdir -p "$out"

build() {
  (cd "$root" && cargo build --release -p tandem-tasks --bin tandem-tasks --target "$1")
  # The wake-phrase keyword spotter (F-CN-07) runs beside the app.
  (cd "$root" && cargo build --release -p tandem-voice --features kws --bin tandem-wake --target "$1")
}

# Copies one built sidecar into binaries/ under Tauri's name.
place() {
  name=$1
  case "$target" in
    universal-apple-darwin)
      lipo -create \
        "$root/target/aarch64-apple-darwin/release/$name" \
        "$root/target/x86_64-apple-darwin/release/$name" \
        -output "$out/$name-$target"
      ;;
    *windows*) cp "$root/target/$target/release/$name.exe" "$out/$name-$target.exe" ;;
    *) cp "$root/target/$target/release/$name" "$out/$name-$target" ;;
  esac
}

case "$target" in
  universal-apple-darwin)
    build aarch64-apple-darwin
    build x86_64-apple-darwin
    ;;
  *) build "$target" ;;
esac
place tandem-tasks
place tandem-wake
echo "sidecars: $out"
