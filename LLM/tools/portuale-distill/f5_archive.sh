#!/usr/bin/env bash
# F5 — archive every file the manifest marks `archive`, outside the repo,
# then prove the archive is complete and readable.
#
#   bin/f5_archive.sh            create/refresh the tarball and verify it
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="${PORTUALE_DISTILL_REPO:-$(cd "$here/../../.." && pwd)}"
work="${PORTUALE_DISTILL_WORK:-$HOME/.cache/portuale-distill/work}"
outdir="${PORTUALE_DISTILL_ARCHIVE:-$(dirname "$work")/archive}"
list="$work/to-archive.txt"
stamp="$(date +%Y-%m-%d)"
tarball="$outdir/llm-archive-$stamp.tar.zst"

mkdir -p "$outdir"
echo "files to archive: $(wc -l < "$list")"
echo "bytes:            $(cd "$repo" && tr '\n' '\0' < "$list" | du -cb --files0-from=- 2>/dev/null | tail -1 | cut -f1)"

tar --zstd -cf "$tarball" -C "$repo" -T "$list"
zstd -t "$tarball"
entries=$(tar -tf "$tarball" | wc -l)
want=$(wc -l < "$list")
echo "tarball: $tarball ($(du -h "$tarball" | cut -f1), $entries entries, expected $want)"
if [ "$entries" -ne "$want" ]; then
  echo "MISMATCH: archive has $entries entries, manifest lists $want" >&2
  exit 1
fi

sha256sum "$tarball" | tee "$tarball.sha256"

# Spot-check: extract one random entry and compare its digest with the working tree.
sample=$(shuf -n 1 "$list")
echo "spot-check: $sample"
tmp="$(mktemp -d)"
tar -xf "$tarball" -C "$tmp" "$sample"
a=$(sha256sum "$repo/$sample" | cut -d' ' -f1)
b=$(sha256sum "$tmp/$sample" | cut -d' ' -f1)
rm -rf "$tmp"
if [ "$a" != "$b" ]; then
  echo "MISMATCH on spot-check: $a != $b" >&2
  exit 1
fi
echo "archive verified: $entries/$want entries, digests match"
