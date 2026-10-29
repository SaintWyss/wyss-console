"""Minimal terminal-screen reconstruction (stdlib only) for A0 PTY evidence.

Replaces the pilot's ANSI *stripper*, which deleted cursor movements and so glued words that
the TUI separates by moving the cursor (recorded: `\\x1b[3Gok\\x1b[6Gfrom\\x1b[11Gmock` ->
"okfrommock"). Here every printable character is placed on a cell grid at the cursor, so
displayed whitespace survives. Supported: CUP/HVP (H/f), CUU/CUD/CUF/CUB (A/B/C/D), CNL/CPL
(E/F), CHA (G), VPA (d), EL (K), ED (J), DECSC/DECRC (ESC 7/8, CSI s/u), CR, LF, BS, TAB,
scrolling with scrollback. SGR, private modes, OSC, charsets, DECSTBM are consumed and ignored.
"""
import re
import unicodedata

CSI = re.compile(rb"\x1b\[([0-9;?<>=]*)([ -/]*)([@-~])")
OSC = re.compile(rb"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)")


def width(ch):
    if unicodedata.combining(ch):
        return 0
    return 2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1


class Screen:
    def __init__(self, cols=120, rows=40):
        self.cols, self.rows = cols, rows
        self.grid = [[" "] * cols for _ in range(rows)]
        self.scrollback = []
        self.x = self.y = 0
        self.saved = (0, 0)
        self.history = []  # every non-empty row text that was displayed, before it changed

    def _remember(self, r):
        t = "".join(self.grid[r]).rstrip()
        if t and (not self.history or self.history[-1] != t):
            self.history.append(t)

    def _scroll(self):
        self.scrollback.append("".join(self.grid.pop(0)).rstrip())
        self.grid.append([" "] * self.cols)

    def _lf(self):
        if self.y == self.rows - 1:
            self._scroll()
        else:
            self.y += 1

    def _put(self, ch):
        w = width(ch)
        if w == 0:
            return
        if self.x + w > self.cols:
            self.x = 0
            self._lf()
        self.grid[self.y][self.x] = ch
        if w == 2 and self.x + 1 < self.cols:
            self.grid[self.y][self.x + 1] = ""
        self.x += w

    def _clamp(self):
        self.x = max(0, min(self.cols - 1, self.x))
        self.y = max(0, min(self.rows - 1, self.y))

    def _csi(self, params, final):
        y0 = self.y
        if final in "KJ":
            for r in range(self.rows):
                self._remember(r)
        self._csi_inner(params, final)
        if self.y != y0:
            self._remember(y0)

    def _csi_inner(self, params, final):
        if params.startswith("?") or params.startswith(">") or params.startswith("<") or params.startswith("="):
            return  # private modes / queries
        nums = [int(p) if p else 0 for p in params.split(";")] if params else []
        n = (nums[0] if nums and nums[0] else 1)
        if final in "Hf":
            r = (nums[0] if len(nums) > 0 and nums[0] else 1)
            c = (nums[1] if len(nums) > 1 and nums[1] else 1)
            self.y, self.x = r - 1, c - 1
        elif final == "A":
            self.y -= n
        elif final == "B":
            self.y += n
        elif final == "C":
            self.x += n
        elif final == "D":
            self.x -= n
        elif final == "E":
            self.y += n
            self.x = 0
        elif final == "F":
            self.y -= n
            self.x = 0
        elif final == "G":
            self.x = n - 1
        elif final == "d":
            self.y = n - 1
        elif final == "K":
            mode = nums[0] if nums else 0
            row = self.grid[self.y]
            rng = {0: range(self.x, self.cols), 1: range(0, self.x + 1), 2: range(self.cols)}.get(mode, ())
            for i in rng:
                if 0 <= i < self.cols:
                    row[i] = " "
        elif final == "J":
            mode = nums[0] if nums else 0
            if mode in (2, 3):
                self.grid = [[" "] * self.cols for _ in range(self.rows)]
            elif mode == 0:
                self._csi_inner("0", "K")
                for r in range(self.y + 1, self.rows):
                    self.grid[r] = [" "] * self.cols
            elif mode == 1:
                self._csi_inner("1", "K")
                for r in range(0, self.y):
                    self.grid[r] = [" "] * self.cols
        elif final == "s":
            self.saved = (self.x, self.y)
        elif final == "u":
            self.x, self.y = self.saved
        # m (SGR), r (DECSTBM), c, h/l and anything else: no cell effect
        self._clamp()

    def feed(self, raw: bytes):
        raw = OSC.sub(b"", raw)
        i, n = 0, len(raw)
        while i < n:
            b = raw[i]
            if b == 0x1B:
                m = CSI.match(raw, i)
                if m:
                    self._csi(m.group(1).decode("latin-1"), m.group(3).decode("latin-1"))
                    i = m.end()
                    continue
                nxt = raw[i + 1:i + 2]
                if nxt == b"7":
                    self.saved = (self.x, self.y)
                elif nxt == b"8":
                    self.x, self.y = self.saved
                elif nxt in (b"(", b")"):
                    i += 3
                    continue
                i += 2
                continue
            if b == 0x0D:
                self.x = 0
            elif b == 0x0A:
                self._remember(self.y)
                self._lf()
            elif b == 0x08:
                self.x = max(0, self.x - 1)
            elif b == 0x09:
                self.x = min(self.cols - 1, (self.x // 8 + 1) * 8)
            elif b < 0x20 or b == 0x7F:
                pass
            else:
                # decode one UTF-8 character
                j = i + 1
                while j < n and j - i < 4 and (raw[j] & 0xC0) == 0x80:
                    j += 1
                ch = raw[i:j].decode("utf-8", "replace")
                for c in ch:
                    self._put(c)
                i = j
                continue
            i += 1

    def lines(self):
        return self.scrollback + ["".join(r).rstrip() for r in self.grid]

    def text(self):
        return "\n".join(self.lines())

    def seen_text(self):
        """Everything that was on screen at some point (history) plus the final screen."""
        return "\n".join(self.history + self.lines())


def render(raw: bytes, cols=120, rows=40):
    s = Screen(cols, rows)
    s.feed(raw)
    return s
