#!/bin/sh
# The key the `sshd` compose service (profile `remote`) accepts for user
# `tandem` (MILESTONES M14 task 5). Made once; git-ignored.
#   bash scripts/gen-ssh-fixture.sh && docker compose -f infra/docker-compose.yml --profile remote up -d --wait sshd
set -e
DIR="$(cd "$(dirname "$0")/../infra/ssh" && pwd)"
if [ ! -f "$DIR/id_ed25519" ]; then
  ssh-keygen -q -t ed25519 -N "" -C "tandem-test" -f "$DIR/id_ed25519"
fi
chmod 600 "$DIR/id_ed25519"
chmod 644 "$DIR/id_ed25519.pub"
echo "$DIR/id_ed25519"
