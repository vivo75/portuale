EAPI=8
DESCRIPTION="fixture package: REQUIRED_USE is unconditionally unsatisfiable"
SLOT="0"
KEYWORDS="amd64"
IUSE="+brokenflag"
REQUIRED_USE="brokenflag? ( !brokenflag )"
