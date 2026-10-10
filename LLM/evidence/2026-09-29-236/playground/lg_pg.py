import sys, os
sys.path.insert(0, "lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
E="8"
ebuilds = {
 "dev-libs/libgit2-0.28.4-r1": {"EAPI":E,"SLOT":"0/28"},
 "dev-libs/libgit2-0.99.0-r1": {"EAPI":E,"SLOT":"0/0.99"},
 "dev-libs/libgit2-1.0.0-r1": {"EAPI":E,"SLOT":"0/1.0"},
 "dev-libs/libgit2-glib-0.28.0.1": {"EAPI":E,"RDEPEND":"<dev-libs/libgit2-0.29:0= >=dev-libs/libgit2-0.26.0"},
 "dev-libs/libgit2-glib-0.99.0.1": {"EAPI":E,"RDEPEND":"<dev-libs/libgit2-1:0= >=dev-libs/libgit2-0.26.0"},
}
pg = ResolverPlayground(ebuilds=ebuilds, debug=False)
try:
    for opts in ({"--pretend":True,"--verbose":True},{"--pretend":True,"--update":True,"--deep":True}):
        res = pg.run(["dev-libs/libgit2-glib"], options=opts)
        print(opts, res.success, res.mergelist)
        res.depgraph.display_problems()
finally:
    pg.cleanup()
