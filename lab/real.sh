#!/usr/bin/env bash
# real.sh: the diagnostic dev GUI on the owner's real Wayland session. It sends NO OS input; the
# page is driven by a DOM script and the dialog is answered from inside GTK. Private HOME/bus.
set -eu
L="$HOME/wyss-lease-lab"; ID="r$(date +%H%M%S)"; RUN="$L/runs/$ID"
mkdir -p -m 700 "$RUN/home"
R="$(mktemp -d /tmp/claude-1000/wr.XXXXXX)"; chmod 700 "$R"; echo "$R" > "$RUN/xdg"
ln -s "/run/user/$(id -u)/${REAL_WAYLAND:-wayland-0}" "$R/wayland-0"
BIN="$HOME/wyss-console-fix/src-tauri/target/debug/examples/f8_local_gui"
mkdir -p -m 700 "$RUN/app"
env -i PATH=/usr/bin:/bin HOME="$RUN/home" XDG_RUNTIME_DIR="$R" LANG=C.UTF-8 WAYLAND_DISPLAY=wayland-0 NO_AT_BRIDGE=1 \
  XDG_CURRENT_DESKTOP=KDE KDE_SESSION_VERSION=6 \
  WYSS_LAB_WORKSPACE="$RUN/app/workspace" WYSS_LAB_TRACE="$RUN/trace.log" WYSS_LAB_DRIVER="${LAB_DRIVER_FILE:-$L/driver/driver.js}" \
  WYSS_LAB_AUTO_ALLOW_MS="${LAB_AUTO:-4000}" \
  setsid dbus-run-session -- "$BIN" "$RUN/app" >"$RUN/gui.log" 2>&1 &
echo $! > "$RUN/gui.pid"
echo "$RUN"
