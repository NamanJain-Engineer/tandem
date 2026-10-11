#!/usr/bin/env bash
# Downloads the sherpa-onnx static libraries the `kws` feature links (F-CN-07)
# into ~/.cache/sherpa-onnx, for machines where the crate's own download
# can't verify TLS (a proxy with its own CA). scripts/gate.sh points
# SHERPA_ONNX_ARCHIVE_DIR at the cache when it holds the archive.
set -euo pipefail
ver=1.13.8
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) name=linux-x64-static-lib ;;
  Linux-aarch64) name=linux-aarch64-static-lib ;;
  *) echo "fetch-sherpa: let the crate download its archive on this platform" >&2; exit 0 ;;
esac
dir=${SHERPA_ONNX_ARCHIVE_DIR:-$HOME/.cache/sherpa-onnx}
mkdir -p "$dir"
file="sherpa-onnx-v$ver-$name.tar.bz2"
[ -s "$dir/$file" ] || curl -fL --retry 4 --retry-delay 2 -o "$dir/$file" \
  "https://github.com/k2-fsa/sherpa-onnx/releases/download/v$ver/$file"
echo "export SHERPA_ONNX_ARCHIVE_DIR=$dir"
