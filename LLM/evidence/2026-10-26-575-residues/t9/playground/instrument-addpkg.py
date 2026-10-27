# Task 9 S0d: which call chain adds a runtime_pkg_mask-masked installed
# package to the graph (pass >= 1); also every installed p569prov _add_pkg,
# _want_installed_pkg and _slot_operator_update_probe call. Wraps depgraph._add_pkg and prints the
# depgraph.py frames (function:line) for every masked pkg it adds.
import sys, traceback
import _emerge.depgraph as D
from _emerge.main import emerge_main
PASS=[0]
o_sf = D.depgraph.select_files
def sf(self, a):
    print(f"\nT9 === pass {PASS[0]}", file=sys.stderr); PASS[0]+=1
    return o_sf(self, a)
D.depgraph.select_files = sf
o_add = D.depgraph._add_pkg
def add_pkg(self, pkg, dep):
    if pkg.installed and pkg.cp == "dev-libs/p569prov":
        fr = [f"{f.name}:{f.lineno}" for f in traceback.extract_stack()[:-1] if f.filename.endswith("depgraph.py")]
        print(f"T9 _add_pkg masked={pkg in self._dynamic_config._runtime_pkg_mask} {pkg.cpv}({pkg.type_name}) parent={getattr(dep.parent,'cpv',dep.parent)} atom={dep.atom} complete_mode={self._dynamic_config._complete_mode} select_package={self._select_package.__name__} chain={' <- '.join(reversed(fr))}", file=sys.stderr)
    return o_add(self, pkg, dep)
D.depgraph._add_pkg = add_pkg
o_up = D.depgraph._slot_operator_update_probe
def up(self, dep, *a, **k):
    r = o_up(self, dep, *a, **k)
    print(f"T9 _slot_operator_update_probe(parent={getattr(dep.parent,'cpv',dep.parent)}, atom={dep.atom}, child={getattr(dep.child,'cpv',None)}) -> {r}", file=sys.stderr)
    return r
D.depgraph._slot_operator_update_probe = up
o_w = D.depgraph._want_installed_pkg
def w(self, pkg):
    r = o_w(self, pkg)
    if pkg.cp == "dev-libs/p569prov":
        print(f"T9 _want_installed_pkg({pkg.cpv}) -> {r}", file=sys.stderr)
    return r
D.depgraph._want_installed_pkg = w
sys.argv = ["emerge"] + sys.argv[1:]
sys.exit(emerge_main())
