#!/bin/bash
# t348 S0.6 (worker c): probe fixture generator, part B (rewritten, one
# package per family+phase; see c-gen-a.sh header for the rationale).
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

# ---- 12. econf flags ----
body_econf() { cat <<'EBEOF'
t_econf() {
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="
	mkdir -p "${S}" || die
	cd "${S}" || die
	{
	printf '%s\n' '#!/bin/bash'
	printf '%s\n' 'if [[ $1 == --help ]]; then'
	printf '%s\n' '  printf "%s\n" --datarootdir --disable-dependency-tracking --disable-silent-rules --enable-shared --enable-static --docdir --htmldir --with-sysroot'
	printf '%s\n' '  exit 0'
	printf '%s\n' 'fi'
	printf '%s\n' 'echo "FAKE-CONFIGURE-ARGV: $*"'
	printf '%s\n' 'exit 0'
	} > configure || die
	chmod +x configure || die
	( econf --with-extra-flag )
	tlog "econf: rc=$?"
}
EBEOF
}

# ---- 13. unpack formats + case ----
body_unpack() { cat <<'EBEOF'
t_unpack() {
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="
	cd "${WORKDIR}" || die
	rm -rf up uout && mkdir up uout || die
	echo content > up/plain.txt || die
	tar czf up/t.tar.gz -C up plain.txt || die
	tar cJf up/t.txz -C up plain.txt || die
	xz -c up/plain.txt > up/p.xz || die
	cp up/t.tar.gz up/up.TGZ || die
	printf 'fake-7z-content' > up/f.7z || die
	printf 'fake-rar-content' > up/f.rar || die
	mkdir -p uout || die
	cd uout || die
	( unpack "${WORKDIR}/up/t.tar.gz" )
	tlog "unpack abs tar.gz: rc=$?"
	( unpack "${WORKDIR}/up/t.txz" )
	tlog "unpack abs txz: rc=$?"
	( unpack "${WORKDIR}/up/p.xz" )
	tlog "unpack abs xz: rc=$?"
	( unpack "${WORKDIR}/up/up.TGZ" )
	tlog "unpack abs uppercase TGZ (case-sensitive in 7/8): rc=$?"
	( unpack "${WORKDIR}/up/f.7z" )
	tlog "unpack abs 7z (7:yes/8:no): rc=$?"
	( unpack "${WORKDIR}/up/f.rar" )
	tlog "unpack abs rar (7:yes/8:no): rc=$?"
	( unpack t.tar.gz )
	tlog "unpack bare name (DISTDIR miss): rc=$?"
	cd "${WORKDIR}" || die
	( unpack ./up/t.tar.gz )
	tlog "unpack ./-relative: rc=$?"
	tlog "uout-listing:"; find uout | sort >> "${LOG}"; find uout | sort
}
EBEOF
}

# ---- 14. best_version / has_version (IPC path) ----
body_verq() { cat <<'EBEOF'
t_verq() {
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="
	tlog "PORTAGE_IPC_DAEMON=[${PORTAGE_IPC_DAEMON-unset}]"
	( has_version sys-apps/portage )
	tlog "has_version sys-apps/portage (hermetic vdb empty): rc=$?"
	( has_version -b sys-apps/portage )
	tlog "has_version -b: rc=$?"
	( has_version -d sys-apps/portage )
	tlog "has_version -d: rc=$?"
	( has_version -r sys-apps/portage )
	tlog "has_version -r: rc=$?"
	( best_version sys-apps/portage )
	tlog "best_version: rc=$? out=[$(best_version sys-apps/portage)]"
	( has_version --host-root sys-apps/portage )
	tlog "has_version --host-root (EAPI 5/6 only): rc=$?"
	( best_version '!!!invalid-atom!!!' )
	tlog "best_version invalid atom: rc=$?"
}
EBEOF
}

# ---- 15. die / nonfatal / assert / pipestatus ----
body_die() { cat <<'EBEOF'
t_die() {
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="
	( die "t348-plain-die" )
	tlog "die msg: rc=$?"
	( true | true; assert "t348-assert-pass" )
	tlog "assert pass: rc=$?"
	( false | true; assert "t348-assert-fail" )
	tlog "assert fail: rc=$?"
	tlog "declare pipestatus: [$(declare -F pipestatus)]"
	( pipestatus )
	tlog "pipestatus call: rc=$?"
	true | false; ( pipestatus -v )
	tlog "pipestatus -v after true|false: rc=$?"
	( die -n "t348-die-dashn-no-nonfatal" )
	tlog "die -n without nonfatal: rc=$?"
	( nonfatal die -n "t348-die-dashn" )
	tlog "nonfatal die -n: rc=$?"
	nonfatal die "t348-nonfatal-die"
	tlog "after nonfatal die: rc=$? (phase continues)"
}
EBEOF
}

