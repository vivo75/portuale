import sys, os
sys.path.insert(0, "lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
ebuilds = {
    "dev-libs/u251x-1": {"EAPI": "8", "BDEPEND": "|| ( dev-util/u251a dev-util/u251b )"},
    "dev-util/u251a-1": {"EAPI": "8", "BDEPEND": "dev-libs/u251x"},
    "dev-util/u251b-1": {"EAPI": "8", "BDEPEND": "dev-libs/u251x"},
    "dev-libs/u251y-1": {"EAPI": "8", "BDEPEND": "|| ( dev-util/u251c dev-util/u251d ) dev-util/u251d"},
    "dev-util/u251c-1": {"EAPI": "8", "BDEPEND": "dev-libs/u251y"},
    "dev-util/u251d-1": {"EAPI": "8", "BDEPEND": "dev-libs/u251y"},
}
pg = ResolverPlayground(ebuilds=ebuilds, user_config={"make.conf": ('USE="foo"',)})
try:
    for atoms in (["dev-util/u251c", "dev-libs/u251y"], ["dev-libs/u251y", "dev-util/u251c"], ["dev-libs/u251y"]):
        res = pg.run(atoms, options={"--pretend": True})
        print("### u251c", atoms, "success", res.success, "mergelist", res.mergelist)
        print("circular_dependency", {str(k.cpv): sorted(str(c.cpv) for c in v) for k, v in res.depgraph._dynamic_config._circular_dependency.items()})
        res.depgraph.display_problems()
finally:
    pg.cleanup()
