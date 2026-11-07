"""#625(b) oracle: the walk takes the second `||` arm (installed slot) while
the first arm's slot also enters the graph later through the chosen arm's own
chain. Portage 3.0.83 ResolverPlayground (3rdparty/portage @ ae324a809).
Run from 3rdparty/portage with PORTAGE_GNUPGHOME=<copy of
lib/portage/tests/.gnupg> and PATH=$PWD/bin:$PATH."""
import sys
sys.path.insert(0, "lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground

def d(**kw):
    x = {"EAPI": "8", "KEYWORDS": "x86"}; x.update(kw); return x

ebuilds = {
    "dev-libs/p625ac-1": d(SLOT="1", RDEPEND="dev-libs/p625late"),
    "dev-libs/p625ac-2": d(SLOT="2"),
    "dev-libs/p625late-1": d(SLOT="0", RDEPEND="dev-libs/p625ac:2"),
    "dev-libs/p625r-1": d(SLOT="0", RDEPEND="|| ( dev-libs/p625ac:2 dev-libs/p625ac:1 )"),
}
installed = {"dev-libs/p625ac-1": d(SLOT="1", RDEPEND="dev-libs/p625late")}
for extra in ([], ["dev-libs/p625r"]):
    inst = dict(installed)
    pg = ResolverPlayground(ebuilds=ebuilds, installed=inst, world=[])
    try:
        for opts in ({"--emptytree": True}, {}):
            r = pg.run(["dev-libs/p625r"], opts)
            print(opts, "SUCCESS", r.success, "MERGELIST", r.mergelist)
    finally:
        pg.cleanup()
    break
