#!/usr/bin/env bash
# Backup, V7 -> V8 migration and rollback of the Wyss Console protocol-2 store (F8).
#
#   f8-activation.sh backup   [--store DIR] [--into DIR]
#   f8-activation.sh rehearse --backup DIR --authority BIN
#   f8-activation.sh migrate  --backup DIR --authority BIN [--state-root DIR]
#   f8-activation.sh rollback [--prefix DIR]
#
# The protocol-1 store (`control.sqlite3`, schema 7) is only ever READ, through SQLite's online
# backup API: the live authority is not stopped and nothing in the store changes. The protocol-2
# store is a different file (`authority.sqlite3`, schema 8) in a state root of its own, created from a
# COPY of the backup by the authority itself (`--migrate-copy`); it refuses to replace one. So a
# rollback never has to restore anything: the protocol-1 store is where it was. Restoring from the
# backup is a separate, explicit step that needs the protocol-1 authority stopped.
set -euo pipefail

home="${HOME:?HOME is not set}"
state_home="${XDG_STATE_HOME:-$home/.local/state}"
store="$state_home/wyss-console"
p2_state="$state_home/wyss-console-f8"
prefix="$home/.local/opt/wyss-console"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

say() { printf 'f8-activation: %s\n' "$*"; }
die() { printf 'f8-activation: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "$1 is required"; }
need sqlite3
need sha256sum

command_="${1:-}"
[ $# -gt 0 ] && shift
into="" backup="" authority="" state_override=""
while [ $# -gt 0 ]; do
  case "$1" in
    --store) store="${2:?}"; shift 2 ;;
    --into) into="${2:?}"; shift 2 ;;
    --backup) backup="${2:?}"; shift 2 ;;
    --authority) authority="${2:?}"; shift 2 ;;
    --state-root) state_override="${2:?}"; shift 2 ;;
    --prefix) prefix="${2:?}"; shift 2 ;;
    *) die "unknown option $1" ;;
  esac
done
[ -z "$state_override" ] || p2_state="$state_override"

# One line per table: name|rows, for the tables the file has.
counts() {
  local db="$1" t
  for t in $(sqlite3 -readonly "$db" "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name"); do
    printf '%s|%s\n' "$t" "$(sqlite3 -readonly "$db" "SELECT count(*) FROM \"$t\"")"
  done
}
check_image() { # file expected_user_version
  local db="$1" want="$2"
  [ "$(sqlite3 -readonly "$db" 'PRAGMA user_version')" = "$want" ] || die "$db: schema is not $want"
  [ "$(sqlite3 -readonly "$db" 'PRAGMA integrity_check')" = "ok" ] || die "$db: integrity_check failed"
  [ -z "$(sqlite3 -readonly "$db" 'PRAGMA foreign_key_check')" ] || die "$db: foreign_key_check found violations"
}

cmd_backup() {
  local live="$store/control.sqlite3"
  [ -f "$live" ] || die "no protocol-1 store at $live"
  [ -n "$into" ] || into="$state_home/wyss-console-backup-$(date +%Y%m%d-%H%M%S)-f8"
  [ ! -e "$into" ] || die "$into already exists"
  umask 077
  mkdir -m 700 "$into"
  # The online backup API: consistent while the authority writes, and read-only for the live file.
  sqlite3 -readonly "$live" ".timeout 5000" ".backup '$into/control.sqlite3'"
  chmod 600 "$into/control.sqlite3"
  check_image "$into/control.sqlite3" 7
  # What the protocol-1 layout keeps beside the store, so a restore is whole.
  for f in control.anchor layout.json; do [ -f "$store/$f" ] && install -m 600 "$store/$f" "$into/$f"; done
  local live_counts copy_counts
  copy_counts="$(counts "$into/control.sqlite3")"
  live_counts="$(counts "$live")"
  local match=true
  # The live store may have taken events since the copy was made: report, do not fail.
  [ "$copy_counts" = "$live_counts" ] || match=false
  {
    printf '{\n  "file": "control.sqlite3",\n  "sha256": "%s",\n  "schema_user_version": 7,\n' "$(sha256sum "$into/control.sqlite3" | cut -d' ' -f1)"
    printf '  "integrity": "ok",\n  "fk_violations": 0,\n  "row_counts_match_live_at_end": %s,\n  "created_at": "%s",\n  "row_counts": {' "$match" "$(date -u +%FT%TZ)"
    printf '%s' "$copy_counts" | awk -F'|' 'BEGIN{s=""} {printf "%s\n    \"%s\": %s", s, $1, $2; s=","} END{printf "\n  }"}'
    printf '\n}\n'
  } >"$into/MANIFEST.json"
  chmod 600 "$into/MANIFEST.json"
  say "backup written to $into (schema 7, integrity ok, 0 FK violations, live counts match: $match)"
  printf '%s\n' "$into"
}

