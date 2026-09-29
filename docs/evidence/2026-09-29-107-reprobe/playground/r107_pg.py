import sys, os
sys.path.insert(0, "lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
E = "8"
ebuilds = {
    "dev-libs/r107lib-1.0": {"EAPI": E, "SLOT": "0/1", "IUSE": "+abi"},
    "dev-libs/r107lib-2.0": {"EAPI": E, "SLOT": "0/2", "IUSE": "+abi"},
    "dev-libs/r107mid-1.0": {"EAPI": E, "RDEPEND": "dev-libs/r107lib:=[abi]"},
    "dev-libs/r107pin-1.0": {"EAPI": E, "RDEPEND": "<dev-libs/r107lib-2.0:="},
    "dev-libs/r107up-1.0": {"EAPI": E},
    "dev-libs/r107up-2.0": {"EAPI": E},
    "dev-libs/r107target-1.0": {"EAPI": E, "RDEPEND": ">=dev-libs/r107mid-1.0 >=dev-libs/r107up-1.0"},
}
installed = {
    "dev-libs/r107lib-1.0": {"EAPI": E, "SLOT": "0/1", "IUSE": "+abi", "USE": "abi"},
    "dev-libs/r107mid-1.0": {"EAPI": E, "RDEPEND": "dev-libs/r107lib:0/1=[abi]"},
    "dev-libs/r107pin-1.0": {"EAPI": E, "RDEPEND": "<dev-libs/r107lib-2.0:0/1="},
    "dev-libs/r107up-1.0": {"EAPI": E},
}
world = ["dev-libs/r107pin"]
opts = {"--pretend": True, "--update": True, "--deep": True, "--oneshot": True, "--verbose": True}
pg = ResolverPlayground(ebuilds=ebuilds, installed=installed, world=world,
                        user_config={"make.conf": ('USE="foo"',)}, debug=bool(os.environ.get("PGDEBUG")))
try:
    res = pg.run(["dev-libs/r107target"], options=opts)
    print("### r107 success", res.success, "mergelist", res.mergelist)
    res.depgraph.display_problems()
finally:
    pg.cleanup()
