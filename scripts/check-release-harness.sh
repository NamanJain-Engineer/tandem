#!/usr/bin/env bash
# The E2E harness is compiled out of release builds (M2 exit checklist).
set -euo pipefail
cd "$(dirname "$0")/.."
pnpm --filter desktop build
cargo build --release -p tandem-desktop --features custom-protocol
bin=target/release/tandem
# grep reads to the end (no -q): with pipefail, `strings` stopped by SIGPIPE
# after an early match would read as nothing found.
found=$(strings "$bin" | { grep -Eo 'test_auth_refresh|test_spawn_log|test_clock_advance|test_browser_eval|test_plugin_connect|test_echo|allow-test-[a-z-]+' || true; } | sort -u)
if [ -n "$found" ]; then
  echo "release binary contains the test harness:" >&2
  echo "$found" >&2
  exit 1
fi
echo "release binary has no test harness"
