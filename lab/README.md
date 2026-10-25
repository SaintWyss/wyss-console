# Lease-hang diagnostic (branch `diag/lease-hang-trace-2026-10-25`, never to be integrated)

Bounded trace (`WYSS_LAB_TRACE=<file>`: event names, 8-char identity prefixes, durations; no prompt,
path or secret), a main-thread heartbeat, a DOM driver (`WYSS_LAB_DRIVER=<js>`) that clicks the page
from inside and reports through `lab_trace`, and a native-dialog answer from inside GTK
(`WYSS_LAB_AUTO_ALLOW_MS`). No OS-level input is ever sent. `WYSS_LAB_WORKSPACE` stands in for the
folder dialog. Scripts: `realprod.sh` (packaged authority + this GUI on the real Wayland session, private
HOME/bus), `real.sh` (in-process fixture authority), `down.sh`/`sweep.sh` (end only processes whose
environment carries the run's private HOME). Build: `npm run build` then
`cargo build --bin wyss-console --features tauri/custom-protocol` (or `--example f8_local_gui --features f8-dev,tauri/custom-protocol`).
