#!/usr/bin/env bash
set -euo pipefail
scratch="${WYSS_TEST_SCRATCH:-$HOME/.cache/wyss-build}"
mkdir -p -- "$scratch"
R=$(mktemp -d "$scratch/ui.XXXXXX")
# Un cookie nuevo y un display elegido por Xvfb aíslan teclado y portapapeles.
touch "$R/xauth"
chmod 600 "$R/xauth"
# FamilyWild antes de conocer el número asignado por -displayfd.
cookie=$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')
printf 'ffff 0000 0000 0012 4d49542d4d414749432d434f4f4b49452d31 0010 %s\n' "$cookie" |
    xauth -f "$R/xauth" nmerge -
Xvfb -displayfd 3 -auth "$R/xauth" -screen 0 1200x800x24 -nolisten tcp \
    3>"$R/display" >"$R/display.log" 2>&1 &
pid=$!
trap 'kill "$pid" 2>/dev/null || true' EXIT
for ((i=0; i<50; i++)); do
    if [[ -s "$R/display" ]]; then
        read -r number < "$R/display"
        xauth -f "$R/xauth" add ":$number" MIT-MAGIC-COOKIE-1 "$cookie"
        printf '%s\n' "$pid" >"$R/display.pid"
        trap - EXIT
        printf '%s\n' "$R"
        exit 0
    fi
    sleep 0.1
done
cat "$R/display.log" >&2
exit 1
