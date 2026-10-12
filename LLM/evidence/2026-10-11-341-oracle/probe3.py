from portage.dep import use_reduce, Atom
from portage.exception import InvalidDependString
def t(s, **kw):
    try:
        r = use_reduce(s, matchall=True, token_class=Atom, flat=True, **kw)
        print("OK ", repr(s), {k:(v if k!='is_valid_flag' else 'fn') for k,v in kw.items()}, [ (str(a), getattr(a,'repo',None), getattr(a,'build_id',None)) for a in r])
    except InvalidDependString as e:
        print("ERR", repr(s), {k:(v if k!='is_valid_flag' else 'fn') for k,v in kw.items()}, "->", str(e), "| errors:", [ (str(x), getattr(x,'category',None)) for x in (e.errors or ())])
iv=lambda f: True
for s in ["dev-libs/foo::gentoo", "=dev-libs/foo-1.0-2", "x? ( dev-libs/foo::gentoo )", "dev-libs/foo:0::gentoo[bar]", "*/foo", "dev-libs/foo[bar(+),bar]", "dev-libs/foo-1", ">=dev-libs/foo-1*"]:
    t(s)                     # installed-package path: eapi None, no is_valid_flag
    t(s, eapi="8")
    t(s, eapi="8", is_valid_flag=iv)  # ebuild path
