#!/bin/bash
# Capture a deterministic manifest of an installed system, for L1+
# filesystem/VDB parity comparison.  (Consumed by diff.py -- slice 2.)
#
#   snapshot.sh <root> <out-prefix>
#
# Produces:
#   <out-prefix>.files.tsv   one line per path, sorted:
#     path \t type \t mode \t uid \t gid \t size \t sha256|- \t link|- \t xattrs|-
#   <out-prefix>.mtimes.tsv  path \t mtime      (separate: lenient check)
#   <out-prefix>.vdb.tar     /var/db/pkg verbatim (normalised by diff.py)
#   <out-prefix>.meta.tsv    fingerprint (root, date, portage/portuale ver)
#
# type: f d l b c p s   (regular/dir/symlink/block/char/fifo/socket)
# mtimes are deliberately NOT in files.tsv (see docs/real-world-testing.md §4.3).

set -euo pipefail
ROOT=${1:?root dir}
OUT=${2:?output prefix}
ROOT=${ROOT%/}

# Paths whose *content* is runtime scratch / regenerated -- excluded
# entirely from the walk (see doc §4.2).
PRUNE=(
  "$ROOT/proc" "$ROOT/sys" "$ROOT/dev" "$ROOT/run" "$ROOT/tmp"
  "$ROOT/var/tmp" "$ROOT/var/cache" "$ROOT/var/log"
  "$ROOT/var/lib/portage/home" "$ROOT/var/db/repos" "$ROOT/usr/src"
  "$ROOT/root/.cache" "$ROOT/home"
)

prune_expr=()
for p in "${PRUNE[@]}"; do prune_expr+=( -path "$p" -prune -o ); done

: > "$OUT.files.tsv"
: > "$OUT.mtimes.tsv"

# find + stat; sha256 only for regular files; xattrs sorted & base64'd.
find "$ROOT" "${prune_expr[@]}" -print0 2>/dev/null |
LC_ALL=C sort -z |
while IFS= read -r -d '' f; do
  rel=${f#"$ROOT"}; rel=${rel:-/}
  # %F kind, %f mode(hex->we take octal via %a), %u %g %s %n %N
  read -r kind mode uid gid size <<<"$(stat -c '%F %a %u %g %s' "$f")"
  case $kind in
    "regular file"|"regular empty file") t=f ;;
    "directory") t=d ;;
    "symbolic link") t=l ;;
    "block special file") t=b ;;
    "character special file") t=c ;;
    "fifo") t=p ;;
    "socket") t=s ;;
    *) t="?" ;;
  esac
  sha=- ; link=-
  if [ "$t" = f ] && [ "$size" != 0 ]; then
    sha=$(sha256sum -- "$f" 2>/dev/null | cut -d' ' -f1); sha=${sha:--}
  fi
  [ "$t" = l ] && link=$(readlink -- "$f")
  xa=$(getfattr -d -m - --absolute-names -- "$f" 2>/dev/null |
       sed -n 's/^\([^=]*\)=\(.*\)$/\1=\2/p' | LC_ALL=C sort | paste -sd, -)
  xa=${xa:--}
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$rel" "$t" "$mode" "$uid" "$gid" "$size" "$sha" "$link" "$xa" >> "$OUT.files.tsv"
  printf '%s\t%s\n' "$rel" "$(stat -c '%Y' "$f")" >> "$OUT.mtimes.tsv"
done

if [ -d "$ROOT/var/db/pkg" ]; then
  tar -C "$ROOT/var/db" --sort=name --numeric-owner \
      --mtime='@0' -cf "$OUT.vdb.tar" pkg
fi

{
  echo "root	$ROOT"
  echo "date_utc	$(date -u +%FT%TZ)"
  command -v emerge >/dev/null && echo "portage	$(emerge --version 2>/dev/null | head -1)"
  [ -x /usr/local/bin/portuale ] && echo "portuale	present"
  echo "files	$(wc -l < "$OUT.files.tsv")"
} > "$OUT.meta.tsv"

echo "snapshot: $(wc -l < "$OUT.files.tsv") paths -> $OUT.files.tsv"
