#!/bin/bash
# S0 Portage side. Runs INSIDE localhost/test-portuale:latest (Portage 3.0.82.2).
# Mounts: pmtest fixtures at /fixtures:ro, pmtest differential-test-bed at
# /TEST:ro, this evidence work dir at /cin (cells-s0.txt in, out/ written).
# Every cell gets a FRESH stage (plan D3): global updates mutate the staged
# vdb, so re-staging per cell keeps cells independent. No --package-moves=n
# except where the cell argv itself carries it (group D).
set -u
OUT=/cin/out
mkdir -p "$OUT"
/usr/sbin/emerge --version > "$OUT/portage-version.txt" 2>&1
trim() { printf '%s' "$1" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//'; }
grep -v '^#' /cin/cells-s0.txt | while IFS='|' read -r raw_name raw_args raw_env; do
  name=$(trim "$raw_name"); [ -z "$name" ] && continue
  args=$(trim "$raw_args"); envspec=$(trim "$raw_env")
  if ! bash /TEST/layers/l0-fixture-oracle/stage.sh /tmp/st > /tmp/s359-stage.log 2>&1; then
    { echo "### STAGE FAILED"; tail -20 /tmp/s359-stage.log; echo "rc=2"; } > "$OUT/$name.txt"
    continue
  fi
  FX=/tmp/st/fixtures
  rm -rf /var/db/pkg; cp -r "$FX/var/db/pkg" /var/db/pkg
  mkdir -p /var/lib/portage; cp "$FX/var/lib/portage/world" /var/lib/portage/world
  delta_note="delta=none"
  for tok in $envspec; do
    case "$tok" in
      DELTA=maskedusedrift)
        printf '%s\n' 'dev-libs/keywordmaskedusepkg -flag' >> "$FX/etc/portage/package.use"
        delta_note="delta=maskedusedrift: appended 'dev-libs/keywordmaskedusepkg -flag' to \$FX/etc/portage/package.use (installed USE=flag vs computed USE= -> USE drift on a keyword-masked installed instance)" ;;
    esac
  done
  export PYTHONHASHSEED=0 LC_ALL=C.UTF-8 TZ=UTC CLEAN_DELAY=0
  export PORTAGE_CONFIGROOT=$FX ROOT=$FX PORTAGE_RUNNING_ROOT=$FX DISTDIR=$FX/distfiles
  unset EMERGE_DEFAULT_OPTS; unset FEATURES
  export PORTAGE_REPOSITORIES="$(cat "$FX/etc/portage/repos.conf/repos.conf")"
  for tok in $envspec; do
    case "$tok" in
      DELTA=*) ;;
      *=*) export "$tok" ;;
    esac
  done
  {
    echo "### emerge $args --color=n"
    echo "### $delta_note"
    # shellcheck disable=SC2086
    timeout 300 /usr/sbin/emerge $args --color=n 2>&1
    echo "rc=$?"
  } > "$OUT/$name.txt"
done
echo "done $(ls "$OUT"/*.txt | wc -l) files"
