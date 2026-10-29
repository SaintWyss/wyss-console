"""Stand-in TUI for testing pty_driver without a provider. Raw mode; Ctrl-C arms a confirmation
shown with cursor moves (like the recorded Claude screen); a second Ctrl-C within WINDOW exits 0;
after WINDOW the line is erased and the arm resets. FAKE_MODE=ignore never shows the prompt.
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
out.write(b"\x1b[2J\x1b[H fake tui ready\r\n\x1b[3Gok\x1b[6Gfrom\x1b[11Gmock\r\n")
out.flush()
armed = None
while True:
    r, _, _ = select.select([fd], [], [], 0.05)
    now = time.monotonic()
    if armed is not None and now - armed > WINDOW:
        out.write(b"\x1b[10;1H\x1b[K")
        out.flush()
        armed = None
    if not r:
        continue
    b = os.read(fd, 1)
    if b != b"\x03" or MODE == "ignore":
        continue
    if armed is not None and now - armed <= WINDOW:
        out.write(b"\r\nbye\r\n")
        out.flush()
        sys.exit(0)
    armed = now
    out.write(b"\x1b[10;1H\x1b[2CPress\x1b[9GCtrl-C\x1b[16Gagain\x1b[22Gto\x1b[25Gexit")
    out.flush()
