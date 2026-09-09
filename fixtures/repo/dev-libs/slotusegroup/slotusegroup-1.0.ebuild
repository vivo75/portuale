EAPI=8
DESCRIPTION="fixture: pulls slotuseplain (resolves 2.0 first) plus slotusex/slotusey, whose USE-deps 2.0 cannot satisfy -- reuse fails per parent and the slot conflict fires with use-keyed parents"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="dev-libs/slotuseplain dev-libs/slotusex dev-libs/slotusey"
