#!/usr/bin/env python3
"""Records the reference-screen corpus for M4: real programs (and scripted sequences) run inside tmux
on a private socket, with the raw bytes they write captured by `pipe-pane -O`, and tmux's own
interpretation of those exact bytes captured as the reference (text, attributes, cursor, modes).

    python3 -I benchmarks/screen_corpus.py --out src-tauri/tests/fixtures/screen [--only name ...]

The Rust test `tests/screen_corpus.rs` replays every `<name>.bin` through the Wyss screen engine and
compares it with `<name>.ref.json` and `<name>.ref.sgr`. tmux is a second, independent
implementation of the terminal state machine; where they disagree, one of them is wrong.

Nothing here touches the owner's tmux: a private `-L` socket, a private HOME and a private temp
directory; the programs run are read-only demonstrations. Every started process is ended through
its own handle (the tmux server by its socket, the stopped program by the pid it wrote).
"""
import argparse
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time

ROWS, COLS = 24, 80
FORMAT = ("#{cursor_x} #{cursor_y} #{alternate_on} #{cursor_flag} #{origin_flag} #{wrap_flag} "
          "#{insert_flag} #{keypad_cursor_flag} #{keypad_flag} #{mouse_any_flag} #{mouse_button_flag} "
          "#{mouse_standard_flag} #{mouse_sgr_flag} #{mouse_utf8_flag} #{scroll_region_upper} "
          "#{scroll_region_lower} #{history_size} #{pane_width} #{pane_height}")
FIELDS = ["cursor_x", "cursor_y", "alternate_on", "cursor_visible", "origin", "wrap", "insert",
          "keypad_cursor", "keypad", "mouse_any", "mouse_button", "mouse_standard", "mouse_sgr",
          "mouse_utf8", "region_upper", "region_lower", "history", "width", "height"]

SAMPLE_PY = '''#!/usr/bin/env python3
"""A small module, for the editor to show."""
import json


class Greeter:
    def __init__(self, name: str = "mundo") -> None:
        self.name = name

    def greet(self) -> str:
        # naïve greeting with accents: ñandú, café, 世界
        return f"hola, {self.name}!"


def main():
    for i in range(3):
        print(Greeter(str(i)).greet())
    print(json.dumps({"ok": True, "n": 3}))


if __name__ == "__main__":
    main()
'''

TEXT_UTF8 = "".join(
    f"{n:03d}  The quick brown fox — ñandú, café, naïve, 世界, 한국어, 日本語 😀  {'x' * (n % 40)}\n"
    for n in range(1, 120))

