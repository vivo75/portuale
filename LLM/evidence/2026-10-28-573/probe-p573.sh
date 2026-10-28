#!/bin/bash
# #573 S0 oracle probe: the dev-lang_perl shape — a bare provider argument whose
# higher-subslot candidate is withheld, with (a) several *in-graph* installed
# consumers recording the same menu atom and (b) *out-of-graph* installed
# consumers recording only the built `:S/SS=` atom.
#
# Mirrors the L0 `dev-lang_perl` probe: `emerge -pv dev-lang/perl` keeps
# perl-5.42.2 installed (`R`), skips 5.44.0, and Portage's skip block lists all
# 69 installed consumers — 6 virtuals that are in the graph and 63 real-package
# consumers that are not. portuale lists only one in-graph consumer's two rows.
#
# Two cells:
#   p569prov  no version change in the graph: only the in-graph pair is a
#             parent. Portage prints four rows (built + menu per consumer).
#   p573up    the same request plus a world member that upgrades `-1 -> -2` in
#             the same graph (`p573up`), which is the version change that turns
#             Portage's complete mode on; the walk then reaches the installed
#             `p573host -> p573real` deep dependency, so the out-of-graph
#             consumer `p573real-1` becomes a parent and is listed.
#
# Fixtures (repo side only; the installed vdb lives in a scratch ROOT so the
# shared fixture tree keeps its vdb):
#   dev-libs/p569prov     5.42 (0/5.42) + 5.44 (0/5.44)   the provided package
#   dev-libs/p573virt     1 (installed)                    in-graph consumer,
#                         same menu+built atom as p573world (mask-dedup half)
#   dev-libs/p573world    1 (installed)                    in-graph consumer
#   dev-libs/p573real     1 (installed)                    out-of-graph consumer
#                                                          (built atom only)
#   dev-libs/p573up       1 (installed) / 2                the settled upgrade
#                                                          that enables complete mode
#   dev-libs/p573host     1 (installed)                    world member whose
#                                                          deep dep is p573real
#
# Usage: bash LLM/evidence/2026-10-28-573/probe-p573.sh
# Out:   LLM/evidence/2026-10-28-573/captures/<pm>-<cell>.{argv,rc,stdout,stderr}
set -u
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
OUT=${OUT:-$HERE/captures}
WORK=${WORK:-$OUT/work}
mkdir -p "$OUT" "$WORK"

PORTUALE_BIN=${PORTUALE_BIN:-$(cd "$HERE/../../.." && pwd)/target/release/portuale}
FIX=$(cd "$HERE/../../../fixtures" && pwd)

# -- scratch ROOTs: shared fixture vdb plus the #573 installed consumers ------
# The bare-provider and control cells keep the S0 ROOT; the complete-mode cell
# needs the settled upgrade and its reachability host beside the same pair.
mkroot() { # mkroot <dir> <world...>
  local root=$1; shift
  rm -rf "$root"
  mkdir -p "$root/var/lib/portage" "$root/var/db"
  cp -a "$FIX/var/db/pkg" "$root/var/db/pkg"
  : > "$root/var/lib/portage/world"
  printf '%s\n' "$@" > "$root/var/lib/portage/world"
}

vdb() { # vdb <root> <cpv> <rdepend>
  local d=$1/var/db/pkg/$2
  mkdir -p "$d"
  printf '%s\n' "${2%%/*}" > "$d/CATEGORY"
  printf '8\n' > "$d/EAPI"
  printf '%s\n' "$3" > "$d/RDEPEND"
  printf '0\n' > "$d/SLOT"
  printf 'testrepo\n' > "$d/repository"
}
# In-graph pair: identical recorded atoms, so both must render.
consumers() { # consumers <root>
  vdb "$1" dev-libs/p573world-1 '=dev-libs/p569prov-5.42* dev-libs/p569prov:0/5.42='
  vdb "$1" dev-libs/p573virt-1  '=dev-libs/p569prov-5.42* dev-libs/p569prov:0/5.42='
  # Out-of-graph consumer: the built atom is its only relationship to the provider.
  vdb "$1" dev-libs/p573real-1  'dev-libs/p569prov:0/5.42='
}

