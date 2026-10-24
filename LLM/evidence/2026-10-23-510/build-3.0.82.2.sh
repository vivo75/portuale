#!/bin/bash
# #510/#529: builds the 3.0.82.2 C dep parser for the probes (no compiled
# module is in the git archive), through the tag's own meson `_parser`
# target -- not a hand-written gcc line that could drift from the packaged
# flags. Usage: build-3.0.82.2.sh <dir>
# Source: 3rdparty/portage tag portage-3.0.82.2, src/meson.build `_parser`
# (and the `ln -srnf` step that links it into lib/portage/dep/).
set -eu
out=${1:?usage: build-3.0.82.2.sh <dir>}
repo=/home/vivo/repo/PORTUALE/portuale/3rdparty/portage
mkdir -p "$out"
# The whole tree: meson needs the root meson.build, not just lib/ and src/.
git -C "$repo" archive portage-3.0.82.2 | tar -x -C "$out"
cd "$out"
meson setup build >/dev/null
meson compile -C build >/dev/null
# src/meson.build symlinks the built extension into the source tree so
# `import portage.dep._parser` works with PYTHONPATH=<out>/lib.
test -L lib/portage/dep/_parser.cpython-314-x86_64-linux-gnu.so
echo "built $out/lib"
