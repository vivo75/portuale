#!/bin/bash
# usage: probe.sh OUTDIR [ROOT...] ; frozen fixtures /var/tmp/764fx, roots /var/tmp/764roots (mkroots.sh), binary $BIN (default
# /var/tmp/764/bin). Per root: fast = mrg --plan-world --json --sort depend, fast-txt = same without --json, alpha = --sort alpha.
F=/var/tmp/764fx
export PORTAGE_CONFIGROOT=$F
export PORTAGE_REPOSITORIES="$(cat $(find $F/etc/portage/repos.conf -type f | sort) | awk 1)"
OUT=$1; shift; mkdir -p "$OUT"
B=${BIN:-/var/tmp/764/bin}
ROOTS=("$@"); [ ${#ROOTS[@]} -eq 0 ] && ROOTS=(v1 v2 v3 ctl)
for rn in "${ROOTS[@]}"; do
  r=/var/tmp/764roots/$rn
  go() { local label=$1; shift
    env ROOT=$r PORTAGE_RUNNING_ROOT=$r DISTDIR=$F/distfiles "$@" >"$OUT/$rn.$label.out" 2>"$OUT/$rn.$label.err"; echo $? >"$OUT/$rn.$label.rc"; }
  go fast $B/mrg --plan-world --json --sort depend
  go fast-txt $B/mrg --plan-world --sort depend
  go alpha $B/mrg --plan-world --sort alpha
done
: > "$OUT/probes.txt"
for rn in "${ROOTS[@]}"; do for l in fast fast-txt alpha; do
  echo "$rn/$l rc=$(cat $OUT/$rn.$l.rc) stdout=$(sha256sum <$OUT/$rn.$l.out|cut -d' ' -f1) stderr=$(sha256sum <$OUT/$rn.$l.err|cut -d' ' -f1)" >> "$OUT/probes.txt"
done; done
