#!/usr/bin/env bash
# Downloads Dictate models into ~/.cache/tandem-models (or $TANDEM_MODEL_CACHE)
# and checks each file's size and SHA-256 against the pinned catalog
# (crates/tandem-voice/models/catalog.json). A file that doesn't match is
# deleted. Usage: bash scripts/fetch-models.sh base.en [parakeet ...]
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cache=${TANDEM_MODEL_CACHE:-$HOME/.cache/tandem-models}
[ $# -gt 0 ] || set -- base.en
for want in "$@"; do
  python3 - "$root/crates/tandem-voice/models/catalog.json" "$want" >"$cache.list.$$" <<'PY' || { rm -f "$cache.list.$$"; exit 1; }
import json, sys
cat = json.load(open(sys.argv[1]))
want = sys.argv[2]
hits = [m for m in cat["models"] if m["id"] == want or m["id"].endswith(want) or want in m["id"]]
if len(hits) != 1:
    sys.exit(f"fetch-models: '{want}' matches {len(hits)} models")
m = hits[0]
files = m.get("files") or [m["url"].rsplit("/", 1)[1]]
for f in files:
    url = m["url"] + f if m.get("files") else m["url"]
    sha = m["sha256"][f] if isinstance(m["sha256"], dict) else m["sha256"]
    size = m["bytes"][f] if isinstance(m["bytes"], dict) else m["bytes"]
    print(m["id"], f, url, sha, size)
PY
  while read -r id file url sha size; do
    dir="$cache/$id"
    mkdir -p "$dir"
    dest="$dir/$file"
    if [ -f "$dest" ] && [ "$(stat -c %s "$dest")" = "$size" ] && echo "$sha  $dest" | sha256sum -c --status; then
      echo "fetch-models: $id/$file cached"
      continue
    fi
    echo "fetch-models: $id/$file ($size bytes)"
    curl -fL --retry 4 --retry-delay 2 -C - -o "$dest.partial" "$url"
    if [ "$(stat -c %s "$dest.partial")" != "$size" ] || ! echo "$sha  $dest.partial" | sha256sum -c --status; then
      rm -f "$dest.partial"
      echo "fetch-models: $id/$file didn't match the pinned SHA-256; deleted" >&2
      exit 1
    fi
    mv "$dest.partial" "$dest"
    printf '%s  %s\n' "$sha" "$file" >>"$dir/.verified"
    sort -u -o "$dir/.verified" "$dir/.verified"
  done <"$cache.list.$$"
  rm -f "$cache.list.$$"
done
