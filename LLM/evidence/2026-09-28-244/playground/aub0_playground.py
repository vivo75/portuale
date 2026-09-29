import sys
sys.path.insert(0, "lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
ebuilds = {
    "app-misc/A-0": {"EAPI": "5", "RDEPEND": "app-misc/D[-foo]"},
    "app-misc/B-0": {"EAPI": "5", "RDEPEND": "app-misc/D[foo]"},
    "app-misc/C-0": {"EAPI": "5", "RDEPEND": ">=app-misc/D-1"},
    "app-misc/D-0": {"EAPI": "5", "IUSE": "foo"},
    "app-misc/D-1": {"EAPI": "5", "IUSE": "bar"},
}
order = sys.argv[1].split(",")
opts = {"--pretend": True}
for kv in sys.argv[2:]:
    k, _, v = kv.partition("=")
    opts[k] = v if v else True
import os
pg = ResolverPlayground(ebuilds=ebuilds, user_config={"make.conf": (os.environ.get("PGUSE", ""),)}, debug=bool(os.environ.get("PGDEBUG")))
try:
    res = pg.run(["app-misc/" + x for x in order], options=opts)
    d = res.depgraph
    print("### ORDER", "/".join(order), opts, "success", res.success, "mergelist", res.mergelist, "use_changes", res.use_changes)
    print("backtrack", d._dynamic_config._backtrack_infos if hasattr(d._dynamic_config, "_backtrack_infos") else "-")
    d.display_problems()
finally:
    pg.cleanup()
