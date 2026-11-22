#!/bin/bash
# usage: probe.sh OUTDIR [ROOT...] ; frozen fixtures /var/tmp/773/fx, scratch roots /var/tmp/773/roots (mkroots.sh),
# frozen binary /var/tmp/773/bin/{emerge,mrg}. Per root, six runs, stdout/stderr/rc kept in OUTDIR/<root>.<label>.{out,err,rc}:
#   fast      mrg --plan-world --json --sort depend          fast-txt  the same without --json
#   df        emerge --pretend --json -uDN @world            df-txt    without --json
#   pg        the same with --solver=pubgrub, PORTUALE_PG_TRACE=1 (stderr must hold ^pg- lines)   pg-txt
# Every resolver run passes --getbinpkg=n (a getbinpkg host turns bdeps off; review G9). PORTAGE_RUNNING_ROOT and DISTDIR as
# pmtest's fixture_env. EXTRA="..." appends argv to the resolver runs (e.g. EXTRA=--usepkg).
F=/var/tmp/773/fx
export PORTAGE_CONFIGROOT=$F
export PORTAGE_REPOSITORIES="$(cat $(find $F/etc/portage/repos.conf -type f | sort) | awk 1)"
OUT=$1; shift; mkdir -p "$OUT"
B=${BIN:-/var/tmp/773/bin}
ROOTS=("$@"); [ ${#ROOTS[@]} -eq 0 ] && ROOTS=($(ls /var/tmp/773/roots))
RES=(--pretend --update --deep --newuse --getbinpkg=n $EXTRA)
for rn in "${ROOTS[@]}"; do
  r=/var/tmp/773/roots/$rn
  go() { # label env-prefix-var cmd...
    local label=$1; shift
    env ROOT=$r PORTAGE_RUNNING_ROOT=$r DISTDIR=$F/distfiles "$@" >"$OUT/$rn.$label.out" 2>"$OUT/$rn.$label.err"; echo $? >"$OUT/$rn.$label.rc"
  }
  go fast     $B/mrg --plan-world --json --sort depend
  go fast-txt $B/mrg --plan-world --sort depend
  go df       $B/emerge --json "${RES[@]}" @world
  go df-txt   $B/emerge "${RES[@]}" @world
  go pg       PORTUALE_PG_TRACE=1 $B/emerge --solver=pubgrub --json "${RES[@]}" @world
  go pg-txt   PORTUALE_PG_TRACE=1 $B/emerge --solver=pubgrub "${RES[@]}" @world
done
# index: argv shape, rc, hashes, pg trace count
: > "$OUT/probes.txt"
for rn in "${ROOTS[@]}"; do for l in fast fast-txt df df-txt pg pg-txt; do
  echo "$rn/$l rc=$(cat $OUT/$rn.$l.rc) stdout=$(sha256sum <$OUT/$rn.$l.out|cut -d' ' -f1) stderr=$(sha256sum <$OUT/$rn.$l.err|cut -d' ' -f1) pg_lines=$(grep -c '^pg-' $OUT/$rn.$l.err)" >> "$OUT/probes.txt"
done; done
