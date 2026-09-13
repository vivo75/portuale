#!/bin/bash
# gpkg-diff.sh -- normalised gpkg-vs-gpkg diff (L2 tooling spec §4).
# Thin wrapper so the spec'd name stays; the implementation is
# TEST/compare/gpkg_diff.py, which imports normalize.py's own
# environment ruleset instead of duplicating it.
#
#   gpkg-diff.sh [--mode strict|payload-tolerant] <a.gpkg.tar> <b.gpkg.tar>
#
# Exit: 0 no hard findings, 1 hard finding(s), 2 usage/IO.
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
exec python3 "$HERE/gpkg_diff.py" "$@"
