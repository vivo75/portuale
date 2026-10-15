#!/bin/bash
# In-container Portage side (localhost/test-portuale:latest, Portage 3.0.82.2).
# Mounts: pmtest fixtures at /fixtures:ro, pmtest differential-test-bed at /TEST:ro,
# this evidence dir's parent work dir at /cin (cells.txt in, out/ written).
bash /TEST/layers/l0-fixture-oracle/stage.sh /tmp/st >/dev/null
FX=/tmp/st/fixtures
# As layers/l0-fixture-oracle/in-container.sh: the running root `/` gets the
# fixture vdb and world, so BDEPEND resolved against `/` (ROOT != "/") sees
# the same installed set as the target root.
rm -rf /var/db/pkg; cp -r "$FX/var/db/pkg" /var/db/pkg
mkdir -p /var/lib/portage; cp "$FX/var/lib/portage/world" /var/lib/portage/world
export PYTHONHASHSEED=0 LC_ALL=C.UTF-8 TZ=UTC CLEAN_DELAY=0 PORTAGE_CONFIGROOT=$FX ROOT=$FX PORTAGE_RUNNING_ROOT=$FX DISTDIR=$FX/distfiles
export PORTAGE_REPOSITORIES="$(cat $FX/etc/portage/repos.conf/repos.conf)"
emerge --version > /cin/out/portage-version.txt 2>&1
grep -v '^#' /cin/cells.txt | while IFS='|' read -r name args; do
  name=$(echo $name); [ -z "$name" ] && continue
  { echo "### emerge$args --color=n"; /usr/sbin/emerge $args --color=n 2>&1; echo "rc=$?"; } > /cin/out/$name.txt
done
E="/usr/sbin/emerge --getbinpkg=n --usepkg=n --color=n"
$E -pvu --debug dev-libs/pg34-dtop > /cin/out/debug-362-dtop-pvu.txt 2>&1
$E -pvuD --debug dev-libs/pg34-blib > /cin/out/debug-361-blib-pvuD.txt 2>&1
$E -pvuD --with-bdeps=y --debug dev-libs/pg34-blib > /cin/out/debug-361-blib-pvuD-bdy.txt 2>&1
$E -pv --debug dev-libs/pg366-rb > /cin/out/debug-369-rb-pv.txt 2>&1
$E -pvuD --debug dev-libs/pg34-gapp > /cin/out/debug-360-gapp-pvuD.txt 2>&1
python3 -c "
import portage,os
t=portage.create_trees(config_root=os.environ['ROOT'],target_root=os.environ['ROOT'])
print('target_eroot',t._target_eroot,'running_eroot',t._running_eroot)
v=t[t._target_eroot]['vartree'].dbapi
print('vdb pg34-blib-1 EAPI/BDEPEND', v.aux_get('dev-libs/pg34-blib-1',['EAPI','BDEPEND']))
" > /cin/out/roots.txt 2>&1
