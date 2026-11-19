#!/usr/bin/env bash
set -eu
R=$1 B=$2 OUT=$(realpath -- "$3")
source "$(dirname "$0")/common.sh"
printf XY | xclip -quiet -selection clipboard >"$fixture/xclip.log" 2>&1 &
clipboard_pid=$!
WYSS_CONSOLE_CMD='stty raw -echo; head -c 5 > bytes' run_window ctrlc.log "$B" 1
xdotool windowfocus --sync "$W"
sleep 1
xdotool key --delay 150 a ctrl+c ctrl+shift+c ctrl+v
sleep 0.5
xdotool key b
for ((i=0; i<30; i++)); do
    [[ -f bytes && $(wc -c < bytes) == 5 ]] && break
    sleep 0.1
done
cmp bytes <(printf 'a\003XYb')
cp -- bytes "$OUT"
od -An -tx1 "$OUT"
