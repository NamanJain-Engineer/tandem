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
    i=$((i + 1)); [ $i -gt 60 ] && echo "docker didn't start (see $LOG)" && kill $STARTED && exit 1
    sleep 1
  done
fi
cd "$ROOT" && pnpm infra:up >/dev/null 2>&1
"$@"
STATUS=$?
if [ -n "$STARTED" ]; then kill $STARTED; wait $STARTED 2>/dev/null; fi
exit $STATUS
