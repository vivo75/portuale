EAPI=8
DESCRIPTION="fixture: slot-blind digraph edge -- RDEPENDs slotorderdual:2 explicitly; the sibling slot :1 is pulled only through slotorderb, so :1 must not gain slotorderroot as a phantom second parent"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="dev-libs/slotordera dev-libs/slotorderb dev-libs/slotorderdual:2"
