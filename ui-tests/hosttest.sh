#!/usr/bin/env bash
# Arranque por la UI, persistencia y reconexión con el host aislado.
set -eu
R=$1 B=$2 S=$(realpath -- "$3")
source "$(dirname "$0")/common.sh"
mkdir -p "$S"
# Primero arranca el host hermano sin WYSS_CONSOLE_HOST. Cerrarlo por su socket privado.
trap 'request shutdown >/dev/null 2>&1 || true; cleanup' EXIT
run_window first.log "$B" 1
test "$(request term_list)" = 1
xdotool windowfocus --sync "$W"
xdotool key ctrl+shift+t
sleep 1
test "$(request term_list)" = 2
xdotool key ctrl+shift+w
sleep 1
test "$(request term_list)" = 1
stop_window
test "$(request term_list)" = 1
WYSS_CONSOLE_SHOT="$S/reabierta.png" run_window second.log "$B" 1
sleep 3
test -s "$S/reabierta.png"
test "$(request term_list)" = 1
echo "Host reutilizado y terminal persistente"
