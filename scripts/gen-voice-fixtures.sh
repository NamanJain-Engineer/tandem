#!/usr/bin/env bash
# Regenerates the committed Dictate WAV fixtures (MILESTONES M8 "Committed
# WAV fixtures") with espeak-ng and sox: 16 kHz mono 16-bit PCM.
set -euo pipefail
out=$(cd "$(dirname "$0")/.." && pwd)/crates/tandem-voice/tests/fixtures
mkdir -p "$out"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
speak() { # name text
  espeak-ng -v en-us -s 150 -w "$tmp/$1.raw.wav" "$2"
  # 300 ms of lead-in and 500 ms of tail, like someone pressing a key.
  sox "$tmp/$1.raw.wav" -r 16000 -c 1 -b 16 "$out/$1.wav" pad 0.3 0.5 norm -3
}
speak hello_world_16k "Hello world."
speak use_tandem_16k "Use tandem."
sox -n -r 16000 -c 1 -b 16 "$out/silence_2s.wav" trim 0.0 2.0
ls -l "$out"
