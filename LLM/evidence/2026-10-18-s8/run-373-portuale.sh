#!/bin/bash
# Portuale side of #373. Usage: run-373-portuale.sh <cells> <outdir> <emerge-binary> <staged-fixtures>
CELLS=$1; OUT=$2; BIN=$3; FX=$4
mkdir -p "$OUT"
trim() { printf '%s' "$1" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//'; }
grep -v '^#' "$CELLS" | while IFS='|' read -r n a e; do
  name=$(trim "$n"); args=$(trim "$a"); envspec=$(trim "$e")
  rm -rf /var/tmp/s8/st373; cp -a "$(dirname "$FX")" /var/tmp/s8/st373
  S=/var/tmp/s8/st373/fixtures
  sed -i "s#$FX#$S#g" $S/etc/portage/repos.conf/repos.conf $S/etc/portage/binrepos.conf 2>/dev/null
  case "$envspec" in REPOKEY=*) kv=${envspec#REPOKEY=}; printf '%s\n' "$kv" | tr ';' '\n' > /var/tmp/s8/kv; python3 - "$S" <<'PY'
import sys
fx=sys.argv[1]; p=fx+'/etc/portage/repos.conf/repos.conf'
kvs=[l.strip().replace('=',' = ',1) for l in open('/var/tmp/s8/kv') if l.strip()]
out=[]
for line in open(p).read().split('\n'):
    out.append(line)
    if line.startswith('location = ') and line.endswith('/fixtures/repo'):
        out.extend(kvs)
open(p,'w').write('\n'.join(out))
PY
  ;; esac
  { echo "### emerge $args"; env -i PATH=/usr/bin:/bin HOME=/var/tmp/s8 PYTHONHASHSEED=0 LC_ALL=C.UTF-8 TZ=UTC CLEAN_DELAY=0 PORTAGE_CONFIGROOT=$S ROOT=$S PORTAGE_RUNNING_ROOT=$S DISTDIR=$S/distfiles $BIN $args --color=n 2>&1; echo "rc=$?"; } > "$OUT/$name.txt"
done
