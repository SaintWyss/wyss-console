"""Offline tests for screen.py. No provider is launched.
Usage: python3 -I test_screen.py <recorded screen.raw>
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from screen import render  # noqa: E402

results = {}


def case(name, ok, detail=""):
    results[name] = ok
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail else ""))


# Synthetic vectors (positive and negative)
case("cha_preserves_gap", "a   b" in render(b"a\x1b[5Gb").text())
case("cuf_preserves_gap", "a  b" in render(b"a\x1b[2Cb").text())
case("cup_positions", render(b"\x1b[2;3Hx").lines()[1] == "  x")
case("el_erases_tail", render(b"hello\r\x1b[2Ck").lines()[0] == "he" if False else
     render(b"hello\x1b[3G\x1b[K").lines()[0] == "he")
case("cr_overwrites", render(b"abc\rX").lines()[0] == "Xbc")
case("save_restore", render(b"ab\x1b7\x1b[5;1Hzz\x1b8c").lines()[0] == "abc")
case("osc_title_ignored", "title" not in render(b"\x1b]0;title\x07ok").text())
case("sgr_ignored", render(b"\x1b[38;5;231mok\x1b[39m").lines()[0] == "ok")
case("scrollback_kept", "first" in render(b"first" + b"\n" * 45 + b"last", rows=5).text())
case("neg_never_written_absent", "ok from mock" not in render(b"\x1b[3Gok\x1b[6Gfromage").seen_text())
case("history_keeps_erased_row", "gone soon" in render(b"gone soon\r\x1b[K\x1b[2Bnext").seen_text())
case("history_neg_erased_not_in_final", "gone soon" not in render(b"gone soon\r\x1b[K").text())

# The recorded pilot screen
raw = open(sys.argv[1], "rb").read()
old = re.sub(rb"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07]*\x07|\x1b[()][A-Za-z0-9]|\x1b[=>]", b"",
             raw).decode("utf-8", "replace")
scr = render(raw, 120, 40)
new = scr.seen_text()
case("pilot_old_stripper_loses_spaces (documents the defect)",
     "ok from mock" not in old and "okfrommock" in old)
case("pilot_recorded_ok_from_mock_reconstructed", "ok from mock" in new)
case("pilot_prompt_text_reconstructed", "Press Ctrl-C again to exit" in new)
case("pilot_banner_auto_mode", "Auto mode is now Claude Code" in new)
print("ALL_PASS" if all(results.values()) else "SOME_FAIL")
sys.exit(0 if all(results.values()) else 1)
