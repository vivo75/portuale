#!/bin/bash
# S4 Portage side for the follow-up cells. Runs INSIDE
# localhost/test-portuale:latest (Portage 3.0.82.2). Same staging and env
# as S0's run-s0-portage.sh (fresh stage per cell, plan D3); cells file at
# /cin/$CELLS (default cells-s4more.txt), captures written to /cin/out/.
#
# Host invocation (scratch tag x359d):
#   podman run --rm --entrypoint /bin/bash -e CELLS=cells-s4more.txt \
#     -v /home/vivo/repo/PORTUALE/pmtest/fixtures:/fixtures:ro \
#     -v /home/vivo/repo/PORTUALE/pmtest/differential-test-bed:/TEST:ro \
#     -v /var/tmp/x359d/s4cells:/cin \
#     localhost/test-portuale:latest /cin/run-s4-portage.sh
set -u
OUT=/cin/out
CELLS="${CELLS:-cells-s4more.txt}"
mkdir -p "$OUT"
/usr/sbin/emerge --version > "$OUT/portage-version.txt" 2>&1
trim() { printf '%s' "$1" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//'; }
grep -v '^#' "/cin/$CELLS" | while IFS='|' read -r raw_name raw_args raw_env; do
  name=$(trim "$raw_name"); [ -z "$name" ] && continue
  args=$(trim "$raw_args"); envspec=$(trim "$raw_env")
  if ! bash /TEST/layers/l0-fixture-oracle/stage.sh /tmp/st > /tmp/s359-stage.log 2>&1; then
    { echo "### STAGE FAILED"; tail -20 /tmp/s359-stage.log; echo "rc=2"; } > "$OUT/$name.txt"
    continue
  fi
  FX=/tmp/st/fixtures
  rm -rf /var/db/pkg; cp -r "$FX/var/db/pkg" /var/db/pkg
  mkdir -p /var/lib/portage; cp "$FX/var/lib/portage/world" /var/lib/portage/world
  export PYTHONHASHSEED=0 LC_ALL=C.UTF-8 TZ=UTC CLEAN_DELAY=0
  export PORTAGE_CONFIGROOT=$FX ROOT=$FX PORTAGE_RUNNING_ROOT=$FX DISTDIR=$FX/distfiles
  unset EMERGE_DEFAULT_OPTS; unset FEATURES
  export PORTAGE_REPOSITORIES="$(cat "$FX/etc/portage/repos.conf/repos.conf")"
  {
    echo "### emerge $args --color=n"
    echo "### delta=none"
    # shellcheck disable=SC2086
    timeout 300 /usr/sbin/emerge $args --color=n 2>&1
    echo "rc=$?"
  } > "$OUT/$name.txt"
done
echo "done $(ls "$OUT"/*.txt | wc -l) files"
