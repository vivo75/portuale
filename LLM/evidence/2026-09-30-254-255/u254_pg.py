import sys, os
sys.path.insert(0, "lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
E = "8"
shape = sys.argv[1]
cons_rdep = {"plain": "app-misc/abiprov:=", "cond": "cflag? ( app-misc/abiprov:= )", "otherslot": "app-misc/abiprov:="}[shape]
ebuilds = {
    "app-misc/abiprov-1": {"EAPI": E, "SLOT": "0/1"},
    "app-misc/abiprov-2": {"EAPI": E, "SLOT": "0/2"},
    "app-misc/abicons-1": {"EAPI": E, "IUSE": "cflag", "RDEPEND": cons_rdep},
    "app-misc/abiforce-1": {"EAPI": E, "RDEPEND": ">=app-misc/abiprov-2"},
}
installed = {
    **({"app-misc/abiprov-0.5": {"EAPI": E, "SLOT": "1/1"}} if shape == "otherslot" else {}),
    "app-misc/abicons-1": {"EAPI": E, "IUSE": "cflag", "USE": "cflag", "RDEPEND": "app-misc/abiprov:0/1="},
}
world = ["app-misc/abicons", "app-misc/abiforce"]
opts = {"--pretend": True, "--update": True, "--deep": True, "--backtrack": 4, "--verbose": True}
pg = ResolverPlayground(ebuilds=ebuilds, installed=installed, world=world,
                        user_config={"make.conf": ('USE="foo"',)}, debug=bool(os.environ.get("PGDEBUG")))
try:
    res = pg.run(["@world"], options=opts)
    print("### u254", shape, "success", res.success, "mergelist", res.mergelist)
    res.depgraph.display(res.depgraph.altlist())
    res.depgraph.display_problems()
finally:
    pg.cleanup()
