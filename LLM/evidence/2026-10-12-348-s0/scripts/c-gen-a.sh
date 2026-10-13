#!/bin/bash
# t348 S0.6 (worker c): probe fixture generator, part A (rewritten).
# One PACKAGE per (family, phase): app-misc/t348-c-<fam>-<phase>, versions
# 7/8 (EAPI 7/8). The test body runs in exactly ONE phase; every other
# phase is death-free (src_prepare runs eapply_user for the tagfile that
# __dyn_prepare enforces). Rationale: Portage's `ebuild <f> <phase>` runs
# the doebuild chain and STOPS at the first phase returning nonzero; a
# contained die also kills the IPC daemon, poisoning the run rc. With a
# single test phase per ebuild the target section always completes.
set -u
R348=/home/vivo/repo/PORTUALE/pmtest/fixtures/repo-348/app-misc

header() {
	echo "EAPI=$2"
	echo "DESCRIPTION=\"t348 S0.6 $1 probe (EAPI $2)\""
	echo 'SLOT="0"'
	echo 'KEYWORDS="amd64"'
	echo 'S="${WORKDIR}"'
	shift 2
	for line in "$@"; do echo "$line"; done
}

TLOG='
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="'

# ---- 1. eapply / eapply_user ----
body_eapply() { cat <<'EBEOF'
t_eapply() {
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="
	cd "${WORKDIR}" || die
	rm -rf eapply-t && mkdir eapply-t && cd eapply-t || die
	printf 'hello\n' > target.txt
	printf -- '--- a/target.txt\n+++ b/target.txt\n@@ -1 +1 @@\n-hello\n+world\n' > fix.patch
	( eapply "${WORKDIR}/eapply-t/fix.patch" )
	tlog "eapply file: rc=$? content=[$(cat target.txt)]"
	printf 'hello\n' > target2.txt
	( eapply_user )
	tlog "eapply_user: rc=$?"
}
EBEOF
}

# ---- 2. edo (absent in 7 and 8) ----
body_edo() { cat <<'EBEOF'
t_edo() {
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="
	tlog "declare edo: [$(declare -F edo)]"
	( edo true )
	tlog "edo true: rc=$?"
	( edo false )
	tlog "edo false: rc=$?"
}
EBEOF
}

# ---- 3. ver_cut / ver_rs / ver_test ----
body_ver() { cat <<'EBEOF'
t_ver() {
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="
	tlog "declare ver_cut: [$(declare -F ver_cut)]"
	tlog "ver_cut 1-2 1.2.3-r4 => [$(ver_cut 1-2 1.2.3-r4)]"
	tlog "ver_rs 1- 1.2.3 => [$(ver_rs 1- 1.2.3)]"
	( ver_test 1.2.3 -gt 1.2.2 )
	tlog "ver_test 1.2.3 -gt 1.2.2: rc=$?"
	( ver_test 1.2.3 -lt 1.2.2 )
	tlog "ver_test 1.2.3 -lt 1.2.2: rc=$?"
	( ver_cut 9-9 1.2.3 )
	tlog "ver_cut bad-range: rc=$?"
}
EBEOF
}

# ---- 4. in_iuse / usex / usev / use_enable / use_with ----
body_useflag() { cat <<'EBEOF'
t_useflag() {
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="
	tlog "declare in_iuse: [$(declare -F in_iuse)]"
	( in_iuse foo )
	tlog "in_iuse foo: rc=$?"
	( in_iuse bogusflag )
	tlog "in_iuse bogusflag: rc=$?"
	( in_iuse )
	tlog "in_iuse no-arg: rc=$?"
	tlog "usex foo => [$(usex foo)]"
	tlog "usex bogusflag => [$(usex bogusflag)]"
	tlog "usex foo A B => [$(usex foo A B)]"
	tlog "usex bogusflag A B => [$(usex bogusflag A B)]"
	( usev foo )
	tlog "usev foo: rc=$? out=[$(usev foo)]"
	( usev foo altout )
	tlog "usev foo altout (2nd arg, EAPI 8+): rc=$? out=[$(usev foo altout)]"
	tlog "use_enable foo => [$(use_enable foo)]"
	tlog "use_enable bogusflag => [$(use_enable bogusflag)]"
	tlog "use_with foo => [$(use_with foo)]"
	tlog "use_enable foo '' => [$(use_enable foo '')]"
}
EBEOF
}

