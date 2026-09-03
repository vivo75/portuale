EAPI=8
DESCRIPTION="fixture: pulls aubreaksub plain, then aubreakwant (needs aubreaksub[brk]) and aubreakunwant (needs aubreaksub[-brk]) -- autounmask can't satisfy both, so it must be abandoned (_autounmask_breakage)"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="dev-libs/aubreaksub dev-libs/aubreakwant dev-libs/aubreakunwant"
