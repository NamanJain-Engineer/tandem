#!/usr/bin/env bash
# Checks an NSIS installer built for Windows (scripts/build-windows-installer.sh,
# release.yml) without running it (D-1703): its files, and the generated
# installer.nsi beside it for the Start Menu shortcut's AppUserModelID (toasts
# show only when it equals the bundle identifier, D-78), the `tandem://` scheme
# and the WebView2 bootstrapper.
#
#   bash scripts/check-windows-bundle.sh target/x86_64-pc-windows-msvc/release/bundle/nsis/Vibezeo_0.4.0_x64-setup.exe
set -euo pipefail

installer="${1:?usage: check-windows-bundle.sh <setup.exe>}"
root="$(cd "$(dirname "$0")/.." && pwd)"
conf="$root/apps/desktop/src-tauri/tauri.conf.json"
fail=0
bad() { echo "check-windows-bundle: $*" >&2; fail=1; }

listing="$(7z l -ba -slt "$installer" | sed -n 's/^Path = //p')"
for want in tandem.exe tandem-tasks.exe tandem-hook.exe DirectML.dll uninstall.exe host-binaries/manifest.json host-binaries/linux-x86_64/tandem-host; do
  grep -qx "$want" <<<"$listing" || bad "the installer has no $want"
done

nsis_dir="$(cd "$(dirname "$installer")/../../nsis/x64" 2>/dev/null && pwd)" || {
  bad "no generated installer.nsi beside $installer"
  exit 1
}
nsi="$nsis_dir/installer.nsi"
id="$(node -p "require('$conf').identifier")"
scheme="$(node -p "require('$conf').plugins['deep-link'].desktop.schemes[0]")"
grep -q "^!define BUNDLEID \"$id\"" "$nsi" || bad "BUNDLEID is not $id"
grep -q 'SetLnkAppUserModelId "$SMPROGRAMS' "$nsi" || bad "the Start Menu shortcut gets no AppUserModelID"
grep -q 'SysAllocString(w "${BUNDLEID}")' "$nsis_dir/utils.nsh" || bad "the shortcut's AppUserModelID is not the bundle identifier"
grep -qF "\"Software\\Classes\\$scheme\" \"URL Protocol\"" "$nsi" || bad "$scheme:// is not registered"
grep -qF "\"Software\\Classes\\$scheme\\shell\\open\\command\"" "$nsi" || bad "$scheme:// has no open command"
grep -q '^!define INSTALLWEBVIEW2MODE "downloadBootstrapper"' "$nsi" || bad "WebView2 is not installed by the bootstrapper"
grep -q '^!define INSTALLMODE "currentUser"' "$nsi" || bad "the installer is not per-user"

if [ "$fail" = 0 ]; then
  echo "check-windows-bundle: $(basename "$installer") ok"
fi
exit "$fail"