# ---- 5. get_libdir ----
body_libdir() { cat <<'EBEOF'
t_libdir() {
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="
	tlog "declare get_libdir: [$(declare -F get_libdir)]"
	tlog "get_libdir => [$(get_libdir)]"
	tlog "ABI=amd64 LIBDIR_amd64=lib64 => [$(ABI=amd64 LIBDIR_amd64=lib64 get_libdir)]"
}
EBEOF
}

# ---- 6. dosym -r ----
body_dosym() { cat <<'EBEOF'
t_dosym() {
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="
	dodir /usr/bin || die
	echo data > "${WORKDIR}/realbin" || die
	cp "${WORKDIR}/realbin" "${D}/usr/bin/myapp" || die
	( dosym -r /usr/bin/myapp /usr/bin/mylink )
	tlog "dosym -r abs: rc=$?"
	( dosym -r relative-target /usr/bin/mylink2 )
	tlog "dosym -r relative-target: rc=$?"
	( dosym /usr/bin/myapp /usr/bin/mylink3 )
	tlog "dosym plain: rc=$?"
	for l in mylink mylink2 mylink3; do
		if [[ -L ${D}/usr/bin/${l} ]]; then
			tlog "readlink ${l} => [$(readlink "${D}/usr/bin/${l}")]"
		else
			tlog "${l}: not a symlink"
		fi
	done
	tlog "D-listing:"; find "${D}" | sort >> "${LOG}"; find "${D}" | sort
}
EBEOF
}

# ---- 7. dostrip / docompress ----
body_strip() { cat <<'EBEOF'
t_strip() {
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="
	tlog "declare dostrip: [$(declare -F dostrip)]"
	( dostrip /usr/bin )
	tlog "dostrip /usr/bin: rc=$?"
	( dostrip -x /usr/bin/skipped )
	tlog "dostrip -x: rc=$?"
	( docompress /usr/share/doc/${PF} )
	tlog "docompress: rc=$?"
	( docompress -x /usr/share/doc/${PF}/html )
	tlog "docompress -x: rc=$?"
	tlog "PORTAGE_DOSTRIP=[${PORTAGE_DOSTRIP[*]}]"
	tlog "PORTAGE_DOSTRIP_SKIP=[${PORTAGE_DOSTRIP_SKIP[*]}]"
	tlog "PORTAGE_DOCOMPRESS=[${PORTAGE_DOCOMPRESS[*]}]"
	tlog "PORTAGE_DOCOMPRESS_SKIP=[${PORTAGE_DOCOMPRESS_SKIP[*]}]"
}
EBEOF
}

# ---- 8. dodoc -r ----
body_dodoc() { cat <<'EBEOF'
t_dodoc() {
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="
	cd "${S}" || die
	rm -rf doc && mkdir -p doc/sub || die
	echo top > doc/a.txt || die
	echo sub > doc/sub/b.txt || die
	( dodoc doc/a.txt )
	tlog "dodoc file: rc=$?"
	( dodoc -r doc/sub )
	tlog "dodoc -r dir: rc=$?"
	tlog "D-listing:"; find "${D}" | sort >> "${LOG}"; find "${D}" | sort
}
EBEOF
}

# ---- 9. doins with symlinks ----
body_doin() { cat <<'EBEOF'
t_doin() {
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="
	cd "${S}" || die
	rm -rf ins && mkdir ins && cd ins || die
	echo data > f.txt
	ln -s f.txt link.txt
	insinto /usr/share/t348c-doin
	( doins f.txt link.txt )
	tlog "doins file+symlink: rc=$?"
	if [[ -L ${D}/usr/share/t348c-doin/link.txt ]]; then
		tlog "symlink preserved"
	else
		tlog "symlink NOT preserved"
	fi
	tlog "D-listing:"; find "${D}" | sort >> "${LOG}"; find "${D}" | sort
}
EBEOF
}

