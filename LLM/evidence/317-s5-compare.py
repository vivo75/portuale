#!/usr/bin/env python3
"""#317 S5: compare two files VDB trees produced by two independent real
Portage runs (one on disk, one through `portuale vdb mount --rw` and
converted back). Equal means: same entries, same file names and modes,
same metadata-stamp state (stamp == the entry dir mtime), same bytes after
normalising what differs between any two runs: BUILD_TIME, the stamp
value, the mtime column of CONTENTS obj/sym lines, and environment.bz2
(its saved environment carries the ROOT path and timestamps; compared by
presence only). Exit 0 when equal, 1 with the differences listed."""
import os, re, sys

def stamp_valid(entry):
    p = os.path.join(entry, "metadata")
    if not os.path.exists(p):
        return "absent"
    m = re.search(rb"^#dir_mtime=(\d+)$", open(p, "rb").read(), re.M)
    return "valid" if m and int(m.group(1)) == os.stat(entry).st_mtime_ns else "stale"

def norm(name, data):
    if name == "environment.bz2":
        return b"<env>"
    if name in ("BUILD_TIME",):
        return b"<time>"
    if name == "metadata":
        data = re.sub(rb"^BUILD_TIME=\d+$", b"BUILD_TIME=<time>", data, flags=re.M)
        return re.sub(rb"^#dir_mtime=\d+$", b"#dir_mtime=<mtime>", data, flags=re.M)
    if name == "CONTENTS":
        return re.sub(rb"^((?:obj|sym) .*) \d+$", rb"\1 <mtime>", data, flags=re.M)
    return data

def tree(vdb):
    out = {}
    if not os.path.isdir(vdb):
        return out  # an empty database converts to no VDB directory
    for cat in sorted(os.listdir(vdb)):
        cdir = os.path.join(vdb, cat)
        if not os.path.isdir(cdir) or cat.startswith("."):
            continue
        for pf in sorted(os.listdir(cdir)):
            e = os.path.join(cdir, pf)
            if not os.path.isdir(e):
                continue
            files = {}
            for n in sorted(os.listdir(e)):
                p = os.path.join(e, n)
                files[n] = (oct(os.stat(p).st_mode & 0o7777), norm(n, open(p, "rb").read()))
            out[f"{cat}/{pf}"] = (stamp_valid(e), oct(os.stat(e).st_mode & 0o7777), files)
    return out

a, b = tree(sys.argv[1]), tree(sys.argv[2])
diffs = []
for k in sorted(set(a) | set(b)):
    if k not in a or k not in b:
        diffs.append(f"{k}: only in {'second' if k in b else 'first'}")
        continue
    (sa, da, fa), (sb, db, fb) = a[k], b[k]
    if sa != sb: diffs.append(f"{k}: stamp {sa} != {sb}")
    if da != db: diffs.append(f"{k}: dir mode {da} != {db}")
    for n in sorted(set(fa) | set(fb)):
        if n not in fa or n not in fb:
            diffs.append(f"{k}/{n}: only in {'second' if n in fb else 'first'}")
        elif fa[n] != fb[n]:
            what = "mode" if fa[n][0] != fb[n][0] else "bytes"
            diffs.append(f"{k}/{n}: {what} differ")
print(f"equal ({len(a)} entries)" if not diffs else "\n".join(diffs))
sys.exit(1 if diffs else 0)
