#!/usr/bin/env python3
"""Compara wyss-console con otras terminales en un compositor aislado.

Mide, por terminal y en N corridas (mediana):
  arranque   ms desde el spawn hasta que el comando adentro corre
  ram_mb     PSS del árbol de la terminal en reposo (sin el comando de adentro)
  salida_s   segundos que tarda `cat` de un archivo sintético de --mb MiB
  cpu_s      CPU (user+sys) del árbol de la terminal durante esa salida

Corre todo con HOME/XDG descartables en $HOME/.cache/wyss-build/bench y en el
socket Wayland que se le pase (ej. `kwin_wayland --virtual --socket wyss-bench`).
Nunca usa el display ni la config del usuario.
"""
import argparse, json, os, shutil, signal, statistics, subprocess, sys, tempfile, time
from pathlib import Path

SCRATCH = Path.home() / ".cache/wyss-build/bench"
CLK = os.sysconf("SC_CLK_TCK")


def tree(pid):
    kids = {}
    for p in Path("/proc").iterdir():
        if p.name.isdigit():
            try:
                pp = int((p / "stat").read_text().rsplit(")", 1)[1].split()[1])
            except (OSError, IndexError):
                continue
            kids.setdefault(pp, []).append(int(p.name))
    out, todo = [], [pid]
    while todo:
        q = todo.pop()
        out.append(q)
        todo.extend(kids.get(q, []))
    return out


def comm(pid):
    try:
        return Path(f"/proc/{pid}/comm").read_text().strip()
    except OSError:
        return ""


def terminal_pids(root, marker_dir):
    """Árbol de la terminal sin el workload (sh/cat/sleep que lanzamos adentro)."""
    inner = {"sh", "cat", "sleep", "date", "dash", "bash"}
    return [p for p in tree(root) if comm(p) not in inner]


def pss_mb(pids):
    total = 0
    for p in pids:
        try:
            for line in Path(f"/proc/{p}/smaps_rollup").read_text().splitlines():
                if line.startswith("Pss:"):
                    total += int(line.split()[1])
        except OSError:
            pass
    return total / 1024


def cpu_s(pids):
    t = 0
    for p in pids:
        try:
            f = Path(f"/proc/{p}/stat").read_text().rsplit(")", 1)[1].split()
            t += int(f[11]) + int(f[12])
        except (OSError, IndexError):
            pass
    return t / CLK


def wait_file(path, timeout):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        if path.exists() and path.stat().st_size > 0:
            return True
        time.sleep(0.005)
    return False


def argv_for(name, binary, script):
    if name == "wyss-console":
        return [binary], {"WYSS_CONSOLE_CMD": f"sh {script}"}
    if name == "ghostty":
        return [binary, "--config-default-files=false", "--gtk-single-instance=false",
                "--quit-after-last-window-closed=true", "-e", "sh", str(script)], {}
    if name == "konsole":
        return [binary, "--separate", "--nofork", "-e", "sh", str(script)], {}
    if name == "alacritty":
        return [binary, "-e", "sh", str(script)], {}
    raise SystemExit(f"terminal sin receta: {name}")


