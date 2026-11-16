"""usage: hostcycle.py --vdb DIR PLAN.json STDERR   (plan 02.764, S0.7/S0.8; witness for residue #772)
Takes the `plan-world: build cycle with no serial order: a -> b -> ... -> a` line of STDERR. For each edge a -> b it lists the
DEPEND/BDEPEND atoms of a's plan entry that name b's cp (USE conditionals and || groups NOT reduced: an over-approximation of the
build atoms) and whether a package of that cp is installed in the VDB. Then prints the literal checker's build-order failure count
for the plan: 0 means the printed order satisfies every build edge, i.e. an order exists although the line says none does."""
import json, re, sys
from pathlib import Path
sys.argv, args = sys.argv[:1], sys.argv[1:]
vdb = Path(args[args.index("--vdb") + 1]); plan, err = [a for a in args if a not in ("--vdb", str(vdb))]
sys.path.insert(0, str(Path(__file__).resolve().parent))
import classify as c
doc = c.literal.parse_json_output(Path(plan).read_text())
line = next((l for l in Path(err).read_text().splitlines() if "build cycle with no serial order" in l), None)
if line is None:
    print("no cycle line"); sys.exit(0)
members = [m.strip() for m in line.split("no serial order:")[1].split("(no order")[0].split("->")]
entries = {f"{e['category']}/{e['package']}": e for e in doc["entries"]}
installed = {c.re.sub(r"-\d.*$", "", p.name) and f"{p.parent.name}/{c.re.sub(r'-[0-9].*$', '', p.name)}" for p in vdb.glob("*/*")}
print(f"cycle members: {len(set(members[:-1]))} ({len(members) - 1} edges)")
met = 0
for a, b in zip(members, members[1:]):
    cpa, cpb = a.split(":")[0], b.split(":")[0]
    e = entries.get(cpa)
    deps = " ".join((e or {}).get("dependencies", {}).get(k, "") for k in ("DEPEND", "BDEPEND"))
    atoms = [x for x in re.findall(r"[<>=~!]*[\w.+-]+/[\w.+-]+[^\s]*", deps) if not x.startswith("!") and c.cp_of(x) == cpb]
    inst = cpb in installed
    met += bool(atoms) and inst
    print(f"  {a} -> {b}: build atoms {atoms or 'none (runtime edge only or USE-conditional)'}; {cpb} installed: {inst}")
lit = [f for f in c.literal.check_document(doc, vdb) if f.property == "build-order" and not f.pre_existing]
print(f"edges with a build atom met by an installed package: {met} of {len(members) - 1}")
print(f"literal checker build-order failures on the plan: {len(lit)}")