# Runs the authority once to create the V8 image from a copy of the backup, then stops it.
migrate_into() { # backup authority state_root
  local from="$1" bin="$2" root="$3" scratch copy
  [ -f "$from/control.sqlite3" ] || die "$from has no control.sqlite3"
  [ -x "$bin" ] || die "$bin is not executable"
  check_image "$from/control.sqlite3" 7
  [ ! -e "$root/wyss-console/authority.sqlite3" ] || die "$root already holds a protocol-2 store: it is never replaced"
  # Short, private, throw-away runtime root: socket paths are limited to 108 bytes.
  scratch="$(mktemp -d /tmp/f8m.XXXXXX)"
  chmod 700 "$scratch"
  copy="$scratch/migrate-source.sqlite3"
  sqlite3 -readonly "$from/control.sqlite3" ".backup '$copy'"
  chmod 600 "$copy"
  mkdir -p -m 700 "$root" "$scratch/rt"
  # The authority creates its image and then serves; `timeout` is its parent and ends it (SIGTERM)
  # after a few seconds, so nothing here signals a process by number. 124 = ended by the timeout.
  local image="$root/wyss-console/authority.sqlite3" status=0
  timeout --signal=TERM 8 "$bin" --protocol 2 --runtime-root "$scratch/rt" --state-root "$root" \
    --migrate-copy "$copy" >"$scratch/authority.log" 2>&1 || status=$?
  if [ "$status" -ne 124 ]; then
    cat "$scratch/authority.log" >&2
    die "the authority did not stay up while creating the image (exit $status)"
  fi
  [ -f "$image" ] || die "no image was written"
  check_image "$image" 8
  # Every protocol-1 table keeps its rows; the runtime row moves to its legacy table.
  local want got t name rows
  want="$(counts "$copy")"
  got="$(counts "$image")"
  while IFS='|' read -r name rows; do
    [ -n "$name" ] || continue
    if [ "$name" = managed_runtimes ]; then
      t="managed_runtimes_legacy_v7"
    else
      t="$name"
    fi
    grep -qx "$t|$rows" <<<"$got" || die "table $name: $rows rows in the backup, not kept as $t in the migrated image"
  done <<<"$want"
  say "migrated: schema 8, integrity ok, 0 FK violations, every protocol-1 table kept its rows ($image)"
  # The scratch directory holds a copy of the store: removed file by file, nothing else.
  rm -f "$copy" "$copy-shm" "$copy-wal" "$copy-journal" "$scratch/authority.log"
  find "$scratch/rt" -mindepth 1 -delete 2>/dev/null || true
  rmdir "$scratch/rt" "$scratch"
}

cmd_rehearse() {
  [ -n "$backup" ] && [ -n "$authority" ] || die "rehearse needs --backup and --authority"
  local root
  root="$(mktemp -d /tmp/f8r.XXXXXX)"
  chmod 700 "$root"
  migrate_into "$backup" "$authority" "$root"
  # The rehearsal's image is a copy of the owner's data: it does not outlive the rehearsal.
  find "$root" -mindepth 1 -delete
  rmdir "$root"
  say "rehearsal ok (the throw-away image was removed)"
}

cmd_migrate() {
  [ -n "$backup" ] && [ -n "$authority" ] || die "migrate needs --backup and --authority"
  if systemctl --user is-active --quiet wyss-console-f8.service 2>/dev/null; then
    die "wyss-console-f8.service is running: stop it first"
  fi
  migrate_into "$backup" "$authority" "$p2_state"
}

cmd_rollback() {
  # Protocol 2 goes away; protocol 1 and its store were never touched.
  systemctl --user disable --now wyss-console-f8.service 2>/dev/null || true
  if [ -x "$prefix/share/wyss-console/install.sh" ] && [ -d "$prefix.previous" ]; then
    "$prefix/share/wyss-console/install.sh" rollback --prefix "$prefix"
  else
    say "no previous package to go back to; the launcher still names protocol 2: reinstall without --f8-service"
  fi
  say "protocol-2 store kept in $p2_state (never deleted by a rollback); the protocol-1 store is untouched"
}

case "$command_" in
  backup) cmd_backup ;;
  rehearse) cmd_rehearse ;;
  migrate) cmd_migrate ;;
  rollback) cmd_rollback ;;
  *) die "usage: f8-activation.sh backup|rehearse|migrate|rollback" ;;
esac