ROOT=$WORK/root
mkroot "$ROOT" dev-libs/p573world dev-libs/p573virt
consumers "$ROOT"

# The settled upgrade that turns complete mode on, and the world member whose
# deep dependency the complete walk reaches (`p573host -> p573real`).
ROOT_UP=$WORK/root-up
mkroot "$ROOT_UP" dev-libs/p573world dev-libs/p573virt dev-libs/p573up dev-libs/p573host
consumers "$ROOT_UP"
vdb "$ROOT_UP" dev-libs/p573up-1   'dev-libs/p569prov:0/5.42='
vdb "$ROOT_UP" dev-libs/p573host-1 'dev-libs/p573real'

mkcfg() { # scratch Portage configroot: fixture config, absolute repo paths
  local cfg=$WORK/cfg-portage
  rm -rf "$cfg"; mkdir -p "$cfg/etc"
  cp -a "$FIX/etc/portage" "$cfg/etc/portage"
  rm -f "$cfg/etc/portage/make.profile"
  ln -s "$FIX/repo/profiles/default" "$cfg/etc/portage/make.profile"
  {
    echo "[DEFAULT]"
    echo "main-repo = testrepo"
    echo
    for spec in "testrepo repo" "overlay overlay" "independentoverlay independentoverlay" "layoutmasteroverlay layoutmasteroverlay"; do
      set -- $spec
      echo "[$1]"
      echo "location = $FIX/$2"
      case $1 in
        overlay) echo "priority = 10" ;;
        independentoverlay) echo "priority = 20"; echo "masters = overlay" ;;
        layoutmasteroverlay) echo "priority = 30" ;;
      esac
      echo
    done
    echo "[repnamesection]"
    echo "location = $FIX/repnamerepo"
    echo "priority = 40"
  } > "$cfg/etc/portage/repos.conf/repos.conf"
  printf '%s\n' "$cfg"
}

CFG=$(mkcfg)

run() { # run <pm> <cell> <root> -- <args...>
  local pm=$1 cell=$2 root=$3; shift 3
  [ "${1:-}" = -- ] && shift
  local name="$pm-$cell"
  if [ "$pm" = portuale ]; then
    printf '%s\n' "portuale emerge $*" > "$OUT/$name.argv"
    PORTAGE_CONFIGROOT=$FIX ROOT=$root PORTAGE_RUNNING_ROOT=$root DISTDIR=$FIX/distfiles \
      PORTAGE_TMPDIR=$WORK/tmp-ptl \
      "$PORTUALE_BIN" emerge "$@" >"$OUT/$name.stdout" 2>"$OUT/$name.stderr"
  else
    printf '%s\n' "emerge $*" > "$OUT/$name.argv"
    PORTAGE_CONFIGROOT=$CFG ROOT=$root PORTAGE_RUNNING_ROOT=$root DISTDIR=$FIX/distfiles \
      PORTAGE_TMPDIR=$WORK/tmp-real PYTHONHASHSEED=0 \
      emerge "$@" >"$OUT/$name.stdout" 2>"$OUT/$name.stderr"
  fi
  printf '%s\n' "$?" > "$OUT/$name.rc"
}

mkdir -p "$WORK/tmp-ptl" "$WORK/tmp-real"
BASE=(--ignore-default-opts --getbinpkg=n --usepkg=n --color=n)
for pm in portage portuale; do
  # The bare-provider arg: the 5.44 candidate is considered, then withheld.
  run "$pm" p569prov "$ROOT" -- -pv "${BASE[@]}" dev-libs/p569prov
  # The complete-mode cell: the same request plus a settled upgrade, which makes
  # Portage walk the installed vdb and reach the out-of-graph consumer.
  run "$pm" p573up "$ROOT_UP" -- -pv "${BASE[@]}" -u dev-libs/p569prov dev-libs/p573up dev-libs/p573host
  # Control: the merge-bound puller shape (#568/#569) keeps its byte-identical block.
  run "$pm" p569pull "$ROOT" -- -pv "${BASE[@]}" dev-libs/p569pull
done

echo "== rc =="; for f in "$OUT"/*.rc; do printf '%s: %s\n' "$(basename "$f" .rc)" "$(cat "$f")"; done
