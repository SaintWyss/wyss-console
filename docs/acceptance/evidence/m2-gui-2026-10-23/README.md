# M2 isolated Tauri GUI evidence — 2026-10-23

These runs use the debug Tauri application with a private virtual display, private
D-Bus, disposable HOME/XDG roots, a fixture-only `claude`/`codex` executable pair,
and a disposable GNOME Secret Service. Each invocation creates and removes its own
temporary root; accepted run records show `leftovers: []`. The fixture CLIs report
`--version` and otherwise print a marker and wait for input. No real provider turn,
global configuration, credential, installed host, or authority is used.

The fixture Secret Service is probed with one marker round trip so template Undo uses
the encrypted restoration path. The exported `.wyss-template` stays inside the
temporary HOME and only its byte count and SHA-256 are retained as evidence.

## Reproduction

From the repository root, start the frontend development server and build the debug
desktop executable:

```sh
npm run dev -- --host 127.0.0.1 --port 1420
cargo build --locked --manifest-path src-tauri/Cargo.toml --bin wyss-console
```

With Vite available at `127.0.0.1:1420`, each command below runs a fresh isolated
desktop scenario. `--shape none` creates no project terminals; Native launch uses the
fixture provider executable and the harness's private HostLink.

```sh
python3 -I benchmarks/m1_gui.py --binary src-tauri/target/debug/wyss-console --shape none --run 5 --out docs/acceptance/evidence/m2-gui-2026-10-23/run --steps docs/acceptance/evidence/m2-gui-2026-10-23/native-preview-start-steps.json --m2-fixtures
python3 -I benchmarks/m1_gui.py --binary src-tauri/target/debug/wyss-console --shape none --run 9 --out docs/acceptance/evidence/m2-gui-2026-10-23/run --steps docs/acceptance/evidence/m2-gui-2026-10-23/native-restart-steps.json --m2-fixtures
python3 -I benchmarks/m1_gui.py --binary src-tauri/target/debug/wyss-console --shape none --run 13 --out docs/acceptance/evidence/m2-gui-2026-10-23/run --steps docs/acceptance/evidence/m2-gui-2026-10-23/config-edit-cancel-steps.json --m2-fixtures
python3 -I benchmarks/m1_gui.py --binary src-tauri/target/debug/wyss-console --shape none --run 15 --out docs/acceptance/evidence/m2-gui-2026-10-23/run --steps docs/acceptance/evidence/m2-gui-2026-10-23/config-edit-apply-steps.json --m2-fixtures
python3 -I benchmarks/m1_gui.py --binary src-tauri/target/debug/wyss-console --shape none --run 19 --out docs/acceptance/evidence/m2-gui-2026-10-23/run --steps docs/acceptance/evidence/m2-gui-2026-10-23/config-edit-undo-apply-steps.json --m2-fixtures
python3 -I benchmarks/m1_gui.py --binary src-tauri/target/debug/wyss-console --shape none --run 27 --out docs/acceptance/evidence/m2-gui-2026-10-23/run --steps docs/acceptance/evidence/m2-gui-2026-10-23/config-template-export-steps.json --m2-fixtures
python3 -I benchmarks/m1_gui.py --binary src-tauri/target/debug/wyss-console --shape none --run 31 --out docs/acceptance/evidence/m2-gui-2026-10-23/run --steps docs/acceptance/evidence/m2-gui-2026-10-23/config-template-import-undo-steps.json --m2-fixtures
```

## Results

- Run 5: Native Claude fixture preview showed `2.1.309 (Claude Code)` and the
  selected `Review, read only` profile. Escape cancelled the project-folder chooser;
  Start launched only the fixture. See [preview](run/cal5-01-profile-preview.png),
  [cancel](run/cal5-02-folder-cancel.png), [started](run/cal5-03-started.png), and
  [Native state](run/native-5-started.json).
- Run 9: Restart left the first session `stopped`, created a new `running` session
  with `restart_of` pointing to the old session and `restarts: 1`. See
  [restart result](run/cal9-06-after-restart.png) and
  [Native state](run/native-9-after-restart.json).
- Run 13: Cancelling the authoring plan left the fixture instruction file at its
  initial 91-byte SHA-256. Run 15 changed it only after the native confirmation; run
  19 Undo restored the exact initial bytes. Hash records are in `run/fixture-13-*`,
  `run/fixture-15-*`, and `run/fixture-19-*`.
- Run 27: Exported the sealed instruction template through the native passphrase
  dialog and file chooser. The temporary encrypted file measured 543 bytes; see
  [export result](run/cal27-27-template-exported.png) and
  [metadata](run/template-31-exported.json).
- Run 31: Reviewed/imported the sealed template, confirmed its normal plan in the
  native confirmation dialog, and then used History → Undo with a second native
  confirmation. The document changed from 31 bytes to the original 91 bytes on
  import, then Undo restored the exact 31-byte pre-import version. See the
  [review](run/cal31-31-import-review-dialog.png),
  [plan](run/cal31-31-import-plan.png),
  [history](run/cal31-31-template-history.png),
  [Undo plan](run/cal31-31-template-undo-plan.png),
  [completed Undo](run/cal31-31-template-undo-restored.png), and SHA-256 records in
  `run/fixture-31-*`.

The workflow proves fixture behavior only. Installed Claude/Codex versions and
supported CLI help were checked read-only; provider-resolved or effective values,
entitlement, reload, and real-provider turns remain **NOT_RUN**.
