"""Offline tests for harness-v2/screen.py, with negative controls against frozen harness/screen.py.
No provider is launched. Usage: python3 -I test_screen_v2.py <A0_ROOT> <out.json>"""
import importlib.util
import json
import os
import random
import sys

ROOT = sys.argv[1]
sys.path.insert(0, os.path.join(ROOT, "harness-v2"))
from screen import Screen, render  # noqa: E402

spec = importlib.util.spec_from_file_location("screen_v1", os.path.join(ROOT, "harness", "screen.py"))
v1 = importlib.util.module_from_spec(spec)
spec.loader.exec_module(v1)
results = {}


def case(name, ok, detail=""):
    results[name] = {"pass": bool(ok), "detail": detail}
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail else ""))


def feed_chunks(mod, raw, sizes):
    s = mod.Screen(120, 40)
    i = 0
    for n in sizes:
        s.feed(raw[i:i + n])
        i += n
    s.feed(raw[i:])
    return s


# --- v1 vectors still hold
case("cha_preserves_gap", "a   b" in render(b"a\x1b[5Gb").visible_text())
case("cuf_preserves_gap", "a  b" in render(b"a\x1b[2Cb").visible_text())
case("cup_positions", render(b"\x1b[2;3Hx").visible_lines()[1] == "  x")
case("el_erases_tail", render(b"hello\x1b[3G\x1b[K").visible_lines()[0] == "he")
case("cr_overwrites", render(b"abc\rX").visible_lines()[0] == "Xbc")
case("save_restore", render(b"ab\x1b7\x1b[5;1Hzz\x1b8c").visible_lines()[0] == "abc")
case("osc_title_ignored", "title" not in render(b"\x1b]0;title\x07ok").seen_text())
case("osc_st_terminator", render(b"\x1b]0;t\x1b\\ok").visible_lines()[0] == "ok")
case("sgr_ignored", render(b"\x1b[38;5;231mok\x1b[39m").visible_lines()[0] == "ok")
case("neg_never_written_absent", "ok from mock" not in render(b"\x1b[3Gok\x1b[6Gfromage").seen_text())

# --- fragmentation (defect: incomplete sequences across feed calls)
for label, path in (("pilot", "evidence/A1-I/markers/screen.raw"),
                    ("retest", "evidence/A1-I-retest/markers/screen.raw")):
    raw = open(os.path.join(ROOT, path), "rb").read()
    whole = render(raw)
    ref = (whole.visible_text(), whole.seen_text())
    bytewise = feed_chunks(sys.modules["screen"], raw, [1] * len(raw))
    ok_all = (bytewise.visible_text(), bytewise.seen_text()) == ref
    rnd = random.Random(1029)
    for seed in range(200):
        sizes = [rnd.randint(1, 9) for _ in range(len(raw) // 2)]
        s = feed_chunks(sys.modules["screen"], raw, sizes)
        if (s.visible_text(), s.seen_text()) != ref:
            ok_all = False
            break
    case(f"v2_fragmentation_invariant_{label}_bytewise_and_200_random", ok_all)
    old_whole = v1.render(raw).text()
    old_bytes = feed_chunks(v1, raw, [1] * len(raw)).text()
    case(f"NEGCTL_v1_breaks_under_fragmentation_{label}", old_bytes != old_whole,
         "v1 differs when fed byte-by-byte (defect reproduced)")
case("pilot_ok_from_mock", "ok from mock" in render(open(os.path.join(ROOT, "evidence/A1-I/markers/screen.raw"), "rb").read()).seen_text())

# split UTF-8 and split CSI
s = Screen()
for part in (b"\x1b", b"[3", b"G", b"\xe2", b"\x97", b"\x8f", b" ok"):
    s.feed(part)
case("split_csi_and_utf8", s.visible_lines()[0] == "  ● ok", repr(s.visible_lines()[0]))
o = v1.Screen()
for part in (b"\x1b", b"[3", b"G", b"\xe2", b"\x97", b"\x8f", b" ok"):
    o.feed(part)
case("NEGCTL_v1_split_csi_and_utf8_wrong", o.lines()[0] != "  ● ok", repr(o.lines()[0]))

# visible now vs scrollback (defect: visible() matched scrollback)
s = render(b"CONFIRM-TEXT" + b"\r\n" * 60, rows=10)
case("scrolled_text_not_visible", "CONFIRM-TEXT" not in s.visible_text())
case("scrolled_text_in_audit_view", "CONFIRM-TEXT" in s.seen_text())
case("NEGCTL_v1_text_includes_scrollback", "CONFIRM-TEXT" in v1.render(b"CONFIRM-TEXT" + b"\r\n" * 60, rows=10).text())

# alternate screen
s = render(b"MAIN\x1b[?1049hALT-ONLY\x1b[?1049l")
case("alt_screen_restores_main", s.visible_lines()[0] == "MAIN" and "ALT-ONLY" not in s.visible_text())
case("alt_screen_content_audited", "ALT-ONLY" in s.seen_text())
# editing ops
case("ich_inserts", render(b"abcd\x1b[2G\x1b[2@").visible_lines()[0] == "a  bcd")
case("dch_deletes", render(b"abcd\x1b[2G\x1b[2P").visible_lines()[0] == "ad")
case("ech_erases", render(b"abcd\x1b[2G\x1b[2X").visible_lines()[0] == "a  d")
s = render(b"1\r\n2\r\n3\x1b[2;1H\x1b[L", rows=5)
case("il_inserts_line", s.visible_lines()[:4] == ["1", "", "2", "3"], s.visible_lines()[:4])
s = render(b"1\r\n2\r\n3\x1b[2;1H\x1b[M", rows=5)
case("dl_deletes_line", s.visible_lines()[:3] == ["1", "3", ""], s.visible_lines()[:3])
s = render(b"\x1b[2;3rA\r\nB\r\nC\r\nD", rows=5)
case("scroll_region_confines_scroll", s.visible_lines()[:4] == ["A", "C", "D", ""], s.visible_lines())
case("autowrap_deferred", render(b"x" * 120 + b"y", cols=120).visible_lines()[:2] == ["x" * 120, "y"])
s = render(b"\x1b[999z")
case("unknown_sequence_counted", s.unknown >= 1, s.unknown)

json.dump(results, open(sys.argv[2], "w"), indent=1)
print("ALL_PASS" if all(r["pass"] for r in results.values()) else "SOME_FAIL")
sys.exit(0 if all(r["pass"] for r in results.values()) else 1)
