"""S0b: static scan of the installed vdb (ROOT=/): every `||` group in an installed
package's RDEPEND/PDEPEND/IDEPEND, in BOTH the vdb text and (dynamic deps) the
repo ebuild's current metadata when the same cpv exists in the repo, reduced with the
installed USE. Flags groups whose first vdb-matching arm is not arm [0].
Output: one line per flagged group."""
import portage
from portage.dep import use_reduce, Atom
eroot = portage.settings["EROOT"]
vardb = portage.db[eroot]["vartree"].dbapi
portdb = portage.db[eroot]["porttree"].dbapi
import os
KEYS = os.environ.get("S0B_KEYS", "RDEPEND PDEPEND IDEPEND").split()

def groups(ds):
    i = 0
    while i < len(ds):
        x = ds[i]
        if x == "||":
            yield ds[i + 1]
            yield from groups(ds[i + 1])
            i += 2
            continue
        if isinstance(x, list):
            yield from groups(x)
        i += 1

def flat(x):
    if isinstance(x, Atom):
        return [x]
    out = []
    for y in x:
        if isinstance(y, list):
            out += flat(y)
        elif isinstance(y, Atom):
            out.append(y)
    return out

def arm_matches(arm):
    atoms = [a for a in flat(arm) if not a.blocker]
    return bool(atoms) and all(vardb.match(a) for a in atoms)

n = 0
for cpv in sorted(vardb.cpv_all()):
    use = vardb.aux_get(cpv, ["USE"])[0].split()
    eapi, repo = vardb.aux_get(cpv, ["EAPI", "repository"])
    srcs = [("vdb", dict(zip(KEYS, vardb.aux_get(cpv, KEYS))))]
    if portdb.cpv_exists(cpv):
        try:
            srcs.append(("ebuild", dict(zip(KEYS, portdb.aux_get(cpv, KEYS, myrepo=repo or None)))))
        except Exception:
            pass
    for src, md in srcs:
        for k in KEYS:
            if not md[k].strip():
                continue
            try:
                ds = use_reduce(md[k], uselist=use, opconvert=True, token_class=Atom, eapi=eapi)
            except Exception as e:
                print(f"PARSE-ERR {cpv} {src} {k}: {e}")
                continue
            # || arms: when opconvert=True, a group is ['||', a, b, ...] -- handle that form
            stack = [ds]
            while stack:
                cur = stack.pop()
                if not isinstance(cur, list):
                    continue
                if cur and cur[0] == "||":
                    arms = cur[1:]
                    m = [i for i, a in enumerate(arms) if arm_matches(a)]
                    if m and m[0] != 0:
                        n += 1
                        desc = " ".join(f"[{i}]{'*' if i in m else ''}{' '.join(map(str, flat(a)))}" for i, a in enumerate(arms))
                        print(f"FLAG {cpv} {src} {k}: || {desc}")
                stack.extend(x for x in cur if isinstance(x, list))
print(f"flagged groups: {n}")
