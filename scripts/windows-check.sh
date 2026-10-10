#!/usr/bin/env bash
# The Windows code paths, checked from Linux (D-1700, docs/OPERATING.md
# "Windows from Linux"). No Windows CI runs until Actions minutes return, so
# this is what keeps `cfg(windows)` code compiling and its tests passing.
#
#   bash scripts/windows-check.sh            # all three
#   bash scripts/windows-check.sh --clippy   # GNU target, every crate but the app (the gate runs this)
#   bash scripts/windows-check.sh --msvc     # MSVC target through cargo-xwin, the whole workspace
#   bash scripts/windows-check.sh --wine     # the platform crates' tests under Wine
#
# --clippy needs mingw-w64 (bootstrap-linux.sh). The app (whisper.cpp, ONNX
# Runtime) doesn't build for the GNU target, so --msvc checks it: cargo-xwin
# and clang 19 or later. --wine needs wine (WineHQ 11 or Ubuntu's 9).
set -euo pipefail
cd "$(dirname "$0")/.."

clippy=0 msvc=0 wine=0
[ $# -eq 0 ] && clippy=1 msvc=1 wine=1
for arg in "$@"; do
  case "$arg" in
    --clippy) clippy=1 ;;
    --msvc) msvc=1 ;;
    --wine) wine=1 ;;
    *) echo "unknown option $arg" >&2; exit 2 ;;
  esac
done

need_target() {
  rustup target list --installed | grep -qx "$1" || rustup target add "$1"
}

if [ "$clippy" = 1 ]; then
  command -v x86_64-w64-mingw32-gcc >/dev/null || {
    echo "missing x86_64-w64-mingw32-gcc: apt-get install gcc-mingw-w64-x86-64 (bootstrap-linux.sh)" >&2
    exit 1
  }
  need_target x86_64-pc-windows-gnu
  cargo clippy --target x86_64-pc-windows-gnu --workspace \
    --exclude tandem-desktop --exclude tandem-invariants --all-targets -- -D warnings
fi

if [ "$msvc" = 1 ]; then
  # The MSVC STL headers refuse clang older than 19 (whisper.cpp needs them).
  for v in 21 20 19; do
    if [ -x "/usr/lib/llvm-$v/bin/clang" ]; then
      export PATH="/usr/lib/llvm-$v/bin:$PATH"
      break
    fi
  done
  command -v cargo-xwin >/dev/null || { echo "missing cargo-xwin: cargo install cargo-xwin --locked" >&2; exit 1; }
  need_target x86_64-pc-windows-msvc
  cargo xwin clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings
fi

if [ "$wine" = 1 ]; then
  command -v wine >/dev/null || { echo "missing wine (docs/OPERATING.md)" >&2; exit 1; }
  need_target x86_64-pc-windows-gnu
  export WINEDEBUG=-all
  export WINEPREFIX="${WINEPREFIX:-$HOME/.wine-tandem}"
  export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER=wine
  # What Wine can't run (each confirmed with a probe, D-1702; HUMAN_TODO
  # keeps them for a real Windows machine):
  # - ConPTY: Wine's conhost rejects `--inheritcursor`, which portable-pty
  #   always passes, and exits, so tests/pty_windows.rs sees no output.
  # - Recursive file watching: Wine misses changes in folders that existed
  #   before the watch started (real ReadDirectoryChangesW reports them).
  # - git isn't installed inside Wine (tandem-host's gitops tests).
  cargo test --target x86_64-pc-windows-gnu -p tandem-pty --lib --test remote
  cargo test --target x86_64-pc-windows-gnu -p tandem-core -- \
    --skip files::watch::tests::f_cd_19_watch_reports_file_changes_in_batches \
    --skip files::watch::tests::f_cd_20_
  cargo test --target x86_64-pc-windows-gnu -p tandem-routines
  cargo test --target x86_64-pc-windows-gnu -p tandem-voice
  cargo test --target x86_64-pc-windows-gnu -p tandem-host --lib -- --skip gitops::
fi
echo "windows-check passed"
