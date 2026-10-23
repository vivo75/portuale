#!/bin/bash
# #510: builds the 3.0.82.2 C dep parser for the probes (no compiled module
# is in the git archive). Usage: build-3.0.82.2.sh <dir>
# Source: 3rdparty/portage tag portage-3.0.82.2, src/meson.build `_parser`.
set -eu
out=${1:?usage: build-3.0.82.2.sh <dir>}
repo=/home/vivo/repo/PORTUALE/portuale/3rdparty/portage
mkdir -p "$out"
git -C "$repo" archive portage-3.0.82.2 lib src | tar -x -C "$out"
cd "$out"
gcc -O2 -shared -fPIC -Isrc -I/usr/include/python3.14 src/dep_parser_core.c src/dep_parser.c \
  -o lib/portage/dep/_parser.cpython-314-x86_64-linux-gnu.so
echo "built $out/lib"
