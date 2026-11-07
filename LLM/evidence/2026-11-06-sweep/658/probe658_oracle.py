"""#658 oracle: the p657try fixture shape (pmtest fixtures, verbatim
RDEPENDs) in Portage 3.0.83's ResolverPlayground. Run from 3rdparty/portage
with PORTAGE_GNUPGHOME=<copy of lib/portage/tests/.gnupg>, PATH=$PWD/bin:$PATH.
Prints the merge list and the display_problems() text (stderr)."""
import sys
sys.path.insert(0, "lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground

E8 = {"EAPI": "8", "KEYWORDS": "x86"}
def d(**kw):
    x = dict(E8); x.update(kw); return x

ebuilds = {
    "dev-libs/p647prov-5.42.2": d(SLOT="0/5.42"),
    "dev-libs/p647prov-5.44.0": d(SLOT="0/5.44"),
    "dev-libs/p657try-1": d(SLOT="0", RDEPEND="virtual/p657mm virtual/p657exp"),
    "virtual/p657exp-1": d(SLOT="0", RDEPEND="|| ( =dev-libs/p647prov-5.44* =dev-libs/p647prov-5.42* ~dev-libs/p657expcore-1 ) dev-libs/p647prov:= !<dev-libs/p657expcore-1 !>dev-libs/p657expcore-1-r999"),
    "virtual/p657mm-1": d(SLOT="0", RDEPEND="|| ( =dev-libs/p647prov-5.42* ~dev-libs/p657mmcore-1 ) dev-libs/p647prov:= !<dev-libs/p657mmcore-1 !>dev-libs/p657mmcore-1-r999"),
    "dev-libs/p657expcore-1": d(SLOT="0"),
    "dev-libs/p657mmcore-1": d(SLOT="0"),
}
installed = {
    "dev-libs/p647prov-5.42.2": d(SLOT="0/5.42"),
    "virtual/p657exp-1": d(SLOT="0", RDEPEND="|| ( =dev-libs/p647prov-5.44* =dev-libs/p647prov-5.42* ~dev-libs/p657expcore-1 ) dev-libs/p647prov:0/5.42= !<dev-libs/p657expcore-1 !>dev-libs/p657expcore-1-r999"),
    "virtual/p657mm-1": d(SLOT="0", RDEPEND="|| ( =dev-libs/p647prov-5.42* ~dev-libs/p657mmcore-1 ) dev-libs/p647prov:0/5.42= !<dev-libs/p657mmcore-1 !>dev-libs/p657mmcore-1-r999"),
}
if len(sys.argv) > 1 and sys.argv[1] == "unbuilt":
    for k in ("virtual/p657exp-1", "virtual/p657mm-1"):
        installed[k]["RDEPEND"] = ebuilds[k]["RDEPEND"]
pg = ResolverPlayground(ebuilds=ebuilds, installed=installed, world=[])
try:
    r = pg.run(["dev-libs/p657try"], {"--pretend": True, "--verbose": True})
    print("SUCCESS", r.success, "MERGELIST", r.mergelist)
    r.depgraph.display_problems()
finally:
    pg.cleanup()
