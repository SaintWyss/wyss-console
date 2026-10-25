#!/usr/bin/env bash
# down.sh <run>: ends only what this run started, by recorded PID, verifying the command first.
RUN="$HOME/wyss-lease-lab/runs/$1"
term() { # pid expected-substring
  [ -n "$1" ] && [ -d "/proc/$1" ] || return 0
  case "$(tr '\0' ' ' </proc/$1/cmdline)" in *"$2"*) kill -TERM "$1"; echo "TERM $1 ($2)";; *) echo "SKIP $1: not $2";; esac
}
term "$(cat "$RUN/authority.pid" 2>/dev/null)" "wyss-control"
sleep 1
term "$(cat "$RUN/kwin.real.pid" 2>/dev/null)" "socket wl-lab"
sleep 3
# survivors that carry this run's private HOME (the terminal host outlives its window by design)
for p in $(pgrep -u "$USER" -f "wyss-console"); do
  if tr '\0' '\n' </proc/$p/environ 2>/dev/null | grep -qx "HOME=$RUN/home"; then kill -TERM "$p"; echo "TERM $p (run HOME)"; fi
done
left="$(pgrep -u "$USER" -f "$RUN" | tr '\n' ' ')"; echo "left(by run path): $left"
