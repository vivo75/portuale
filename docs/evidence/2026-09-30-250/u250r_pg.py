import sys, os
sys.path.insert(0, "lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
ebuilds = {
    "dev-libs/u250json-1": {"EAPI": "8", "BDEPEND": "|| ( dev-util/u250make[foo] dev-util/u250make-bootstrap )"},
    "dev-util/u250make-bootstrap-1": {"EAPI": "8"},
    "dev-util/u250make-1": {"EAPI": "8", "IUSE": "+foo", "BDEPEND": "dev-libs/u250json:0="},
}
installed = {"dev-util/u250make-1": {"EAPI": "8", "IUSE": "foo", "USE": "", "BDEPEND": "dev-libs/u250json:0/0="}}
pg = ResolverPlayground(ebuilds=ebuilds, installed=installed, user_config={"make.conf": ('USE="foo"',)}, debug=bool(os.environ.get("PGDEBUG")))
try:
    for atoms, opts in ((["dev-util/u250make"], {"--pretend": True}), (["dev-libs/u250json"], {"--pretend": True}), (["dev-util/u250make"], {"--pretend": True, "--newuse": True, "--update": True, "--deep": True})):
        res = pg.run(atoms, options=opts)
        print("### u250", atoms, opts, "success", res.success, "mergelist", res.mergelist)
        res.depgraph.display_problems()
finally:
    pg.cleanup()
