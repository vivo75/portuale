EAPI=8
DESCRIPTION="fixture: slice 2 in-bin ordering -- || ( oringraphinstalled oringraphpulled ) ties at the Installed bin (cp-installed vs already-in-graph); real promotes the in-graph one over the first-listed installed-only one"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="|| ( dev-libs/oringraphinstalled dev-libs/oringraphpulled )"