SCRIPTS = {
    "sgr-palette": r"""
printf '\033[2J\033[H'
for i in $(seq 0 15); do printf '\033[48;5;%dm  \033[0m' $i; done; printf '\n'
for i in $(seq 16 231); do printf '\033[48;5;%dm \033[0m' $i; [ $(( (i-15) % 36 )) -eq 0 ] && printf '\n'; done
for i in $(seq 232 255); do printf '\033[48;5;%dm \033[0m' $i; done; printf '\n'
for r in 0 64 128 192 255; do for g in 0 128 255; do printf '\033[48;2;%d;%d;0m \033[0m' $r $g; done; done; printf '\n'
printf '\033[1mbold\033[0m \033[2mdim\033[0m \033[3mitalic\033[0m \033[4munder\033[0m \033[5mblink\033[0m \033[7minverse\033[0m \033[9mstrike\033[0m\n'
printf '\033[1;31;47mbold red on white\033[0m \033[38;5;208morange\033[0m \033[38;2;10;200;90mtruecolor\033[0m \033[91;104mbright\033[0m\n'
printf '\033[38:2::200:100:50mcolon-form\033[0m \033[4:3mcurly\033[0m done\n'
""",
    "unicode-wide": r"""
printf '\033[2J\033[H'
printf 'ascii | ñandú café naïve | 世界你好 | 한국어 | 日本語かな | ｆｕｌｌ\n'
printf 'combining: e\xcc\x81 a\xcc\x88 n\xcc\x83 and a lone mark\xcc\x81 here\n'
printf 'emoji: 😀😃🎉 end\n'
printf 'box: ┌──┬──┐\n     │世│界│\n     └──┴──┘\n'
printf '\033(0lqqqwqqqk\n\033(0x   x   x\n\033(0mqqqvqqqj\033(B  line-drawing charset\n'
printf '%s\n' "$(printf 'x%.0s' $(seq 1 79))世" "$(printf 'y%.0s' $(seq 1 78))世界"
printf 'wrap at the edge: %s\n' "$(printf '界%.0s' $(seq 1 45))"
""",
    "scroll-regions": r"""
printf '\033[2J\033[H'
for i in $(seq 1 24); do printf '\033[%d;1Hrow %02d ......' $i $i; done
printf '\033[5;15r\033[15;1H\n\n\n'
printf '\033[5;1H\033M\033M'
printf '\033[8;1H\033[2L\033[10;1H\033[3M'
printf '\033[r\033[24;1Hafter reset'
printf '\033[2;3H\033[2S\033[1T'
""",
    "alt-screen-roundtrip": r"""
printf 'normal line one\nnormal line two\n'
printf '\0337\033[?1049h\033[2J\033[Halternate screen\033[5;10Hmiddle\033[?25l'
sleep 0.2
printf '\033[?1049l\0338back on normal\n'
printf '\033[?1049h\033[1;1Hback in alt \033[?25h'
""",
    "tabs-wrap-edit": r"""
printf '\033[2J\033[H'
printf 'a       b       c       d\n\r1       2       3\n'
printf '\033[?7l%s\n\033[?7h' "$(printf 'W%.0s' $(seq 1 100))"
printf '%s\n' "$(printf 'V%.0s' $(seq 1 100))"
printf 'abcdef\033[3D\033[2@XY\033[2P\033[2X\n'
printf 'ab\033[4hINSERT\033[4l end\n'
printf 'rep: x\033[5b\n'
printf '\033[1;7H\033[1K\033[3;1H\033[0J'
""",
    "cursor-save-origin": r"""
printf '\033[2J\033[H'
printf '\033[3;10H\0337\033[10;40Hfar\0338saved'
printf '\033[5;12r\033[?6h\033[1;1Horigin top\033[20;1Hclamped\033[?6l\033[1;1Habsolute'
printf '\033[r\033[?6l\033[20;1H\033[31m\0337\033[0m\033[18;1H\0338red again\033[0m'
""",
    "mouse-and-modes": r"""
printf '\033[?1000h\033[?1002h\033[?1006h\033[?1004h\033[?2004h\033[?1h\033=\033[?25l\033[2 q'
printf 'modes set\n'
printf '\033[?1002l\033[?1000l'
""",
    "hostile-output": r"""
printf '\033[2J\033[H'
printf 'start\n'
printf '\033]52;c;aGVsbG8=\007\033]8;;http://example.invalid/\007link\033]8;;\007\n'
printf '\033[8;5;20t\033[c\033[6n\033[>c\n'
printf '\033_Gf=24,s=1,v=1;AAAA\033\\\033^private\033\\\033Xsos\033\\\n'
printf '\033]0;a title with \001 control\007\033]2;second\033\\\n'
printf 'after hostile output\n'
""",
    "bash-session": None,
    "python-curses": None,
    "vim-edit": None,
    "htop": None,
    "less-utf8": None,
    "man-page": None,
    "tmux-nested": None,
    "top": None,
    "ls-color": None,
}


def tmux(sock, conf, *args, check=True, capture=True):
    cmd = ["tmux", "-L", sock, "-f", conf, *args]
    out = subprocess.run(cmd, capture_output=capture, text=True, check=False)
    if check and out.returncode != 0:
        raise SystemExit(f"tmux {' '.join(args)} failed: {out.stderr}")
    return out.stdout


