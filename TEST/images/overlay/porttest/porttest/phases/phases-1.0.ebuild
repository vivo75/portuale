# synthetic L1 merge-parity fixture -- see ../../README.md
EAPI=8
DESCRIPTION="porttest: every pkg_* phase writes a marker (merge phase order + env)"
HOMEPAGE="https://example.invalid/porttest"
SLOT="0"
KEYWORDS="amd64"
LICENSE="GPL-2"
S="${WORKDIR}"

# a marker dir under ROOT that the merge-time phases append to. The
# comparison: pkg_preinst / pkg_postinst run at merge for BOTH PMs, so
# the lines they append (and their order) must match. pkg_setup runs
# too. pkg_pretend does NOT run for a binpkg merge (real portage).
PT_LOG='${ROOT%/}/var/lib/porttest/phase.log'

_pt_mark() {
	local d="${ROOT%/}/var/lib/porttest"
	mkdir -p "${d}"
	printf '%s eapi=%s ebuild_phase=%s merge_type=%s p=%s pf=%s\n' \
		"$1" "${EAPI}" "${EBUILD_PHASE:-?}" "${MERGE_TYPE:-?}" "${P}" "${PF}" \
		>> "${d}/phase.log"
}

pkg_pretend()  { _pt_mark pretend; }
pkg_setup()    { _pt_mark setup; }
pkg_preinst()  { _pt_mark preinst; }
pkg_postinst() { _pt_mark postinst; }
pkg_prerm()    { _pt_mark prerm; }
pkg_postrm()   { _pt_mark postrm; }
pkg_config()   { _pt_mark config; }
pkg_info()     { _pt_mark info; }

src_install() {
	dodir /var/lib/porttest
	# a payload so CONTENTS isn't empty
	echo "phases fixture" > "${ED}"/var/lib/porttest/phases.txt
}
