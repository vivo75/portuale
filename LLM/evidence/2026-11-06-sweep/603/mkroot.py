"""mkroot.py <root> <spec.json>: vdb entries + world file for an ad-hoc ROOT."""
import json, os, sys
root, spec = sys.argv[1], json.load(open(sys.argv[2]))
os.makedirs(f"{root}/var/lib/portage", exist_ok=True)
open(f"{root}/var/lib/portage/world", "w").write("".join(a + "\n" for a in spec.get("world", [])))
for cpv, meta in spec.get("installed", {}).items():
    cat, pf = cpv.split("/")
    meta = dict({"EAPI": "8", "SLOT": "0", "repository": "testrepo", "CATEGORY": cat}, **meta)
    d = f"{root}/var/db/pkg/{cat}/{pf}"; os.makedirs(d, exist_ok=True)
    for k, v in meta.items(): open(f"{d}/{k}", "w").write(v + "\n")