# ---- 10. newins (incl. stdin) ----
body_newins() { cat <<'EBEOF'
t_newins() {
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="
	cd "${S}" || die
	rm -rf new && mkdir new && cd new || die
	echo data > f.txt
	insinto /usr/share/t348c-newins
	( newins f.txt renamed.txt )
	tlog "newins file: rc=$?"
	echo piped-data | ( newins - piped.txt )
	tlog "newins stdin: rc=$?"
	tlog "piped content=[$(cat "${D}/usr/share/t348c-newins/piped.txt" 2>/dev/null)]"
	tlog "D-listing:"; find "${D}" | sort >> "${LOG}"; find "${D}" | sort
}
EBEOF
}

# ---- 11. einstalldocs ----
body_einstalldocs() { cat <<'EBEOF'
t_einstalldocs() {
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="
	tlog "declare einstalldocs: [$(declare -F einstalldocs)]"
	cd "${S}" || die
	echo edoc > edoc.txt || die
	echo ehtml > ehtml.html || die
	( einstalldocs )
	tlog "einstalldocs: rc=$?"
	tlog "D-listing:"; find "${D}" | sort >> "${LOG}"; find "${D}" | sort
}
EBEOF
}

# gen <fam> <phase> <testfunc> [extra head lines...]
# Package: t348-c-<fam>-<phase>. Test body runs ONLY in <phase>.
gen() {
	local fam=$1 phase=$2 tfunc=$3; shift 3
	local pkg="t348-c-${fam}-${phase}"
	mkdir -p "${R348}/${pkg}"
	for E in 7 8; do
		{
		header "$pkg" "$E" "$@"
		"$tfunc"
		case $phase in
			unpack) echo 'src_unpack() { t_'$fam'; }'; echo 'src_prepare() { eapply_user; }'; echo 'src_configure() { :; }'; echo 'src_compile() { :; }'; echo 'src_test() { :; }'; echo 'src_install() { :; }';;
			prepare) echo 'src_unpack() { :; }'; echo 'src_prepare() { eapply_user; t_'$fam'; }'; echo 'src_configure() { :; }'; echo 'src_compile() { :; }'; echo 'src_test() { :; }'; echo 'src_install() { :; }';;
			configure) echo 'src_unpack() { :; }'; echo 'src_prepare() { eapply_user; }'; echo 'src_configure() { t_'$fam'; }'; echo 'src_compile() { :; }'; echo 'src_test() { :; }'; echo 'src_install() { :; }';;
			compile) echo 'src_unpack() { :; }'; echo 'src_prepare() { eapply_user; }'; echo 'src_configure() { :; }'; echo 'src_compile() { t_'$fam'; }'; echo 'src_test() { :; }'; echo 'src_install() { :; }';;
			install) echo 'src_unpack() { :; }'; echo 'src_prepare() { eapply_user; }'; echo 'src_configure() { :; }'; echo 'src_compile() { :; }'; echo 'src_test() { :; }'; echo 'src_install() { t_'$fam'; }';;
		esac
		} > "${R348}/${pkg}/${pkg}-${E}.ebuild"
	done
	echo "wrote ${pkg} -7/-8"
}

gen eapply prepare body_eapply
gen eapply compile body_eapply
gen edo compile body_edo
gen edo install body_edo
gen ver compile body_ver
gen ver install body_ver
gen useflag compile body_useflag 'IUSE="+foo bar"'
gen useflag install body_useflag 'IUSE="+foo bar"'
gen libdir compile body_libdir
gen libdir install body_libdir
gen dosym compile body_dosym
gen dosym install body_dosym
gen strip compile body_strip
gen strip install body_strip
gen dodoc compile body_dodoc
gen dodoc install body_dodoc
gen doin compile body_doin
gen doin install body_doin
gen newins compile body_newins
gen newins install body_newins
gen einstalldocs compile body_einstalldocs 'DOCS=( edoc.txt )' 'HTML_DOCS=( ehtml.html )'
gen einstalldocs install body_einstalldocs 'DOCS=( edoc.txt )' 'HTML_DOCS=( ehtml.html )'
