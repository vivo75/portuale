#!/bin/bash
# L3 -- source-build parity: both package managers build+merge the same
# atom list FROM SOURCE (`--emptytree --oneshot --usepkg=n`) into two
# fresh, identical containers under the determinism block, then the full
# installed tree + VDB are snapshotted and diffed.
#
#   TEST/run/l3-source-parity.sh [atomlist]     (default l3-smoke.txt)
#
# Env:
#   L3_PM=both|portage|portuale  (default both; one-side iteration)
#   L3_CONTROL=1                 also run a second portage container and
#                                diff the two portage runs (noise floor)
#   L3_BUILD_ARGS="--emptytree --oneshot --usepkg=n --color=n"
#   L3_DISTFILES                 default $LOGS_DIR/_l2-distfiles
#   L3_TIMEOUT=28800             per-container wall-clock cap
#   L3_SKIP_PORTAGE_UPGRADE=0
#   L3_KEEP_TMP=1                keep /var/tmp/portage (build logs)
#
# Exit: 0 green (candidate diff and control pair 0 unexplained),
#       1 unexplained finding(s), 2 setup error.

set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
. "$HERE/lib.sh"

ATOMLIST=$(realpath -e "${1:-$TEST_DIR/atomlists/l3-smoke.txt}" 2>/dev/null) \
  || { echo "atom list not found" >&2; exit 2; }
