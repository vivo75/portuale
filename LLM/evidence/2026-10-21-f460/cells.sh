#!/bin/bash
# usage: cells.sh <emerge>   (needs setup.sh first; EMERGE_DEFAULT_OPTS=--package-moves=n)
# #460 oracle cells: --resume after a lone postinst failure (A), a lone
# compile failure (B), and a partially-merged pair (C), plus --skipfirst (D).
S=${S7:-/var/tmp/f460/probe}
export PORTAGE_CONFIGROOT=$S PORTAGE_TMPDIR=$S/tmp ROOT=$S/root DISTDIR=$S/dist
export EMERGE_DEFAULT_OPTS=--package-moves=n
EM=$1
run() { local name=$1; shift
  rm -rf $S/root $S/world-check; mkdir -p $S/root $S/dist
  echo "=== $name"
  out=$("$@" </dev/null 2>$S/stderr.txt ); rc=$?
  echo "$out"; echo "rc=$rc"; echo "--- stderr"; grep -v summary.log $S/stderr.txt
  echo "--- world:"; cat $S/root/var/lib/portage/world 2>/dev/null || echo "(no world file)"
  echo "--- vdb:"; ls $S/root/var/db/pkg/dev-util/ 2>/dev/null || echo "(no vdb)"
  echo "--- resume:"; python3 -c "
import json,sys
try:
    d = json.load(open('$S/root/var/cache/edb/mtimedb'))
    print(json.dumps(d.get('resume')))
except Exception as e:
    print('(no mtimedb: %s)' % e)
  "
}
cont() { local name=$1; shift
  echo "=== $name"
  out=$("$@" </dev/null 2>$S/stderr.txt ); rc=$?
  echo "$out"; echo "rc=$rc"; echo "--- stderr"; grep -v summary.log $S/stderr.txt
  echo "--- world:"; cat $S/root/var/lib/portage/world 2>/dev/null || echo "(no world file)"
  echo "--- vdb:"; ls $S/root/var/db/pkg/dev-util/ 2>/dev/null || echo "(no vdb)"
  echo "--- resume:"; python3 -c "
import json,sys
try:
    d = json.load(open('$S/root/var/cache/edb/mtimedb'))
    print(json.dumps(d.get('resume')))
except Exception as e:
    print('(no mtimedb: %s)' % e)
  "
}
run "A1 lone-postinstfail" $EM -q dev-util/postinstfail
cont "A2 resume-after-lone-postinstfail" $EM --resume
cont "A3 resume-pretend-after-lone-postinstfail" $EM --resume --pretend
run "B1 lone-compilefail" $EM -q dev-util/bad
cont "B2 resume-after-compilefail" $EM --resume
cont "B3 resume-pretend-after-compilefail" $EM --resume --pretend
run "C1 postinstfail-plus-good" $EM -q dev-util/postinstfail dev-util/good
cont "C2 resume-after-partial" $EM --resume
cont "C3 resume-pretend-after-partial" $EM --resume --pretend
run "D0 lone-postinstfail-for-skipfirst" $EM -q dev-util/postinstfail
cont "D1 resume-skipfirst-after-lone-postinstfail" $EM --resume --skipfirst
