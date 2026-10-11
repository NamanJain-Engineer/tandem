#!/usr/bin/env bash
# Smoke test of a built Vibezeo.app on a Mac (D-2103; .github/workflows/macos.yml,
# codemagic.yaml). Fresh TANDEM_HOME, no account (TANDEM_HOSTED=0 builds):
#   1. `--run-routines`, the LaunchAgent's headless pass, exits 0 at once;
#   2. the app starts, is still running after 25 s and logged no panic;
#   3. a screenshot of the welcome screen lands in $OUT (best effort: a CI
#      Mac without Screen Recording permission records only the desktop).
#   bash scripts/ci/macos-smoke.sh path/to/Vibezeo.app [out-dir]
set -uo pipefail
app="${1:?usage: macos-smoke.sh APP [OUT]}"
out="${2:-${OUT:-$PWD/macos-smoke}}"
[ "$(uname -s)" = Darwin ] || { echo "macos-smoke: runs on macOS only" >&2; exit 1; }
exe="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")"
bin="$app/Contents/MacOS/$exe"
[ -x "$bin" ] || { echo "macos-smoke: no executable at $bin" >&2; exit 1; }
rm -rf "$out"
mkdir -p "$out/home"
export TANDEM_HOME="$out/home/.tandem"
fail=0

echo "== Info.plist"
/usr/libexec/PlistBuddy -c 'Print :LSMinimumSystemVersion' "$app/Contents/Info.plist" | tee "$out/min-version.txt"
grep -q '^14\.0' "$out/min-version.txt" || { echo "FAIL: LSMinimumSystemVersion isn't 14.0"; fail=1; }
/usr/libexec/PlistBuddy -c 'Print :NSMicrophoneUsageDescription' "$app/Contents/Info.plist" ||
  { echo "FAIL: no NSMicrophoneUsageDescription"; fail=1; }

echo "== --run-routines (the LaunchAgent's pass)"
start=$(date +%s)
"$bin" --run-routines >"$out/run-routines.log" 2>&1
code=$?
took=$(( $(date +%s) - start ))
echo "exit $code after ${took}s"
[ "$code" = 0 ] || { echo "FAIL: --run-routines exited $code"; cat "$out/run-routines.log"; fail=1; }
[ "$took" -lt 60 ] || { echo "FAIL: --run-routines took ${took}s"; fail=1; }

echo "== launch"
"$bin" >"$out/app.log" 2>&1 &
pid=$!
sleep 25
if kill -0 "$pid" 2>/dev/null; then
  echo "running after 25 s (pid $pid)"
else
  wait "$pid"
  echo "FAIL: the app exited early with $?"; fail=1
fi
screencapture -x "$out/welcome.png" 2>/dev/null && echo "screenshot: $out/welcome.png" || echo "no screenshot (Screen Recording not allowed here)"
kill "$pid" 2>/dev/null
sleep 2
kill -9 "$pid" 2>/dev/null
if grep -qi 'panicked at' "$out/app.log"; then
  echo "FAIL: the app panicked"; grep -i -A5 'panicked at' "$out/app.log"; fail=1
fi
echo "logs: $out"
exit "$fail"
