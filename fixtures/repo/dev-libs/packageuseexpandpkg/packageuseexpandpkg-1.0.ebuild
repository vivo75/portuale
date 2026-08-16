EAPI=8
DESCRIPTION="fixture package: package.use's own USE_EXPAND-prefix shorthand (PYTHON_TARGETS: python3_12) expands to python_targets_python3_12, exactly like an ordinary package.use-enabled flag would"
SLOT="0"
KEYWORDS="amd64"
IUSE="python_targets_python3_12"
RDEPEND="python_targets_python3_12? ( dev-libs/newpkg )"
