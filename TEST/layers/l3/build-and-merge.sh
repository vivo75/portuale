#!/bin/bash
# L3 -- source-build parity, in-container half: ONE package manager runs
# `emerge --emptytree --oneshot --usepkg=n` on the same atom list into
# its own throwaway `/`, under the determinism block, then the whole
# installed tree + VDB is snapshotted. Run once per PM in two identical
# fresh containers; the host orchestrator diffs the snapshots.
#
# Usage (from the host orchestrator):
#   podman run ... IMAGE /TEST/layers/l3/build-and-merge.sh <portage|portuale> \
#       /TEST/atomlists/l3-smoke.txt /TEST/logs/<run>/<pm>
#
# Env:
#   L3_PORTAGE_PIN=3.0.82.2
#   L3_SKIP_PORTAGE_UPGRADE=0
#   L3_BUILD_ARGS="--emptytree --oneshot --usepkg=n --color=n"
#
# Exit: the PM's own rc (0 green); a non-zero run still writes
# `<out>.partial` and the merged-cpvs list so triage has data.

set -u
PM=${1:?portage|portuale}
ATOMLIST=${2:?atom list}
OUT=${3:?output prefix}
PIN=${L3_PORTAGE_PIN:-3.0.82.2}
SKIP_UPGRADE=${L3_SKIP_PORTAGE_UPGRADE:-0}
BUILD_ARGS=${L3_BUILD_ARGS:---emptytree --oneshot --usepkg=n --color=n}

case $PM in
  portage)  EM=/usr/sbin/emerge ;;
  portuale) EM=/usr/local/bin/emerge ;;
  *) echo "PM must be portage or portuale" >&2; exit 2 ;;
esac

export PORTAGE_CONFIGROOT=/ ROOT=/ PORTAGE_RUNNING_ROOT=/
export LC_ALL=C.UTF-8 TZ=UTC
export EMERGE_DEFAULT_OPTS=""
umask 022
mkdir -p "$(dirname "$OUT")"

log() { printf '[l3-build %s] %s\n' "$PM" "$*"; }

# --- determinism block -------------------------------------------------
# docs/real-world-testing.md §2: `SOURCE_DATE_EPOCH`, `MAKEOPTS=-j1`,
# `EMERGE_DEFAULT_OPTS=""`, `LC_ALL`/`TZ`/`umask`. The FEATURES pin keeps
# the execution model both PMs implement: `userpriv`/`usersandbox`/
# `userfetch`/`usersync` are portuale non-goals, `test` is its own axis,
# and `splitdebug` stays on so #38 remains under test. Put it in
# `/etc/portage/make.conf` (the resolved-config layer both PMs read)
# rather than the process env, so #37's config path is what is exercised.
if ! grep -q 'portuale-l3 determinism block' /etc/portage/make.conf 2>/dev/null; then
  # A second `FEATURES=` in the same file would *replace* the first for
  # real portage (make.conf is sourced bash-style; the last assignment
  # is that file's incremental layer), so replace the image's own line
  # in place instead of appending -- keeping the two PMs on the same
  # value, which is the whole point of the block.
  if grep -qE '^[[:space:]]*FEATURES=' /etc/portage/make.conf; then
    sed -i 's|^[[:space:]]*FEATURES=.*|FEATURES="-buildpkg -sign -ccache -distcc -cgroup -userpriv -usersandbox -userfetch -usersync sandbox pid-sandbox xattr filecaps splitdebug"|' \
      /etc/portage/make.conf
  else
    printf 'FEATURES="-buildpkg -sign -ccache -distcc -cgroup -userpriv -usersandbox -userfetch -usersync sandbox pid-sandbox xattr filecaps splitdebug"\n' \
      >> /etc/portage/make.conf
  fi
  cat >> /etc/portage/make.conf <<'EOF'

# portuale-l3 determinism block (TEST/layers/l3/build-and-merge.sh)
MAKEOPTS="-j1"
SOURCE_DATE_EPOCH=1740000000
EMERGE_DEFAULT_OPTS=""
EOF
fi
# The image's signed, different-revision gentoo binrepo must not
# substitute for the set under test (backlog #43); `--usepkg=n` already
# forbids it, this is belt and braces for a stray `-k`.
rm -f /etc/portage/binrepos.conf/gentoo.conf

# --- same base on both sides -------------------------------------------
if [ "$SKIP_UPGRADE" != 1 ]; then
  cur=$(/usr/sbin/emerge --version 2>/dev/null | sed -n 's/^Portage \([0-9.]*\).*/\1/p')
  if [ "$cur" != "$PIN" ]; then
    log "upgrading portage $cur -> $PIN"
    FEATURES="-cgroup" ACCEPT_KEYWORDS="~amd64" /usr/sbin/emerge -q -1 --usepkg=n "=sys-apps/portage-$PIN" \
      || { log "!!! portage upgrade failed"; exit 1; }
  fi
fi
log "$($EM --version 2>/dev/null | head -1 || echo "$PM")"

atoms=()
while IFS= read -r line || [ -n "$line" ]; do
  line=${line%%#*}; line=$(printf '%s' "$line" | tr -d '[:space:]')
  [ -n "$line" ] && atoms+=("$line")
done < "$ATOMLIST"
[ "${#atoms[@]}" -gt 0 ] || { log "empty atom list $ATOMLIST"; exit 2; }

installed_cpvs() { ( cd /var/db/pkg && ls -d */*/ 2>/dev/null | sed 's:/$::' ) | LC_ALL=C sort; }
installed_cpvs > "$OUT.installed-before.txt"

log "building ${#atoms[@]} atoms: $EM $BUILD_ARGS ${atoms[*]}"
set +e
# `L3_BUILD_ARGS` already carries `--oneshot` (and `--emptytree
# --usepkg=n`): the set must be rebuilt from source, never satisfied
# from `$PKGDIR`/installed state.
$EM $BUILD_ARGS "${atoms[@]}" > "$OUT.merge.log" 2>&1
rc=$?
set -e
installed_cpvs > "$OUT.installed-after.txt"
comm -13 "$OUT.installed-before.txt" "$OUT.installed-after.txt" > "$OUT.merged-cpvs.txt"
log "merge rc=$rc, $(wc -l < "$OUT.merged-cpvs.txt") packages touched"

{
  echo "pm	$PM"
  echo "merge_rc	$rc"
  echo "atoms	${#atoms[@]}"
  echo "merged	$(wc -l < "$OUT.merged-cpvs.txt")"
  echo "portage_version	$(/usr/sbin/emerge --version 2>/dev/null | head -1)"
  [ "$PM" = portuale ] && echo "portuale_bin	$(/usr/local/bin/emerge --help 2>&1 | head -1)"
  echo "date_utc	$(date -u +%FT%TZ)"
  echo "source_date_epoch	1740000000"
  echo "profile	$(readlink -f /etc/portage/make.profile 2>/dev/null || echo none)"
} > "$OUT.meta.tsv"

if [ "$rc" != 0 ]; then
  echo "partial" > "$OUT.partial"
  log "!!! build failed (rc=$rc) -- partial state captured, not graded"
  tail -40 "$OUT.merge.log" | sed 's/^/    /'
  exit "$rc"
fi

log "snapshotting full tree + full VDB -> $OUT.*"
if ! /TEST/compare/snapshot.sh / "$OUT" 2> "$OUT.snapshot.err"; then
  log "!!! snapshot.sh exited non-zero -- see $OUT.snapshot.err"
  tail -5 "$OUT.snapshot.err" | sed 's/^/    /'
  exit 2
fi
log "done"
