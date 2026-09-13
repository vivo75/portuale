#!/bin/bash
# test-gpkg-structure.sh -- host-side self-test for gpkg-structure.sh.
#
# Stays honest by construction: it first proves the validator is clean on
# real Portage-built archives (the L1 pkgcache is a 78-archive oracle),
# then injects one mutation per check category and requires the
# validator to catch each one.
#
#   TEST/compare/test-gpkg-structure.sh [source-archive.gpkg.tar]
#
# Exit: 0 all good, 1 a case failed, 2 setup error. No container needed.
# If no archive is given and the L1 pkgcache has none, the test SKIPs
# (exit 0) -- run `TEST/run/l1-merge-from-binpkg.sh` first for full
# coverage.

set -u
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TEST_DIR=$(cd "$HERE/.." && pwd)
CHECK="$HERE/gpkg-structure.sh"
LOGS="$TEST_DIR/logs"

SRC=${1:-}
if [ -z "$SRC" ]; then
  SRC=$(find "$LOGS/_l1-pkgcache" -name '*.gpkg.tar' 2>/dev/null | LC_ALL=C sort | head -1)
fi
if [ -z "$SRC" ] || [ ! -f "$SRC" ]; then
  echo "test-gpkg-structure: SKIP -- no source archive (run TEST/run/l1-merge-from-binpkg.sh first)"
  exit 0
fi
SRC=$(realpath "$SRC")

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
PASS=0; FAIL=0
ok()  { PASS=$((PASS + 1)); printf 'ok   - %s\n' "$1"; }
bad() { FAIL=$((FAIL + 1)); printf 'FAIL - %s\n' "$1"; }

expect_rc() {  # <want-rc> <label> <cmd...>
  local want=$1 label=$2; shift 2
  local out rc
  out=$("$@" 2>&1); rc=$?
  if [ "$rc" = "$want" ]; then ok "$label"; else
    bad "$label (rc=$rc, want $want)"; printf '%s\n' "$out" | sed 's/^/      /' | head -5
  fi
}
expect_hit() {  # <label> <category> <cmd...>
  local label=$1 cat=$2; shift 2
  local out
  out=$("$@" 2>&1) || true
  if printf '%s\n' "$out" | grep -q "^\[$cat\]"; then ok "$label"; else
    bad "$label (no [$cat] finding)"; printf '%s\n' "$out" | sed 's/^/      /' | head -5
  fi
}

# find the pkgdir root (nearest ancestor with a Packages file)
PKGDIR_SRC=""
d=$(dirname "$SRC")
while [ "$d" != "/" ]; do
  [ -f "$d/Packages" ] && { PKGDIR_SRC=$d; break; }
  d=$(dirname "$d")
