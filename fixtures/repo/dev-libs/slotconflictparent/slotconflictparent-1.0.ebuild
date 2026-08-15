EAPI=8
DESCRIPTION="fixture package: pulls in both dev-libs/slotconflictnewconsumer and dev-libs/slotconflictoldconsumer, whose own RDEPENDs disagree about which version of dev-libs/slotconflicttarget:0 they need"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="dev-libs/slotconflictnewconsumer dev-libs/slotconflictoldconsumer"
