#!/usr/bin/env bash
# Type-checks (and lints) the voice crate's platform code for Windows and
# macOS from Linux: `cargo check`/`clippy` don't link, so no SDK is needed.
# ring's C build is given stub headers for darwin. Usage:
#   bash scripts/agent/check-cross.sh [check|clippy]
set -euo pipefail
cmd=${1:-clippy}
root=$(cd "$(dirname "$0")/../.." && pwd)
inc=$(mktemp -d)
trap 'rm -rf "$inc"' EXIT
printf '#define TARGET_OS_MAC 1\n#define TARGET_OS_OSX 1\n#define TARGET_OS_IPHONE 0\n#define TARGET_OS_IOS 0\n#define TARGET_OS_SIMULATOR 0\n' >"$inc/TargetConditionals.h"
echo '#define assert(x) ((void)0)' >"$inc/assert.h"
printf '#include <stddef.h>\nvoid *memcpy(void*,const void*,size_t);\nvoid *memset(void*,int,size_t);\nvoid *memmove(void*,const void*,size_t);\nint memcmp(const void*,const void*,size_t);\nsize_t strlen(const char*);\n' >"$inc/string.h"
clang_inc=$(clang -print-resource-dir)/include
extra=()
[ "$cmd" = clippy ] && extra=(--all-targets -- -D warnings)
rustup target add x86_64-pc-windows-gnu aarch64-apple-darwin >/dev/null
cd "$root"
echo "== windows"
cargo "$cmd" -p tandem-voice --target x86_64-pc-windows-gnu --no-default-features "${extra[@]}"
echo "== macos"
CC_aarch64_apple_darwin=clang \
CFLAGS_aarch64_apple_darwin="-nostdinc -isystem $clang_inc -I$inc" \
  cargo "$cmd" -p tandem-voice --target aarch64-apple-darwin --no-default-features "${extra[@]}"
