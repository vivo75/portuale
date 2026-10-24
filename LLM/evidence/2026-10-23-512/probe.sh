#!/bin/bash
# #512 oracle: scheduler items, Portage 3.0.83-r1 (host emerge) versus
# portuale, real `-j` merges in a hermetic ROOT.
#
# Tree ($T): repo `mw512` with the upstream `test_merge_wait_scope.py`
# ebuilds (glibc RDEPEND runtime-dep RDEPEND deeper-runtime-dep, sysextra,
# build-dep, unrelated DEPEND build-dep) plus timing ebuilds (slow, cycA,
# cycB, ppA, ppB). Profile `packages`: `*sys-libs/glibc`,
# `*app-misc/sysextra` (the upstream test's system set).
#
# Upstream commits (3rdparty/portage, tag portage-3.0.83; host Scheduler.py
# and _find_deep_system_runtime_deps.py are byte-identical to the tag):
#   3b1fc4238 --merge-wait-scope (deep default preserves behaviour exactly)
#   1c8b75d73 + 5157db0d2 parallel pkg_pretend (markers overlap, replayed
#     in mergelist order)
#   3d677b7b2 cycle post-merge deps not waited on (bug 452172)
#   8ac550470 + bfa615f06 SIGINT clears the merge-wait queue (bug 961044)
#   55beac58c + a6fca880a cgroup `=== Resource usage for {build,install}
#     of {cpv}: ...` + background --verbose echo
#
# NOTE (from --merge-wait-scope help text): the scope only increases
# parallelism for packages not held back by FEATURES="merge-wait" anyway,
# so the barrier shapes run with FEATURES="-merge-wait" in make.conf
# (shape 05 keeps the default FEATURES as a control).
#
# Usage: probe.sh <portage|portuale> <outdir> [only-<id>...]
# Each shape writes <id>.<pm>.{out,err,rc} (stdout timestamped per line,
# stderr raw) and <id>.<pm>.cast. Env: REBUILD=1 forces the repo rebuild.
set -u
pm=$1; out=$(readlink -f "$2"); shift 2
only="$*"
here=$(cd "$(dirname "$0")" && pwd)
FX=${FX:-$(readlink -f $here/../../../fixtures)}; T=/var/tmp/ev512-tree
mkdir -p "$out"

FAST=2
SLOW=20
CYC=8
PRETEND_SLEEP=5

eb() { # dir, name, ver, deps..., body extras via env EB_EXTRA
  local d=$1 n=$2 v=$3; shift 3
  mkdir -p "$T/repo/$d/$n"
  {
    printf 'EAPI=8\nDESCRIPTION="oracle: scheduler %s"\nHOMEPAGE="https://example.com"\nS="${WORKDIR}/src"\nLICENSE="MIT"\nSLOT="0"\nKEYWORDS="amd64"\n' "$n"
    for dep in "$@"; do printf '%s\n' "$dep"; done
    printf 'src_unpack() { mkdir -p "${S}"; }\nsrc_compile() { sleep %s; }\nsrc_install() { :; }\n' "$FAST"
  } > "$T/repo/$d/$n/$n-$v.ebuild"
}

slow_eb() { # dir, name, ver, secs, extra-dep-lines...
  local d=$1 n=$2 v=$3 s=$4; shift 4
  mkdir -p "$T/repo/$d/$n"
  {
    printf 'EAPI=8\nDESCRIPTION="oracle: scheduler %s"\nHOMEPAGE="https://example.com"\nS="${WORKDIR}/src"\nLICENSE="MIT"\nSLOT="0"\nKEYWORDS="amd64"\n' "$n"
    for dep in "$@"; do printf '%s\n' "$dep"; done
    printf 'src_unpack() { mkdir -p "${S}"; }\nsrc_compile() { sleep %s; }\nsrc_install() { :; }\n' "$s"
  } > "$T/repo/$d/$n/$n-$v.ebuild"
}

