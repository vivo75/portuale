import sys, os
from portage.dep import Atom, _c_dep_parser
from portage.exception import InvalidAtom
cases = """dev-libs/foo
dev-libs/foo::gentoo
=dev-libs/foo-1.0-2
=dev-libs/foo-1.0-02
=dev-libs/foo-1.0-r1-2
>=dev-libs/foo-1.0-2
*/foo
dev-libs/*
*/*
=dev-libs/foo-1*
>=dev-libs/foo-1*
=dev-libs/foo-1.0-r1*
=dev-libs/foo-*1*
dev-libs/foo:
dev-libs/foo:=
dev-libs/foo:0/1=
dev-libs/foo:0/1
dev-libs/foo:*
dev-libs/foo:0*
dev-libs/foo:0=
dev-libs/foo:/1
dev-libs/foo:.a
dev-libs/foo:-a
dev-libs/foo:_a
dev-libs/foo:+a
dev-libs/foo:a+
dev-libs/foo[bar(+)]
dev-libs/foo[bar(-)?]
dev-libs/foo[-bar?]
dev-libs/foo[!bar]
dev-libs/foo[!bar?]
dev-libs/foo[!bar=]
dev-libs/foo[bar=]
dev-libs/foo[-bar]
dev-libs/foo[bar,bar(+)]
dev-libs/foo[bar(+),bar(-)]
dev-libs/foo[bar(+),-bar(+)]
dev-libs/foo[_bar]
dev-libs/foo[bar@x]
dev-libs/foo[]
dev-libs/foo[bar,]
dev-libs/foo[bar][baz]
dev-libs/foo:0[bar]::gentoo
dev-libs/foo:0::gentoo[bar]
!dev-libs/foo
!!dev-libs/foo
!!>=dev-libs/foo-1
!!!dev-libs/foo
dev-libs/foo-1
dev-libs/foo-1-r1
dev-libs/foo-bar-1x
>=dev-libs/foo
=dev-libs/foo-1.0_alpha1_beta2-r3
=dev-libs/foo-1.0_alpha1_beta2-r3_p1
=dev-libs/foo-01.002
=dev-libs/foo-1.0a
=dev-libs/foo-1.0ab
=dev-libs/foo-1.0_p
=dev-libs/foo-1.0-r
=dev-libs/foo-1.0-r01
~dev-libs/foo-1.0-r1
dev-libs/Foo+bar
dev-libs/+foo
+dev/foo
-cat/pkg
cat/-pkg
cat/pkg-
cat/pkg--
.cat/pkg
cat./pkg
_cat/_pkg
cat/pkg.x
cat/pkg-1a
cat/81-libretro
dev-libs/foo::-foo
dev-libs/foo::foo_bar
dev-libs/foo::foo-bar
dev-libs/foo::_x
dev-libs/foo::
dev-libs/foo/bar
foo
virtual/foo-1.0:0
=virtual/foo-1.0:0""".split("\n")
modes = [
 ("None", dict()),
 ("e5", dict(eapi="5")),
 ("e7", dict(eapi="7")),
 ("e8", dict(eapi="8")),
 ("e9", dict(eapi="9")),
 ("eX", dict(eapi="10")),
 ("e8+bid", dict(eapi="8", allow_build_id=True)),
 ("None-repo", dict(allow_repo=False, allow_build_id=False)),
 ("wild", dict(allow_wildcard=True, allow_repo=True)),
]
tag = "C" if _c_dep_parser is not None else "PY"
for s in cases:
    row=[]
    for name, kw in modes:
        try:
            a = Atom(s, **kw)
            extra = []
            if a.repo: extra.append("repo="+a.repo)
            if a.build_id is not None: extra.append("bid=%s"%a.build_id)
            if a.extended_syntax: extra.append("ext")
            if a.slot or a.slot_operator: extra.append("slot=%s/%s%s"%(a.slot,a.sub_slot,a.slot_operator or ""))
            if a.operator: extra.append("op="+a.operator)
            r = "OK" + ("("+",".join(extra)+")" if extra else "")
        except InvalidAtom as e:
            r = "ERR[" + str(e) + "]"
        except Exception as e:
            r = "EXC[%s:%s]" % (type(e).__name__, e)
        row.append("%s=%s" % (name, r))
    print(tag, repr(s), " | ".join(row))
