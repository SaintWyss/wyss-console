#!/usr/bin/env bash
set -euo pipefail
R=$(cd -- "$1" && pwd)
read -r pid < "$R/display.pid"
[[ "$pid" =~ ^[0-9]+$ ]]
if [[ -r "/proc/$pid/cmdline" ]] &&
    tr '\0' '\n' < "/proc/$pid/cmdline" | grep -Fxq -- "$R/xauth"; then
    kill -- "$pid"
fi
