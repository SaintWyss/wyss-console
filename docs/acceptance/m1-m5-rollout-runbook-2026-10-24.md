# M1–M5 controlled rollout and rollback — not authorized for execution

**PRODUCTION NO-GO. Source consolidation through PR #27 is separately authorized.** These are future owner actions, not actions performed by the release session. Use the
release artifact summary for the immutable candidate revision, archive and SHA-256. Do not merge,
install, start the GUI against real stores, restart a service or migrate a database on this document's
authority. No lingering or privilege change belongs to this rollout.

## Stage 1: reversible side-by-side package staging

After explicit approval of package installation only, set `archive` to the verified release archive
and `revision` to the exact release HEAD from the approval packet. Run as the ordinary owner:

```bash
archive="$HOME/Github_repos/wyss-console-release/.artifacts/wyss-console-m1-m5-candidate.tar.gz"
revision="$(git -C "$HOME/Github_repos/wyss-console-release" rev-parse HEAD)"
prefix="$HOME/.local/opt/wyss-console-rc-$revision"
cd "$(dirname "$archive")"
sha256sum --check SHA256SUMS
test ! -e "$prefix"
mkdir -m 0755 "$prefix"
tar --extract --gzip --file "$archive" --directory "$prefix" --strip-components=1 --no-same-owner
python3 - "$prefix" <<'PY'
import hashlib,json,pathlib,sys
root=pathlib.Path(sys.argv[1])
m=json.loads((root/'share/wyss-console/package.json').read_text())
for relative,digest in m['files'].items():
    assert hashlib.sha256((root/relative).read_bytes()).hexdigest()==digest, relative
print('package files match manifest')
PY
```

This only adds a new directory. Do not call `install.sh` for this step: its standard integration
rewrites the normal launcher, and its default install/rollback stops the terminal host. The ordinary
launcher, desktop entry and services remain pointed at the old installation. Merely reverting a
launcher is not a store rollback.

## Stage 2: separate GUI/authority activation approval

Before any launch, repeat the read-only service/socket/version/hash inventory and verify active work
has finished voluntarily. Do not terminate a host or an unrelated provider session to make a test pass.

The current GUI observes the authority through `AuthorityBootstrap::observe_only`; absence and
Retry cannot install or start it. `--gui-only` creates private HOME/XDG directories and a private
bus. Final graphical acceptance is still NOT_PROVEN. Launching against installed state remains a
separate owner approval; the following is not an instruction to execute during source integration:

```bash
"$prefix/bin/wyss-console" --f8-authority "$XDG_RUNTIME_DIR/wyss-console-f8"
```

This command may connect to the existing v2 authority; it does not invoke a mutating v1 bootstrap. It does
not authorize an F8 unit rewrite, service restart, capacity increase, V9 migration, real Claude
credential, B1 admission or lingering. Preserve the running host/PTYs until their sessions end; the
new helper cannot be retrofitted into a running old host by copying a GUI. If startup requires a
service replacement outside the approved effects, stop the rollout, not the sessions.

Verification after authorized activation: exact GUI/package digest; both authority hello versions;
Native session start/stop in an approved disposable project; existing session reattach; owner-approved
Codex check; Claude/B1/metrics/model gates remain closed; helper reports every required sandbox layer;
no service beyond the approved set changed. No paid turn is part of a smoke test by default.

## Stage 3: authority/store upgrade — independently blocked

Do not execute this stage under source-integration authorization. V9 writers/replay and the explicit
`wyss-control --prepare-v9-copy SOURCE BACKUP_V8 CANDIDATE_V9` tool now exist. The tool requires three
absolute paths, reads the source and creates new private outputs without publishing a live store.
Synthetic validation does not authorize accessing an installed source or performing a live migration.
The protected credential handoff remains gated and has no operational production broker wiring.

Required future sequence: quiescent authority; no active lease/turn/pending approval/in-flight
operation; explicit downtime approval; sole-writer lock; full V8 SQLite backup with all committed
history; byte digests and schema-8 replay validation; prepare a separate V9 file; validate exact
schema, frozen boundaries, rows and mixed-provider replay; atomically publish only the validated
image; start only the matching qualified authority. Retain the V8 backup and its binary together.
Do not copy just the main SQLite file while WAL-backed work is active.

The current service units and data locations must be re-read when approval occurs; do not infer the
running binary from ExecStart. At inspection the v2 process still executed a pre-M1 binary although
the unit pointed at the current package. Replacing that path does not replace its running process.

## Rollback

Package staging without launch: leave the ordinary launcher alone. The extra directory can remain
inert; deleting it is optional and does not affect the active installation.

GUI activation without schema change: after the candidate window closes voluntarily, start the
previous installed GUI through its existing launcher. Do not use `install.sh rollback` while terminals
are active, because that command stops the host. If the v1 bootstrap replaced an authority, rollback
must separately restore its approved binary/unit after quiescence; the package-only rollback does
not promise to undo that effect. That bootstrap warning applies to historical packages only;
the current GUI is observation-only.

V9: an old binary must never open the V9 image. Preserve the failed/new V9 store and its sidecars for
recovery, restore the validated pre-migration V8 snapshot as a complete store, verify V8 replay, then
start the pinned old authority. Any post-backup V9-only work is outside that snapshot and needs an
explicit owner loss/reconciliation decision. A binary symlink change cannot recover it.

The disposable tests exercised V8 → V9 image preparation, old-validator refusal, V8 restoration and
an actual child crash before rebuild commit. Package/install rehearsal is recorded in the artifact
summary. **Production service rollback and live migration were not executed.**
