EAPI=8
DESCRIPTION="fixture: || ( foo[a] foo[b] ) ordering -- unsat_use_* bins must come AFTER preferred_non_installed, so the second alternative (satisfiable by its own default) wins with no autounmask change at all"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="|| ( dev-libs/unsatuseordertarget[unsatuseorflag] dev-libs/unsatuseordertarget[unsatuseotherflag] )"
