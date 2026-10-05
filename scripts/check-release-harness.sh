#!/usr/bin/env bash
# The E2E harness is compiled out of release builds (M2 exit checklist).
set -euo pipefail
cd "$(dirname "$0")/.."
pnpm --filter desktop build
cargo build --release -p tandem-desktop --features custom-protocol
bin=target/release/tandem
if strings "$bin" | grep -Eq 'test_auth_refresh|test_spawn_log|test_clock_advance|allow-test-'; then
  echo "release binary contains the test harness" >&2
  exit 1
fi
echo "release binary has no test harness"
