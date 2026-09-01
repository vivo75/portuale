EAPI=8
DESCRIPTION="fixture package: FEATURES=sandbox (SCOPE_BACKLOG Part 2.D) -- src_install writes a legit file into \${D} and also attempts a write outside the build tree, which the sandbox binary must deny"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	echo "hello from fssandboxpkg" > "${T}/hello.txt" || die
	insinto /usr/share/${PN}
	doins "${T}/hello.txt"

	# The misbehaviour: a write outside ${D}/${WORKDIR}/${T}. Not fatal
	# on its own -- FEATURES=sandbox makes the `sandbox` binary log the
	# access to ${SANDBOX_LOG} and exit non-zero, failing the phase.
	# Without sandbox it just fails with EACCES (/var/lib is root-owned)
	# and is recorded here.
	local target=/var/lib/portage-pilot-sandbox-probe
	if echo escaped > "${target}" 2>"${T}/escape.err"; then
		einfo "fssandboxpkg: wrote ${target} (NOT sandboxed)"
	else
		einfo "fssandboxpkg: could not write ${target}: $(<"${T}/escape.err")"
	fi
}