case $ATOMLIST in
  "$TEST_DIR"/*) REL_ATOMLIST="/TEST/${ATOMLIST#"$TEST_DIR"/}" ;;
  *) echo "atom list must live under $TEST_DIR/" >&2; exit 2 ;;
esac

MODE=${L3_PM:-both}
case $MODE in both|portage|portuale) ;; *) echo "L3_PM must be both|portage|portuale" >&2; exit 2 ;; esac

DISTFILES=${L3_DISTFILES:-$LOGS_DIR/_l2-distfiles}
TIMEOUT=${L3_TIMEOUT:-28800}
RUN="l3-$(timestamp)"
OUT="$LOGS_DIR/$RUN"
mkdir -p "$OUT" "$DISTFILES"
ln -sfn "$RUN" "$LOGS_DIR/l3-latest"

ensure_portuale_built
ensure_image

run_pm() {  # <label> <portage|portuale>
  local label=$1 pm=$2
  echo ">>> L3 build+merge: $label ($pm)"
  set +e
  # `timeout` cannot run a shell function, so this mirrors
  # `podman_run_portuale`'s own `podman run` arguments inline (the
  # portuale binaries and the repo at its build-time path, /TEST, the
  # shared logs + distfiles).
  timeout "$TIMEOUT" "$PODMAN" run --rm --name "porttest-l3-$label-$$" \
    --security-opt seccomp=unconfined --cgroups=enabled --cgroupns=private \
    --hostname porttest-l3 \
    -v "$REPO_ROOT/rust/target/release:/usr/local/bin:ro" \
    -v "$REPO_ROOT:$REPO_ROOT:ro" \
    -v "$TEST_DIR:/TEST:ro" -v "$LOGS_DIR:/TEST/logs" \
    -v "$DISTFILES:/distfiles" \
    -e DISTDIR=/distfiles \
    -e "SNAPSHOT_PRUNE=$REPO_ROOT" \
    -e "L3_SKIP_PORTAGE_UPGRADE=${L3_SKIP_PORTAGE_UPGRADE:-0}" \
    -e "L3_BUILD_ARGS=${L3_BUILD_ARGS:---emptytree --oneshot --usepkg=n --color=n}" \
    --entrypoint /bin/bash "$IMAGE" \
    /TEST/layers/l3/build-and-merge.sh "$pm" "$REL_ATOMLIST" "/TEST/logs/$RUN/$label" \
    2>&1 | tee "$OUT/$label.container.log"
  local rc=${PIPESTATUS[0]}
  set -e
  echo ">>> $label rc=$rc"
  return 0
}

if [ "$MODE" = both ] || [ "$MODE" = portage ]; then
  run_pm portage portage
fi
if [ "$MODE" = both ] || [ "$MODE" = portuale ]; then
  run_pm portuale portuale
fi
if [ "${L3_CONTROL:-0}" = 1 ]; then
  run_pm control-a portage
  run_pm control-b portage
fi

for label in portage portuale control-a control-b; do
  [ -f "$OUT/$label.files.tsv" ] || continue
  python3 "$TEST_DIR/compare/normalize.py" "$OUT/$label" >/dev/null
done

DIFF_RC=0
CTRL_RC=0
if [ -f "$OUT/portage.files.tsv" ] && [ -f "$OUT/portuale.files.tsv" ]; then
  echo ">>> diff portage vs portuale (--layer l3 --tolerate-payload)"
  set +e
  python3 "$TEST_DIR/compare/diff.py" --layer l3 --tolerate-payload \
    "$OUT/portage" "$OUT/portuale" "$TEST_DIR/compare/known-divergences.yaml" \
    | tee "$OUT/candidate.txt"
  DIFF_RC=${PIPESTATUS[0]}
  set -e
fi
if [ -f "$OUT/control-a.files.tsv" ] && [ -f "$OUT/control-b.files.tsv" ]; then
  echo ">>> diff control-a vs control-b (portage-vs-portage noise floor)"
  set +e
  python3 "$TEST_DIR/compare/diff.py" --layer l3 --tolerate-payload \
    "$OUT/control-a" "$OUT/control-b" "$TEST_DIR/compare/known-divergences.yaml" \
    > "$OUT/control.txt" 2>&1
  CTRL_RC=$?
  set -e
fi

# --- report -------------------------------------------------------------
summary() {  # <file> <label>
  local f=$1 label=$2
  [ -f "$f" ] || { echo "  (no $label run)"; return; }
  sed -n '/^## summary/,/^$/p' "$f" | sed '1d;$d'
}
{
  echo "# L3 source-build parity report"
  echo
  echo "atoms  : $REL_ATOMLIST"
  echo "mode   : $MODE (control=${L3_CONTROL:-0})"
  echo "dates  : $(date -u +%FT%TZ)"
  echo "portage: $OUT/portage.merge.log"
  echo "portuale: $OUT/portuale.merge.log"
  echo
  echo "## candidate (portage vs portuale)"
  summary "$OUT/candidate.txt" candidate
  echo "## control (portage vs portage)"
  summary "$OUT/control.txt" control
  echo
  echo "## per-PM touched packages"
  for label in portage portuale control-a control-b; do
    [ -f "$OUT/$label.merged-cpvs.txt" ] || continue
    echo "  $label: $(wc -l < "$OUT/$label.merged-cpvs.txt")"
  done
} > "$OUT/l3-report.txt"
ln -sfn "$RUN/l3-report.txt" "$LOGS_DIR/l3-report.txt"
cat "$OUT/l3-report.txt"

# --- metrics (docs/real-world-testing.md §8) ---------------------------
python3 - "$OUT" "$REL_ATOMLIST" "$CTRL_RC" <<'PY'
import json, os, re, sys
out, atomlist, ctrl_rc = sys.argv[1], sys.argv[2], int(sys.argv[3])

def summary(path):
    if not os.path.exists(path):
        return {}
    text = open(path).read()
    m = re.search(r"## summary\n(.*?)\n\n", text, re.S)
    fields = {}
    if m:
        for line in m.group(1).splitlines():
            k, _, v = line.partition(":")
            fields[k.strip()] = v.strip()
    return fields

cand = summary(os.path.join(out, "candidate.txt"))
ctrl = summary(os.path.join(out, "control.txt"))
def num(d, k):
    try:
        return int(d.get(k, "0").split()[0])
    except ValueError:
        return 0

def touched(label):
    p = os.path.join(out, label + ".merged-cpvs.txt")
    return sum(1 for _ in open(p)) if os.path.exists(p) else 0

metrics = {
    "layer": "l3",
    "date": os.path.basename(out),
    "atomlist": atomlist,
    "control": {
        "unexplained": num(ctrl, "UNEXPLAINED"),
        "payload_diffs": num(ctrl, "payload diffs"),
    },
    "candidate": {
        "unexplained": num(cand, "UNEXPLAINED"),
        "explained": num(cand, "explained"),
        "payload_diffs": num(cand, "payload diffs"),
        "hard": num(cand, "hard findings"),
    },
    "touched": {label: touched(label) for label in ("portage", "portuale")},
    "parity_rate": None,
}
total = metrics["touched"]["portage"] or metrics["touched"]["portuale"]
if metrics["candidate"]["hard"] is not None and total:
    metrics["parity_rate"] = round(
        (total - metrics["candidate"]["unexplained"]) / total, 4
    )
os.makedirs(os.path.join(os.path.dirname(out), "metrics"), exist_ok=True)
mpath = os.path.join(os.path.dirname(out), "metrics", metrics["date"] + ".json")
json.dump(metrics, open(mpath, "w"), indent=2, sort_keys=True)
print(">>> metrics: " + mpath)
PY

rc=0
# A PM that produced no snapshot means the run is invalid (setup error
# or timeout), never "green": grade only complete same-run pairs.
expected=(portage portuale)
if [ "${L3_CONTROL:-0}" = 1 ]; then
  expected+=(control-a control-b)
fi
for label in "${expected[@]}"; do
  if [ ! -f "$OUT/$label.files.tsv" ]; then
    echo ">>> missing snapshot: $label -- run invalid"
    rc=2
  fi
done
[ "$DIFF_RC" = 0 ] || rc=1
if [ -f "$OUT/control-a.files.tsv" ] && [ -f "$OUT/control-b.files.tsv" ] && [ "$CTRL_RC" != 0 ]; then
  echo ">>> control pair is NOT clean ($CTRL_RC unexplained) -- instrument invalid"
  rc=1
fi
echo ">>> report: $OUT/l3-report.txt   (rc=$rc)"
exit "$rc"
