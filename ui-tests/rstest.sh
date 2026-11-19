#!/usr/bin/env bash
set -eu
R=$1 B=$2 S=$(realpath -- "$3")
source "$(dirname "$0")/common.sh"
mkdir -p "$S"
WYSS_CONSOLE_CMD="sh -i" run_window resize.log "$B" 1
echo "ventana $W: $(timeout 2 xdotool getwindowgeometry $W | tail -1)"
sleep 1; timeout 3 xdotool windowsize --sync $W 520 300; sleep 1.2
echo "tras achicar: $(timeout 2 xdotool getwindowgeometry $W | tail -1)"
timeout 5 import -window root "$S/ui-4-resize.png"
timeout 3 xdotool windowsize --sync $W 1100 700; sleep 1.2
echo "tras agrandar: $(timeout 2 xdotool getwindowgeometry $W | tail -1)"
timeout 5 import -window root "$S/ui-5-resize.png"
