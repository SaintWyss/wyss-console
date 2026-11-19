#!/usr/bin/env bash
set -eu
R=$1 B=$2 S=$(realpath -- "$3")
source "$(dirname "$0")/common.sh"
mkdir -p "$S"
WYSS_CONSOLE_CMD='printf "hola mundo\r\nsegunda línea  \r\ntercera fila final\r\n"; sleep 60' run_window selection.log "$B" 1
sleep 1.5; timeout 3 xdotool windowfocus $W 2>/dev/null
# celda: x = 4 + col*cw + cw/2 ; y = 4 + row*ch + ch/2 (relativo al área de la terminal, que arranca bajo la barra de pestañas)
cell(){ echo "$((4 + $2*CW + CW/2)) $((OFF + 4 + $1*CH + CH/2))"; }
CW=${CW:-8}; CH=${CH:-20}; OFF=${OFF:-38}
clip(){ timeout 2 xclip -o -selection "$1" 2>/dev/null | od -c | head -3; }
# 1) arrastre de (0,5) a (1,6)
read x1 y1 <<<"$(cell 0 5)"; read x2 y2 <<<"$(cell 1 6)"
timeout 3 xdotool mousemove --window $W $x1 $y1 mousedown 1 mousemove --window $W $((x1+20)) $y1 mousemove --window $W $x2 $y2 mouseup 1; sleep 0.4
timeout 5 import -window root "$S/sel-1-arrastre.png"
echo "PRIMARY tras arrastre:"; clip primary
timeout 3 xdotool key ctrl+c; sleep 0.3; echo "CLIPBOARD tras Ctrl+C:"; clip clipboard
# 2) doble clic en 'segunda'
read x y <<<"$(cell 1 2)"; timeout 3 xdotool mousemove --window $W $x $y click --repeat 2 --delay 80 1; sleep 0.4
echo "PRIMARY doble clic:"; clip primary
# 3) triple clic en fila 2
read x y <<<"$(cell 2 3)"; timeout 3 xdotool mousemove --window $W $x $y click --repeat 3 --delay 80 1; sleep 0.4
echo "PRIMARY triple clic:"; clip primary
timeout 5 import -window root "$S/sel-2-triple.png"
