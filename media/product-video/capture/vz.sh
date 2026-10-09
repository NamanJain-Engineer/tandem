#!/bin/bash
# Drives the released Vibezeo AppImage on a 2x virtual display for the product video.
#   vz.sh setup | start [deeplink] | shot NAME | click X Y | key KEYS | type TEXT | rec-start | rec-stop | stop
# Coordinates are logical pixels (the display is 2x).
ROOT="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
W="${VZ_WORK:-/tmp/vz}"
export DISPLAY=:77
APPIMAGE="${APPIMAGE:-$(ls "$ROOT"/dist/Vibezeo_*_amd64-unsigned.AppImage | tail -1)}"
x2() { echo $(( $1 * 2 )); }
case "$1" in
  setup)
    rm -rf "$W" && mkdir -p "$W/home" "$W/shots"
    git clone -q --depth 80 "file://$ROOT" "$W/vibezeo"
    git -C "$W/vibezeo" config alias.recent "!git --no-pager log --oneline --decorate --color=always -n 14 -i --invert-grep --grep=tandem --grep=early --grep='20[0-9][0-9]' -- crates apps/desktop"
    cat > "$W/home/.bashrc" <<'RC'
export PATH=/opt/node22/bin:/root/.cargo/bin:/usr/local/bin:/usr/bin:/bin
export CARGO_HOME=/root/.cargo RUSTUP_HOME=/root/.rustup CARGO_TARGET_DIR=$ROOT/target CARGO_NET_OFFLINE=true
export TERM=xterm-256color
PS1='\[\e[38;5;80m\]\W\[\e[0m\] \[\e[38;5;245m\]$\[\e[0m\] '
RC
    # The quoted heredoc above keeps $ROOT literal: define it first.
    sed -i "1i ROOT=$ROOT" "$W/home/.bashrc"
    echo '. ~/.bashrc' > "$W/home/.bash_profile"
    # Warm the test build so `cargo test` in the video runs in seconds.
    (cd "$W/vibezeo/crates/tandem-core" && CARGO_TARGET_DIR=$ROOT/target cargo test -q --lib --no-run 2>&1 | tail -1)
    ;;
  xstart)
    Xvfb :77 -screen 0 2880x1800x24 -nolisten tcp > "$W/xvfb.log" 2>&1 &
    sleep 1
    openbox > "$W/openbox.log" 2>&1 &
    sleep 1
    ;;
  start)
    cd "$W"
    HOME="$W/home" TANDEM_HOME="$W/home/.tandem" SHELL=/bin/bash APPIMAGE_EXTRACT_AND_RUN=1 NO_AT_BRIDGE=1 GDK_SCALE=2 \
      setsid unshare -n -- dbus-run-session -- "$APPIMAGE" ${2:+"$2"} >> "$W/app.log" 2>&1 < /dev/null &
    echo $! > "$W/app.pid"
    ;;
  shot) ffmpeg -y -loglevel error -f x11grab -video_size 2880x1800 -i :77 -frames:v 1 -vf scale=1440:-1 "$W/shots/$2.png" ;;
  click) xdotool mousemove $(x2 $2) $(x2 $3) sleep 0.15 click 1 ;;
  move) xdotool mousemove $(x2 $2) $(x2 $3) ;;
  key) shift; xdotool key --delay 80 "$@" ;;
  type) xdotool type --delay "${3:-45}" "$2" ;;
  rec-start)
    ffmpeg -y -loglevel error -use_wallclock_as_timestamps 1 -thread_queue_size 512 -f x11grab -framerate 30 -video_size 2880x1800 -draw_mouse 1 -i :77 \
      -c:v libx264 -preset ultrafast -crf 14 -threads 2 -pix_fmt yuv420p -fps_mode passthrough "$W/${2:-raw}.mkv" < /dev/null > "$W/rec.log" 2>&1 &
    echo $! > "$W/rec.pid"
    ;;
  rec-stop) kill -INT "$(cat "$W/rec.pid")"; sleep 2 ;;
  kill9) pkill -9 -f 'squashfs-root/usr/bin/tandem|AppRun' ; pkill -9 -x tandem ;;
  stop) pkill -f "Xvfb :77"; pkill -x openbox; pkill -9 -x tandem ;;
esac
