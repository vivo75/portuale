"""usage: permute.py PLAN.json OUT.json PKG...   (plan 02.764, S0.6)
Writes PLAN with the entries named PKG... (package names, e.g. w764au) put in the given relative order, in the positions those
entries held; every other entry stays where it is; merge_order is renumbered. `identity` as the only PKG copies the plan unchanged
(control: the output parses equal to the input)."""
import json, sys
plan, out, *names = sys.argv[1:]
d = next(json.loads(l) for l in open(plan) if l.startswith('{"entries":'))
if names != ["identity"]:
    es = d["entries"]
    idx = [i for i, e in enumerate(es) if e["package"] in names]
    assert sorted(es[i]["package"] for i in idx) == sorted(names), "names do not match the plan entries"
    by = {e["package"]: e for e in es}
    for i, n in zip(idx, names):
        es[i] = by[n]
    for k, e in enumerate(es):
        e["merge_order"] = k
open(out, "w").write(json.dumps(d, separators=(",", ":")) + "\n")
