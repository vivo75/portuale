EAPI=8
DESCRIPTION="fixture package: own IUSE default-enables eqflag; RDEPENDs on a [eqflag=] use-dep, which evaluates to [eqflag] since eqflag is ON here"
SLOT="0"
KEYWORDS="amd64"
IUSE="+eqflag"
RDEPEND="dev-libs/useeqchildpkg[eqflag=]"
