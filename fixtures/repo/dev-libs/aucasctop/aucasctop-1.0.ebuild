EAPI=8
DESCRIPTION="fixture: RDEPENDs aucascmid (resolved cascade-off first) then aucasclate; aucasclate then needs aucascmid[cascade], so autounmask must re-resolve aucascmid with cascade on -- pulling in aucascleaf that a single pass would miss"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="dev-libs/aucascmid dev-libs/aucasclate"
