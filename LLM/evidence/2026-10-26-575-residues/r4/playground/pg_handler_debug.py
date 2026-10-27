"""Rebuild slot_conflict_handler on the final depgraph with --debug narration.
Usage: python3 -I pg_handler_debug.py <spec.py> [request-index]"""
import sys
P = "/home/vivo/repo/PORTUALE/portuale/3rdparty/portage"
sys.path.insert(0, P + "/lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
from _emerge.resolver.slot_collision import slot_conflict_handler
import portage.output; portage.output.nocolor()
spec = {}; exec(open(sys.argv[1]).read(), spec)
pg = ResolverPlayground(ebuilds=spec["ebuilds"], installed=spec.get("installed", {}), world=spec.get("world", []))
try:
    idx = [int(sys.argv[2])] if len(sys.argv) > 2 else range(len(spec["requests"]))
    for i in idx:
        atoms, opts = spec["requests"][i]
        res = pg.run(atoms, options=dict(opts))
        d = res.depgraph
        d._frozen_config.myopts["--debug"] = True
        print(f"### {atoms} {opts} success={res.success}", flush=True)
        for c in d._dynamic_config._package_tracker.slot_conflicts():
            for p in c.pkgs:
                print("  conflict pkg", p, "use", sorted(d._pkg_use_enabled(p)), "parents", [(str(a), str(b)) for a, b in d._dynamic_config._parent_atoms.get(p, [])])
        h = slot_conflict_handler(d)
        sys.stderr.flush()
        print("  changes:", [{p.cpv: v for p, v in c.items()} for c in h.changes], "solutions:", len(h.solutions), flush=True)
        print("  explanation:", repr(h.get_explanation()), flush=True)
finally:
    pg.cleanup()
