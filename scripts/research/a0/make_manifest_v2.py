"""MANIFEST v2 for run 20261029-pilot-e98a8e (written 2026-10-29, after remediation step 1).

Covers EVERY file under the run root plus the A0 documents. Each entry carries a category and
whether it was in the frozen v1 package manifest (98 entries). IMPORTANT: hashes of files that
were not in the v1 manifest (primary captures: requests.jsonl, screen.raw, specs, markers…) are
taken NOW. They bind those bytes from this moment on; they do NOT prove the bytes are the ones
that existed when the captures were made. The original v1 manifest is preserved unchanged.
Usage: python3 -I make_manifest_v2.py <A0_ROOT> <docs_dir> <v1_manifest.txt> <out.json>
"""
import hashlib
import json
import os
import sys
import time

root, docs, v1_path, out = (os.path.realpath(p) for p in sys.argv[1:5])
v1 = {}
for line in open(v1_path):
    parts = line.rstrip("\n").split("  ", 1)
    if len(parts) == 2 and len(parts[0]) == 64:
        v1[parts[1]] = parts[0]


def sha(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for c in iter(lambda: f.read(1 << 20), b""):
            h.update(c)
    return h.hexdigest()


def category(rel):
    top = rel.split("/", 1)[0]
    if rel in v1:
        return "v1-manifested"
    return {"harness": "historical-v1-harness-unmanifested", "evidence": "historical-evidence-unmanifested",
            "harness-v2": "harness-v2", "remediation": "remediation", "cases": "historical-staging",
            "homes": "historical-staging", "work": "historical-staging", "cases-v2": "v2-staging"}.get(top, "other")


entries = {}
for d, dirs, files in os.walk(root):
    dirs[:] = sorted(x for x in dirs if x != "__pycache__")
    for n in sorted(files):
        p = os.path.join(d, n)
        rel = os.path.relpath(p, root)
        if p == out or os.path.islink(p):
            continue
        st = os.stat(p)
        e = {"sha256": sha(p), "size": st.st_size, "mtime": int(st.st_mtime), "category": category(rel),
             "in_v1_manifest": rel in v1}
        if rel in v1:
            e["v1_sha256_matches"] = v1[rel] == e["sha256"]
        entries[rel] = e
doc_entries = {}
for n in sorted(os.listdir(docs)):
    p = os.path.join(docs, n)
    if os.path.isfile(p) and n.startswith("WYSS-ARSENAL-"):
        doc_entries[n] = {"sha256": sha(p), "size": os.path.getsize(p), "in_v1_manifest": n in v1,
                          **({"v1_sha256_matches": v1[n] == sha(p)} if n in v1 else {})}
cats = {}
for e in entries.values():
    cats[e["category"]] = cats.get(e["category"], 0) + 1
v1_rows = [e for e in list(entries.values()) + list(doc_entries.values()) if e.get("in_v1_manifest")]
manifest = {
    "schema": "wyss.a0.manifest.v2",
    "generated_at": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
    "run_root": root.replace(os.path.expanduser("~"), "~"),
    "statement": ("Hashes for files not in the v1 manifest were computed at generated_at. They bind these "
                  "bytes from now on and do NOT retroactively prove integrity at capture time."),
    "v1_manifest": {"path": v1_path.replace(os.path.expanduser("~"), "~"), "entries": len(v1),
                    "rechecked": len(v1_rows), "all_match": all(e["v1_sha256_matches"] for e in v1_rows)},
    "counts": {"files": len(entries), "documents": len(doc_entries), "by_category": cats},
    "files": entries, "documents": doc_entries,
}
json.dump(manifest, open(out, "w"), indent=1, sort_keys=True)
print(json.dumps({k: manifest[k] for k in ("v1_manifest", "counts")}, indent=1))
