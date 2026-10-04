#!/usr/bin/env bash
set -u
missing=0
check() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing: $1"
    missing=1
  else
    echo "ok: $1 ($($1 --version 2>/dev/null | head -n 1))"
  fi
}
check node
check pnpm
check cargo
check rustc
if ! command -v docker >/dev/null 2>&1; then
  echo "optional missing: docker (needed for pnpm infra:up)"
fi
exit "$missing"
