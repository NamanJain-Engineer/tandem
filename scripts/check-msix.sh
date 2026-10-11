#!/usr/bin/env bash
# Checks an MSIX built by scripts/build-msix.sh (D-1713): makemsix unpacks it
# (which validates the package and its manifest), the app's files are in it,
# and the manifest has the version, full trust, both sidecars' folder, the
# tandem:// protocol and the execution alias Task Scheduler uses.
#
#   bash scripts/check-msix.sh dist/Vibezeo_<version>_x64.msix
set -euo pipefail

pkg="${1:?usage: check-msix.sh <file.msix>}"
root="$(cd "$(dirname "$0")/.." && pwd)"
conf="$root/apps/desktop/src-tauri/tauri.conf.json"
makemsix="$(bash "$root/scripts/get-makemsix.sh")"
dir="$(mktemp -d)"
trap 'rm -rf "$dir"' EXIT
# -ss: unsigned is expected; the Store signs it.
"$makemsix" unpack -ss -p "$pkg" -d "$dir" >/dev/null
fail=0
bad() { echo "check-msix: $*" >&2; fail=1; }

for want in tandem.exe tandem-tasks.exe tandem-hook.exe tandem-wake.exe DirectML.dll host-binaries/manifest.json \
  Assets/StoreLogo.png Assets/Square44x44Logo.png Assets/Square150x150Logo.png AppxManifest.xml AppxBlockMap.xml; do
  [ -e "$dir/$want" ] || bad "no $want"
done
m="$dir/AppxManifest.xml"
version="$(node -p "require('$conf').version")"
scheme="$(node -p "require('$conf').plugins['deep-link'].desktop.schemes[0]")"
alias="$(sed -n 's/^pub const STORE_ALIAS: &str = "\(.*\)";/\1/p' "$root/crates/tandem-routines/src/launcher.rs")"
grep -q "Version=\"$version.0\"" "$m" || bad "the version is not $version.0"
grep -q '<rescap:Capability Name="runFullTrust" />' "$m" || bad "no runFullTrust"
grep -q 'Executable="tandem.exe" EntryPoint="Windows.FullTrustApplication"' "$m" || bad "the app isn't a full-trust tandem.exe"
grep -q "<uap:Protocol Name=\"$scheme\">" "$m" || bad "$scheme:// isn't declared"
grep -q "Alias=\"$alias\"" "$m" || bad "no $alias execution alias"
grep -q '<DeviceCapability Name="microphone" />' "$m" || bad "no microphone capability"
if grep -q '@[A-Z_]*@' "$m"; then bad "unfilled placeholders: $(grep -o '@[A-Z_]*@' "$m" | sort -u | tr '\n' ' ')"; fi
[ "$fail" = 0 ] && echo "check-msix: $(basename "$pkg") ok"
exit "$fail"
