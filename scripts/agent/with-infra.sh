#!/bin/sh
# Starts dockerd and the compose infra (Postgres, Redis, Mailpit, stripe-mock),
# runs "$@" from the repo root, then stops dockerd. The gate's API tests need it:
#   sh scripts/agent/with-infra.sh bash scripts/gate.sh
export CARGO_INCREMENTAL=0
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
LOG="${TMPDIR:-/tmp}/dockerd-run.log"
STARTED=""
if ! docker info >/dev/null 2>&1; then
  dockerd > "$LOG" 2>&1 &
  STARTED=$!
  i=0
  while ! docker info >/dev/null 2>&1; do
    i=$((i + 1))
    # dockerd may already have exited (say, started while another was still
    # shutting down): stop waiting either way.
    if [ $i -gt 60 ]; then echo "docker didn't start (see $LOG)"; kill $STARTED 2>/dev/null; exit 1; fi
    sleep 1
  done
fi
cd "$ROOT" || exit 1
# A failed pull (Docker Hub's 429) otherwise shows up later as API errors.
if ! pnpm infra:up >"${TMPDIR:-/tmp}/infra-up.log" 2>&1; then
  echo "with-infra: infra:up failed, see ${TMPDIR:-/tmp}/infra-up.log (docs/OPERATING.md §3)" >&2
fi
"$@"
STATUS=$?
if [ -n "$STARTED" ]; then kill $STARTED; wait $STARTED 2>/dev/null; fi
exit $STATUS