def keys(sock, conf, *items, gap=0.25):
    for item in items:
        if isinstance(item, (int, float)):
            time.sleep(item)
        elif item.startswith("lit:"):
            tmux(sock, conf, "send-keys", "-t", "c", "-l", item[4:])
            time.sleep(gap)
        else:
            tmux(sock, conf, "send-keys", "-t", "c", item)
            time.sleep(gap)


INTERACTIVE = {
    "vim-edit": dict(
        cmd="vim -u NONE -N -c 'syntax on' -c 'set mouse=a' {work}/sample.py",
        script=["lit:i", "lit:# typed in vim: ñandú 世界", "Escape", ":", "lit:vsplit", "Enter",
                ":", "lit:set number", "Enter", "lit:/greet", "Enter", 0.4],
        stop=True),
    # Only processes this script started are shown: the machine's own process list is not recorded.
    "htop": dict(cmd="htop -d 5 -p $WATCH", script=[1.2, "lit:t", 0.8, "F2", 0.6, "Escape", 0.6], stop=True,
                 pre="sleep 600 & sleep 600 & WATCH=$(jobs -p | paste -sd, -)"),
    "less-utf8": dict(cmd="less -R {work}/text.txt", script=["Space", "Space", "lit:/世界", "Enter", 0.4,
                                                              "lit:G", 0.3], stop=False),
    "man-page": dict(cmd="env MANPAGER='less -R' man ls", script=[0.8, "Space", 0.4], stop=False),
    "tmux-nested": dict(
        cmd="tmux -L inner-{pid} -f {work}/inner.conf new-session -x 78 -y 22 'printf \"inner pane: \\033[1;32mgreen\\033[0m\\n\"; "
            "sleep 30'", script=[1.0, "C-b", "lit:%", 0.6], stop=True),
    "top": dict(cmd="top -d 1 -p $WATCH", script=[1.6, "lit:1", 0.6], stop=True,
                pre="sleep 600 & sleep 600 & WATCH=$(jobs -p | paste -sd, -)"),
    "python-curses": dict(
        cmd="python3 {work}/curses_demo.py", script=[0.8], stop=True),
    "bash-session": dict(
        cmd="env -i HOME={work} TERM=xterm-256color PATH=/usr/bin:/bin 'PS1=\\[\\033[1;32m\\]user@host\\[\\033[0m\\]:\\w\\$ ' bash --norc -i",
        script=[0.5, "lit:echo héllo 世界 😀", "Enter", "lit:ls --color=always /usr | head -5", "Enter",
                "lit:seq 1 40", "Enter", "lit:printf '\\033[31mred\\033[0m\\n'", "Enter", 0.5],
        stop=False),
    "ls-color": dict(cmd="ls -la --color=always /usr/bin | head -60", script=[0.6], stop=False, keep=True),
}

CURSES_DEMO = '''import curses
def main(s):
    curses.curs_set(0)
    curses.start_color(); curses.use_default_colors()
    for i in range(1, 8): curses.init_pair(i, i, -1)
    s.border()
    s.addstr(1, 2, "curses demo", curses.A_BOLD | curses.color_pair(2))
    s.addstr(3, 2, "reverse", curses.A_REVERSE)
    s.addstr(4, 2, "underline", curses.A_UNDERLINE)
    s.addstr(5, 2, "ñandú 世界", curses.color_pair(5))
    w = curses.newwin(6, 30, 8, 10); w.box(); w.addstr(1, 1, "a sub window"); w.refresh()
    s.refresh()
    s.getch()
curses.wrapper(main)
'''


