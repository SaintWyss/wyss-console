#!/usr/bin/env bash
set -eu
R=$1 B=$2 OUT=$(realpath -- "$3") IM=$4
source "$(dirname "$0")/common.sh"
case "$IM" in
    ibus) export GTK_IM_MODULE=ibus ;;
    simple) export GTK_IM_MODULE=gtk-im-context-simple ;;
    *) exit 2 ;;
esac
export WYSS_CONSOLE_CMD='stty raw -echo; head -c 14 > bytes'
run_window keyboard.log bash -c 'if [[ $GTK_IM_MODULE == ibus ]]; then ibus-daemon -dx; sleep 2; fi; exec "$1" 1' _ "$B"
setxkbmap -layout latam -variant deadtilde
timeout 3 xdotool windowfocus --sync $W; sleep 0.5
timeout 3 xdotool key --delay 80 h o l a space dead_acute a dead_tilde n ctrl+shift+c Return Up
for ((i=0; i<30; i++)); do
    [[ -f bytes && $(wc -c < bytes) == 14 ]] && break
    sleep 0.1
done
cmp bytes <(printf 'hola áñ\003\r\033[A')
cp -- bytes "$OUT"
od -An -tx1 "$OUT"
