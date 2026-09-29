import sys, os
sys.path.insert(0, "3rdparty/portage/lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
E = "8"
ebuilds = {
    "dev-libs/r266lib-1.0": {"EAPI": E, "SLOT": "1", "IUSE": "-bar"},
    "dev-libs/r266lib-2.0": {"EAPI": E, "SLOT": "2", "IUSE": "-bar"},
    "dev-libs/r266lib-3.0": {"EAPI": E, "SLOT": "3", "IUSE": "+bar"},
    "dev-libs/r266mid-1.0": {"EAPI": E, "RDEPEND": ">=dev-libs/r266lib-1[bar] <dev-libs/r266lib-3"},
}
for profile, user_config in [
    ("B-default", {}),
    ("A-bar-on", {"make.conf": ('USE="bar"',)}),
]:
    pg = ResolverPlayground(ebuilds=ebuilds, installed={}, world=[], debug=False, user_config=user_config)
    try:
        res = pg.run(["dev-libs/r266mid"], options={"--pretend": True, "--verbose": True})
        print("### profile", profile, "success", res.success, "mergelist", res.mergelist)
        res.depgraph.display_problems()
    finally:
        pg.cleanup()
