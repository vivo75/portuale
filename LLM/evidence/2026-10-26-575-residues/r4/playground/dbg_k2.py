import sys
P = "/home/vivo/repo/PORTUALE/portuale/3rdparty/portage"
sys.path.insert(0, P + "/lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
from portage._sets.base import InternalPackageSet
from portage.dep import Atom
for eapi in ("2", "5", "7", "8"):
    eb = {"sci-libs/K-1": {"IUSE": "+foo", "EAPI": eapi},
          "sci-libs/M-1": {"RDEPEND": "sci-libs/K[foo=]", "IUSE": "+foo", "EAPI": eapi}}
    pg = ResolverPlayground(ebuilds=eb)
    try:
        res = pg.run(["sci-libs/M"], options={})
        d = res.depgraph
        k = [p for p in d._dynamic_config.digraph if getattr(p, "cpv", "") == "sci-libs/K-1"][0]
        a = Atom("sci-libs/K[-foo]")
        i = InternalPackageSet(initial_atoms=(a,))
        c = k.copy(); 
        print("EAPI", eapi, "match(set())", i.findAtomForPackage(k, set()), "| copy.use.enabled after USE='':", end=" ")
        c._metadata["USE"] = ""
        print(sorted(c.use.enabled), "| k._use", type(k._use).__name__ if hasattr(k, "_use") else None)
    finally:
        pg.cleanup()
