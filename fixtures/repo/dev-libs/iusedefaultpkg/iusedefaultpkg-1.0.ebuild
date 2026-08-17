EAPI=8
DESCRIPTION="fixture package: real IUSE +/- default markers, unmentioned by any other USE source"
SLOT="0"
KEYWORDS="amd64"
IUSE="+enableddefault -disableddefault plainflag"
REQUIRED_USE="enableddefault !disableddefault"
