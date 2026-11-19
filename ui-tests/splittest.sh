#!/usr/bin/env bash
set -eu
R=$1 B=$2 S=$(realpath -- "$3")
source "$(dirname "$0")/common.sh"
mkdir -p "$S"
start_host
count() { request term_list; }
shot(){ sleep 0.8; timeout 5 import -window root "$S/split-$1.png"; }
k(){ timeout 10 xdotool type --delay 50 "$1"; timeout 3 xdotool key Return; sleep 0.7; }
run_window split.log "$B"; sleep 2.5
timeout 3 xdotool windowsize $W 1200 800; timeout 3 xdotool windowfocus $W 2>/dev/null; sleep 0.8
timeout 3 xdotool mousemove --window $W 600 400 click 1; sleep 0.3
k "echo uno"; echo "inicio: $(count)"
timeout 3 xdotool key ctrl+shift+d; sleep 1.5; echo "Ctrl+Shift+D: $(count)"
k "echo panel derecho"
timeout 3 xdotool key ctrl+shift+Left; sleep 0.4; k "echo panel izquierdo"; shot 1-split
timeout 3 xdotool key ctrl+shift+e; sleep 1.5; echo "Ctrl+Shift+E: $(count)"; k "echo abajo"; shot 2-tres
timeout 3 xdotool key ctrl+shift+w; sleep 1.5; echo "Ctrl+Shift+W: $(count)"; shot 3-cerrado
k "cd /"; sleep 3; shot 4-cd
X=$(python3 -c 'print(52+ (1200-52)//2)'); timeout 3 xdotool mousemove --window $W $X 400 mousedown 1 mousemove --window $W $((X-150)) 400 mousemove --window $W $((X-250)) 400 mouseup 1; sleep 1; k "stty size"; shot 5-arrastre
timeout 3 xdotool key ctrl+shift+t; sleep 1.5; echo "Ctrl+Shift+T: $(count)"; k "echo pestaña dos"
timeout 3 xdotool key ctrl+shift+Prior; sleep 0.8; shot 6-pgup
test "$(count)" -ge 2
