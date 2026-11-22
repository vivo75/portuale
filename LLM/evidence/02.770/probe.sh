#!/bin/bash
# usage: probe.sh BIN_DIR OUT ; runs the plan 02.770 probe set on both solvers, twice-comparable.
B=$1; OUT=$2; F=/var/tmp/770/fx; mkdir -p "$OUT"
export PORTAGE_CONFIGROOT=$F
export PORTAGE_REPOSITORIES="$(cat $(find $F/etc/portage/repos.conf -type f | sort) | awk 1)"
: > "$OUT/probes.txt"
run() { # label root argv...
  local label=$1 root=$2; shift 2
  for sv in df pg; do
    local extra=(); [ $sv = pg ] && extra=(--solver=pubgrub)
    env ROOT=$root PORTAGE_RUNNING_ROOT=$root DISTDIR=$F/distfiles PORTUALE_PG_TRACE=$([ $sv = pg ] && echo 1) \
      $B/emerge "${extra[@]}" "$@" >"$OUT/$label.$sv.out" 2>"$OUT/$label.$sv.err"; local rc=$?
    { echo "argv: $label/$sv: emerge ${extra[*]} $*"
      echo "rc=$rc stdout=$(sha256sum <"$OUT/$label.$sv.out" | cut -d' ' -f1) stderr=$(grep -v '^pg-' "$OUT/$label.$sv.err" | sha256sum | cut -d' ' -f1) pg_lines=$(grep -c '^pg-' "$OUT/$label.$sv.err")"
      grep -E '^\[ebuild|causes rebuilds for:|^    \(|^NOTE|its dependency|No visible version|not merged' "$OUT/$label.$sv.out"
      echo; } >> "$OUT/probes.txt"
  done
}
P=/var/tmp/770/roots/pull
run pull-json      $P --pretend --json --update --deep --newuse --getbinpkg=n @world
run pull-uDN       $P --pretend --verbose --update --deep --newuse --getbinpkg=n @world
run pull-DN        $P --pretend --verbose --deep --newuse --getbinpkg=n @world
run pull-D         $P --pretend --verbose --deep --getbinpkg=n @world
run pull-uD        $P --pretend --verbose --update --deep --getbinpkg=n @world
run pull-u         $P --pretend --verbose --update --getbinpkg=n @world
run pull-bt0       $P --pretend --verbose --update --deep --newuse --backtrack=0 --getbinpkg=n @world
C=/var/tmp/770/roots/ctl
run tree-uD-pull   $C --pretend --verbose --update --deep --getbinpkg=n dev-libs/p569pull
run tree-D-pull    $C --pretend --verbose --deep --getbinpkg=n dev-libs/p569pull
run tree-uD-bpull  $C --pretend --verbose --update --deep --getbinpkg=n dev-libs/p589bpull
run tree-D-bpull   $C --pretend --verbose --deep --getbinpkg=n dev-libs/p589bpull
for r in ctl prov r703; do run $r-json /var/tmp/770/roots/$r --pretend --json --update --deep --newuse --getbinpkg=n @world; done
if [ -d /var/tmp/770/roots/twohop ]; then run twohop-uDN /var/tmp/770/roots/twohop --pretend --verbose --update --deep --newuse --getbinpkg=n @world; fi
