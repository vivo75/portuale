EAPI=8
DESCRIPTION="fixture package: same version (1.0) exists in both the overlay (priority 10) and the main repo (priority -1000, default) -- this copy's RDEPEND pulls in dev-libs/newpkg, proving the higher-priority overlay's copy is the one actually used once the tie is broken"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="dev-libs/newpkg"
