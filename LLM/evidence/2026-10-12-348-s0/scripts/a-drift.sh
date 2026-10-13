#!/usr/bin/env bash
# S0.2 (worker a): vendored-script drift check for portuale bin/.
# Compares every portuale-owned-tree file under bin/ against
#   (a) the pinned upstream tag (3rdparty/repos.toml [portage] ref portage-3.0.82.2),
#       read via `git show <tag>:bin/...` -- never the tag checkout's working tree;
#   (b) the host Portage's PORTAGE_BIN_PATH (/usr/lib/portage/python3.14), where
#       a same-named file exists.
# Portuale-owned files (no upstream counterpart) and bin/README.md are skipped
# for (a); they are listed, not diffed.
# Usage: a-drift.sh <portuale-root> <out-dir>
# Writes: <out-dir>/drift-tag.txt, <out-dir>/drift-host.txt (human-readable).
set -u
ROOT=${1:?portuale root}
OUT=${2:?output dir}
TAG=portage-3.0.82.2
CHECKOUT="$ROOT/3rdparty/portage"
HOSTBIN=/usr/lib/portage/python3.14
mkdir -p "$OUT"

# Portuale-owned files: present in bin/, absent upstream by design (bin/README.md
# "Portuale-owned files" section + ecompress-file note).
OWNED="portuale-python chmod-lite ebuild-ipc portageq-wrapper README.md"

{
echo "== S0.2 drift vs tag $TAG (worker a, $(date -u +%FT%TZ))"
echo "== ref: 3rdparty/repos.toml [portage] ref=$TAG"
echo "== tag commit: $(git -C "$CHECKOUT" rev-parse "$TAG^{commit}")"
echo "== host portage.VERSION: $(python3 -c 'import portage; print(portage.VERSION)')"
echo
} > "$OUT/drift-tag.txt"

{
echo "== S0.2 drift vs host PORTAGE_BIN_PATH=$HOSTBIN (worker a, $(date -u +%FT%TZ))"
echo
} > "$OUT/drift-host.txt"

cd "$ROOT/bin" || exit 1
n_same=0; n_diff=0; n_owned=0; n_missing_upstream=0
# shellcheck disable=SC2044
while IFS= read -r f; do
  base=$(basename "$f")
  skip=0
  for o in $OWNED; do [ "$base" = "$o" ] && skip=1; done
  if [ $skip = 1 ]; then
    echo "OWNED (portuale-only, not diffed): $f" >> "$OUT/drift-tag.txt"
    n_owned=$((n_owned+1))
    continue
  fi
  if ! git -C "$CHECKOUT" cat-file -e "$TAG:bin/$f" 2>/dev/null; then
    echo "MISSING-UPSTREAM (no bin/$f at tag $TAG): $f" >> "$OUT/drift-tag.txt"
    n_missing_upstream=$((n_missing_upstream+1))
    continue
  fi
  if git -C "$CHECKOUT" show "$TAG:bin/$f" | cmp -s - "$ROOT/bin/$f"; then
    n_same=$((n_same+1))
  else
    {
    echo "--- DIFF $f (bin/ vs tag $TAG)"
    git -C "$CHECKOUT" show "$TAG:bin/$f" | diff -u - "$ROOT/bin/$f" | head -60
    echo
    } >> "$OUT/drift-tag.txt"
    n_diff=$((n_diff+1))
  fi
  # host comparison, same relative path where it exists
  if [ -f "$HOSTBIN/$f" ]; then
    if cmp -s "$HOSTBIN/$f" "$ROOT/bin/$f"; then
      : # identical; stay quiet
    else
      {
      echo "--- DIFF $f (bin/ vs host $HOSTBIN/$f)"
      diff -u "$HOSTBIN/$f" "$ROOT/bin/$f" | head -40
      echo
      } >> "$OUT/drift-host.txt"
    fi
  else
    echo "NO-HOST-COUNTERPART: $f" >> "$OUT/drift-host.txt"
  fi
done < <(find . -type f | sed 's|^\./||' | sort)

{
echo
echo "== summary: same=$n_same diff=$n_diff owned=$n_owned missing-upstream=$n_missing_upstream"
} >> "$OUT/drift-tag.txt"
echo "tag-summary: same=$n_same diff=$n_diff owned=$n_owned missing-upstream=$n_missing_upstream"
echo "host diff lines: $(grep -c '^--- DIFF' "$OUT/drift-host.txt")"
