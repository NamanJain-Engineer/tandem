#!/usr/bin/env bash
# Packs Vibezeo as an MSIX for the Microsoft Store (D-1713), from Windows
# binaries that are already built, then checks it (scripts/check-msix.sh).
# The Store signs what it publishes; this package is unsigned.
#
#   bash scripts/build-msix.sh --from-installer target/x86_64-pc-windows-msvc/release/bundle/nsis/Vibezeo_<v>_x64-setup.exe
#   bash scripts/build-msix.sh --from <folder with tandem.exe, tandem-tasks.exe, tandem-hook.exe, tandem-wake.exe, DirectML.dll, host-binaries/>
#   [--out dist/Vibezeo_<v>_x64.msix]
#
# The binaries come from scripts/build-windows-installer.sh (needs the MSVC
# licence, D-1711) or a Windows build. The package identity comes from
# Partner Center (Product identity) in MSIX_IDENTITY_NAME, MSIX_PUBLISHER and
# MSIX_PUBLISHER_DISPLAY_NAME; without them it gets dev values a Store upload
# refuses. Needs makemsix (scripts/get-makemsix.sh) and 7z for an installer.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
conf="$root/apps/desktop/src-tauri/tauri.conf.json"
from="" installer="" out=""
while [ $# -gt 0 ]; do
  case "$1" in
    --from) from="$2"; shift ;;
    --from-installer) installer="$2"; shift ;;
    --out) out="$2"; shift ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
  shift
done
if [ -z "$from$installer" ]; then
  echo "usage: build-msix.sh (--from <dir> | --from-installer <setup.exe>) [--out <file.msix>]" >&2
  exit 2
fi

version="$(node -p "require('$conf').version")"
scheme="$(node -p "require('$conf').plugins['deep-link'].desktop.schemes[0]")"
# The Store wants four parts with the last one 0.
msix_version="$version.0"
out="${out:-$root/dist/Vibezeo_${version}_x64.msix}"
makemsix="$(bash "$root/scripts/get-makemsix.sh")"

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
if [ -n "$installer" ]; then
  # The NSIS installer holds the files as they install; its own plugins and
  # uninstaller aren't part of the app.
  7z x -y -o"$stage" "$installer" >/dev/null
  rm -rf "$stage/\$PLUGINSDIR" "$stage/uninstall.exe"
else
  cp -r "$from"/. "$stage"/
fi
for want in tandem.exe tandem-tasks.exe tandem-hook.exe tandem-wake.exe DirectML.dll host-binaries/manifest.json; do
  [ -e "$stage/$want" ] || { echo "build-msix: $want is missing from the app's files" >&2; exit 1; }
done

mkdir -p "$stage/Assets"
for logo in StoreLogo Square44x44Logo Square150x150Logo; do
  cp "$root/apps/desktop/src-tauri/icons/$logo.png" "$stage/Assets/$logo.png"
done
sed -e "s|@IDENTITY_NAME@|${MSIX_IDENTITY_NAME:-Vibezeo.Dev}|" \
    -e "s|@PUBLISHER@|${MSIX_PUBLISHER:-CN=Vibezeo Dev}|" \
    -e "s|@PUBLISHER_DISPLAY_NAME@|${MSIX_PUBLISHER_DISPLAY_NAME:-Vibezeo (dev)}|" \
    -e "s|@VERSION@|$msix_version|" \
    -e "s|@SCHEME@|$scheme|" \
    "$root/packaging/msix/AppxManifest.xml.in" >"$stage/AppxManifest.xml"

mkdir -p "$(dirname "$out")"
rm -f "$out"
"$makemsix" pack -d "$stage" -p "$out"
bash "$root/scripts/check-msix.sh" "$out"
echo "msix: $out"
