EAPI=8
DESCRIPTION="fixture package: --debug's real PORTAGE_DEBUG plumbing (task #56) -- records the exported value so a test can prove it, without capturing bash's own set -x trace output"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	echo -n "${PORTAGE_DEBUG}" > "${T}/portage-debug-value.txt" || die
}
