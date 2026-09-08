#!/bin/bash
# Capture a deterministic manifest of an installed system, for L1+
# filesystem/VDB parity comparison.  (Consumed by diff.py.)
#
#   snapshot.sh [--paths <file>] <root> <out-prefix>
#
# --paths <file>: restrict the walk to the newline-separated ROOT-
# relative paths in <file>. An entry ending in `/` is a directory to
# **recurse**; every other entry is stat'd alone (`-maxdepth 0` -- a
# file, or a directory whose own mode/owner we want but not its
# contents, since CONTENTS `dir` lines include shared parents like
# /usr/lib64). Without --paths the whole tree under <root> is walked.
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
trap 'echo "snapshot.sh: FAILED at line $LINENO (exit $?)" >&2' ERR
PATHS_FILE=""
VDB_LIST=""
while true; do
  case ${1:-} in
    --paths)    PATHS_FILE=${2:?--paths needs a file}; shift 2 ;;
    # --vdb-list <file>: only tar these `cat/pf` vdb dirs (one per line),
    # not all of /var/db/pkg. L1 passes just the merged packages.
    --vdb-list) VDB_LIST=${2:?--vdb-list needs a file}; shift 2 ;;
    *) break ;;
  esac
done
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
  # /var/db/pkg is compared via the vdb.tar (norm_vdb / diff_vdb), never
  # the files manifest -- otherwise every BUILD_TIME/COUNTER shows up
  # twice, once un-normalised as a CONTENT diff.
  "$ROOT/var/db/pkg"
)

prune_expr=()
for p in "${PRUNE[@]}"; do prune_expr+=( -path "$p" -prune -o ); done

: > "$OUT.files.tsv"
: > "$OUT.mtimes.tsv"

emit_null() {  # feed to the stat loop below
  if [ -z "$PATHS_FILE" ]; then
    find "$ROOT" "${prune_expr[@]}" -print0 2>/dev/null
    return
  fi
  local recurse=() single=()
  while IFS= read -r p; do
    [ -n "$p" ] || continue
    local rec=0
    case $p in */) rec=1; p=${p%/} ;; esac
    case $p in /*) : ;; *) p=/$p ;; esac
    if [ -e "$ROOT$p" ] || [ -L "$ROOT$p" ]; then
      if [ "$rec" = 1 ]; then recurse+=("$ROOT$p"); else single+=("$ROOT$p"); fi
    fi
  done < "$PATHS_FILE"
  if [ ${#recurse[@]} -gt 0 ]; then
    find "${recurse[@]}" "${prune_expr[@]}" -print0 2>/dev/null || true
  fi
  if [ ${#single[@]} -gt 0 ]; then
    find "${single[@]}" -maxdepth 0 -print0 2>/dev/null || true
  fi
  return 0
}

# find + stat; sha256 only for regular files; xattrs sorted & base64'd.
emit_null |
LC_ALL=C sort -z -u |
while IFS= read -r -d '' f; do
  rel=${f#"$ROOT"}; rel=${rel:-/}
  # `%F` is multi-word ("regular file", "symbolic link") -- use a `|`
  # delimiter, not whitespace. %a octal mode, %u %g %s %Y.
  sr=$(stat -c '%F|%a|%u|%g|%s|%Y' "$f" 2>/dev/null) || sr=
  [ -n "$sr" ] || continue   # vanished mid-walk
  IFS='|' read -r kind mode uid gid size mtime <<<"$sr" || true
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
    sha=$(sha256sum -- "$f" 2>/dev/null | cut -d' ' -f1) || sha=
    sha=${sha:--}
  fi
  [ "$t" = l ] && { link=$(readlink -- "$f") || link=-; }
  # `-h`: read the node's own xattrs, never dereference -- a dangling
  # symlink (symfarm fixture) would otherwise make getfattr fail ENOENT
  # and, under `set -o pipefail`, abort the whole walk. `|| xa=` keeps a
  # genuine getfattr error from doing the same.
  xa=$(getfattr -h -d -m - --absolute-names -- "$f" 2>/dev/null |
       sed -n 's/^\([^=]*\)=\(.*\)$/\1=\2/p' | LC_ALL=C sort | paste -sd, -) || xa=
  xa=${xa:--}
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$rel" "$t" "$mode" "$uid" "$gid" "$size" "$sha" "$link" "$xa" >> "$OUT.files.tsv"
  printf '%s\t%s\n' "$rel" "$mtime" >> "$OUT.mtimes.tsv"
done || echo "snapshot.sh: walk pipeline returned non-zero -- files.tsv may be short (continuing to VDB tar)" >&2

# --paths can list a dir and a file inside it -> dedup.
LC_ALL=C sort -u -o "$OUT.files.tsv" "$OUT.files.tsv"
LC_ALL=C sort -u -o "$OUT.mtimes.tsv" "$OUT.mtimes.tsv"

if [ -d "$ROOT/var/db/pkg" ]; then
  vdb_members=()
  if [ -n "$VDB_LIST" ]; then
    while IFS= read -r cp; do
      [ -n "$cp" ] || continue
      if [ -d "$ROOT/var/db/pkg/$cp" ]; then vdb_members+=("pkg/$cp"); fi
    done < "$VDB_LIST"
  else
    vdb_members=(pkg)
  fi
  if [ ${#vdb_members[@]} -gt 0 ]; then
    tar -C "$ROOT/var/db" --sort=name --numeric-owner --mtime='@0' \
        --warning=no-file-changed -cf "$OUT.vdb.tar" "${vdb_members[@]}" \
      || echo "!!! vdb tar exited $? (partial $OUT.vdb.tar)" >&2
  else
    : > "$OUT.vdb.tar" || true
    tar -cf "$OUT.vdb.tar" -T /dev/null
  fi
fi

{
  echo "root	$ROOT"
  echo "date_utc	$(date -u +%FT%TZ)"
  command -v emerge >/dev/null && echo "portage	$(emerge --version 2>/dev/null | head -1)"
  [ -x /usr/local/bin/portuale ] && echo "portuale	present"
  echo "files	$(wc -l < "$OUT.files.tsv")"
} > "$OUT.meta.tsv"

echo "snapshot: $(wc -l < "$OUT.files.tsv") paths -> $OUT.files.tsv"
