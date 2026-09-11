EAPI=8
DESCRIPTION="fixture: slice 2 in-bin ordering -- || ( foo:1 foo:2 ), both slots already installed (tied at the Installed bin); real promotes the upgrade (slot 2, higher version) over the first-listed slot 1"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="|| ( dev-libs/orupgradealt:1 dev-libs/orupgradealt:2 )"
