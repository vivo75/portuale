EAPI=8
DESCRIPTION="fixture package: #37 S2 resolved phase env reaches keepdir (SLOT) and the phase itself (USE/FEATURES)"
SLOT="0"
KEYWORDS="amd64"
IUSE=""

src_install() {
	# The external `ebuild-helpers/keepdir` subprocess reads `${SLOT}`
	# from its exported environment: `.keep_<cat>_<pn>-<slot>` is the
	# observable proof the resolved SLOT reached it.
	keepdir /var/lib/phaseenvtest
	# The resolved USE/SLOT/FEATURES the phase itself sees, for direct
	# assertions: the per-phase extra_env must win over the base env.
	echo "USE=${USE}" > "${T}/phase-env-use.txt" || die
	echo "SLOT=${SLOT}" > "${T}/phase-env-slot.txt" || die
	echo "FEATURES=${FEATURES}" > "${T}/phase-env-features.txt" || die
}