def run_entry(name, work, out_dir):
    sock = f"wyss-corpus-{os.getpid()}"
    conf = os.path.join(work, "tmux.conf")
    with open(conf, "w") as f:
        f.write("set -g default-terminal xterm-256color\nset -g status off\n"
                "set -g escape-time 0\nset -g history-limit 2000\nset -g allow-passthrough off\n"
                "set -g set-clipboard off\n")
    pidfile = os.path.join(work, f"{name}.pid")
    stream = os.path.join(work, f"{name}.bin")
    open(stream, "wb").close()
    spec = INTERACTIVE.get(name)
    if spec:
        body = spec["cmd"].format(work=work, pid=os.getpid())
        script = spec["script"]
        stop = spec["stop"]
    else:
        path = os.path.join(work, f"{name}.sh")
        with open(path, "w") as f:
            f.write(SCRIPTS[name])
        body = f"sh {path}"
        script, stop = [0.8], False
        spec = {"keep": True}
    runner = os.path.join(work, f"{name}.run.sh")
    with open(runner, "w") as f:
        # A command that ends would leave a dead pane (and tmux's own message in it): it is held.
        tail = f"{body}\nsleep 60\n" if spec and spec.get("keep") else f"exec {body}\n"
        pre = (spec or {}).get("pre", "")
        f.write(f"echo $$ > {pidfile}\n{pre}\nsleep 0.8\n{tail}")
    env_cmd = f"sh {runner}"
    tmux(sock, conf, "new-session", "-d", "-c", work, "-x", str(COLS), "-y", str(ROWS), "-s", "c", env_cmd)
    tmux(sock, conf, "pipe-pane", "-O", "-t", "c", f"cat >> {stream}")
    time.sleep(1.2)
    keys(sock, conf, *script)
    time.sleep(0.5)
    pid = None
    try:
        pid = int(open(pidfile).read().strip())
    except (OSError, ValueError):
        pass
    if stop and pid:
        try:
            os.kill(pid, signal.SIGSTOP)
        except ProcessLookupError:
            pass
        time.sleep(0.4)
    tmux(sock, conf, "pipe-pane", "-t", "c", check=False)
    time.sleep(0.2)
    text = tmux(sock, conf, "capture-pane", "-p", "-t", "c")
    sgr = tmux(sock, conf, "capture-pane", "-p", "-e", "-N", "-t", "c")
    facts = tmux(sock, conf, "display-message", "-p", "-t", "c", FORMAT).split()
    tmux(sock, conf, "kill-server", check=False)
    # The nested entry started a second server on a socket of its own: ended through that socket.
    subprocess.run(["tmux", "-L", f"inner-{os.getpid()}", "kill-server"], capture_output=True)
    if pid:
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    data = open(stream, "rb").read()
    meta = dict(zip(FIELDS, [int(x) for x in facts]))
    meta.update(name=name, rows=ROWS, cols=COLS, bytes=len(data),
                text=text.rstrip("\n").split("\n") if text.strip("\n") else [])
    # tmux prints exactly pane_height lines; keep them aligned with the screen rows.
    lines = text.split("\n")
    meta["text"] = (lines + [""] * ROWS)[:ROWS]
    with open(os.path.join(out_dir, f"{name}.bin"), "wb") as f:
        f.write(data)
    with open(os.path.join(out_dir, f"{name}.ref.json"), "w") as f:
        json.dump(meta, f, ensure_ascii=False, indent=1)
        f.write("\n")
    with open(os.path.join(out_dir, f"{name}.ref.sgr"), "w") as f:
        f.write(sgr)
    print(f"{name}: {len(data)} bytes, alt={meta['alternate_on']} cursor=({meta['cursor_x']},{meta['cursor_y']})")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--only", nargs="*")
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    work = tempfile.mkdtemp(prefix="wyss-corpus-")
    try:
        with open(os.path.join(work, "sample.py"), "w") as f:
            f.write(SAMPLE_PY)
        with open(os.path.join(work, "text.txt"), "w") as f:
            f.write(TEXT_UTF8)
        with open(os.path.join(work, "curses_demo.py"), "w") as f:
            f.write(CURSES_DEMO)
        with open(os.path.join(work, "inner.conf"), "w") as f:
            f.write("set -g status off\nset -g default-shell /bin/sh\nset -g default-command \"env -i PS1='$ ' TERM=xterm-256color /bin/sh\"\n")
        for name in SCRIPTS:
            if args.only and name not in args.only:
                continue
            run_entry(name, work, args.out)
    finally:
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
