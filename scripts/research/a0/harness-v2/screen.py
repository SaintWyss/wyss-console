"""Terminal-screen model for A0 PTY evidence (v2, stdlib only).

v1 defects fixed (independent review 2026-10-29):
- incomplete ANSI/UTF-8 sequences split across feed() calls were mis-parsed -> an incremental
  parser keeps a pending tail and only consumes complete sequences;
- visible() also matched scrollback -> `visible_text()` is the current grid only; `seen_text()`
  is history + scrollback + grid, for audit only;
- ignored terminal operations -> alternate screen (?47/?1047/?1049), scroll region (DECSTBM),
  IL/DL, ICH/DCH, ECH, SU/SD, IND/RI/NEL, deferred autowrap, DECSC/DECRC, CR/LF/BS/TAB.
SGR, other private modes and OSC are consumed without cell effect. Unknown sequences are
counted in `unknown` so a test can detect unsupported input instead of silently passing.
"""
import re
import unicodedata

_CSI = re.compile(rb"\x1b\[([0-9;?<>=]*)([ -/]*)([@-~])")
_CSI_PREFIX = re.compile(rb"\x1b\[[0-9;?<>=]*[ -/]*\Z")


def width(ch):
    if unicodedata.combining(ch):
        return 0
    return 2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1


