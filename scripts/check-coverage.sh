#!/usr/bin/env bash
# Every F-XX-nn in PRODUCT_SPEC is owned by exactly one milestone, and P2 IDs
# only by M16, M17 or M18, the extras (MILESTONES §20.1, D-700, D-2401).
set -euo pipefail
cd "$(dirname "$0")/.."
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
grep -E '^#### F-[A-Z]{2}-[0-9]{2} ' build-kit/PRODUCT_SPEC.md \
  | sed -E 's/^#### (F-[A-Z]{2}-[0-9]{2}) [^(]*\([^P]*(P[0-9]).*/\1 \2/' | sort > "$tmp/spec_ids.txt"
awk '/^## M[0-9]+ ·/{m=$2} /^\*\*Owns/{o=1} o&&/^\*\*(Early|Completes|Extends|Prerequisites)/{o=0}
     o{while(match($0,/F-[A-Z][A-Z]-[0-9][0-9]/)){print substr($0,RSTART,RLENGTH), m; $0=substr($0,RSTART+RLENGTH)}}' \
  build-kit/MILESTONES.md | sort > "$tmp/owned.txt"
dups="$(cut -d' ' -f1 "$tmp/owned.txt" | uniq -d)"
if [ -n "$dups" ]; then echo "owned by more than one milestone: $dups"; exit 1; fi
if ! diff <(cut -d' ' -f1 "$tmp/spec_ids.txt") <(cut -d' ' -f1 "$tmp/owned.txt") > "$tmp/diff.txt"; then
  echo "spec IDs vs owned IDs differ:"; cat "$tmp/diff.txt"; exit 1
fi
join "$tmp/spec_ids.txt" "$tmp/owned.txt" | awk '($2=="P2")!=($3=="M16"||$3=="M17"||$3=="M18"){print "misplaced:",$0; bad=1} END{exit bad}'
echo "coverage: $(wc -l < "$tmp/spec_ids.txt") spec IDs, all owned exactly once"
