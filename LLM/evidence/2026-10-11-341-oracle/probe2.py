from portage.dep import Atom
from portage.exception import InvalidAtom
def t(s, **kw):
    try:
        a = Atom(s, **kw)
        print("OK ", repr(s), kw.get('eapi'), {k:v for k,v in kw.items() if k!='is_valid_flag'}, "cp=%s ver=%s op=%s bid=%s repo=%s ext=%s use=%s" % (a.cp, a.version, a.operator, a.build_id, a.repo, a.extended_syntax, a.use and str(a.use)))
    except InvalidAtom as e:
        print("ERR", repr(s), {k:v for k,v in kw.items() if k!='is_valid_flag'}, "->", str(e), "category=%s" % getattr(e,'category',None))
W=dict(allow_wildcard=True, allow_repo=True)
for s in ["*/foo::gentoo","*/*:0","dev-*/foo","*-libs/f*o","=dev-libs/foo-1.0-2*","=*/foo-*1*","=dev-libs/fo*-*1.0*","dev-libs/foo-1.0","*/foo-1.0","*/foo[bar]","!*/foo","dev-libs/foo**","=dev-libs/foo-**","**/foo","=dev-libs/foo-1.0-2:0::r[bar]","~dev-libs/foo-1.0-2","<dev-libs/foo-1.0-2","=dev-libs/foo-1.0-2:0[bar]","=dev-libs/foo-1.0-0","=dev-libs/foo-1.0-2147483648999","=dev-libs/foo-1*::r", "dev-libs/foo:0/1::r", "dev-libs/foo:=::r", "*/foo:0*", "*/*::gentoo"]:
    t(s, **W)
iv = lambda f: f in ("a",)
for s in ["dev-libs/foo[b?]","dev-libs/foo[!b=]","dev-libs/foo[a?,b=]","dev-libs/foo[b]","dev-libs/foo[-b(+)]"]:
    t(s, eapi="8", is_valid_flag=iv)
    t(s, is_valid_flag=iv)
t("=dev-libs/foo-1.0-2", eapi="8", allow_build_id=True)
t("=dev-libs/foo-1.0-2", allow_build_id=False)
t("dev-libs/foo::gentoo", eapi="8", allow_repo=True)
t("!!dev-libs/foo", eapi="1")
t("dev-libs/foo:0", eapi="0")
t("dev-libs/foo:0=", eapi="4")
t("dev-libs/foo[a(+)]", eapi="3")
t("dev-libs/foo[a]", eapi="1")
t("dev-libs/foo:0/1", eapi="4")
t("dev-libs/foo:0/1", eapi="9-pre1")
t("dev-libs/foo", eapi=8)
