#!/usr/bin/env bash
# Builds `makemsix` from Microsoft's open-source MSIX SDK
# (github.com/microsoft/msix-packaging, MIT) at a pinned commit, with packing
# on (which also turns on manifest schema validation), into
# ~/.cache/tandem-tools/makemsix (D-1713). Needs cmake 3.29 or later
# (`pip install cmake` where the distro's is older), clang and make.
#
#   bash scripts/get-makemsix.sh        # prints the path of makemsix
set -euo pipefail

commit="25a65f5c1690930813bcc10cdf1d59fa865f2bb1" # main, 24 August 2026
dest="${TANDEM_TOOLS_DIR:-$HOME/.cache/tandem-tools}/makemsix-$commit"
bin="$dest/src/.vs/bin/makemsix"
if [ -x "$bin" ]; then
  echo "$bin"
  exit 0
fi
mkdir -p "$dest"
if [ ! -d "$dest/src/.git" ]; then
  git init -q "$dest/src"
  git -C "$dest/src" fetch -q --depth 1 https://github.com/microsoft/msix-packaging.git "$commit"
  git -C "$dest/src" checkout -q FETCH_HEAD
fi
(cd "$dest/src" && CC=clang CXX=clang++ ./makelinux.sh --pack --skip-samples --skip-tests >"$dest/build.log" 2>&1) || {
  echo "makemsix build failed; see $dest/build.log" >&2
  exit 1
}
echo "$bin"
