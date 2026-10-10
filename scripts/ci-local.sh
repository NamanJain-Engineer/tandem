#!/usr/bin/env bash
# Runs the CI jobs locally, without GitHub (MILESTONES M0 task 16).
set -euo pipefail
cd "$(dirname "$0")/.."
export TANDEM_FAKE_KEYCHAIN=1
step() { printf '\n== %s ==\n' "$1"; }
step "install"; pnpm install --frozen-lockfile
step "gate"; bash scripts/gate.sh
step "windows"
# The app for the MSVC target and the platform crates' tests under Wine
# (D-1700): minutes, not seconds, and tools the gate can't assume.
if command -v cargo-xwin >/dev/null 2>&1 && command -v wine >/dev/null 2>&1; then
  bash scripts/windows-check.sh --msvc --wine
else
  echo "cargo-xwin or wine missing: see docs/OPERATING.md, Windows from Linux" >&2
  exit 1
fi
step "coverage"; bash scripts/check-coverage.sh
step "audit"; pnpm tsx scripts/check-waivers.ts
if command -v cargo-audit >/dev/null 2>&1; then
  cargo audit --json > target/audit.json || true
else
  echo "cargo-audit not installed: cargo install cargo-audit --locked"
fi
pnpm audit --audit-level high --json > target/pnpm-audit.json || true
pnpm tsx scripts/check-waivers.ts --cargo target/audit.json --pnpm target/pnpm-audit.json
if [ "${TANDEM_CI_INTEGRATION:-1}" = "1" ] && docker info >/dev/null 2>&1; then
  step "integration"
  pnpm infra:up
  if [ -f scripts/smoke-dev.sh ]; then bash scripts/smoke-dev.sh; fi
  if command -v xvfb-run >/dev/null 2>&1 && command -v tauri-driver >/dev/null 2>&1; then
    xvfb-run -a pnpm --filter desktop e2e
  fi
fi
echo "ci:local passed"
