#!/bin/bash
# L1 -- merge the whole $PKGDIR set with ONE package manager into the
# container's own `/`, then snapshot exactly what the merge touched.
# Run once per PM in two identical fresh containers; the host then diffs
# the two snapshots.
#
# Usage (from the host orchestrator):
#   podman run ... IMAGE /TEST/layers/l1/consume.sh <portage|portuale> \
#       /TEST/atomlists/l1-merge.txt /TEST/logs/<run>/<pm>
#
# Env:
#   PKGDIR                     (required) the shared binpkg dir, ro mount
#   L1_PORTAGE_PIN=3.0.82.2
#   L1_SKIP_PORTAGE_UPGRADE=0  (portage consumer only)

set -u
PM=${1:?portage|portuale}
ATOMLIST=${2:?atom list}
OUT=${3:?output prefix}
PIN=${L1_PORTAGE_PIN:-3.0.82.2}
: "${PKGDIR:?PKGDIR must be set}"

case $PM in
  portage)  EM=/usr/sbin/emerge ;;
  portuale) EM=/usr/local/bin/emerge ;;
  *) echo "PM must be portage or portuale" >&2; exit 2 ;;
esac

export PORTAGE_CONFIGROOT=/ ROOT=/ PORTAGE_RUNNING_ROOT=/
export LC_ALL=C.UTF-8 TZ=UTC
export PKGDIR
export EMERGE_DEFAULT_OPTS=""
export FEATURES="-buildpkg -cgroup -ccache -distcc -sign"
umask 022
mkdir -p "$(dirname "$OUT")"

log() { printf '[l1-consume %s] %s\n' "$PM" "$*"; }

# Both containers must start from an IDENTICAL `/` -- so BOTH upgrade
# portage to <PIN> (the portage consumer merges with it; the portuale
# consumer just needs the installed state to match). `-1` (oneshot) so
# `world` / `COUNTER` are not perturbed.
if [ "${L1_SKIP_PORTAGE_UPGRADE:-0}" != 1 ]; then
  cur=$(/usr/sbin/emerge --version 2>/dev/null | sed -n 's/^Portage \([0-9.]*\).*/\1/p')
  if [ "$cur" != "$PIN" ]; then
    log "upgrading portage $cur -> $PIN (oneshot, ~amd64 for that one atom only)"
    ACCEPT_KEYWORDS="~amd64" /usr/sbin/emerge -q -1 --usepkg=n "=sys-apps/portage-$PIN" \
      || { log "!!! upgrade failed"; exit 1; }
  fi
fi

installed_cpvs() { ( cd /var/db/pkg && ls -d */*/ 2>/dev/null | sed 's:/$::' ) | LC_ALL=C sort; }

atoms=()
while IFS= read -r line || [ -n "$line" ]; do
  line=${line%%#*}; line=$(printf '%s' "$line" | tr -d '[:space:]')
  [ -n "$line" ] && atoms+=("$line")
done < "$ATOMLIST"

installed_cpvs > "$OUT.installed-before.txt"
log "merging ${#atoms[@]} atoms (+deps) from \$PKGDIR with $($EM --version 2>/dev/null | head -1 || echo "$PM")"

# `-k --getbinpkg`: prefer a $PKGDIR binpkg, fall back to an ebuild (so
# an installed dep with no binpkg is just satisfied). Both PMs run the
# identical command. Notes (TEST/findings/l1.md):
#  * NOT `--usepkgonly`/`-K`: portuale doesn't treat an installed dep
#    with no binpkg as satisfied under it (L1-a).
#  * portuale needs `--getbinpkg` to actually *execute* a local-$PKGDIR
#    binary merge where real portage merges on `-k` alone (L1-b); real
#    portage accepts the redundant `--getbinpkg` (no binhost -> no-op).
# The builder built the whole closure, so nothing is built here -- a
# `>>> Emerging (` line in the log breaks that invariant (asserted below).
set -o pipefail
if ! $EM -k --getbinpkg --oneshot --verbose --color=n "${atoms[@]}" 2>&1 | tee "$OUT.merge.log"; then
  log "!!! merge failed -- see $OUT.merge.log"
  MERGE_RC=1
else
  MERGE_RC=0
fi
set +o pipefail

if grep -qE '^>>> Emerging \(' "$OUT.merge.log"; then
  log "!!! a package was built from source -- \$PKGDIR was incomplete; L1 parity is invalid"
  MERGE_RC=2
fi

installed_cpvs > "$OUT.installed-after.txt"
comm -13 "$OUT.installed-before.txt" "$OUT.installed-after.txt" > "$OUT.merged-cpvs.txt"
log "merged $(wc -l < "$OUT.merged-cpvs.txt") packages"

# --- build the path list the snapshot restricts to -------------------
# every merged package's own installed obj/sym/dir (from its CONTENTS --
# a trailing "/" would recurse, so dirs are stat-only: CONTENTS `dir`
# lines include shared parents like /usr/lib64), plus any CONFIG_PROTECT
# `._cfg*` files the merge created and the env-update targets it
# rewrites. /var/db/pkg is NOT here -- compared via the vdb.tar.
{
  while read -r cpv; do
    cat="${cpv%%/*}"; pf="${cpv#*/}"
    d="/var/db/pkg/$cat/$pf"
    [ -f "$d/CONTENTS" ] && awk '$1=="obj"||$1=="sym"||$1=="dir" {print $2}' "$d/CONTENTS"
  done < "$OUT.merged-cpvs.txt"
  find /etc -name '._cfg????_*' 2>/dev/null
  printf '%s\n' \
    /var/lib/portage/world /var/lib/portage/config \
    /etc/ld.so.cache /etc/ld.so.conf /etc/profile.env /etc/csh.env \
    /etc/environment /etc/environment.d/ /usr/share/info/dir
} | LC_ALL=C sort -u > "$OUT.paths.txt"

log "snapshotting $(wc -l < "$OUT.paths.txt") path entries + $(wc -l < "$OUT.merged-cpvs.txt") vdb dirs -> $OUT.*"
bash /TEST/compare/snapshot.sh \
  --paths "$OUT.paths.txt" --vdb-list "$OUT.merged-cpvs.txt" / "$OUT"

{
  echo "pm	$PM"
  echo "merge_rc	$MERGE_RC"
  echo "portage_version	$(/usr/sbin/emerge --version 2>/dev/null | head -1)"
  [ "$PM" = portuale ] && echo "portuale_bin	$(/usr/local/bin/emerge --help 2>&1 | head -1)"
  echo "merged_count	$(wc -l < "$OUT.merged-cpvs.txt")"
} >> "$OUT.meta.tsv"

log "done (merge_rc=$MERGE_RC)"
exit "$MERGE_RC"
