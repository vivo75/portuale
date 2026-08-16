EAPI=8
DESCRIPTION="fixture package: RDEPENDs on dev-libs/requiredusebadpkg, whose own REQUIRED_USE is violated -- proves a REQUIRED_USE violation reached only as a dependency still aborts the whole run, not just a top-level atom's own"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="dev-libs/requiredusebadpkg"
