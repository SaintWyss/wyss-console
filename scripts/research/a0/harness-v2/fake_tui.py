"""Stand-in TUI for testing pty_driver without a provider (v2).
FAKE_MODE:
  normal         Ctrl-C arms a cursor-positioned confirmation; 2nd Ctrl-C within WINDOW exits 0
  ignore         never confirms
  burst_exit     on start writes BURST bytes then BURST-END and exits immediately (drain test)
  scroll_confirm on Ctrl-C prints the confirmation then scrolls it off-screen (visible-now test)
  split_confirm  writes the confirmation in tiny fragments, splitting CSI and UTF-8 sequences
  hang           ignores everything (teardown test)
"""
import os
import select
import sys
import time
import tty

WINDOW = float(os.environ.get("FAKE_WINDOW", "0.8"))
MODE = os.environ.get("FAKE_MODE", "normal")
fd = sys.stdin.fileno()
tty.setraw(fd)
out = sys.stdout.buffer


def w(b):
    out.write(b)
    out.flush()


if MODE == "burst_exit":
    w(b"fake tui ready\r\n")
    for i in range(4096):
        w(b"burst-%05d " % i + b"." * 100 + b"\r\n")
    w(b"BURST-END\r\n")
    os._exit(0)
w(b"\x1b[2J\x1b[H fake tui ready\r\n\x1b[3Gok\x1b[6Gfrom\x1b[11Gmock\r\n")
armed = None
while True:
    r, _, _ = select.select([fd], [], [], 0.05)
    now = time.monotonic()
    if armed is not None and now - armed > WINDOW:
        w(b"\x1b[10;1H\x1b[K")
        armed = None
    if not r:
        continue
    b = os.read(fd, 1)
    if MODE in ("ignore", "hang") or b != b"\x03":
        continue
    if armed is not None and now - armed <= WINDOW:
        w(b"\r\nbye\r\n")
        sys.exit(0)
    armed = now
    if MODE == "scroll_confirm":
        w(b"\x1b[40;1HPress Ctrl-C again to exit" + b"\r\n" * 60)
    elif MODE == "split_confirm":
        msg = b"\x1b[10;1H\x1b[2C\xe2\x97\x8f Press\x1b[11GCtrl-C\x1b[18Gagain\x1b[24Gto\x1b[27Gexit"
        for i in range(0, len(msg), 3):
            w(msg[i:i + 3])
            time.sleep(0.005)
    else:
        w(b"\x1b[10;1H\x1b[2CPress\x1b[9GCtrl-C\x1b[16Gagain\x1b[22Gto\x1b[25Gexit")
