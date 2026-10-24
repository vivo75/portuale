#!/bin/bash
# #512 oracle, item (4): SIGINT while a merge waits in the merge-wait queue.
# Upstream: 8ac550470 (clear the queue on terminate, bug 961044) +
# bfa615f06 (test). Shape: A fast + B slow under the DEFAULT features
# (merge-wait on), `-j2`. A's build finishes while B still builds, so A
# sits in the merge-wait queue when SIGINT arrives. Expected (fixed
# Portage): rc 130, no `>>> Installing` line at all, A never merges.
# Usage: sigint.sh <portage|portuale> <outdir>
set -u
pm=$1; out=$(readlink -f "$2")
here=$(cd "$(dirname "$0")" && pwd)
T=/var/tmp/ev512-tree
[ -f $T/.built ] || { echo "run probe.sh first"; exit 2; }
mkdir -p "$out"
cp "$T/cfg/etc/portage/make.conf.default-features" "$T/cfg/etc/portage/make.conf"
rm -rf "$T/root" "$T/tmp"; mkdir -p "$T/root" "$T/tmp"
export ROOT=$T/root PORTAGE_CONFIGROOT=$T/cfg PORTAGE_TMPDIR=$T/tmp
if [ "$pm" = portage ]; then bin=$(command -v emerge)
else mkdir -p /var/tmp/ev512-bin; ln -sf "$(readlink -f ${PORTUALE_BIN:-$here/../../../target/release/portuale})" /var/tmp/ev512-bin/emerge; bin=/var/tmp/ev512-bin/emerge; fi

id=13-sigint
# No `timeout(1)` parent and no timestamping pipe here: `$!` must be emerge
# itself so SIGINT lands on it (`timeout` does not forward SIGINT, and a
# pipe would hand us awk's PID). A watchdog KILLs a hung run instead.
# Order pins come from the VDB and the Installing lines, not timestamps.
( cd "$T" || exit 2
  # `$!` must be emerge itself (no `&&` chain before the `&`: that would
  # background a subshell): SIGINT lands on emerge, which terminates its
  # builds, clears the merge-wait queue and exits 130.
  # `PYTHONUNBUFFERED=1` (portuale ignores it): Portage block-buffers
  # stdout to a file, so without it the `merge wait` poll below matches
  # ~16 s late, mid-drain instead of mid-wait.
  env ROOT=$T/root PORTAGE_CONFIGROOT=$T/cfg PORTAGE_TMPDIR=$T/tmp PYTHONUNBUFFERED=1 \
  stdbuf -o0 -e0 "$bin" --oneshot --color=n --jobs=2 \
    =app-misc/sysextra-1.0 =app-misc/slow-1.0 \
    >"$out/$id.$pm.out" 2>"$out/$id.$pm.err" &
  child=$!
  ( sleep 120; kill -KILL $child 2>/dev/null ) &
  watchdog=$!
  # A live timestamped side copy (the primary stays raw): a `tail -F`
  # follower, so the signal moment below can be placed on the timeline.
  ( tail -n +1 -F "$out/$id.$pm.out" 2>/dev/null | awk '{ print strftime("%H:%M:%S"), $0; fflush() }' >"$out/$id.$pm.ts" & echo $! >"$out/$id.$pm.follower" )
  # Wait until sysextra's build has finished into the merge-wait queue
  # (the `>>> Jobs:` line says `merge wait`), like the upstream test
  # waits for the queue, then interrupt mid-wait. Deadline 60 s.
  for _ in $(seq 1 120); do
    grep -q "merge wait" "$out/$id.$pm.out" 2>/dev/null && break
    kill -0 $child 2>/dev/null || break
    sleep 0.5
  done
  date +%H:%M:%S >"$out/$id.$pm.killts"
  if kill -0 $child 2>/dev/null; then kill -INT $child; fi
  wait $child; echo $? >"$out/$id.$pm.rc"
  kill $watchdog 2>/dev/null
  sleep 1; kill -KILL $(cat "$out/$id.$pm.follower" 2>/dev/null) 2>/dev/null; rm -f "$out/$id.$pm.follower" )
echo "rc $(cat $out/$id.$pm.rc)"
grep -cE ">>> Installing" "$out/$id.$pm.out" || true
ls "$T/root/var/db/pkg/app-misc" 2>/dev/null || echo "no app-misc in VDB"
