#!/usr/bin/env sh
# Builds the LAN phone web app (F-RM-04) into the desktop's resources, where
# LAN mode serves it at /remote. Release builds run this before bundling.
set -eu
cd "$(dirname "$0")/.."
out=apps/desktop/src-tauri/resources/remote-web
find "$out" -mindepth 1 ! -name README.md -exec rm -rf {} +
pnpm --filter web build:remote-lan
test -f "$out/index.html"
echo "remote-web: $(find "$out" -type f ! -name README.md | wc -l) files in $out"
