import sys, os
sys.path.insert(0, "lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
ebuilds = {
    "dev-libs/u251m-1": {"EAPI": "8", "BDEPEND": "|| ( dev-util/u251ta dev-util/u251tb )"},
    "dev-util/u251ta-1": {"EAPI": "8", "BDEPEND": "dev-libs/u251m", "RDEPEND": "!dev-util/u251olda"},
    "dev-util/u251tb-1": {"EAPI": "8", "RDEPEND": "!dev-util/u251oldb"},
    "dev-util/u251olda-1": {"EAPI": "8"},
    "dev-util/u251oldb-1": {"EAPI": "8"},
}
installed = {"dev-util/u251olda-1": {"EAPI": "8"}, "dev-util/u251oldb-1": {"EAPI": "8"}}
pg = ResolverPlayground(ebuilds=ebuilds, installed=installed, user_config={"make.conf": ('USE="foo"',)})
try:
    for atoms, opts in ((["dev-util/u251ta"], {"--pretend": True}), (["dev-libs/u251m"], {"--pretend": True}), (["dev-util/u251ta"], {"--pretend": True, "--verbose": True})):
        res = pg.run(atoms, options=opts)
        print("### u251a", atoms, opts, "success", res.success, "mergelist", res.mergelist)
        res.depgraph.display(res.depgraph.altlist())
        res.depgraph.display_problems()
finally:
    pg.cleanup()
