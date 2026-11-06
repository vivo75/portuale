from portage.tests.resolver.ResolverPlayground import ResolverPlayground
def run(orphan_rdep, request, opts, with_ebuild=False):
    eb = {
        "sys-libs/glibc-2.0": {"EAPI": "8", "SLOT": "2.2", "KEYWORDS": "x86", "BDEPEND": "dev-libs/y"},
        "virtual/libc-1": {"EAPI": "8", "SLOT": "0", "KEYWORDS": "x86", "RDEPEND": "sys-libs/glibc"},
        "dev-libs/x-1": {"EAPI": "8", "SLOT": "0", "KEYWORDS": "x86", "RDEPEND": "dev-libs/orphan"},
        "dev-libs/y-1": {"EAPI": "8", "SLOT": "0", "KEYWORDS": "x86"},
    }
    if with_ebuild:
        eb['dev-libs/orphan-1'] = {'EAPI': '8', 'SLOT': '0', 'KEYWORDS': 'x86'}
    inst = {
        "sys-libs/glibc-2.0": {"EAPI": "8", "SLOT": "2.2", "KEYWORDS": "x86", "BDEPEND": "dev-libs/y"},
        "virtual/libc-1": {"EAPI": "8", "SLOT": "0", "KEYWORDS": "x86", "RDEPEND": "sys-libs/glibc"},
        "dev-libs/x-0.5": {"EAPI": "8", "SLOT": "0", "KEYWORDS": "x86", "RDEPEND": "dev-libs/orphan"},
        "dev-libs/orphan-1": {"EAPI": "8", "SLOT": "0", "KEYWORDS": "x86", "RDEPEND": orphan_rdep},
    }
    pg = ResolverPlayground(ebuilds=eb, installed=inst, world=["dev-libs/orphan", "dev-libs/x"])
    try:
        r = pg.run(request, opts)
        print("with_ebuild=%s" % with_ebuild, repr(orphan_rdep), request, opts, "->", r.mergelist)
    finally:
        pg.cleanup()
for rd in (">=sys-libs/glibc-2.0", ""):
    run(rd, ["dev-libs/x", "dev-libs/y", "sys-libs/glibc"], {})
run(">=sys-libs/glibc-2.0", ["dev-libs/x", "dev-libs/y", "sys-libs/glibc"], {}, with_ebuild=True)
