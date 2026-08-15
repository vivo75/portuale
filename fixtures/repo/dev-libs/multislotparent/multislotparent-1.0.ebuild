EAPI=8
DESCRIPTION="fixture package: RDEPENDs on two explicit slots of dev-libs/multislotpkg, proving they resolve as two independent, coexisting graph entries rather than one silently overwriting the other"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="dev-libs/multislotpkg:0 dev-libs/multislotpkg:1"