build_tree() {
rm -rf "$T"; mkdir -p "$T/cfg/etc/portage/repos.conf" "$T/profile" "$T/repo" "$T/tmp"
eb sys-libs glibc 1.0 'RDEPEND="dev-libs/runtime-dep"'
eb app-misc sysextra 1.0
eb dev-libs runtime-dep 1.0 'RDEPEND="dev-libs/deeper-runtime-dep"'
eb dev-libs deeper-runtime-dep 1.0
eb dev-libs build-dep 1.0
slow_eb app-misc unrelated 1.0 "$SLOW" 'DEPEND="dev-libs/build-dep"'
slow_eb app-misc slow 1.0 "$SLOW"
slow_eb dev-libs cycA 1.0 "$CYC" 'RDEPEND="dev-libs/cycB"'
slow_eb dev-libs cycB 1.0 "$CYC" 'PDEPEND="dev-libs/cycA"'
for p in ppA ppB; do
  mkdir -p "$T/repo/dev-libs/$p"
  cat > "$T/repo/dev-libs/$p/$p-1.0.ebuild" <<EB
EAPI=8
DESCRIPTION="oracle: scheduler $p"
HOMEPAGE="https://example.com"
S="\${WORKDIR}/src"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
pkg_pretend() {
	einfo "PRETEND-BEGIN ${p} \$(date +%s.%N)"
	einfo "PRETEND-BODY ${p}"
	sleep $PRETEND_SLEEP
	einfo "PRETEND-END ${p} \$(date +%s.%N)"
}
src_unpack() { mkdir -p "\${S}"; }
src_compile() { sleep $FAST; }
src_install() { :; }
EB
done
mkdir -p "$T/repo/profiles" "$T/repo/metadata"
printf 'sys-libs app-misc dev-libs\n' > "$T/repo/profiles/categories"
printf 'mw512\n' > "$T/repo/profiles/repo_name"
printf 'masters = testrepo\nprofile-formats = portage-2\n' > "$T/repo/metadata/layout.conf"
printf '%s\n' "$FX/repo/profiles/base" "$FX/repo/profiles/arch/amd64" > "$T/profile/parent"
printf '*sys-libs/glibc\n*app-misc/sysextra\n' > "$T/profile/packages"
ln -sfn "$T/profile" "$T/cfg/etc/portage/make.profile"
printf '[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = %s/repo\n\n[mw512]\nlocation = %s/repo\n' "$FX" "$T" > "$T/cfg/etc/portage/repos.conf/r.conf"
printf 'FEATURES="-news -sandbox -usersandbox -merge-wait"\nACCEPT_LICENSE="*"\nACCEPT_KEYWORDS="amd64"\nPORTAGE_TMPDIR="%s/tmp"\n' "$T" > "$T/cfg/etc/portage/make.conf"
printf 'FEATURES="-news -sandbox -usersandbox"\nACCEPT_LICENSE="*"\nACCEPT_KEYWORDS="amd64"\nPORTAGE_TMPDIR="%s/tmp"\n' "$T" > "$T/cfg/etc/portage/make.conf.default-features"
mkdir -p "$T/root"
export PORTAGE_CONFIGROOT=$T/cfg ROOT=$T/root
for f in $(find "$T/repo" -name '*.ebuild'); do ebuild "$f" manifest >/dev/null 2>&1 || { echo "MANIFEST FAILED $f"; exit 1; }; done
touch "$T/.built"
}

reset_root() {
  rm -rf "$T/root" "$T/tmp"; mkdir -p "$T/root" "$T/tmp"
}

if [ "${REBUILD:-0}" = 1 ] || [ ! -f "$T/.built" ]; then build_tree; fi

export PORTAGE_CONFIGROOT=$T/cfg ROOT=$T/root PORTAGE_TMPDIR=$T/tmp
if [ "$pm" = portage ]; then bin=$(command -v emerge)
else mkdir -p /var/tmp/ev512-bin; ln -sf "$(readlink -f ${PORTUALE_BIN:-$here/../../../target/release/portuale})" /var/tmp/ev512-bin/emerge; bin=/var/tmp/ev512-bin/emerge; fi

want() { # true when no filter or id is listed
  [ -z "$only" ] && return 0
  case " $only " in *" $1 "*) return 0;; *) return 1;; esac
}

