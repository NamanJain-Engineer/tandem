#!/usr/bin/env bash
# Cross-builds an unsigned Windows installer (NSIS, x86_64-pc-windows-msvc)
# from Linux with cargo-xwin, the route Tauri documents (D-1703). Not for
# publishing: Windows stays "Coming soon" until a build has run on a real
# Windows machine (HUMAN_TODO). Only NSIS cross-builds; the MSI needs WiX on
# Windows (release.yml).
#
#   bash scripts/build-windows-installer.sh [--skip-host] [--skip-frontend]
#
# --skip-host:     bundle whatever tandem-host helpers (for SSH hosts) are
#                  already in resources/tandem-host instead of building the
#                  Linux ones (cargo-zigbuild).
# --skip-frontend: reuse apps/desktop/dist.
#
# Needs (docs/OPERATING.md §3): rustup target x86_64-pc-windows-msvc,
# cargo-xwin, nsis (makensis), clang/lld/llvm 19 or later (the MSVC STL
# headers refuse older clang), p7zip-full for the check, and
# TANDEM_MSVC_LICENSE_ACCEPTED=1 once the owner has accepted the licence of
# the Microsoft CRT and SDK that cargo-xwin downloads (D-1711). TANDEM_HOSTED passes
# through as in release-local.sh (0 for an early-access build).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
target="x86_64-pc-windows-msvc"
host=1
frontend=1
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-host) host=0 ;;
    --skip-frontend) frontend=0 ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
  shift
done

# cargo-xwin downloads Microsoft's CRT and SDK, which accepts Microsoft's
# licence: only the owner may do that (HUMAN_TODO, D-1711).
if [ "${TANDEM_MSVC_LICENSE_ACCEPTED:-0}" != 1 ]; then
  echo "the MSVC cross-build needs the owner's acceptance of the MSVC licence: set TANDEM_MSVC_LICENSE_ACCEPTED=1 (HUMAN_TODO)" >&2
  exit 1
fi
# whisper.cpp builds with clang-cl against the MSVC headers: clang 19+.
for v in 21 20 19; do
  if [ -x "/usr/lib/llvm-$v/bin/clang" ]; then
    export PATH="/usr/lib/llvm-$v/bin:$PATH"
    break
  fi
done
for tool in cargo-xwin makensis clang lld-link llvm-rc 7z; do
  command -v "$tool" >/dev/null || { echo "missing $tool (docs/OPERATING.md §3, Windows from Linux)" >&2; exit 1; }
done
clang_major="$(clang --version | sed -n 's/.*clang version \([0-9]*\).*/\1/p' | head -1)"
if [ "${clang_major:-0}" -lt 19 ]; then
  echo "clang $clang_major is too old for the MSVC STL headers; install clang-19" >&2
  exit 1
fi

if [ "$host" = 1 ]; then
  hostdir="$root/apps/desktop/src-tauri/resources/tandem-host"
  find "$hostdir" -mindepth 1 -maxdepth 1 -type d -exec rm -rf {} +
  rm -f "$hostdir/manifest.json"
  (cd "$root" && sh scripts/ci/build-tandem-host.sh linux-x86_64 && sh scripts/ci/build-tandem-host.sh linux-aarch64)
  (cd "$root" && TANDEM_HOST_PREBUILT=1 sh scripts/build-tandem-host.sh >/dev/null)
fi
if [ "$frontend" = 1 ]; then
  (cd "$root" && pnpm --filter desktop build)
fi
# xwin's SDK has each import library in lower and upper case only; the
# ONNX Runtime and whisper.cpp builds ask for `DirectML.lib` and `PathCch.lib`.
xwin_um="${XWIN_CACHE_DIR:-$HOME/.cache/cargo-xwin}/xwin/sdk/lib/um/x86_64"
if [ ! -d "$xwin_um" ]; then
  # First run: cargo-xwin downloads the SDK on its first build.
  (cd "$root" && cargo xwin build --release --target "$target" -p tandem-tasks)
fi
for lib in DirectML PathCch; do
  lower="$(echo "$lib" | tr '[:upper:]' '[:lower:]')"
  [ -e "$xwin_um/$lib.lib" ] || ln -s "$lower.lib" "$xwin_um/$lib.lib"
done
# The tasks MCP server ships beside the app (bundle.externalBin).
sh "$root/scripts/build-sidecars.sh" "$target"
# ONNX Runtime (Parakeet) is linked against DirectML.dll; ship ort's copy
# beside the app instead of trusting an older one in System32 (D-1704). It
# exists once ort-sys has built, which has to happen before the app's build
# script copies resources.
(cd "$root" && cargo xwin build --release --target "$target" -p tandem-voice --features whisper,parakeet)
cp -L "$root/target/$target/release/DirectML.dll" "$root/apps/desktop/src-tauri/binaries/DirectML.dll"
(cd "$root/apps/desktop" && pnpm tauri build --runner cargo-xwin --target "$target" \
  --bundles nsis --config src-tauri/tauri.sidecars.conf.json --config src-tauri/tauri.directml.conf.json)

installer="$(ls -t "$root/target/$target/release/bundle/nsis/"*-setup.exe | head -1)"
bash "$root/scripts/check-windows-bundle.sh" "$installer"
echo "unsigned installer: $installer"
