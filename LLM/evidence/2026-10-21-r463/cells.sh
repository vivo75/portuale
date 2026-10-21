#!/bin/bash
# usage: cells.sh <emerge>   (needs setup.sh first; EMERGE_DEFAULT_OPTS=--package-moves=n)
# The wall-clock seconds are cut so the two PMs' outputs diff cleanly.
EM=$1
S7=${S7:-/var/tmp/r463e}; S459=${S459:-/var/tmp/r467}
norm() { sed -E 's/took [0-9.]+ s/took/;s|/root/|/root|g'; }
f458() { export PORTAGE_CONFIGROOT=$S7 PORTAGE_TMPDIR=$S7/tmp ROOT=$S7/root DISTDIR=$S7/dist PKGDIR=$S7/pkgdir EMERGE_DEFAULT_OPTS=--package-moves=n
         rm -rf $S7/root $S7/pkgdir $S7/tmp/portage; mkdir -p $S7/root/var/cache/edb $S7/pkgdir; }
mtimedb() { # <myopts-json> <cpv>...
  local opts=$1; shift; local list=""
  for c in "$@"; do list="$list${list:+,}[\"ebuild\", \"$S7/root/\", \"$c\", \"merge\"]"; done
  echo "{\"resume\": {\"mergelist\": [$list], \"favorites\": [], \"myopts\": $opts}, \"starttime\": 0}" > $S7/root/var/cache/edb/mtimedb; }
cell() { local name=$1; shift; echo "=== $name"; out=$("$@" </dev/null 2>$S7/err.txt); rc=$?; echo "$out" | norm; echo "rc=$rc"; echo "--- stderr"; norm < $S7/err.txt
         echo "--- mtimedb resume mergelist:"; python3 -c "
import json
try: r = json.load(open('$S7/root/var/cache/edb/mtimedb')).get('resume')
except Exception: r = None
print('(no resume key)' if r is None else [x[2] for x in r.get('mergelist', [])])"; }
# 463: binpkg step lines on stdout
f458; cell "463 buildpkgonly" $EM --buildpkgonly dev-libs/schedparent
# 464: skipfirst on a one-item list / no list
for args in "--resume" "-p --resume" "--resume --skipfirst" "-p --resume --skipfirst" "-pq --resume --skipfirst" "--tree --resume --skipfirst" "-q --resume --skipfirst"; do
  f458; case "$args" in "--resume"|"-p --resume") ;; *) mtimedb '{"--oneshot": true}' dev-libs/schedparent-1.0;; esac
  cell "464 $args" $EM $args; done
# 465: saved --quiet
f458; mtimedb '{"--quiet": true}' dev-libs/schedleaf-a-1.0; cell "465 resume saved --quiet" $EM --resume
f458; mtimedb '{}' dev-libs/schedleaf-a-1.0; cell "465 resume plain" $EM --resume
# 466: --pretend header
f458; for args in "-p dev-libs/schedparent" "-pq dev-libs/schedparent" "-pt dev-libs/schedparent" "-p --nodeps dev-libs/schedparent" "-p --buildpkgonly dev-libs/schedparent"; do cell "466 $args" $EM $args; done
# 467: per-entry PORTAGE_TMPDIR on a binary merge (calling env must not carry PORTAGE_TMPDIR)
export PORTAGE_CONFIGROOT=$S459 ROOT=$S459/root DISTDIR=$S459/dist PKGDIR=$S459/pkgdir EMERGE_DEFAULT_OPTS=--package-moves=n; unset PORTAGE_TMPDIR
for variant in "present" "missing"; do
  rm -rf $S459/root $S459/tmp/portage $S459/tmp2; [ $variant = present ] && mkdir -p $S459/tmp2; mkdir -p $S459/root
  echo "=== 467 tmp2 $variant"; $EM -q --usepkgonly dev-util/f459-postinstfail </dev/null 2>&1 | grep -E "^>>>  '|build log is located" | norm
  echo "run tmpdir:   $(ls -d $S459/tmp/portage/dev-util/* 2>/dev/null || echo none)"
  echo "entry tmpdir: $(ls -d $S459/tmp2/portage/dev-util/* 2>/dev/null || echo none)"; done
