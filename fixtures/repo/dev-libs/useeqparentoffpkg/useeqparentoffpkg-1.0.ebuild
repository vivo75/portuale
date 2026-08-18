EAPI=8
DESCRIPTION="fixture package: own IUSE default-disables eqflag; RDEPENDs on the same [eqflag=] use-dep, which evaluates to [-eqflag] since eqflag is OFF here -- must fail to match useeqchildpkg's own default-enabled eqflag"
SLOT="0"
KEYWORDS="amd64"
IUSE="eqflag"
RDEPEND="dev-libs/useeqchildpkg[eqflag=]"
