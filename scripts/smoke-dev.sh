#!/usr/bin/env bash
# E2E-M0-2: `pnpm dev --no-desktop` answers every health endpoint within 60 s,
# and SIGINT stops api and web while compose services stay up.
set -euo pipefail
cd "$(dirname "$0")/.."
log="$(mktemp)"
setsid pnpm dev --no-desktop > "$log" 2>&1 &
pid=$!
cleanup() { kill -INT -"$pid" 2>/dev/null || true; }
trap cleanup EXIT
deadline=$((SECONDS + 60))
until curl -sf http://127.0.0.1:8787/v1/health >/dev/null && curl -sf http://localhost:3000/api/health >/dev/null; do
  if [ "$SECONDS" -ge "$deadline" ]; then echo "health endpoints not up within 60 s"; tail -50 "$log"; exit 1; fi
  sleep 1
done
curl -sf http://127.0.0.1:8025/api/v1/info >/dev/null
curl -sf -u sk_test_123: http://127.0.0.1:12111/v1/customers >/dev/null
echo "all health endpoints answered in ${SECONDS}s"
kill -INT -"$pid"
trap - EXIT
for _ in $(seq 1 20); do
  if ! curl -sf http://127.0.0.1:8787/v1/health >/dev/null && ! curl -sf http://localhost:3000/api/health >/dev/null; then
    break
  fi
  sleep 1
done
if curl -sf http://127.0.0.1:8787/v1/health >/dev/null; then echo "api still running after SIGINT"; exit 1; fi
if curl -sf http://localhost:3000/api/health >/dev/null; then echo "web still running after SIGINT"; exit 1; fi
healthy=$(docker compose -f infra/docker-compose.yml ps --format '{{.Service}} {{.State}}' | grep -c running)
[ "$healthy" -ge 4 ] || { echo "compose services stopped"; exit 1; }
echo "smoke-dev passed"
