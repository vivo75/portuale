#!/bin/bash
# usage: cells.sh <emerge>   (needs setup.sh + built pkgdir first; EMERGE_DEFAULT_OPTS=--package-moves=n)
S=${S459:-/var/tmp/f459/oracle}
export PORTAGE_CONFIGROOT=$S PORTAGE_TMPDIR=$S/tmp ROOT=$S/root DISTDIR=$S/dist
EM=$1
run() { local name=$1; shift
  rm -rf $S/root $S/world-check; mkdir -p $S/root $S/dist
  echo "=== $name"
  out=$( "$@" </dev/null 2>$S/stderr.txt ); rc=$?
  echo "$out"; echo "rc=$rc"; echo "--- stderr"; grep -v summary.log $S/stderr.txt
  echo "--- world:"; cat $S/root/var/lib/portage/world 2>/dev/null || echo "(no world file)"
  echo "--- resume:"; python3 -c "
import json,sys
try:
    d = json.load(open('$S/root/var/cache/edb/mtimedb'))
    print(json.dumps(d.get('resume'), indent=None))
except Exception as e:
    print('(no mtimedb: %s)' % e)
  "
  echo "--- builddir:"; ls -d $S/tmp/portage/dev-util/f459-postinstfail-1 2>/dev/null || echo "(builddir gone)"
  echo "--- vdb:"; ls $S/root/var/db/pkg/dev-util/ 2>/dev/null || echo "(no vdb)"
  echo "--- pkgdir:"; ls $S/pkgdir/dev-util/ 2>/dev/null || echo "(no pkgdir)"
}
run "lone-postinstfail" $EM -q --usepkgonly dev-util/f459-postinstfail
run "postinstfail-plus-good" $EM -q --usepkgonly dev-util/f459-postinstfail dev-util/f459-good
run "keep-going" $EM -q --keep-going --usepkgonly dev-util/f459-postinstfail dev-util/f459-good dev-util/f459-top
run "foreground-postinstfail" $EM --usepkgonly dev-util/f459-postinstfail
