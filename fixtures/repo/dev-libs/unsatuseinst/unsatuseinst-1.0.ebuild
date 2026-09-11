EAPI=8
DESCRIPTION="fixture: installed, matched by an AlreadyInstalled resolve; RDEPENDs the same unsatuseor || group so a --deep walk (enqueue_dependencies) hits the unsat_use_* dispatch, not just the main New/Upgrade walk"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="|| ( dev-libs/unsatusealt[unsatuseorflag] dev-libs/doesnotexist-unsatuseor )"
