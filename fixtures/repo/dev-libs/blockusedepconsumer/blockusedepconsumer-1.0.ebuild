EAPI=8
DESCRIPTION="fixture: !!blockusedeptarget[wantblock] -- target is installed WITHOUT wantblock, so the blocker is a satisfied no-op and must not print"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="!!dev-libs/blockusedeptarget[wantblock]
	!!dev-libs/blockusedeptarget[-gone(+)]"