# run_shape <id> <timeout-s> -- <emerge args...>
# stdout is timestamped per line ( sleeps are >= 2 s, 1 s resolution is
# plenty); stderr stays raw. A timeout writes rc 124 like `timeout(1)`.
run_shape() {
  local id=$1 timeout_s=$2; shift 2
  [ "$1" = -- ] && shift
  want "$id" || return 0
  reset_root
  # Shape 05 runs under the default FEATURES (with merge-wait); every
  # other shape strips it so the scope alone decides who barriers.
  if [ "$id" = 05-default-features ]; then
    cp "$T/cfg/etc/portage/make.conf.default-features" "$T/cfg/etc/portage/make.conf"
  else
    printf 'FEATURES="-news -sandbox -usersandbox -merge-wait"\nACCEPT_LICENSE="*"\nACCEPT_KEYWORDS="amd64"\nPORTAGE_TMPDIR="%s/tmp"\n' "$T" > "$T/cfg/etc/portage/make.conf"
  fi
  echo "--- $id [$pm]: $bin $*"
  (cd "$T" && env ROOT=$T/root PORTAGE_CONFIGROOT=$T/cfg PORTAGE_TMPDIR=$T/tmp \
    timeout "$timeout_s" stdbuf -o0 -e0 "$bin" "$@" \
    2>"$out/$id.$pm.err" | awk '{ print strftime("%H:%M:%S"), $0; fflush() }' >"$out/$id.$pm.out"; echo "${PIPESTATUS[0]}" >"$out/$id.$pm.rc")
  (cd "$T" && env ROOT=$T/root PORTAGE_CONFIGROOT=$T/cfg PORTAGE_TMPDIR=$T/tmp \
    asciinema rec --headless --overwrite -q -c "$bin $*" "$out/$id.$pm.cast" >/dev/null 2>&1) || true
  echo "    rc $(cat $out/$id.$pm.rc)"
}

ATOMS_SCOPE="=sys-libs/glibc-1.0 =app-misc/slow-1.0"
run_shape 01-scope-deep 120 -- --oneshot --color=n --jobs=2 --merge-wait-scope=deep $ATOMS_SCOPE
run_shape 02-scope-system 120 -- --oneshot --color=n --jobs=2 --merge-wait-scope=system $ATOMS_SCOPE
run_shape 03-scope-toolchain 120 -- --oneshot --color=n --jobs=2 --merge-wait-scope=toolchain $ATOMS_SCOPE
run_shape 04-scope-none 120 -- --oneshot --color=n --jobs=2 --merge-wait-scope=none $ATOMS_SCOPE
run_shape 05-default-features 120 -- --oneshot --color=n --jobs=2 $ATOMS_SCOPE
run_shape 06-deep-closure 150 -- --oneshot --color=n --jobs=2 --merge-wait-scope=deep =sys-libs/glibc-1.0 =app-misc/sysextra-1.0 =app-misc/slow-1.0
run_shape 07-system-closure 150 -- --oneshot --color=n --jobs=2 --merge-wait-scope=system =sys-libs/glibc-1.0 =app-misc/sysextra-1.0 =app-misc/slow-1.0
run_shape 08-toolchain-sysextra 120 -- --oneshot --color=n --jobs=2 --merge-wait-scope=toolchain =app-misc/sysextra-1.0 =app-misc/slow-1.0
run_shape 09-cycle 150 -- --oneshot --color=n --jobs=2 =dev-libs/cycA-1.0 =dev-libs/cycB-1.0
run_shape 10-pretend 120 -- --oneshot --color=n --jobs=2 =dev-libs/ppA-1.0 =dev-libs/ppB-1.0
run_shape 11-bad-scope 60 -- --oneshot --color=n --merge-wait-scope=bogus =app-misc/slow-1.0
run_shape 12-missing-scope 60 -- --oneshot --color=n --merge-wait-scope

# Shapes 15-17 (Task 8, backlog #544-#546): pretend scheduling. PKGDIR lives
# under $T so the buildpkgonly/mixed shapes never touch the host store.
export PKGDIR=$T/pkgdir
mkdir -p "$T/pkgdir"
# 15: serial `-j1` pretend runs up front, in order, before any build.
run_shape 15-pretend-j1 150 -- --oneshot --color=n --jobs=1 =dev-libs/ppA-1.0 =dev-libs/ppB-1.0
# 16: `--buildpkgonly -j2` replays both pretends up front, then builds.
run_shape 16-pretend-buildpkgonly 150 -- --oneshot --color=n --buildpkgonly --jobs=2 =dev-libs/ppA-1.0 =dev-libs/ppB-1.0
# 17: mixed `-k -j2`: ppA prebuilt to $PKGDIR (same-$pm build, ppB stays
# source-only), then the binary pretend (from the binpkg metadata) and the
# source pretend replay up front in merge-list order, before either merge.
if want 17-mixed-pretend; then
  rm -rf "$T/pkgdir"; mkdir -p "$T/pkgdir"
  (cd "$T" && env ROOT=$T/root PORTAGE_CONFIGROOT=$T/cfg PORTAGE_TMPDIR=$T/tmp PKGDIR=$T/pkgdir \
    timeout 120 $bin --oneshot --color=n --buildpkgonly =dev-libs/ppA-1.0 >/dev/null 2>&1)
fi
run_shape 17-mixed-pretend 150 -- --oneshot --color=n -k --jobs=2 =dev-libs/ppA-1.0 =dev-libs/ppB-1.0
