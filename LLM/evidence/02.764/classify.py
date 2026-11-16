"""usage: classify.py [--vdb DIR] [--filter SUBSTR] PLAN.json...   (plan 02.764, S0.5)
Runs the literal checker (pmtest health_checker.py, charter 2/2a as of #749) and the strict PMS 8.1 model
(strict_health_checker.py next to this file: pmtest's checker at cc3026c^, unchanged) on each plan and prints, per plan:
  literal            non-pre_existing failures of the literal model
  binding-transition strict "not usable yet" rows whose unmet atom is a sub-slot binding (:slot/sub=) on a cp the plan moves to
                     another sub-slot (the #737/#749 binding class), set aside BEFORE the depth split
  depth1 / depth2+   the remaining "not usable yet" rows (build-order or cycle-noted); depth 1 when the provider named after
                     "RDEPEND of" is the package matching the build atom, else depth>=2; with the late package
  other-build-order  strict build-order rows the other patterns do not fit (must stay 0; never dropped silently)
  cycle-excused      strict cycle-noted rows without "not usable yet" (non-failures, reported as their own class)
--filter keeps only rows whose package contains SUBSTR (for a fixture shape inside a bigger plan). Env PMTEST_CHECKERS overrides
the directory of the literal checker (default ../../../../pmtest/pytests-contract-suite)."""
import importlib.util, json, os, re, sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    spec.loader.exec_module(mod)
    return mod

literal = load("hc_literal", Path(os.environ.get("PMTEST_CHECKERS", HERE / "../../../../pmtest/pytests-contract-suite")) / "health_checker.py")
strict = load("hc_strict", HERE / "strict_health_checker.py")

NOT_USABLE = re.compile(r"^(\w+) (.+?) is not usable yet: RDEPEND of (\S+) is (.*)$")
BIND = re.compile(r":([^/\s\[:]+)/([^=\s\[]+)=")

def cp_of(atom):
    a = re.sub(r"^[<>=~!]+", "", atom.split("[")[0].split(":")[0])
    return re.sub(r"-\d.*$", "", a)

def classify(doc, vdb):
    moved = {}
    for e in doc.get("entries", []):
        moved[f"{e['category']}/{e['package']}"] = (e.get("slot"), e.get("sub_slot"))
    lit = [f for f in literal.check_document(doc, vdb) if f.is_failure and not f.pre_existing]
    out = {"literal": [(f.package, f.reason) for f in lit], "binding-transition": [], "depth1": [], "depth2+": [], "cycle-excused": [], "other-build-order": []}
    for f in strict.check_document(doc, vdb):
        if f.property not in ("build-order", "cycle-noted") or f.pre_existing:
            continue
        m = NOT_USABLE.match(f.reason)
        if not m:
            out["cycle-excused" if f.property == "cycle-noted" else "other-build-order"].append((f.package, f.reason))
            continue
        b = BIND.search(f.atom)
        if b and cp_of(f.atom) in moved and moved[cp_of(f.atom)][1] not in (None, b.group(2)):
            out["binding-transition"].append((f.package, f.atom))
            continue
        _, build_atom, provider, rest = m.groups()
        provider_cp = re.sub(r"-\d.*$", "", provider)
        late = re.search(r"met only by (\S+?),? merged later", rest)
        row = (f.package, f"{f.property}: {build_atom} via {provider}; late {late.group(1) if late else 'a later package of the same cycle'}")
        out["depth1" if provider_cp in {cp_of(a) for a in re.findall(r"[<>=~]*[\w.+-]+/[\w.+-]+", build_atom)} else "depth2+"].append(row)
    return out

def main(argv):
    vdb, flt = None, ""
    while argv and argv[0] in ("--vdb", "--filter"):
        if argv[0] == "--vdb": vdb = Path(argv[1])
        else: flt = argv[1]
        argv = argv[2:]
    for name in argv:
        text = Path(name).read_text(errors="replace")
        doc = literal.parse_json_output(text)
        print(f"== {name}")
        if doc is None or doc.get("aborted"):
            print("skipped: no document / aborted"); continue
        res = classify(doc, vdb)
        for k, rows in res.items():
            rows = [r for r in rows if flt in r[0]]
            print(f"{k}: {len(rows)}")
            shown = rows if k != "binding-transition" else rows[:0]
            for pkg, why in shown:
                print(f"  {pkg}: {why}")
    return 0

if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
