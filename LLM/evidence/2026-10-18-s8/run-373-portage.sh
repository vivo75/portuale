#!/bin/bash
# Portage side of #373, INSIDE localhost/test-portuale:latest. REPOKEY=k=v[;k=v] lines go into [testrepo].
trim() { printf '%s' "$1" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//'; }
OUT=/cin/out373; mkdir -p $OUT
grep -v '^#' /cin/cells-373.txt | while IFS='|' read -r n a e; do
  name=$(trim "$n"); args=$(trim "$a"); envspec=$(trim "$e")
  bash /TEST/layers/l0-fixture-oracle/stage.sh /tmp/st >/dev/null 2>&1
  FX=/tmp/st/fixtures
  rm -rf /var/db/pkg; cp -r $FX/var/db/pkg /var/db/pkg; mkdir -p /var/lib/portage; cp $FX/var/lib/portage/world /var/lib/portage/world
  case "$envspec" in REPOKEY=*) kv=${envspec#REPOKEY=}; printf '%s\n' "$kv" | tr ';' '\n' | sed 's/=/ = /;s/ = /=/' > /tmp/kv; python3 - "$FX" <<'PY'
import sys
fx=sys.argv[1]; p=fx+'/etc/portage/repos.conf/repos.conf'
kvs=[l.strip().replace('=',' = ',1) for l in open('/tmp/kv') if l.strip()]
out=[]
for line in open(p).read().split('\n'):
    out.append(line)
    if line.startswith('location = ') and 'fixtures/repo' in line and line.endswith('/repo'):
        out.extend(kvs)
open(p,'w').write('\n'.join(out))
PY
  ;; esac
  export PYTHONHASHSEED=0 LC_ALL=C.UTF-8 TZ=UTC CLEAN_DELAY=0 PORTAGE_CONFIGROOT=$FX ROOT=$FX PORTAGE_RUNNING_ROOT=$FX DISTDIR=$FX/distfiles
  unset EMERGE_DEFAULT_OPTS FEATURES
  export PORTAGE_REPOSITORIES="$(cat $FX/etc/portage/repos.conf/repos.conf)"
  { echo "### emerge $args"; timeout 300 /usr/sbin/emerge $args --color=n 2>&1; echo "rc=$?"; } > $OUT/$name.txt
done
