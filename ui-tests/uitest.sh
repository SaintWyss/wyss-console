#!/usr/bin/env bash
set -eu
R=$1 B=$2 S=$(realpath -- "$3")
source "$(dirname "$0")/common.sh"
mkdir -p "$S"
WYSS_CONSOLE_CMD="bash --norc --noprofile -i" run_window scroll.log "$B" 1
sleep 2; timeout 3 xdotool windowsize $W 900 560; timeout 3 xdotool windowfocus $W 2>/dev/null; sleep 0.5
k(){ timeout 10 xdotool type --delay 60 "$1"; timeout 3 xdotool key Return; sleep 0.6; }
k "echo uno dos tres cuatro"; k "seq 1 300"
timeout 5 import -window root "$S/ui-1.png"
timeout 3 xdotool mousemove --window $W 400 300; for i in 1 2 3 4 5 6 7 8; do timeout 3 xdotool click 4; done; sleep 0.5
timeout 5 import -window root "$S/ui-2-rueda.png"
timeout 3 xdotool key Return; sleep 0.3
timeout 3 xdotool windowsize $W 520 300; sleep 1; k "stty size"
timeout 5 import -window root "$S/ui-3-chica.png"
k "exit"
