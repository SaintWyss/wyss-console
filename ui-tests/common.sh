#!/usr/bin/env bash
# Cargar después de asignar R (directorio de ejecución) y B (binario).
[[ -d "$R" && -O "$R" && -f "$R/display" ]] || { echo "Ejecutá nest-up.sh" >&2; exit 2; }
R=$(cd -- "$R" && pwd)
B=$(realpath -- "$B")
test -x "$B"
read -r number < "$R/display"
[[ "$number" =~ ^[0-9]+$ ]] || exit 2
fixture=$(mktemp -d "$R/t.XXXXXX")
mkdir -m 700 -- "$fixture"/{home,run,config,state,data,cache}
unset WAYLAND_DISPLAY DBUS_SESSION_BUS_ADDRESS WYSS_CONSOLE_HOST WYSS_CONSOLE_CMD
unset CLAUDE_CONFIG_DIR CODEX_HOME WYSS_ARSENAL_SYNC WYSS_CONSOLE_SHOT WYSS_CONSOLE_EXIT_AFTER XDG_CONFIG_DIRS XDG_DATA_DIRS
export DISPLAY=":$number" GDK_BACKEND=x11 NO_AT_BRIDGE=1 XAUTHORITY="$R/xauth"
export HOME="$fixture/home" XDG_RUNTIME_DIR="$fixture/run" XDG_CONFIG_HOME="$fixture/config"
export XDG_STATE_HOME="$fixture/state" XDG_DATA_HOME="$fixture/data" XDG_CACHE_HOME="$fixture/cache"
export TMPDIR="$fixture" SHELL=/bin/sh
# Los scripts de teclado colocan los resultados seleccionados en este árbol.
cd -- "$fixture"
app_pid=
host_pid=
clipboard_pid=
stop_window() {
    if [[ -n "$app_pid" ]]; then
        kill -TERM -- "-$app_pid" 2>/dev/null || true
        wait "$app_pid" 2>/dev/null || true
        app_pid=
    fi
}
cleanup() {
    stop_window
    if [[ -n "$host_pid" ]]; then
        request shutdown >/dev/null 2>&1 || kill "$host_pid" 2>/dev/null || true
        wait "$host_pid" 2>/dev/null || true
    fi
    if [[ -n "$clipboard_pid" ]]; then kill "$clipboard_pid" 2>/dev/null || true; fi
}
trap cleanup EXIT
run_window() {
    local log=$1
    shift
    setsid dbus-run-session -- "$@" >"$fixture/$log" 2>&1 &
    app_pid=$!
    W=$(timeout 12 xdotool search --sync --onlyvisible --name 'Consola|Wyss' | head -1)
    test -n "$W"
}
request() {
    python3 -I - "$XDG_RUNTIME_DIR/wyss-console/host.sock" "$1" <<'PY'
import json, socket, struct, sys
with socket.socket(socket.AF_UNIX) as sock:
    sock.settimeout(3)
    sock.connect(sys.argv[1])
    body = json.dumps({"op": sys.argv[2], "params": {}}).encode()
    sock.sendall(struct.pack(">I", len(body)) + body)
    with sock.makefile("rb") as reply:
        size, = struct.unpack(">I", reply.read(4))
        answer = json.loads(reply.read(size))["ok"]
        print(len(answer) if isinstance(answer, list) else json.dumps(answer))
PY
}
start_host() {
    "${B%/*}/wyss-host" >"$fixture/host.log" 2>&1 &
    host_pid=$!
    for ((i=0; i<50; i++)); do
        if request hello >/dev/null 2>&1; then return; fi
        sleep 0.1
    done
    cat "$fixture/host.log" >&2
    return 1
}
