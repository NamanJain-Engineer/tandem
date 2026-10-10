#!/usr/bin/env bash
# F-DI-02 AC2: the Windows keyboard hook only listens. A low-level hook
# consumes an event by returning non-zero instead of CallNextHookEx's
# result; this guard rejects any such return in the hook file.
set -euo pipefail
file=${1:-$(cd "$(dirname "$0")/../.." && pwd)/crates/tandem-voice/src/hotkey/windows.rs}
fail=0
# A literal LRESULT built by hand, or `return 1`/`return LRESULT…` anywhere.
if grep -nE 'LRESULT\(|return[[:space:]]+(1|-1|LRESULT|true)\b' "$file"; then
  echo "no-consuming-hook: a hook callback may only return CallNextHookEx(...)" >&2
  fail=1
fi
# Every hook procedure's last expression is CallNextHookEx.
procs=$(grep -cE 'extern "system" fn [a-z_]+_proc' "$file" || true)
nexts=$(grep -cE 'unsafe \{ CallNextHookEx\(None, code, wparam, lparam\) \}' "$file" || true)
if [ "$procs" -lt 1 ] || [ "$procs" != "$nexts" ]; then
  echo "no-consuming-hook: $procs hook procedures but $nexts CallNextHookEx tails" >&2
  fail=1
fi
[ "$fail" = 0 ] && echo "no-consuming-hook: ok ($procs listen-only hook procedures)"
exit "$fail"
