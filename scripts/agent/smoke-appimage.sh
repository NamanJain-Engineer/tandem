#!/bin/sh
# Runs a release AppImage with a fresh home and NO network, then screenshots
# the welcome screen and the first setup step into $OUT (default /tmp/smoke).
#   xvfb-run -a -s "-screen 0 1440x900x24" sh scripts/agent/smoke-appimage.sh [AppImage]
# Look at the PNGs: the welcome says "Free · no account · nothing sent to Vibezeo", Start opens setup. To go
# further, run the app on a persistent display (Xvfb :77 &) and step through
# with `xdotool mousemove X Y click 1` plus `ffmpeg -f x11grab` screenshots.
set -u
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
APP="${1:-$(ls "$ROOT"/dist/Vibezeo_*_amd64-unsigned.AppImage | tail -1)}"
OUT="${OUT:-/tmp/smoke}"
rm -rf "$OUT/home" 2>/dev/null; mkdir -p "$OUT/home"
export HOME="$OUT/home" TANDEM_HOME="$OUT/home/.tandem" APPIMAGE_EXTRACT_AND_RUN=1 NO_AT_BRIDGE=1
cd "$OUT"
unshare -n -- dbus-run-session -- "$APP" > "$OUT/app.log" 2>&1 &
PID=$!
sleep 15
ffmpeg -y -loglevel error -f x11grab -video_size 1440x900 -i "$DISPLAY" -frames:v 1 "$OUT/welcome.png"
# "Start using Vibezeo" (the welcome card is centred: its button at y≈451 on 1440x900).
xdotool mousemove 720 451 click 1
sleep 6
ffmpeg -y -loglevel error -f x11grab -video_size 1440x900 -i "$DISPLAY" -frames:v 1 "$OUT/started.png"
echo "app alive: $(kill -0 $PID 2>/dev/null && echo yes || echo no) · screenshots in $OUT"
kill $PID 2>/dev/null
