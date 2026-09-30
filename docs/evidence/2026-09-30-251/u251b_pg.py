import sys, os
sys.path.insert(0, "lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
ebuilds = {
    "dev-libs/u251p-1": {"EAPI": "8", "SLOT": "1", "RDEPEND": "|| ( dev-util/u251q dev-util/u251r )"},
    "dev-libs/u251p-2": {"EAPI": "8", "SLOT": "2", "BDEPEND": "|| ( dev-util/u251q dev-util/u251r )"},
    "dev-util/u251q-1": {"EAPI": "8", "BDEPEND": "dev-libs/u251p:2"},
    "dev-util/u251r-1": {"EAPI": "8"},
}
installed = {"dev-libs/u251p-1": {"EAPI": "8", "SLOT": "1", "RDEPEND": "|| ( dev-util/u251q dev-util/u251r )"}}
pg = ResolverPlayground(ebuilds=ebuilds, installed=installed, world=["dev-libs/u251p:1"], user_config={"make.conf": ('USE="foo"',)})
try:
    for atoms, opts in ((["dev-util/u251q", "dev-libs/u251p:2"], {"--pretend": True, "--deep": True}), (["dev-libs/u251p:2", "dev-util/u251q"], {"--pretend": True, "--deep": True}), (["dev-util/u251q", "dev-libs/u251p:2", "@world"], {"--pretend": True, "--deep": True, "--update": True})):
        res = pg.run(atoms, options=opts)
        print("### u251b", atoms, "success", res.success, "mergelist", res.mergelist)
        print("circular_dependency", {str(k.cpv): sorted(str(c.cpv) for c in v) for k, v in res.depgraph._dynamic_config._circular_dependency.items()})
        res.depgraph.display_problems()
finally:
    pg.cleanup()
