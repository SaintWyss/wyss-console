"""Wyss pre-launch overlay gate prototype (v2). Deterministic, no provider, never raises.

v1 defects fixed (independent review 2026-10-29): membership tests on unvalidated values
raised TypeError for list/object values; names allowed by the Rust compiler (`.`) were not
handled consistently; an empty override map was accepted.

Accepts only: strict UTF-8 JSON (no BOM, no duplicate keys, no NaN/Infinity), a top-level
object with exactly one key `skillOverrides`, whose value is a non-empty object mapping
names that exist (exact, case-sensitive) in the supplied inventory to one of the four
documented string states. Every other input is rejected with machine-readable reasons.
"""
import json
import re

STATES = ("on", "name-only", "user-invocable-only", "off")
ALLOWED_TOP = ("skillOverrides",)
MAX_BYTES = 64 * 1024
MAX_ENTRIES = 512
NAME = re.compile(r"\A[A-Za-z0-9._:-]{1,64}\Z")  # same alphabet as profile::skill_ok


class _Reject(Exception):
    pass


def _no_dupes(pairs):
    seen = set()
    for k, _ in pairs:
        if k in seen:
            raise _Reject("duplicate_key")
        seen.add(k)
    return dict(pairs)


def _no_constants(name):
    raise _Reject(f"non_finite_number:{name}")


def validate(raw, inventory):
    """Return a list of rejection reasons; empty list = accepted. Never raises."""
    try:
        if not isinstance(raw, (bytes, bytearray)):
            return ["not_bytes"]
        if len(raw) > MAX_BYTES:
            return ["too_large"]
        if raw.startswith(b"\xef\xbb\xbf"):
            return ["bom_not_allowed"]
        try:
            text = bytes(raw).decode("utf-8", "strict")
        except UnicodeDecodeError:
            return ["not_utf8"]
        try:
            doc = json.loads(text, object_pairs_hook=_no_dupes, parse_constant=_no_constants)
        except _Reject as e:
            return [str(e)]
        except (ValueError, RecursionError) as e:
            return ["malformed:" + str(e).split("\n")[0][:80]]
        if not isinstance(doc, dict):
            return ["not_an_object"]
        errors = [f"forbidden_key:{k[:40]}" for k in doc if k not in ALLOWED_TOP]
        so = doc.get("skillOverrides")
        if not isinstance(so, dict):
            errors.append("skillOverrides_missing_or_not_object")
            return errors
        if not so:
            errors.append("skillOverrides_empty")
        if len(so) > MAX_ENTRIES:
            errors.append("too_many_entries")
            return errors
        inv = {n for n in inventory if isinstance(n, str)}
        for name, state in so.items():
            if not NAME.match(name):
                errors.append(f"invalid_name_shape:{name[:40]!r}")
            elif name not in inv:
                errors.append(f"unknown_skill:{name}")
            if not isinstance(state, str):
                errors.append(f"state_not_string:{name[:40]}:{type(state).__name__}")
            elif state not in STATES:
                errors.append(f"invalid_state:{name[:40]}={state[:20]!r}")
        return errors
    except Exception as e:  # defensive: the gate must fail closed, never crash
        return [f"internal_error:{type(e).__name__}"]


if __name__ == "__main__":
    import sys
    try:
        data = open(sys.argv[1], "rb").read()
    except OSError:
        print(json.dumps({"accepted": False, "errors": ["unreadable"]}))
        sys.exit(1)
    errs = validate(data, set(sys.argv[2:]))
    print(json.dumps({"accepted": not errs, "errors": errs}))
    sys.exit(0 if not errs else 1)
