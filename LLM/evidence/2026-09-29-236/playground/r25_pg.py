import sys, os
sys.path.insert(0, "lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
shape = sys.argv[1]
E8 = "8"
if shape == "r25":
    ebuilds = {
        "dev-libs/r25lib-1.0": {"EAPI": E8, "SLOT": "0/1"},
        "dev-libs/r25lib-2.0": {"EAPI": E8, "SLOT": "0/2"},
        "dev-libs/r25mid-1.0": {"EAPI": E8, "RDEPEND": "dev-libs/r25lib:="},
        "dev-libs/r25up-1.0": {"EAPI": E8},
        "dev-libs/r25up-2.0": {"EAPI": E8},
        "dev-libs/r25target-1.0": {"EAPI": E8, "RDEPEND": ">=dev-libs/r25mid-1.0 >=dev-libs/r25up-1.0"},
        "dev-libs/r25consumer-1.0": {"EAPI": E8, "RDEPEND": "<dev-libs/r25lib-2.0:="},
    }
    installed = {
        "dev-libs/r25lib-1.0": {"EAPI": E8, "SLOT": "0/1"},
        "dev-libs/r25mid-1.0": {"EAPI": E8, "RDEPEND": ">=dev-libs/r25lib-1.0:0/1="},
        "dev-libs/r25up-1.0": {"EAPI": E8},
        "dev-libs/r25consumer-1.0": {"EAPI": E8, "RDEPEND": "<dev-libs/r25lib-2.0:0/1="},
    }
    world = ["dev-libs/r25consumer"]
    atoms = ["dev-libs/r25target"]
elif shape in ("slotop", "slotopw"):
    ebuilds = {
        "dev-libs/provpkg-1.0": {"EAPI": E8, "SLOT": "0/1"},
        "dev-libs/provpkg-2.0": {"EAPI": E8, "SLOT": "0/2"},
        "dev-libs/consrdep-1.0": {"EAPI": E8, "RDEPEND": "dev-libs/provpkg:="},
    }
    installed = {
        "dev-libs/provpkg-1.0": {"EAPI": E8, "SLOT": "0/1"},
        "dev-libs/consrdep-1.0": {"EAPI": E8, "RDEPEND": ">=dev-libs/provpkg-1.0:0/1="},
    }
    world = ["dev-libs/provpkg", "dev-libs/consrdep"]
    atoms = ["dev-libs/consrdep"] if shape == "slotop" else ["@world"]
opts = {"--pretend": True, "--update": True, "--deep": True, "--newuse": True, "--verbose": True}
if shape != "slotopw": opts["--oneshot"] = True
for kv in sys.argv[2:]:
    k, _, v = kv.partition("=")
    opts[k] = v if v else True
pg = ResolverPlayground(ebuilds=ebuilds, installed=installed, world=world, debug=bool(os.environ.get("PGDEBUG")))
try:
    res = pg.run(atoms, options=opts)
    d = res.depgraph
    print("### SHAPE", shape, "success", res.success, "mergelist", res.mergelist)
    bi = d._dynamic_config._backtrack_infos
    print("backtrack_infos", bi)
    print("need_restart", d._dynamic_config._need_restart)
    d.display_problems()
finally:
    pg.cleanup()
