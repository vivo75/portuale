import sys
P = "/home/vivo/repo/PORTUALE/portuale/3rdparty/portage"
sys.path.insert(0, P + "/lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
from portage._sets.base import InternalPackageSet
spec = {}; exec(open(sys.argv[1]).read(), spec)
pg = ResolverPlayground(ebuilds=spec["ebuilds"], installed=spec.get("installed", {}))
try:
    res = pg.run(["sci-libs/L", "sci-libs/M"], options={"--backtrack": 0})
    d = res.depgraph
    for c in d._dynamic_config._package_tracker.slot_conflicts():
        for p in c.pkgs:
            if p.installed: continue
            for pp, a in d._dynamic_config._parent_atoms[p]:
                ne = a.unevaluated_atom.evaluate_conditionals(set())
                print("pkg", p, "use.enabled", sorted(p.use.enabled), "pkg_use_enabled", sorted(d._pkg_use_enabled(p)))
                print(" parent", pp, "atom", a, "unevaluated", a.unevaluated_atom, "eval(-foo)", ne, type(ne))
                i = InternalPackageSet(initial_atoms=(ne,))
                print(" match with set()", i.findAtomForPackage(p, set()), "match with frozenset()", i.findAtomForPackage(p, frozenset()))
                print(" iuse", p.iuse.all, "eapi", p.eapi)
finally:
    pg.cleanup()
