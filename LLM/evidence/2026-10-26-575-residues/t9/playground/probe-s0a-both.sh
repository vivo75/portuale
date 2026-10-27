#!/bin/bash
# Task 9 S0a: can one installed consumer's `||` be evaluated in the normal walk AND
# again in complete mode in the same pass? Cell: R4-like root (repo = fixture copy)
# where an installed pin forces mlocaml-4.01.0, so _solve_non_slot_operator_slot_conflicts
# removes the 4.02.1 update and re-walks its Package parents (D:2073-2100).
# Consumers are installed-only cpns (no tree ebuild), so dynamic deps cannot
# replace the vdb RDEPEND.
# Usage: WORK=<scratch dir with fix/ and r4.cfg from probe-s0a.sh> bash probe-s0a-both.sh
set -u
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd); OUT=$HERE/../captures; WORK=${WORK:?}
vdb() { local root=$1 cat=$2 pf=$3 slot=$4; shift 4; local d=$root/var/db/pkg/$cat/$pf; mkdir -p "$d"
  echo "$cat" > "$d/CATEGORY"; echo "$slot" > "$d/SLOT"; echo testrepo > "$d/repository"
  local kv; for kv in "$@"; do printf '%s\n' "${kv#*=}" > "$d/${kv%%=*}"; done; }
C=$WORK/both; rm -rf "$C" "$C.cfg"; mkdir -p "$C/var/lib/portage"; cp -a "$WORK/r4.cfg" "$C.cfg"
printf 'dev-libs/mlocaml\ndev-libs/t9cons\ndev-libs/a9pin\n' > "$C/var/lib/portage/world"
vdb "$C" dev-libs mlocaml-4.01.0 0/4.01.0 EAPI=7
vdb "$C" dev-libs t9cons-1 0 EAPI=7 'RDEPEND=dev-libs/mlocaml || ( =dev-libs/p569prov-5.40* ~dev-libs/p569core-1.0 )'
vdb "$C" dev-libs p569prov-5.42 0/5.42 EAPI=8
vdb "$C" dev-libs a9pin-1 0 EAPI=7 'RDEPEND=<dev-libs/mlocaml-4.02'
for deep in "--deep" ""; do
  name=both${deep:+-deep}; name=${name}${deep:-"-nodeep"}; name=${name/--deep/}
  args=(--ignore-default-opts --getbinpkg=n --usepkg=n --color=n -pv --update $deep @world)
  { echo "### argv: PORTAGE_CONFIGROOT=\$W/both.cfg ROOT=\$W/both PYTHONHASHSEED=0 T9_CP='t9cons' python3 t9/playground/views.py ${args[*]}"
    PORTAGE_CONFIGROOT=$C.cfg ROOT=$C PORTAGE_RUNNING_ROOT=$C DISTDIR=$WORK/fix/distfiles PORTAGE_TMPDIR=$WORK/tmp \
      PYTHONHASHSEED=0 T9_CP=t9cons python3 "$HERE/views.py" "${args[@]}" 2>&1
    echo "### rc: $?"; } | grep -E '^###|^T9 |^\[ebuild|backtrack:|skipped due|^!!! [^RS]' | sed -e "s#$WORK#\$W#g" -e 's/depth=<object[^>]*>/depth=UNREACHABLE/' | cut -c1-700 > "$OUT/s0a-$name.txt"
done
