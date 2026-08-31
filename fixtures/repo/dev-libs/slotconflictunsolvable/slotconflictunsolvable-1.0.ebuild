EAPI=8
DESCRIPTION="fixture package: pulls in dev-libs/slotconflictnewpin and dev-libs/slotconflictoldpin, whose RDEPENDs on dev-libs/slotconflicttarget:0 have no common satisfying version (>=2.0 vs <2.0) -- an unsolvable slot conflict that backtracking cannot reconcile"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="dev-libs/slotconflictnewpin dev-libs/slotconflictoldpin"
