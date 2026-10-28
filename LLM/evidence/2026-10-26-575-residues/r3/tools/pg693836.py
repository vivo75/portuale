# Portage's own ResolverPlayground on the bug-693836 case, counting D:3505-3515 hits.
import sys
P="/home/vivo/repo/PORTUALE/portuale/3rdparty/portage"
sys.path.insert(0, P+"/lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
import _emerge.depgraph as D
hits=[]
orig=D.depgraph._add_dep
def wrapped(self, dep, allow_unsatisfied):
    before=self._dynamic_config._skip_restart
    r=orig(self, dep, allow_unsatisfied)
    if self._dynamic_config._skip_restart and not before and r==0:
        hits.append((str(dep.parent), str(dep.atom)))
    return r
D.depgraph._add_dep=wrapped
mod=__import__("portage.tests.resolver.test_aggressive_backtrack_downgrade", fromlist=["x"])
import inspect
src=inspect.getsource(mod.AgressiveBacktrackDowngradeTestCase.testAgressiveBacktrackDowngrade)
ns={}
exec(compile("\n".join(l[8:] for l in src.splitlines()[1:]).split("        playground =")[0].split("playground =")[0], "x", "exec"), {"ResolverPlaygroundTestCase":__import__("portage.tests.resolver.ResolverPlayground",fromlist=["x"]).ResolverPlaygroundTestCase}, ns)
pg=ResolverPlayground(ebuilds=ns["ebuilds"], installed=ns["installed"], world=ns["world"], debug=False)
try:
    for opts in ({"--update":True,"--deep":True},{"--update":True,"--deep":True,"--backtrack":0}):
        hits.clear()
        r=pg.run(["@world"], options=dict(opts))
        dg=r.depgraph
        print("opts",opts,"success",r.success,"mergelist",r.mergelist,"skip_restart_hits",hits)
finally:
    pg.cleanup()
