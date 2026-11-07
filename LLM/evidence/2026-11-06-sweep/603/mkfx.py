"""mkfx.py <fixtures-root> <spec.json>: write fixture ebuilds (pmtest style),
their md5-cache entries, and vdb entries. Optional "world" list appends."""
import hashlib, json, os, sys

root, spec = sys.argv[1], json.load(open(sys.argv[2]))
ORDER = ["SLOT", "KEYWORDS", "IUSE", "DEPEND", "RDEPEND", "PDEPEND", "BDEPEND"]


def split(cpv):
    cat, pf = cpv.split("/")
    parts = pf.split("-")
    if parts[-1].startswith("r") and parts[-1][1:].isdigit():
        pn = "-".join(parts[:-2])
    else:
        pn = "-".join(parts[:-1])
    return cat, pn, pf


desc_prefix = spec.get("desc", "fixture package")
for cpv, meta in spec.get("ebuilds", {}).items():
    cat, pn, pf = split(cpv)
    meta = dict({"SLOT": "0", "KEYWORDS": "amd64"}, **meta)
    desc = f"{desc_prefix} {pf}"
    lines = ["EAPI=8", f'DESCRIPTION="{desc}"']
    for k in ORDER:
        if k in meta:
            lines.append(f'{k}="{meta[k]}"')
    txt = "\n".join(lines) + "\n"
    d = f"{root}/repo/{cat}/{pn}"
    os.makedirs(d, exist_ok=True)
    with open(f"{d}/{pf}.ebuild", "w") as f:
        f.write(txt)
    cache = {k: v for k, v in meta.items() if v}
    cache.update(DEFINED_PHASES="-", DESCRIPTION=desc, EAPI="8")
    cd = f"{root}/repo/metadata/md5-cache/{cat}"
    os.makedirs(cd, exist_ok=True)
    with open(f"{cd}/{pf}", "w") as f:
        f.write("".join(f"{k}={cache[k]}\n" for k in sorted(cache)))
        f.write(f"_md5_={hashlib.md5(txt.encode()).hexdigest()}\n")
for cpv, meta in spec.get("installed", {}).items():
    cat, pn, pf = split(cpv)
    meta = dict({"EAPI": "8", "SLOT": "0", "repository": "testrepo", "CATEGORY": cat}, **meta)
    d = f"{root}/var/db/pkg/{cat}/{pf}"
    os.makedirs(d, exist_ok=True)
    for k, v in meta.items():
        with open(f"{d}/{k}", "w") as f:
            f.write(v + "\n")
if spec.get("world"):
    with open(f"{root}/var/lib/portage/world", "a") as w:
        for a in spec["world"]:
            w.write(a + "\n")
