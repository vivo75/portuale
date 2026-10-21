#!/bin/bash
# usage: cells.sh <emerge>   (needs setup.sh first; EMERGE_DEFAULT_OPTS=--package-moves=n)
S=${S7:-/var/tmp/b444}
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
  echo "--- builddir:"; ls -d $S/tmp/portage/dev-util/postinstfail-1 2>/dev/null || echo "(builddir gone)"
}
run "lone-postinstfail" $EM -q dev-util/postinstfail
run "postinstfail-plus-good" $EM -q dev-util/postinstfail dev-util/good
run "keep-going" $EM -q --keep-going dev-util/postinstfail dev-util/good dev-util/posttop
run "jobs-keep-going" $EM --keep-going --jobs=2 dev-util/postinstfail dev-util/good dev-util/posttop
run "noclean-postinstfail" env FEATURES=noclean $EM -q dev-util/postinstfail
run "foreground-postinstfail" $EM dev-util/postinstfail
