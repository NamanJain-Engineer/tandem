#!/bin/bash
# One continuous take of the released app for the product video. Writes
# $W/take.mkv and $W/cues.txt (seconds from the start of the recording).
ROOT="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
W="${VZ_WORK:-/tmp/vz}"
V="$(dirname "$0")/vz.sh"
export DISPLAY=:77
: > "$W/cues.txt"
now() { date +%s.%N; }
cue() { awk -v a="$(now)" -v b="$T0" -v n="$1" 'BEGIN{printf "%s %.2f\n", n, a-b}' >> "$W/cues.txt"; }
# Smooth mouse glide in logical px.
glide() {
  read -r X0 Y0 < <(xdotool getmouselocation --shell | awk -F= '/^X=/{x=$2}/^Y=/{y=$2}END{print x, y}')
  local x1=$(( $1 * 2 )) y1=$(( $2 * 2 )) n=24
  for i in $(seq 1 $n); do
    local t; t=$(awk -v i=$i -v n=$n 'BEGIN{p=i/n; print p<0.5 ? 2*p*p : 1-((-2*p+2)^2)/2}')
    xdotool mousemove "$(awk -v a=$X0 -v b=$x1 -v t=$t 'BEGIN{printf "%d", a+(b-a)*t}')" "$(awk -v a=$Y0 -v b=$y1 -v t=$t 'BEGIN{printf "%d", a+(b-a)*t}')"
    sleep 0.022
  done
}
typeit() { xdotool type --delay "${2:-55}" "$1"; }

rm -f "$W/take.mkv"
T0=$(now)
$V rec-start take
until [ -s "$W/take.mkv" ]; do sleep 0.05; done
cue start
sleep 2.5
cue grid-hover-claude
glide 721 491
sleep 1.6
glide 965 491
sleep 0.5
cue terminal-click
xdotool click 1
sleep 1.4
cue git-recent
typeit "git recent"
xdotool key Return
sleep 1.6
cue split-right
xdotool key ctrl+shift+e
sleep 0.9
glide 809 471
sleep 0.25
xdotool click 1
sleep 1.1
typeit "htop"
xdotool key Return
sleep 1.4
cue split-down
xdotool key ctrl+shift+d
sleep 0.9
glide 809 471
sleep 0.25
xdotool click 1
sleep 1.1
typeit "(cd crates/*core && cargo test -q --lib 2>&1 | tail -n 12)" 32
xdotool key Return
sleep 0.8
glide 550 470
xdotool click 1
sleep 0.4
cue split-down-left
xdotool key ctrl+shift+d
sleep 0.9
glide 809 471
sleep 0.25
xdotool click 1
sleep 1.1
typeit "(cd crates/*engines/src && vim +/PreToolUse hook.rs)" 32
xdotool key Return
sleep 2.0
glide 1250 760
cue four-panes
sleep 5.0
cue palette
xdotool key ctrl+k
sleep 0.7
typeit "split" 110
sleep 2.0
xdotool key Escape
sleep 1.6
cue kill
pkill -9 -x tandem
sleep 2.2
cue relaunch
$V start
until xdotool search --name Vibezeo >/dev/null 2>&1; do sleep 0.1; done
cue window
sleep 12
cue end
$V rec-stop
cat "$W/cues.txt"
