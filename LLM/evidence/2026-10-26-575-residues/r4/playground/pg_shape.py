"""Run one ad-hoc shape through Portage's ResolverPlayground and log the
need_config_change calls, the handler, the backtrack count and display_problems.
Usage: python3 -I pg_shape.py <spec.py>  (spec defines ebuilds, installed, world, requests=[(atoms, options)])"""
import sys, io, contextlib, json
P = "/home/vivo/repo/PORTUALE/portuale/3rdparty/portage"
sys.path.insert(0, P + "/lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
import _emerge.depgraph as D
spec = {}
exec(open(sys.argv[1]).read(), spec)
log = []
o_ncc = D.depgraph.need_config_change
def ncc(self):
    dc = self._dynamic_config
    was_none = dc._slot_conflict_handler is None
    r = o_ncc(self)
    h = dc._slot_conflict_handler
    log.append(("need_config_change", dict(ret=r, allow_bt=dc._allow_backtracking, built_here=was_none and h is not None,
        changes=[{p.cpv: v for p, v in c.items()} for c in h.changes] if (was_none and h is not None) else None,
        runtime_pkg_mask=sorted(str(p.cpv) + ":" + p.type_name for p in dc._backtrack_infos and [] or self._dynamic_config._runtime_pkg_mask))))
    return r
D.depgraph.need_config_change = ncc
o_bt = D._backtrack_depgraph
def bt(*a, **k):
    r = o_bt(*a, **k)
    log.append(("backtrack_depgraph", dict(success=r[0], backtracked=r[3], final_allow_bt=r[1]._dynamic_config._allow_backtracking,
        final_runtime_pkg_mask=sorted(str(p.cpv) + ":" + p.type_name for p in r[1]._dynamic_config._runtime_pkg_mask))))
    return r
D._backtrack_depgraph = bt
pg = ResolverPlayground(ebuilds=spec["ebuilds"], installed=spec.get("installed", {}), world=spec.get("world", []))
try:
    for atoms, opts in spec["requests"]:
        log.clear()
        res = pg.run(atoms, options=dict(opts))
        d = res.depgraph
        h = d._dynamic_config._slot_conflict_handler
        print(f"### {atoms} {opts}: success={res.success} mergelist={res.mergelist}")
        for ev, k in log:
            print("  ", ev, k)
        err = io.StringIO()
        with contextlib.redirect_stderr(err), contextlib.redirect_stdout(err):
            import portage.output; portage.output.nocolor()
            d.display_problems()
        h = d._dynamic_config._slot_conflict_handler
        if h is not None:
            print("   final handler changes:", [{p.cpv: v for p, v in c.items()} for c in h.changes], "version:", h.is_a_version_conflict, "unspecific:", h.conflict_is_unspecific)
        print("   display_problems text:")
        for line in err.getvalue().splitlines():
            if line.strip():
                print("     |" + line)
finally:
    pg.cleanup()
