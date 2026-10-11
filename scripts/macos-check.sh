#!/usr/bin/env bash
# Type-checks (or lints) the whole workspace for macOS from Linux (D-2105):
# every crate, Apple silicon and Intel, all targets (tests too). `cargo
# check` and `clippy` don't link, so no Apple SDK is needed, with three
# stand-ins, none of which changes what is checked in Rust:
#   - C and C++ sources (SQLite, zlib, ring, libgit2, whisper.cpp) compile
#     to empty objects through a stand-in compiler;
#   - whisper-rs uses its bundled bindings and skips its CMake build;
#   - the app is checked without `os-audio`: cpal's CoreAudio bindings are
#     generated from the SDK's headers. tandem-voice's own cpal code
#     (capture_cpal.rs, chimes) is the only Rust left unchecked; macos.yml
#     and codemagic.yaml lint it on a Mac. tests/invariants dev-depends on
#     the app with its defaults, so it is left to them too (no macOS code).
#   bash scripts/macos-check.sh [--clippy] [--target aarch64-apple-darwin|x86_64-apple-darwin]
set -euo pipefail
cmd=check
targets=(aarch64-apple-darwin x86_64-apple-darwin)
while [ $# -gt 0 ]; do
  case "$1" in
    --clippy) cmd=clippy ;;
    --target) targets=("$2"); shift ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
  shift
done
root="$(cd "$(dirname "$0")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
cat >"$tmp/cc" <<'EOF'
#!/bin/sh
# Stand-in C/C++ compiler for type-checking only: `-c … -o x.o` writes an
# empty object for the target; everything else (cc-rs's probes) is clang's.
out=""; prev=""; compile=0
for a in "$@"; do
  [ "$prev" = "-o" ] && out="$a"
  case "$a" in -c) compile=1 ;; esac
  prev="$a"
done
if [ "$compile" = 1 ] && [ -n "$out" ]; then
  exec clang --target="$STANDIN_TARGET" -c -x c /dev/null -o "$out"
fi
exec clang "$@"
EOF
chmod +x "$tmp/cc"
extra=(--all-targets)
[ "$cmd" = clippy ] && extra=(--all-targets -- -D warnings)
rustup target add "${targets[@]}" >/dev/null
cd "$root"
for t in "${targets[@]}"; do
  envt="${t//-/_}"
  echo "== $t: every crate but the voice crate and the app"
  env "CC_$envt=$tmp/cc" "CXX_$envt=$tmp/cc" STANDIN_TARGET="$t" \
    WHISPER_DONT_GENERATE_BINDINGS=1 DOCS_RS=1 \
    cargo "$cmd" --workspace --exclude tandem-desktop --exclude tandem-voice --exclude tandem-invariants --target "$t" "${extra[@]}"
  echo "== $t: tandem-voice (no cpal)"
  env "CC_$envt=$tmp/cc" "CXX_$envt=$tmp/cc" STANDIN_TARGET="$t" \
    WHISPER_DONT_GENERATE_BINDINGS=1 DOCS_RS=1 \
    cargo "$cmd" -p tandem-voice --no-default-features --features whisper,parakeet --target "$t" "${extra[@]}"
  echo "== $t: the app (no os-audio)"
  env "CC_$envt=$tmp/cc" "CXX_$envt=$tmp/cc" STANDIN_TARGET="$t" \
    WHISPER_DONT_GENERATE_BINDINGS=1 DOCS_RS=1 \
    cargo "$cmd" -p tandem-desktop --no-default-features --target "$t" "${extra[@]}"
done
echo "macOS $cmd: ok (${targets[*]})"