def run_one(name, binary, data, wayland, idle_s):
    fx = Path(tempfile.mkdtemp(prefix=f"{name}.", dir=SCRATCH))
    for d in ("home", "run", "config", "state", "data", "cache"):
        (fx / d).mkdir(mode=0o700)
    start_m, ready_m, end_m, go = fx / "start", fx / "ready", fx / "end", fx / "go"
    script = fx / "work.sh"
    # date de coreutils da ns; el spawn se toma con el mismo reloj (CLOCK_REALTIME).
    script.write_text(
        f"date +%s%N > {start_m}\n"
        f"while [ ! -e {go} ]; do sleep 0.05; done\n"
        f"date +%s%N > {ready_m}\n"
        f"cat {data}\n"
        f"date +%s%N > {end_m}\n"
        "sleep 600\n")
    env = {k: v for k, v in os.environ.items() if k in ("PATH", "LANG", "LC_ALL", "USER", "LOGNAME")}
    env.update(HOME=str(fx / "home"), XDG_RUNTIME_DIR=os.environ["XDG_RUNTIME_DIR"],
               XDG_CONFIG_HOME=str(fx / "config"), XDG_STATE_HOME=str(fx / "state"),
               XDG_DATA_HOME=str(fx / "data"), XDG_CACHE_HOME=str(fx / "cache"),
               TMPDIR=str(fx), SHELL="/bin/sh", WAYLAND_DISPLAY=wayland,
               GDK_BACKEND="wayland", QT_QPA_PLATFORM="wayland", NO_AT_BRIDGE="1",
               DBUS_SESSION_BUS_ADDRESS=f"unix:path={fx}/no-bus")
    argv, extra = argv_for(name, binary, script)
    env.update(extra)
    t0 = time.time_ns()
    proc = subprocess.Popen(argv, env=env, stdout=subprocess.DEVNULL, stderr=open(fx / "log", "wb"),
                            process_group=0)
    res = {"terminal": name}
    keep = False
    try:
        if not wait_file(start_m, 30):
            res["error"] = "no arrancó el comando en 30 s (ver log)"
            res["log"] = str(fx / "log")
            keep = True
            return res
        res["arranque_ms"] = (int(start_m.read_text()) - t0) / 1e6
        time.sleep(idle_s)
        pids = terminal_pids(proc.pid, fx)
        res["ram_mb"] = pss_mb(pids)
        c0 = cpu_s(pids)
        go.touch()
        if not wait_file(end_m, 300):
            res["error"] = "la salida no terminó en 300 s"
            return res
        # cat termina al escribir en el pty; esperar a que la terminal deje de consumir.
        prev = cpu_s(terminal_pids(proc.pid, fx))
        for _ in range(100):
            time.sleep(0.1)
            now = cpu_s(terminal_pids(proc.pid, fx))
            if now - prev < 0.005:
                break
            prev = now
        res["salida_s"] = (int(end_m.read_text()) - int(ready_m.read_text())) / 1e9
        res["cpu_s"] = cpu_s(terminal_pids(proc.pid, fx)) - c0
        res["ram_post_mb"] = pss_mb(terminal_pids(proc.pid, fx))
        return res
    finally:
        try:
            os.killpg(proc.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            proc.wait(5)
        except subprocess.TimeoutExpired:
            os.killpg(proc.pid, signal.SIGKILL)
        if not keep:
            shutil.rmtree(fx, ignore_errors=True)


def make_data(mb):
    path = SCRATCH / f"salida-{mb}m.txt"
    if path.exists() and path.stat().st_size >= mb << 20:
        return path
    line = "".join(f"\x1b[3{i % 8}mlinea sintetica {i:07d} " + "x" * 40 + "\x1b[0m\n" for i in range(1000))
    with open(path, "w") as f:
        while f.tell() < mb << 20:
            f.write(line)
    return path


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--wayland", required=True, help="socket Wayland aislado (no el del usuario)")
    ap.add_argument("--term", action="append", required=True, metavar="NOMBRE=BINARIO")
    ap.add_argument("--runs", type=int, default=5)
    ap.add_argument("--mb", type=int, default=20)
    ap.add_argument("--idle", type=float, default=3.0)
    ap.add_argument("--json", type=Path)
    a = ap.parse_args()
    if a.wayland == os.environ.get("WAYLAND_DISPLAY"):
        sys.exit("ese es el display del usuario; usá un compositor aislado")
    SCRATCH.mkdir(parents=True, exist_ok=True)
    data = make_data(a.mb)
    runs = []
    for i in range(a.runs):
        for spec in a.term:
            name, binary = spec.split("=", 1)
            r = run_one(name, binary, data, a.wayland, a.idle)
            r["run"] = i
            runs.append(r)
            print(json.dumps(r), file=sys.stderr)
    keys = ["arranque_ms", "ram_mb", "ram_post_mb", "salida_s", "cpu_s"]
    print(f"{'terminal':14}" + "".join(f"{k:>13}" for k in keys) + "   ok")
    for spec in a.term:
        name = spec.split("=", 1)[0]
        rs = [r for r in runs if r["terminal"] == name]
        good = [r for r in rs if "error" not in r]
        cells = "".join(f"{statistics.median(r[k] for r in good):13.3f}" if good else f"{'-':>13}" for k in keys)
        print(f"{name:14}{cells}   {len(good)}/{len(rs)}")
    if a.json:
        a.json.write_text(json.dumps(runs, indent=1))


if __name__ == "__main__":
    main()
