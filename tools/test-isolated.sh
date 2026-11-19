#!/usr/bin/env bash
set -euo pipefail
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd -- "$repo"
scratch="${WYSS_TEST_SCRATCH:-$HOME/.cache/wyss-build}"
mkdir -p -- "$scratch"
sandbox=$(mktemp -d "$scratch/wt.XXXXXX")
trap 'rm -rf -- "$sandbox"' EXIT
mkdir -m 700 -- "$sandbox"/{home,run,state,data,config,cache}

export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$repo/target}"
unset DISPLAY WAYLAND_DISPLAY CLAUDE_CONFIG_DIR CODEX_HOME WYSS_ARSENAL_SYNC
unset XDG_DATA_DIRS XDG_CONFIG_DIRS WYSS_CONSOLE_HOST WYSS_PERSONALIZATION_CHILD WYSS_ISOLATED_SECRET_SERVICE
export HOME="$sandbox/home" XDG_RUNTIME_DIR="$sandbox/run" XDG_STATE_HOME="$sandbox/state"
export XDG_CONFIG_HOME="$sandbox/config" XDG_CACHE_HOME="$sandbox/cache" XDG_DATA_HOME="$sandbox/data"
export TMPDIR="$sandbox" WYSS_TEST_SCRATCH="$sandbox"
export DBUS_SESSION_BUS_ADDRESS="unix:path=$sandbox/no-bus"

cargo test --release --offline --locked --features test-fixtures --no-fail-fast -- "$@"
