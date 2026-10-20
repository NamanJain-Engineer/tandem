#!/usr/bin/env bash
# Checks Conductor's free primary against Google's real Gemini Live API
# (HUMAN_TODO "Check against a real session", D-1018): the API's GeminiLive
# mints a token, and the app's own client opens it, sends its setup and
# speech, and answers an act. --long adds a call past Google's 10-minute
# reconnect (about 12 more minutes). Needs GEMINI_API_KEY; tokens live in a
# file only you can read and are never printed.
#   GEMINI_API_KEY=… bash scripts/agent/gemini-live-check.sh [--long]
set -euo pipefail
cd "$(dirname "$0")/../.."
long=0
[ "${1:-}" = "--long" ] && long=1
if [ -z "${GEMINI_API_KEY:-}" ]; then
  echo "gemini-live-check: set GEMINI_API_KEY (an AI Studio key) first" >&2
  exit 2
fi
export CARGO_INCREMENTAL=0

# Build first: a token's first connection must open within 60 s of minting.
echo "== building the live test"
cargo test -p tandem-voice --test gemini_live --no-run --quiet

dir="$(mktemp -d)"
chmod 700 "$dir"
trap 'rm -rf "$dir"' EXIT
mint() { (cd apps/api && pnpm exec tsx scripts/gemini-live-token.ts "$1"); }

echo "== a call: setup, speech, an act and the answer"
mint "$dir/session.json"
TANDEM_GEMINI_SESSION="$dir/session.json" \
  cargo test -p tandem-voice --test gemini_live --quiet -- --ignored --nocapture \
  f_cn_01_a_real_gemini_session_hears_speech_answers_and_calls_act

if [ "$long" = 1 ]; then
  echo "== a call past the 10-minute reconnect (about 12 minutes)"
  mint "$dir/long.json"
  TANDEM_GEMINI_SESSION_LONG="$dir/long.json" \
    cargo test -p tandem-voice --test gemini_live --quiet -- --ignored --nocapture \
    f_cn_01_a_real_gemini_session_carries_on_past_its_10_minute_reconnect
fi
echo "gemini-live-check: ok"