class Screen:
    def __init__(self, cols=120, rows=40):
        self.cols, self.rows = cols, rows
        self.grid = self._blank()
        self.scrollback = []
        self.history = []
        self.x = self.y = 0
        self.saved = (0, 0)
        self.top, self.bottom = 0, rows - 1
        self.wrap_pending = False
        self.alt = None  # saved main screen while the alternate screen is active
        self.pending = b""
        self.unknown = 0

    # ---- helpers
    def _blank(self):
        return [[" "] * self.cols for _ in range(self.rows)]

    def _row_text(self, r):
        return "".join(self.grid[r]).rstrip()

    def _remember(self, r):
        t = self._row_text(r)
        if t and (not self.history or self.history[-1] != t):
            self.history.append(t)

    def _clamp(self):
        self.x = max(0, min(self.cols - 1, self.x))
        self.y = max(0, min(self.rows - 1, self.y))

    def _scroll_up(self, n=1):
        for _ in range(n):
            self._remember(self.top)
            line = self.grid.pop(self.top)
            if self.top == 0 and self.alt is None:
                self.scrollback.append("".join(line).rstrip())
            self.grid.insert(self.bottom, [" "] * self.cols)

    def _scroll_down(self, n=1):
        for _ in range(n):
            self._remember(self.bottom)
            self.grid.pop(self.bottom)
            self.grid.insert(self.top, [" "] * self.cols)

    def _lf(self):
        self._remember(self.y)
        if self.y == self.bottom:
            self._scroll_up()
        elif self.y < self.rows - 1:
            self.y += 1

    def _put(self, ch):
        w = width(ch)
        if w == 0:
            return
        if self.wrap_pending or self.x + w > self.cols:
            self.x = 0
            self._lf()
            self.wrap_pending = False
        self.grid[self.y][self.x] = ch
        if w == 2 and self.x + 1 < self.cols:
            self.grid[self.y][self.x + 1] = ""
        if self.x + w >= self.cols:
            self.x = self.cols - 1
            self.wrap_pending = True
        else:
            self.x += w

    # ---- CSI
    def _csi(self, params, inter, final):
        y0 = self.y
        if final in "KJ":
            for r in range(self.rows):
                self._remember(r)
        self.wrap_pending = False
        if params[:1] == "?":
            self._private(params[1:], final)
        elif params[:1] in ("<", ">", "="):
            pass  # queries / key-protocol negotiation: no cell effect
        else:
            self._std(params, final)
        if self.y != y0:
            self._remember(y0)

    def _nums(self, params):
        try:
            return [int(p) if p else 0 for p in params.split(";")] if params else []
        except ValueError:
            self.unknown += 1
            return []

    def _private(self, params, final):
        modes = self._nums(params)
        if final in "hl" and any(m in (47, 1047, 1049) for m in modes):
            if final == "h" and self.alt is None:
                self.alt = (self.grid, self.x, self.y, self.saved)
                for r in range(self.rows):
                    self._remember(r)
                self.grid = self._blank()
                self.x = self.y = 0
            elif final == "l" and self.alt is not None:
                for r in range(self.rows):
                    self._remember(r)
                self.grid, self.x, self.y, self.saved = self.alt
                self.alt = None
        # other DEC private modes (cursor visibility, bracketed paste, mouse, sync output…): no cells

    def _std(self, params, final):
        nums = self._nums(params)
        n = nums[0] if nums and nums[0] else 1
        g = self.grid
        if final in "Hf":
            r = nums[0] if len(nums) > 0 and nums[0] else 1
            c = nums[1] if len(nums) > 1 and nums[1] else 1
            self.y, self.x = r - 1, c - 1
        elif final == "A":
            self.y -= n
        elif final == "B" or final == "e":
            self.y += n
        elif final == "C" or final == "a":
            self.x += n
        elif final == "D":
            self.x -= n
        elif final == "E":
            self.y += n
            self.x = 0
        elif final == "F":
            self.y -= n
            self.x = 0
        elif final in "G`":
            self.x = n - 1
        elif final == "d":
            self.y = n - 1
        elif final == "K":
            mode = nums[0] if nums else 0
            rng = {0: range(self.x, self.cols), 1: range(0, self.x + 1), 2: range(self.cols)}.get(mode, ())
            for i in rng:
                if 0 <= i < self.cols:
                    g[self.y][i] = " "
        elif final == "J":
            mode = nums[0] if nums else 0
            if mode in (2, 3):
                self.grid = self._blank()
            elif mode == 0:
                for i in range(self.x, self.cols):
                    g[self.y][i] = " "
                for r in range(self.y + 1, self.rows):
                    g[r] = [" "] * self.cols
            elif mode == 1:
                for i in range(0, min(self.x + 1, self.cols)):
                    g[self.y][i] = " "
                for r in range(0, self.y):
                    g[r] = [" "] * self.cols
        elif final == "X":
            for i in range(self.x, min(self.cols, self.x + n)):
                g[self.y][i] = " "
        elif final == "@":
            row = g[self.y]
            g[self.y] = (row[:self.x] + [" "] * n + row[self.x:])[:self.cols]
        elif final == "P":
            row = g[self.y]
            g[self.y] = row[:self.x] + row[self.x + n:] + [" "] * n
            g[self.y] = g[self.y][:self.cols]
        elif final == "L" and self.top <= self.y <= self.bottom:
            for _ in range(n):
                self._remember(self.bottom)
                g.pop(self.bottom)
                g.insert(self.y, [" "] * self.cols)
        elif final == "M" and self.top <= self.y <= self.bottom:
            for _ in range(n):
                self._remember(self.y)
                g.pop(self.y)
                g.insert(self.bottom, [" "] * self.cols)
        elif final == "S":
            self._scroll_up(n)
        elif final == "T":
            self._scroll_down(n)
        elif final == "r":
            t = nums[0] if len(nums) > 0 and nums[0] else 1
            b = nums[1] if len(nums) > 1 and nums[1] else self.rows
            if 1 <= t < b <= self.rows:
                self.top, self.bottom = t - 1, b - 1
            else:
                self.top, self.bottom = 0, self.rows - 1
            self.x = self.y = 0
        elif final == "s":
            self.saved = (self.x, self.y)
        elif final == "u":
            self.x, self.y = self.saved
        elif final in "mcnthlq":
            pass  # SGR, DA, DSR, window ops, modes, cursor style: no cell effect
        else:
            self.unknown += 1
        self._clamp()

    # ---- input
    def feed(self, chunk: bytes):
        raw = self.pending + chunk
        self.pending = b""
        i, n = 0, len(raw)
        while i < n:
            b = raw[i]
            if b == 0x1B:
                if i + 1 >= n:
                    self.pending = raw[i:]
                    return
                nxt = raw[i + 1]
                if nxt == 0x5B:  # CSI
                    m = _CSI.match(raw, i)
                    if not m:
                        if _CSI_PREFIX.match(raw, i):
                            self.pending = raw[i:]
                            return
                        self.unknown += 1
                        i += 2
                        continue
                    self._csi(m.group(1).decode("latin-1"), m.group(2).decode("latin-1"),
                              m.group(3).decode("latin-1"))
                    i = m.end()
                    continue
                if nxt == 0x5D:  # OSC, terminated by BEL or ST
                    j = i + 2
                    end = None
                    while j < n:
                        if raw[j] == 0x07:
                            end = j + 1
                            break
                        if raw[j] == 0x1B and j + 1 < n and raw[j + 1] == 0x5C:
                            end = j + 2
                            break
                        j += 1
                    if end is None:
                        self.pending = raw[i:]
                        return
                    i = end
                    continue
                if nxt in (0x28, 0x29, 0x2A, 0x2B):  # charset designation ESC ( X
                    if i + 2 >= n:
                        self.pending = raw[i:]
                        return
                    i += 3
                    continue
                self.wrap_pending = False
                if nxt == 0x37:
                    self.saved = (self.x, self.y)
                elif nxt == 0x38:
                    self.x, self.y = self.saved
                elif nxt == 0x44:  # IND
                    self._lf()
                elif nxt == 0x45:  # NEL
                    self.x = 0
                    self._lf()
                elif nxt == 0x4D:  # RI
                    if self.y == self.top:
                        self._scroll_down()
                    else:
                        self.y = max(0, self.y - 1)
                elif nxt in (0x3D, 0x3E, 0x63):  # keypad modes, RIS (treated as no-op)
                    pass
                else:
                    self.unknown += 1
                i += 2
                continue
            if b == 0x0D:
                self.x = 0
                self.wrap_pending = False
            elif b == 0x0A or b == 0x0B or b == 0x0C:
                self.wrap_pending = False
                self._lf()
            elif b == 0x08:
                self.wrap_pending = False
                self.x = max(0, self.x - 1)
            elif b == 0x09:
                self.x = min(self.cols - 1, (self.x // 8 + 1) * 8)
            elif b < 0x20 or b == 0x7F:
                pass
            else:
                need = 1 if b < 0x80 else 2 if b >> 5 == 0b110 else 3 if b >> 4 == 0b1110 else 4 if b >> 3 == 0b11110 else 1
                if i + need > n:
                    self.pending = raw[i:]
                    return
                for c in raw[i:i + need].decode("utf-8", "replace"):
                    self._put(c)
                i += need
                continue
            i += 1

    # ---- views
    def visible_lines(self):
        return ["".join(r).rstrip() for r in self.grid]

    def visible_text(self):
        """Only what is on the screen right now (no scrollback, no history)."""
        return "\n".join(self.visible_lines())

    def lines(self):
        return self.scrollback + self.visible_lines()

    def text(self):
        return "\n".join(self.lines())

    def seen_text(self):
        """Audit view: everything displayed at some point, plus the current screen."""
        return "\n".join(self.history + self.scrollback + self.visible_lines())


def render(raw: bytes, cols=120, rows=40):
    s = Screen(cols, rows)
    s.feed(raw)
    return s
