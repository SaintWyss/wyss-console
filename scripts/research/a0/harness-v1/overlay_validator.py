"""Prototype of the Wyss pre-launch overlay gate (question 5). Deterministic, no provider.

Accepts only a strict-JSON object whose single top-level key is `skillOverrides`, mapping
skill names that exist in the known inventory to one of the four documented states. Anything
else (unknown/misspelled keys, hook/permission/command-bearing keys, invalid values,
unknown names, case variants, duplicate keys, malformed JSON) is rejected *before* launch.
Usage: python3 -I overlay_validator.py <overlay-file> <inventory-name> [...]
"""
import json
import sys

STATES = {"on", "name-only", "user-invocable-only", "off"}
ALLOWED_TOP = {"skillOverrides"}
MAX_BYTES = 64 * 1024


def _no_dupes(pairs):
    keys = [k for k, _ in pairs]
    if len(keys) != len(set(keys)):
        raise ValueError("duplicate_key")
    return dict(pairs)


def validate(raw: bytes, inventory):
    if len(raw) > MAX_BYTES:
        return ["too_large"]
    try:
        doc = json.loads(raw.decode("utf-8"), object_pairs_hook=_no_dupes)
    except ValueError as e:
        return [f"malformed:{e}".split("\n")[0][:80]]
    if not isinstance(doc, dict):
        return ["not_an_object"]
    errors = [f"forbidden_key:{k}" for k in doc if k not in ALLOWED_TOP]
    so = doc.get("skillOverrides")
    if not isinstance(so, dict):
        errors.append("skillOverrides_missing_or_not_object")
        return errors
    for name, state in so.items():
        if name not in inventory:
            errors.append(f"unknown_skill:{name}")
        if state not in STATES:
            errors.append(f"invalid_state:{name}={state!r}")
    return errors


if __name__ == "__main__":
    try:
        raw = open(sys.argv[1], "rb").read()
    except OSError:
        print(json.dumps({"accepted": False, "errors": ["unreadable"]}))
        sys.exit(1)
    errs = validate(raw, set(sys.argv[2:]))
    print(json.dumps({"accepted": not errs, "errors": errs}))
    sys.exit(0 if not errs else 1)
