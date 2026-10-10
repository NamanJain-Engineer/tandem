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
}

case "$target" in
  universal-apple-darwin)
    build aarch64-apple-darwin
    build x86_64-apple-darwin
    lipo -create \
      "$root/target/aarch64-apple-darwin/release/tandem-tasks" \
      "$root/target/x86_64-apple-darwin/release/tandem-tasks" \
      -output "$out/tandem-tasks-$target"
    ;;
  *windows*)
    build "$target"
    cp "$root/target/$target/release/tandem-tasks.exe" "$out/tandem-tasks-$target.exe"
    ;;
  *)
    build "$target"
    cp "$root/target/$target/release/tandem-tasks" "$out/tandem-tasks-$target"
    ;;
esac
echo "sidecars: $out"
