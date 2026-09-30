import sys, os
sys.path.insert(0, "lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
ebuilds = {
    "dev-libs/u249json-1": {"EAPI": "8", "BDEPEND": "virtual/u249make"},
    "dev-util/u249make-bootstrap-1": {"EAPI": "8"},
    "dev-util/u249make-1": {"EAPI": "8", "BDEPEND": "dev-libs/u249json:0="},
    "virtual/u249make-0": {"EAPI": "8", "RDEPEND": "|| ( dev-util/u249make-bootstrap dev-util/u249make )"},
}
pg = ResolverPlayground(ebuilds=ebuilds, user_config={"make.conf": ('USE="foo"',)}, debug=bool(os.environ.get("PGDEBUG")))
try:
    for opts in ({"--pretend": True}, {"--pretend": True, "--backtrack": 0}):
        res = pg.run(["dev-util/u249make"], options=opts)
        print("### u249", opts, "success", res.success, "mergelist", res.mergelist)
        print("circular_dependency", {str(k.cpv): sorted(str(c.cpv) for c in v) for k, v in res.depgraph._dynamic_config._circular_dependency.items()})
        res.depgraph.display_problems()
finally:
    pg.cleanup()
