#!/usr/bin/env bash
# Posts a signed Stripe webhook fixture for a user (MILESTONES M1 task 11).
#   bash scripts/stripe-fixture.sh invoice.paid pro@tandem.test
set -euo pipefail
cd "$(dirname "$0")/../apps/api"
exec npx tsx scripts/stripe-fixture.ts "$@"
