#!/bin/bash
# usage: probe.sh OUTFILE ; frozen fixtures /var/tmp/765fx, scratch roots /var/tmp/765roots (mkroots.sh),
# binary /tmp/opencode/765bin/emerge (symlink to portuale). Every control runs on BOTH solvers
# (default = reference side, pg = --solver=pubgrub). PORTAGE_RUNNING_ROOT and DISTDIR as pmtest's fixture_env.
F=/var/tmp/765fx
export PORTAGE_CONFIGROOT=$F
export PORTAGE_REPOSITORIES="$(cat $(find $F/etc/portage/repos.conf -type f | sort) | awk 1)"
OUT=$1; : > $OUT
E=/tmp/opencode/765bin/emerge
TMP=$(mktemp -d)
run() { # ROOTNAME SOLVERLABEL argv...   (SOLVERLABEL df|pg ; TRACE prefix on argv via env TR=1)
  local rn=$1 sl=$2; shift 2
  local root=$F; [ "$rn" != fx ] && root=/var/tmp/765roots/$rn
  local extra=(); [ "$sl" = pg ] && extra=(--solver=pubgrub)
  env ROOT=$root PORTAGE_RUNNING_ROOT=$root DISTDIR=$F/distfiles ${TR:+PORTUALE_PG_TRACE=1} "$E" "${extra[@]}" "$@" >$TMP/o 2>$TMP/e; local rc=$?
  echo "argv: [$rn/$sl]${TR:+ TRACE} emerge ${extra[*]} $*" >>$OUT
  echo "rc=$rc stdout=$(sha256sum <$TMP/o|cut -d' ' -f1) stderr=$(sha256sum <$TMP/e|cut -d' ' -f1) note=$(grep -c 'stale slot-operator binding' $TMP/o) jsonkey=$(grep -c '"stale-bindings"' $TMP/o) trace_lines=$(grep -c '^pg-' $TMP/e)" >>$OUT
  if grep -q 'stale slot-operator binding' $TMP/o; then grep -B1 -A4 'stale slot-operator binding' $TMP/o | sed 's/^/  | /' >>$OUT; fi
  echo >>$OUT
}
both() { local rn=$1; shift; run $rn df "$@"; run $rn pg "$@"; }
P=--pretend
# witness + stale-note variants, shared fixtures
for a in "--update dev-libs/p569prov" "--quiet --update dev-libs/p569prov" "--json --update dev-libs/p569prov" "--json dev-libs/newpkg" \
         "dev-libs/p569prov" "--deep dev-libs/p569prov" "--update --deep dev-libs/p569prov" "--emptytree dev-libs/p569prov" \
         "dev-libs/p569pull dev-libs/p569prov" "--update dev-libs/p569pull dev-libs/p569prov" "--update --deep dev-libs/p569pull dev-libs/p569prov" \
         "dev-libs/p569prov dev-libs/p569core" "--update --deep @world" "--update --deep dev-libs/p569core"; do
  both fx $P $a
done
TR=1 run fx pg $P --update dev-libs/p569prov
# 04.755 / 02.767 controls (argv without solver flag)
for a in "dev-libs/newpkg" "dev-libs/diamond" "dev-libs/slotconfgroup" "dev-libs/maskedpkg" "dev-libs/graphblockerparent" \
         "dev-libs/blockerpkg" "dev-libs/hardcyclea" "dev-libs/slotconflicttarget" "-v dev-libs/pkgusemaskforcepkg" \
         "-v --update --deep dev-libs/pg34-gapp" "-v --update --deep dev-libs/pg34-arg" "-v dev-libs/pg34-arg" "-v --emptytree dev-libs/diamond"; do
  both fx $P $a
done
# #698/#727 root (every holder has a route)
for a in "dev-libs/p727top" "dev-libs/p727top dev-libs/p569prov" "--update --deep dev-libs/p727top" "--update dev-libs/p569prov" \
         "--update --deep @world" "dev-libs/p727top2" "dev-libs/p569prov"; do
  both r698 $P $a
done
# #703 root (a holder without a route: holder_fix note must never name the same consumer as the new note)
for a in "dev-libs/p569prov" "--update dev-libs/p569prov" "--quiet dev-libs/p569prov" "--update --deep @world" "--json dev-libs/p569prov"; do
  both r703 $P $a
done
rm -rf $TMP
