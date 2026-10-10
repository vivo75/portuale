import sys, os
sys.path.insert(0, "lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
E = "8"
shape = sys.argv[1]
bdep = {"a": "app-misc/u256prov:=", "b": "<app-misc/u256prov-2:="}[shape]
ebuilds = {
    "app-misc/u256prov-2": {"EAPI": E, "SLOT": "2/2"},
    "app-misc/u256pa-1": {"EAPI": E, "RDEPEND": "app-misc/u256prov:="},
    "app-misc/u256pb-1": {"EAPI": E, "RDEPEND": bdep},
}
installed = {
    "app-misc/u256pa-1": {"EAPI": E, "RDEPEND": ">=app-misc/u256prov-1:0/1="},
    "app-misc/u256pb-1": {"EAPI": E, "RDEPEND": ">=app-misc/u256prov-1:0/1="},
}
world = ["app-misc/u256pa", "app-misc/u256pb"]
atoms = sys.argv[2].split(",")
opts = {"--pretend": True, "--update": True, "--deep": True, "--verbose": True}
for kv in sys.argv[3:]:
    k, _, v = kv.partition("=")
    opts[k] = v if v else True
pg = ResolverPlayground(ebuilds=ebuilds, installed=installed, world=world,
                        user_config={"make.conf": ('USE="foo"',)}, debug=bool(os.environ.get("PGDEBUG")))
try:
    res = pg.run(atoms, options=opts)
    print("### u256", shape, atoms, "success", res.success, "mergelist", res.mergelist)
    res.depgraph.display_problems()
finally:
    pg.cleanup()
