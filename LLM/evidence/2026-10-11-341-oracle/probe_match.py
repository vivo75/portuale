from portage.dep import Atom, match_from_list, best_match_to_list
from portage.versions import _pkg_str
c = [_pkg_str("dev-libs/foo-1.0", build_id=1, slot="0", repo="gentoo"),
     _pkg_str("dev-libs/foo-1.0", build_id=2, slot="0", repo="gentoo"),
     _pkg_str("dev-libs/foo-1.0", slot="0", repo="gentoo"),          # ebuild-like, build_id None
     _pkg_str("dev-libs/foo-1.0-r1", build_id=3, slot="0", repo="gentoo"),
     _pkg_str("dev-libs/foo-10", build_id=4, slot="0", repo="gentoo"),
     _pkg_str("dev-libs/foobar-1.12", slot="0", repo="x")]
def show(l): return ["%s#%s" % (x, x.build_id) for x in l]
for a in ["=dev-libs/foo-1.0-2", "=dev-libs/foo-1.0", ">=dev-libs/foo-1.0-2", "~dev-libs/foo-1.0-2", "<dev-libs/foo-10-1", "=dev-libs/foo-1*", "=dev-libs/foo-1.0*", "dev-libs/*", "*/foo*", "=*/foo-*1*", "=dev-libs/foo*-*12*", "dev-libs/foo::x", "*/*::x"]:
    try:
        at = Atom(a, allow_wildcard=True, allow_repo=True)
        print("%-24s -> %s" % (a, show(match_from_list(at, c))))
    except Exception as e:
        print("%-24s -> EXC %s: %s" % (a, type(e).__name__, e))
print("plain-string candidates, =dev-libs/foo-1.0-2 ->", match_from_list(Atom("=dev-libs/foo-1.0-2"), ["dev-libs/foo-1.0", "dev-libs/foo-1.0-r0"]))
pk = _pkg_str("dev-libs/foo-1.0", build_id=2, slot="0", repo="gentoo")
L = [Atom(x, allow_wildcard=True, allow_repo=True) for x in ["*/*", "dev-libs/*", "dev-libs/foo", "*/*:0", "=*/foo-*1*", "dev-libs/foo:0", ">=dev-libs/foo-1", "=dev-libs/foo-1*", "~dev-libs/foo-1.0", "=dev-libs/foo-1.0-2", "=dev-libs/foo-1.0"]]
for i in range(len(L)):
    print("best_match_to_list(prefix up to %-22s) = %s" % (L[i], best_match_to_list(pk, L[:i+1])))
