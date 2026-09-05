EAPI=8
DESCRIPTION="fixture package: pulls in dev-libs/scusenewpin and dev-libs/scuseoldpin, whose RDEPENDs on dev-libs/scusetarget:0 (>=2.0 vs <2.0) form an unsolvable slot conflict -- used to check pkg_use_display in the slot-collision notice"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="dev-libs/scusenewpin dev-libs/scuseoldpin"
