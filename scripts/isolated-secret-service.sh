#!/bin/sh
# Runs the tests that need a real Secret Service against a private one: a private D-Bus
# session and a throw-away keyring under a temporary XDG_DATA_HOME. The owner's wallet is
# never reached (the session bus address is replaced), and a fictitious key is all it holds.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export WYSS_ISOLATED_SECRET_SERVICE=1
exec dbus-run-session -- sh -c '
  set -eu
  export XDG_DATA_HOME="$1/data" XDG_RUNTIME_DIR="$1/run"
  mkdir -p "$XDG_DATA_HOME" "$XDG_RUNTIME_DIR"; chmod 700 "$XDG_RUNTIME_DIR"
  printf "%s" "throwaway" | gnome-keyring-daemon --unlock --components=secrets >/dev/null
  cd "$2/src-tauri"
  cargo test --locked -- --ignored --test-threads=1 \
    the_real_secret_service_keeps_and_forgets_a_key \
    a_key_goes_through_a_real_secret_service_and_is_lost_with_its_key
' sh "$tmp" "$root"
