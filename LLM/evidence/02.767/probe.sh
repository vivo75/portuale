#!/bin/bash
# usage: probe.sh OUTFILE ; fixed root and binary
R=/var/tmp/767fx
export ROOT=$R PORTAGE_CONFIGROOT=$R
export PORTAGE_REPOSITORIES="$(cat $(find $R/etc/portage/repos.conf -type f | sort) | awk 1)"
OUT=$1; : > $OUT
E=/tmp/opencode/767bin/emerge
run() { # [env VAR=1] argv...
  local tr=0; if [ "$1" = TRACE ]; then tr=1; shift; fi
  local o e rc
  if [ $tr = 1 ]; then PORTUALE_PG_TRACE=1 "$E" "$@" >/tmp/p.o 2>/tmp/p.e; rc=$?; else "$E" "$@" >/tmp/p.o 2>/tmp/p.e; rc=$?; fi
  echo "argv: ${tr/1/PORTUALE_PG_TRACE=1 }emerge $*" | sed 's/^argv: 0/argv: /' >>$OUT
  echo "rc=$rc stdout=$(sha256sum </tmp/p.o|cut -d' ' -f1) stderr=$(sha256sum </tmp/p.e|cut -d' ' -f1) closure_expanded=$(grep -c 'pg-closure.*expanded' /tmp/p.e) closure_shallow=$(grep -c 'pg-closure.*shallow' /tmp/p.e)" >>$OUT
  echo >>$OUT
}
P="--pretend"
for a in "dev-libs/newpkg" "--solver=pubgrub dev-libs/newpkg" "--solver=pubgrub dev-libs/diamond" "dev-libs/diamond" "--solver=pubgrub dev-libs/slotconfgroup" "--solver=pubgrub dev-libs/maskedpkg" "--solver=pubgrub dev-libs/graphblockerparent" "--solver=pubgrub dev-libs/blockerpkg" "--solver=pubgrub dev-libs/hardcyclea" "--solver=pubgrub dev-libs/slotconflicttarget" "-v --solver=pubgrub dev-libs/pkgusemaskforcepkg" "dev-libs/slotconfgroup" "--solver=pubgrub dev-libs/p569prov"; do run $P $a; done
X="$P -v --solver=pubgrub"
for t in "" TRACE; do
 run $t $X --update --deep dev-libs/pg34-gapp
 run $t $X --update --deep dev-libs/pg34-arg
 run $t $X dev-libs/pg34-arg
 run $t $X --emptytree dev-libs/diamond
 run $t $X --update --deep @world
done
