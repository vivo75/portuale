import sys
sys.path.insert(0, sys.argv[1]) if len(sys.argv) > 1 else None
import portage.dep as D
from portage.dep import Atom, use_reduce
from portage.exception import InvalidDependString
NAMES = ["games-emulation/81-libretro","games-emulation/2048-libretro","games-arcade/2048",
 "dev-util/diff-mode-","dev-util/timidity--","dev-util/frob---","dev-util/diffball-9-",
 "dev-util/foo-2048+","dev-util/foo-9-bar","cat/pkg[a(+),-a]","cat/pkg[a(+),a(-)]","cat/pkg[a,a(+)]","cat/pkg[a(+),b,-a(+)]","cat/pkg[a(+),-a(+)]"]
cpar = D._c_dep_parser
def run(s, c):
    D._c_dep_parser = cpar if c else None
    try: return "OK %s" % [str(x) for x in use_reduce(s, token_class=Atom, eapi="8")]
    except InvalidDependString as e: return "ERR"
for s in NAMES:
    c, p = run(s, True), run(s, False)
    print("%-28s C=%-45s py=%-45s %s" % (s, c, p, "same" if c == p else "DIFF"))