done
REL=${SRC#"${PKGDIR_SRC:-/nonexistent}"/}

# --- 1. positive controls ----------------------------------------------
expect_rc 0 "clean on the real source archive" "$CHECK" "$SRC"
if [ -n "$PKGDIR_SRC" ]; then
  mkdir -p "$TMP/clean-pkgdir/$(dirname "$REL")"
  cp -a "$SRC" "$TMP/clean-pkgdir/$REL"
  cp "$PKGDIR_SRC/Packages" "$TMP/clean-pkgdir/Packages"
  expect_rc 0 "clean --dir --packages on a one-archive pkgdir" \
    "$CHECK" --dir "$TMP/clean-pkgdir" --packages
fi

PREFIX=$(tar -tf "$SRC" | head -1 | cut -d/ -f1)
META=$(tar -tf "$SRC" | sed -n 's:^'"$PREFIX"'/\(metadata\.tar[^/]*\)$:\1:p')
IMG=$(tar -tf "$SRC" | sed -n 's:^'"$PREFIX"'/\(image\.tar[^/]*\)$:\1:p')
[ -n "$META" ] && [ -n "$IMG" ] || { echo "cannot discover inner members of $SRC" >&2; exit 2; }

unpack() { rm -rf "$2"; mkdir -p "$2"; tar -xf "$1" -C "$2"; }
repack() { tar -C "$1" -cf "$3" "$2"; }

# --- 2. OUTER: truncated container -------------------------------------
head -c 1024 "$SRC" > "$TMP/trunc.gpkg.tar"
expect_hit "truncated container -> OUTER" OUTER "$CHECK" "$TMP/trunc.gpkg.tar"

# --- 3. OUTER: a required member removed -------------------------------
unpack "$SRC" "$TMP/d1"; rm -f "$TMP/d1/$PREFIX/Manifest"
repack "$TMP/d1" "$PREFIX" "$TMP/nomanifest.gpkg.tar"
expect_hit "Manifest removed -> OUTER" OUTER "$CHECK" "$TMP/nomanifest.gpkg.tar"

# --- 4. MANIFEST: a DATA record dropped --------------------------------
unpack "$SRC" "$TMP/d2"
grep -vF "DATA $IMG " "$TMP/d2/$PREFIX/Manifest" > "$TMP/d2/$PREFIX/Manifest.new"
mv "$TMP/d2/$PREFIX/Manifest.new" "$TMP/d2/$PREFIX/Manifest"
repack "$TMP/d2" "$PREFIX" "$TMP/manifest.gpkg.tar"
expect_hit "Manifest DATA record dropped -> MANIFEST" MANIFEST "$CHECK" "$TMP/manifest.gpkg.tar"

# --- 5. METADATA: a required key removed --------------------------------
# rewrite the inner metadata tar (and refresh the Manifest record for it)
unpack "$SRC" "$TMP/d5"
mkdir -p "$TMP/keys"
zstd -dc "$TMP/d5/$PREFIX/$META" | tar -xf - -C "$TMP/keys"
rm -f "$TMP/keys/metadata/EAPI"
tar -C "$TMP/keys" -cf "$TMP/new-meta.tar" metadata
zstd -q -f "$TMP/new-meta.tar" -o "$TMP/d5/$PREFIX/$META"
python3 - "$TMP/d5/$PREFIX" "$META" <<'PY'
import hashlib, os, sys
p, member = sys.argv[1], sys.argv[2]
path = os.path.join(p, member)
data = open(path, "rb").read()
out = []
for line in open(os.path.join(p, "Manifest")):
    parts = line.split()
    if len(parts) == 7 and parts[0] == "DATA" and parts[1] == member:
        parts[2] = str(len(data))
        parts[4] = hashlib.sha512(data).hexdigest()
        parts[6] = hashlib.blake2b(data).hexdigest()
        line = " ".join(parts) + "\n"
    out.append(line)
open(os.path.join(p, "Manifest"), "w").writelines(out)
PY
repack "$TMP/d5" "$PREFIX" "$TMP/nokeys.gpkg.tar"
expect_hit "required metadata key removed -> METADATA" METADATA "$CHECK" "$TMP/nokeys.gpkg.tar"

# --- 6. INNER: corrupted inner compressor stream ------------------------
unpack "$SRC" "$TMP/d6"
head -c 64 /dev/urandom > "$TMP/d6/$PREFIX/$META"
repack "$TMP/d6" "$PREFIX" "$TMP/badzstd.gpkg.tar"
expect_hit "corrupt metadata.tar.zst -> INNER" INNER "$CHECK" "$TMP/badzstd.gpkg.tar"

# --- 7. MULTI: filename suffix vs metadata BUILD_ID ---------------------
BASE=$(basename "$SRC")
case $BASE in
  *-[0-9]*.gpkg.tar)
    BADNAME=${BASE%-*}-99999.gpkg.tar
    cp "$SRC" "$TMP/$BADNAME"
    expect_hit "filename BUILD_ID mismatch -> MULTI" MULTI "$CHECK" "$TMP/$BADNAME" ;;
  *) echo "skip - multi-instance mutation (source name has no -<id> suffix)" ;;
esac

# --- 8/9. INDEX: spoofed MD5, then a missing REPO -----------------------
if [ -n "$PKGDIR_SRC" ]; then
  mkdir -p "$TMP/pkgdir/$(dirname "$REL")"
  cp -a "$SRC" "$TMP/pkgdir/$REL"
  cp "$PKGDIR_SRC/Packages" "$TMP/pkgdir/Packages"
  sed_edit_stanza() {  # <field> <replacement-or-empty-to-delete>
    python3 - "$TMP/pkgdir/Packages" "$REL" "$1" "$2" <<'PY'
import sys
path, want, field, value = sys.argv[1:5]
out, found = [], False
for block in open(path).read().split("\n\n"):
    lines = block.splitlines()
    if any(l == f"PATH: {want}" for l in lines):
        found = True
        new = []
        for l in lines:
            if l.startswith(field + ": "):
                if value != "":
                    new.append(f"{field}: {value}")
                continue
            new.append(l)
        block = "\n".join(new)
    out.append(block)
assert found, "stanza not found"
open(path, "w").write("\n\n".join(out))
PY
  }
  sed_edit_stanza MD5 00000000000000000000000000000000
  expect_hit "spoofed Packages MD5 -> INDEX" INDEX "$CHECK" --dir "$TMP/pkgdir" --packages
  sed_edit_stanza MD5 "$(md5sum "$TMP/pkgdir/$REL" | cut -d' ' -f1)"
  sed_edit_stanza REPO ""
  expect_hit "Packages stanza without REPO -> INDEX" INDEX "$CHECK" --dir "$TMP/pkgdir" --packages
else
  echo "skip - INDEX mutations (no Packages root found above the archive)"
fi

echo
echo "test-gpkg-structure: $PASS passed, $FAIL failed"
[ "$FAIL" = 0 ]
