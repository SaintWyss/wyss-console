"""Offline tests for harness-v2/overlay_validator.py: case table, structured fuzz (never raises,
never accepts anything outside the strict grammar), with negative controls against frozen v1.
Usage: test_validator_v2.py <A0_ROOT> <out.json>"""
import importlib.util
import json
import os
import random
import sys

ROOT = sys.argv[1]
sys.path.insert(0, os.path.join(ROOT, "harness-v2"))
from overlay_validator import STATES, validate  # noqa: E402

s1 = importlib.util.spec_from_file_location("ov1", os.path.join(ROOT, "harness", "overlay_validator.py"))
v1 = importlib.util.module_from_spec(s1)
s1.loader.exec_module(v1)
INV = {"a0-alpha", "a0-beta", "a0-gamma", "a0-proj", "a0-hidden", "pkg.skill"}
results = {}


def case(name, ok, detail=""):
    results[name] = {"pass": bool(ok), "detail": detail}
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail else ""))


ACCEPT = [b'{"skillOverrides":{"a0-beta":"off"}}', b'{"skillOverrides":{"a0-alpha":"name-only","a0-proj":"off"}}',
          b'{"skillOverrides":{"pkg.skill":"user-invocable-only"}}', b'{"skillOverrides":{"a0-beta":"on"}}']
REJECT = {
    "list_value": b'{"skillOverrides":{"a0-beta":["off"]}}',
    "object_value": b'{"skillOverrides":{"a0-beta":{"v":"off"}}}',
    "number_value": b'{"skillOverrides":{"a0-beta":1}}',
    "null_value": b'{"skillOverrides":{"a0-beta":null}}',
    "bool_value": b'{"skillOverrides":{"a0-beta":true}}',
    "invalid_state": b'{"skillOverrides":{"a0-beta":"bogus"}}',
    "case_variant_state": b'{"skillOverrides":{"a0-beta":"OFF"}}',
    "unknown_name": b'{"skillOverrides":{"a0-betta":"off"}}',
    "case_variant_name": b'{"skillOverrides":{"A0-BETA":"off"}}',
    "bad_name_shape": b'{"skillOverrides":{"a0 beta":"off"}}',
    "misspelled_key": b'{"skillOverride":{"a0-beta":"off"}}',
    "extra_hooks": b'{"skillOverrides":{"a0-beta":"off"},"hooks":{}}',
    "disableAllHooks": b'{"skillOverrides":{"a0-beta":"off"},"disableAllHooks":true}',
    "permissions": b'{"skillOverrides":{"a0-beta":"off"},"permissions":{"defaultMode":"bypassPermissions"}}',
    "statusLine_cmd": b'{"skillOverrides":{"a0-beta":"off"},"statusLine":{"type":"command","command":"x"}}',
    "apiKeyHelper": b'{"skillOverrides":{"a0-beta":"off"},"apiKeyHelper":"x"}',
    "env": b'{"skillOverrides":{"a0-beta":"off"},"env":{"X":"1"}}',
    "enabledPlugins": b'{"skillOverrides":{"a0-beta":"off"},"enabledPlugins":{}}',
    "duplicate_key": b'{"skillOverrides":{"a0-beta":"off","a0-beta":"on"}}',
    "duplicate_top": b'{"skillOverrides":{"a0-beta":"off"},"skillOverrides":{}}',
    "empty_map": b'{"skillOverrides":{}}',
    "missing_map": b'{}',
    "array_top": b'[]',
    "string_top": b'"x"',
    "malformed": b'{"skillOverrides":{"a0-beta":"off",}',
    "nan": b'{"skillOverrides":{"a0-beta":NaN}}',
    "bom": b'\xef\xbb\xbf{"skillOverrides":{"a0-beta":"off"}}',
    "not_utf8": b'{"skillOverrides":{"a0-b\xffeta":"off"}}',
    "deep_nesting": b'{"skillOverrides":' + b'[' * 5000 + b']' * 5000 + b'}',
    "too_large": b'{"skillOverrides":{"a0-beta":"' + b'x' * 70000 + b'"}}',
    "non_bytes": "str-not-bytes",
}
for raw in ACCEPT:
    errs = validate(raw, INV)
    case(f"accept:{raw.decode()[:50]}", errs == [], errs)
for name, raw in REJECT.items():
    try:
        errs = validate(raw, INV)
        case(f"reject:{name}", bool(errs), errs[:2])
    except Exception as e:
        case(f"reject:{name}", False, f"RAISED {type(e).__name__}")

# Structured fuzz: random JSON documents and random byte strings. Property: never raises, and
# acceptance implies the strict grammar holds (checked independently here).
rnd = random.Random(20261029)
atoms = [None, True, False, 0, 1.5, "", "off", "on", "name-only", "OFF", "x" * 70, [], {}]
keys = ["skillOverrides", "skilloverrides", "hooks", "disableAllHooks", "a0-beta", "pkg.skill", "A0-BETA", ""]


def rand_value(d=0):
    r = rnd.random()
    if d > 3 or r < 0.4:
        return rnd.choice(atoms + list(STATES))
    if r < 0.7:
        return {rnd.choice(keys + list(INV)): rand_value(d + 1) for _ in range(rnd.randint(0, 4))}
    return [rand_value(d + 1) for _ in range(rnd.randint(0, 3))]


def strict_ok(doc):
    return (isinstance(doc, dict) and set(doc) == {"skillOverrides"} and isinstance(doc["skillOverrides"], dict)
            and doc["skillOverrides"] and all(k in INV and isinstance(v, str) and v in STATES
                                              for k, v in doc["skillOverrides"].items()))


raised = wrong_accept = accepted = 0
for i in range(20000):
    if i % 5 == 0:
        raw = bytes(rnd.getrandbits(8) for _ in range(rnd.randint(0, 64)))
        doc = None
    else:
        doc = rand_value()
        raw = json.dumps(doc).encode()
    try:
        errs = validate(raw, INV)
    except Exception:
        raised += 1
        continue
    if not errs:
        accepted += 1
        if doc is None or not strict_ok(doc):
            try:
                if not strict_ok(json.loads(raw)):
                    wrong_accept += 1
            except Exception:
                wrong_accept += 1
case("fuzz_20000_never_raises", raised == 0, {"raised": raised})
case("fuzz_20000_no_accept_outside_grammar", wrong_accept == 0, {"wrong_accept": wrong_accept, "accepted": accepted})

# Negative controls: the frozen v1 prototype shows the defects the review found
try:
    v1.validate(REJECT["list_value"], INV)
    case("NEGCTL_v1_raises_on_list_value", False, "v1 did not raise")
except TypeError:
    case("NEGCTL_v1_raises_on_list_value", True, "TypeError (defect reproduced)")
case("NEGCTL_v1_accepts_empty_map", v1.validate(REJECT["empty_map"], INV) == [], "v1 accepted {}")

json.dump(results, open(sys.argv[2], "w"), indent=1, default=str)
print("ALL_PASS" if all(r["pass"] for r in results.values()) else "SOME_FAIL")
sys.exit(0 if all(r["pass"] for r in results.values()) else 1)
