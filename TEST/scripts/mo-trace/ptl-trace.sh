#!/bin/bash
# Run portuale's emerge with PORTUALE_MO_SEL=1 and split the merge-order
# trace into its own file -- the portuale half of the harness described
# in TEST/scripts/mo-trace/README.md.
#
#   ptl-trace.sh OUT-PREFIX [--] <emerge args...>
#
# Writes:
#   OUT-PREFIX.portuale.out   stdout
#   OUT-PREFIX.portuale.err   stderr (includes the MO_SEL lines)
#   OUT-PREFIX.portuale.trace just the MO_SEL lines (align-traces input)
#
# Env: PORTUALE_BIN overrides the binary (default: the repo's release
# multicall `emerge` symlink). PORTUALE_MO_SEL is forced on.

set -euo pipefail

if [ $# -lt 2 ]; then
    echo "usage: ptl-trace.sh OUT-PREFIX [--] <emerge args...>" >&2
    exit 2
fi

OUT=$1
shift
[ "${1:-}" = "--" ] && shift

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$HERE/../../.." && pwd)
BIN=${PORTUALE_BIN:-"$REPO_ROOT/rust/target/release/emerge"}

if [ ! -x "$BIN" ]; then
    echo "ptl-trace: no executable at $BIN (build with cargo build --release -p portuale)" >&2
    exit 2
fi

PORTUALE_MO_SEL=1 "$BIN" "$@" >"$OUT.portuale.out" 2>"$OUT.portuale.err"
rc=$?
grep '^MO_SEL ' "$OUT.portuale.err" >"$OUT.portuale.trace" || true
echo "ptl-trace: rc=$rc  $(wc -l <"$OUT.portuale.trace") MO_SEL lines -> $OUT.portuale.trace"
exit $rc
