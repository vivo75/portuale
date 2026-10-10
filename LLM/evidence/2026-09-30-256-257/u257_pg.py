import sys, os
sys.path.insert(0, "lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
E = "8"
ebuilds = {
    "app-misc/u257prov-2": {"EAPI": E, "SLOT": "2/2"},
    "app-misc/u257par-1": {"EAPI": E, "RDEPEND": "app-misc/u257prov:="},
}
binpkgs = {
    "app-misc/u257par-1": {"EAPI": E, "RDEPEND": "app-misc/u257prov:0/1="},
}
atoms = sys.argv[1].split(",")
opts = {"--pretend": True, "--verbose": True}
for kv in sys.argv[2:]:
    k, _, v = kv.partition("=")
    opts[k] = v if v else True
pg = ResolverPlayground(ebuilds=ebuilds, binpkgs=binpkgs,
                        user_config={"make.conf": ('USE="foo"',)}, debug=bool(os.environ.get("PGDEBUG")))
try:
    res = pg.run(atoms, options=opts)
    print("### u257", atoms, opts, "success", res.success, "mergelist", res.mergelist)
    res.depgraph.display_problems()
finally:
    pg.cleanup()
