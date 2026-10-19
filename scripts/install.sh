#!/usr/bin/env bash
# Installs, upgrades, rolls back and removes a Wyss Console package for the current user (F13).
#
#   install.sh install   [--package DIR] [--prefix DIR] [--host-service]
#   install.sh rollback  [--prefix DIR]
#   install.sh uninstall [--prefix DIR] [--purge]
#   install.sh status    [--prefix DIR]
#
# A package is the directory `npm run desktop:package` writes (bin/, libexec/, share/). Nothing is
# installed system-wide and nothing needs privileges. The package's own list of SHA-256 digests
# is checked first; the new version is put beside the old one and swapped in with one rename, and
# the old one is kept as `<prefix>.previous` so `rollback` can bring it back. Uninstalling never
# touches the agents' own configuration (~/.claude, ~/.codex), and removes the Console's own state
# only with --purge.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
home="${HOME:?HOME is not set}"
command_="${1:-status}"
[ $# -gt 0 ] && shift
package=""
prefix="$home/.local/opt/wyss-console"
purge=0
service=0
while [ $# -gt 0 ]; do
  case "$1" in
    --package) package="${2:?--package needs a directory}"; shift 2 ;;
    --prefix) prefix="${2:?--prefix needs a directory}"; shift 2 ;;
    --purge) purge=1; shift ;;
    --host-service) service=1; shift ;;
    *) echo "install: unknown option $1" >&2; exit 2 ;;
  esac
done

bin_dir="${WYSS_INSTALL_BIN:-$home/.local/bin}"
apps_dir="${WYSS_INSTALL_APPS:-$home/.local/share/applications}"
icons_dir="${WYSS_INSTALL_ICONS:-$home/.local/share/icons/hicolor/128x128/apps}"
units_dir="${WYSS_INSTALL_UNITS:-$home/.config/systemd/user}"
state_dir="${XDG_STATE_HOME:-$home/.local/state}/wyss-console"
previous="$prefix.previous"
link="$bin_dir/wyss-console"
desktop="$apps_dir/wyss-console.desktop"
icon="$icons_dir/wyss-console.png"
unit="$units_dir/wyss-console-host.service"

say() { printf 'install: %s\n' "$*"; }
die() { printf 'install: %s\n' "$*" >&2; exit 1; }

stop_host() {
  # A running host holds terminals: it is asked to stop (it closes them), never killed.
  if [ -x "$prefix/bin/wyss-console" ]; then
    "$prefix/bin/wyss-console" --wyss-host-stop >/dev/null 2>&1 || true
  fi
}

verify_package() {
  local dir="$1" manifest="$1/share/wyss-console/package.json"
  [ -f "$manifest" ] || die "$dir is not a package: share/wyss-console/package.json is missing"
  # Every file the manifest lists must hash to what it says; one line per file.
  local listing
  listing="$(sed -n 's/^ *"\(bin\/[^"]*\|libexec\/[^"]*\)": *"\([0-9a-f]\{64\}\)".*/\2  \1/p' "$manifest")"
  [ -n "$listing" ] || die "the manifest lists no files"
  (cd "$dir" && printf '%s\n' "$listing" | sha256sum --check --quiet --strict -) \
    || die "a file of the package does not match its digest: nothing was installed"
}

write_integration() {
  mkdir -p "$bin_dir" "$apps_dir" "$icons_dir"
  ln -sfn "$prefix/bin/wyss-console" "$link"
  if [ -f "$prefix/share/wyss-console/icon.png" ]; then
    install -m 0644 "$prefix/share/wyss-console/icon.png" "$icon"
  fi
  cat >"$desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Wyss Console
Comment=Terminals, agents and their configuration
Exec=$prefix/bin/wyss-console
Icon=wyss-console
Terminal=false
Categories=Development;Utility;
StartupWMClass=wyss-console
DESKTOP
  if [ "$service" -eq 1 ]; then
    mkdir -p "$units_dir"
    cat >"$unit" <<UNIT
[Unit]
Description=Wyss Console host (terminals and agent sessions that outlive the window)

[Service]
ExecStart=$prefix/bin/wyss-console --wyss-host
Restart=on-failure
# Exit 2 means a host already runs, or its path is not safe: trying again changes nothing.
RestartPreventExitStatus=2

[Install]
WantedBy=default.target
UNIT
    say "wrote $unit (enable it with: systemctl --user enable --now wyss-console-host.service)"
  fi
}

cmd_install() {
  [ -n "$package" ] || package="$(cd "$here/../.." 2>/dev/null && pwd)"
  verify_package "$package"
  local version
  version="$(sed -n 's/^ *"version": *"\([^"]*\)".*/\1/p' "$package/share/wyss-console/package.json" | head -1)"
  local staging="$prefix.new.$$"
  rm -rf "$staging"
  mkdir -p "$(dirname "$prefix")"
  cp -a "$package" "$staging"
  chmod -R go-w "$staging"
  stop_host
  if [ -d "$prefix" ]; then
    rm -rf "$previous"
    mv "$prefix" "$previous"
  fi
  mv "$staging" "$prefix"
  write_integration
  printf '{\n  "version": "%s",\n  "installed_at": "%s",\n  "previous": %s\n}\n' \
    "$version" "$(date -u +%FT%TZ)" "$([ -d "$previous" ] && echo true || echo false)" \
    >"$prefix/share/wyss-console/install.json"
  say "installed Wyss Console ${version:-?} in $prefix"
  [ -d "$previous" ] && say "the version before it is kept in $previous (install.sh rollback)"
  return 0
}

cmd_rollback() {
  [ -d "$previous" ] || die "there is no previous version to go back to"
  stop_host
  local swap="$prefix.swap.$$"
  mv "$prefix" "$swap"
  mv "$previous" "$prefix"
  mv "$swap" "$previous"
  write_integration
  say "rolled back; the version that was installed is now $previous"
}

cmd_uninstall() {
  stop_host
  rm -f "$link" "$desktop" "$icon" "$unit"
  rm -rf "$prefix" "$previous"
  say "removed the application"
  if [ "$purge" -eq 1 ]; then
    rm -rf "$state_dir"
    say "removed $state_dir (the Console's own state)"
  else
    say "kept $state_dir (use --purge to remove it)"
  fi
  say "the agents' own configuration (~/.claude, ~/.codex) was not touched; a key kept in your Secret Service under 'wyss-console' stays until you delete it there"
}

cmd_status() {
  if [ -f "$prefix/share/wyss-console/install.json" ]; then
    say "installed in $prefix"; cat "$prefix/share/wyss-console/install.json"
  else
    say "not installed in $prefix"
  fi
  [ -d "$previous" ] && say "previous version kept in $previous"
  [ -L "$link" ] && say "launcher: $link -> $(readlink "$link")"
  if [ -x "$prefix/bin/wyss-console" ]; then
    "$prefix/bin/wyss-console" --diagnose || true
  fi
}

case "$command_" in
  install) cmd_install ;;
  rollback) cmd_rollback ;;
  uninstall) cmd_uninstall ;;
  status) cmd_status ;;
  *) die "unknown command $command_" ;;
esac
