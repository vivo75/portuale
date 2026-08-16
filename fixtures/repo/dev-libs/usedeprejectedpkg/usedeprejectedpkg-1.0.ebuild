EAPI=8
DESCRIPTION="fixture package: RDEPEND's own USE-dep atom is genuinely unsatisfiable (useflagpkg's own "foo" is enabled globally, so "-foo" never matches), proving a rejected dependency-level USE-dep atom reports NoVisibleCandidate for that entry without failing the whole graph"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="dev-libs/useflagpkg[-foo]"