# ---- 16. ENV_UNSET ----
body_envunset() { cat <<'EBEOF'
t_envunset() {
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="
	tlog "ENV_UNSET=[${ENV_UNSET-unset}]"
	tlog "T348_SENTINEL=[${T348_SENTINEL-unset}]"
}
EBEOF
}

# ---- 17. keepdir ----
body_keepdir() { cat <<'EBEOF'
t_keepdir() {
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="
	( keepdir /var/lib/t348c )
	tlog "keepdir new: rc=$?"
	( keepdir /var/lib/t348c )
	tlog "keepdir existing: rc=$?"
	tlog "D-listing:"; find "${D}" | sort >> "${LOG}"; find "${D}" | sort
}
EBEOF
}

# ---- 18. failglob in global scope ----
body_failglob() { cat <<'EBEOF'
src_unpack() { :; }
src_prepare() { eapply_user; }
src_configure() { :; }
src_compile() { einfo "FAILGLOB-COMPILE-REACHED"; }
src_test() { :; }
src_install() { :; }
EBEOF
}

# ---- 19. banned: dohtml / dosed / dohard ----
body_banned() { cat <<'EBEOF'
t_banned() {
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="
	cd "${S}" || die
	echo '<html></html>' > a.html || die
	echo content > f.txt || die
	( dohtml a.html )
	tlog "dohtml (banned 7/8): rc=$?"
	( dosed 's/content/changed/' f.txt )
	tlog "dosed (banned 4+): rc=$?"
	( dohard /usr/bin/a /usr/bin/b )
	tlog "dohard (banned 4+): rc=$?"
}
EBEOF
}

# ---- 20. hasq / hasv / useq ----
body_hasq() { cat <<'EBEOF'
t_hasq() {
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="
	( hasq foo "foo bar" )
	tlog "hasq hit: rc=$?"
	( hasq zz "foo bar" )
	tlog "hasq miss: rc=$?"
	( hasv foo "foo bar" )
	tlog "hasv hit: rc=$? out=[$(hasv foo 'foo bar')]"
	( useq foo )
	tlog "useq foo (flag disabled): rc=$?"
	( useq )
	tlog "useq no-arg: rc=$?"
}
EBEOF
}

# ---- 21. einstall (banned 6+) ----
body_einstall() { cat <<'EBEOF'
t_einstall() {
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="
	tlog "declare einstall: [$(declare -F einstall)]"
	( einstall )
	tlog "einstall (banned 6+): rc=$?"
}
EBEOF
}

# ---- 22. insopts/exeopts (respected in 7, ignored in 8) ----
body_insopts() { cat <<'EBEOF'
t_insopts() {
	LOG="${T}/t348.log"
	: > "${LOG}"
	unset EBUILD_MASTER_PID
	tlog() { echo "$*" >> "${LOG}"; einfo "$*"; }
	tlog "=== P=${P} EAPI=${EAPI} PHASE=${EBUILD_PHASE} ==="
	cd "${S}" || die
	echo confdata > f.conf || die
	echo initdata > f.init || die
	insopts -m0600
	( doconfd f.conf )
	tlog "doconfd with insopts -m0600: rc=$?"
	exeopts -m0750
	( doinitd f.init )
	tlog "doinitd with exeopts -m0750: rc=$?"
	tlog "confd mode=[$(stat -c %a "${D}/etc/conf.d/f.conf" 2>/dev/null)]"
	tlog "initd mode=[$(stat -c %a "${D}/etc/init.d/f.init" 2>/dev/null)]"
	( doenvd f.conf )
	tlog "doenvd with insopts: rc=$?"
	tlog "envd listing:"; ls "${D}/etc/env.d/" 2>/dev/null >> "${LOG}"; ls "${D}/etc/env.d/" 2>/dev/null
}
EBEOF
}

gen() { # gen <fam> <phase> <bodyfunc> [extra head lines...]
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

gen econf configure body_econf
gen econf compile body_econf
gen unpack unpack body_unpack
gen unpack prepare body_unpack
gen verq compile body_verq
gen verq install body_verq
gen die compile body_die
gen die install body_die
gen envunset compile body_envunset
gen envunset install body_envunset
gen keepdir compile body_keepdir
gen keepdir install body_keepdir
gen banned compile body_banned
gen banned install body_banned
gen hasq compile body_hasq 'IUSE="foo"'
gen hasq install body_hasq 'IUSE="foo"'
gen einstall compile body_einstall
gen einstall install body_einstall
gen insopts compile body_insopts
gen insopts install body_insopts
# failglob: single package, sourcing fails in EAPI 7 and 8.
mkdir -p "${R348}/t348-c-failglob"
for E in 7 8; do
	{
	header t348-c-failglob "$E"
	echo 'T348_GLOB=( "${WORKDIR}"/t348-c-failglob-no-such-file-* )'
	body_failglob
	} > "${R348}/t348-c-failglob/t348-c-failglob-${E}.ebuild"
done
echo "wrote t348-c-failglob -7/-8"
