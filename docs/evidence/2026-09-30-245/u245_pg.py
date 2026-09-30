import sys, os
sys.path.insert(0, "lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
ebuilds = {
    "app-misc/g216top-1.0": {"EAPI": "8", "RDEPEND": "dev-libs/g216mid"},
    "dev-lang/g216boot-1.0": {"EAPI": "8"},
    "dev-lang/g216comp-1.0": {"EAPI": "8", "BDEPEND": "|| ( >=dev-lang/g216comp-1.0 dev-lang/g216boot )"},
    "dev-libs/g216mid-1.0": {"EAPI": "8", "RDEPEND": "dev-lang/g216comp"},
}
pg = ResolverPlayground(ebuilds=ebuilds, user_config={"make.conf": ('USE="foo"',)}, debug=bool(os.environ.get("PGDEBUG")))
try:
    res = pg.run(["app-misc/g216top"], options={"--pretend": True, "--backtrack": 0})
    d = res.depgraph
    print("### u245 success", res.success, "mergelist", res.mergelist)
    g = d._dynamic_config.digraph
    print("digraph.order", [str(n) for n in g.order])
    for n in g.order:
        print("NODE", n, "children", [(str(c), str(g.nodes[n][0][c])) for c in g.child_nodes(n)])
    d.display_problems()
finally:
    pg.cleanup()
