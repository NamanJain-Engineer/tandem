#!/usr/bin/env bash
# Installs the Linux build and test dependencies (Ubuntu 22.04+/Debian 12).
set -euo pipefail
SUDO=""
[ "$(id -u)" -ne 0 ] && SUDO="sudo"
$SUDO apt-get update
$SUDO env DEBIAN_FRONTEND=noninteractive apt-get -o Dpkg::Options::=--force-confold install -y \
  libwebkit2gtk-4.1-dev webkit2gtk-driver xvfb librsvg2-dev libayatana-appindicator3-dev \
  libasound2-dev libxdo-dev libssl-dev libdbus-1-dev clang cmake pkg-config \
  espeak-ng sox vim htop less patchelf file \
  dbus xclip openbox xdotool gcc-mingw-w64-x86-64
command -v tauri-driver >/dev/null || cargo install tauri-driver --locked
