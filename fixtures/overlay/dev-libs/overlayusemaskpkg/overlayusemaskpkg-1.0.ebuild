EAPI=8
DESCRIPTION="fixture package: its IUSE default-on overlaymaskflag is masked off only because the OVERLAY's own profiles/package.use.mask masks it -- without that entry the flag would default on and pull the dependency in"
SLOT="0"
KEYWORDS="amd64"
IUSE="+overlaymaskflag"
RDEPEND="overlaymaskflag? ( dev-libs/newpkg )"
