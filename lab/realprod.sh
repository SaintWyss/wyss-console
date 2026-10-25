#!/usr/bin/env bash
# realprod.sh: production topology on the real Wayland session. Packaged wyss-control (protocol 2,
# disposable store, real Codex 0.159.0 binary, no credentials) + the diagnostic main binary.
# No OS input is sent: the page is driven from inside and the dialog is answered from inside GTK.
set -eu
L="$HOME/wyss-lease-lab"; ID="p$(date +%H%M%S)"; RUN="$L/runs/$ID"
PKG=~/wyss-console-qa-0e9a534/.artifacts/package/wyss-console-0.2.0-linux-x64
CODEX="$HOME/.codex/packages/standalone/releases/0.159.0-x86_64-unknown-linux-musl/bin/codex"
mkdir -p -m 700 "$RUN"/{home,state,rt,codexhome,work/repo}
python3 - "$RUN" <<'P'
import json,sys,os
d=json.load(open(os.path.expanduser('~/.codex/models_cache.json'))); d['identity']='lab'
json.dump(d,open(sys.argv[1]+'/codexhome/models_cache.json','w'))
P
chmod 600 "$RUN/codexhome/models_cache.json"
( cd "$RUN/work/repo" && git init -q && git -c user.email=l@l -c user.name=lab commit -q --allow-empty -m init )
R="$(mktemp -d /tmp/claude-1000/wr.XXXXXX)"; chmod 700 "$R"; echo "$R" > "$RUN/xdg"
ln -s "/run/user/$(id -u)/${REAL_WAYLAND:-wayland-0}" "$R/wayland-0"
env -i PATH=/usr/bin:/bin HOME="$RUN/home" XDG_RUNTIME_DIR="$R" LANG=C.UTF-8 \
  setsid "$PKG/libexec/wyss-console/wyss-control" --protocol 2 --runtime-root "$RUN/rt" --state-root "$RUN/state" \
  --create --codex "$CODEX" --codex-home "$RUN/codexhome" >"$RUN/authority.log" 2>&1 &
echo $! > "$RUN/authority.pid"
for i in $(seq 1 50); do [ -S "$RUN/rt/control.sock" ] && break; sleep 0.2; done
BIN="$HOME/wyss-console-fix/src-tauri/target/debug/wyss-console"
env -i PATH=/usr/bin:/bin HOME="$RUN/home" XDG_RUNTIME_DIR="$R" LANG=C.UTF-8 WAYLAND_DISPLAY=wayland-0 NO_AT_BRIDGE=1 \
  XDG_CURRENT_DESKTOP=KDE KDE_SESSION_VERSION=6 \
  WYSS_LAB_WORKSPACE="$RUN/work/repo" WYSS_LAB_TRACE="$RUN/trace.log" WYSS_LAB_DRIVER="${LAB_DRIVER_FILE:-$L/driver/driver.js}" \
  WYSS_LAB_AUTO_ALLOW_MS="${LAB_AUTO:-2500}" \
  setsid dbus-run-session -- "$BIN" --f8-authority "$RUN/rt" >"$RUN/gui.log" 2>&1 &
echo $! > "$RUN/gui.pid"
echo "$RUN"
