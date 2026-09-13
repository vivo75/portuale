#!/bin/bash
# test-gpkg-diff.sh -- host-side self-test for gpkg-diff.sh.
#
# Positive controls: a strict pair of deterministic `porttest` archives
# (two BUILD_ID instances) must be hard-clean; a payload-tolerant pair
# of real packages likewise (compiled bytes may differ, structure may
# not). Then one mutation per hard bucket, plus payload severity
# flipping with --mode.
#
#   TEST/compare/test-gpkg-diff.sh
#
# Exit 0 all good, 1 a case failed, 2 setup error. SKIPs (exit 0) when
# the L1 pkgcache has no suitable pair -- run
# `TEST/run/l1-merge-from-binpkg.sh TEST/atomlists/l1-porttest.txt` first.

set -u
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TEST_DIR=$(cd "$HERE/.." && pwd)
DIFF="$HERE/gpkg-diff.sh"
P="$TEST_DIR/logs/_l1-pkgcache"

first_pair() {  # <glob-dir>  echoes "a b" when two instances exist
  local d=$1
  local files; files=$(find "$d" -name '*.gpkg.tar' 2>/dev/null | LC_ALL=C sort)
  local n; n=$(printf '%s\n' "$files" | grep -c . || true)
  [ "$n" -ge 2 ] || return 1
  printf '%s %s\n' "$(printf '%s\n' "$files" | sed -n 1p)" "$(printf '%s\n' "$files" | sed -n 2p)"
}
read -r SA SB < <(first_pair "$P/porttest/phases" || true)
read -r TA TB < <(first_pair "$P/app-text/tree" || true)
if [ -z "${SA:-}" ] || [ -z "${TA:-}" ]; then
  echo "test-gpkg-diff: SKIP -- need two instances each of porttest/phases and app-text/tree (run the L1 porttest set first)"
  exit 0
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
PASS=0; FAIL=0
ok()  { PASS=$((PASS + 1)); printf 'ok   - %s\n' "$1"; }
bad() { FAIL=$((FAIL + 1)); printf 'FAIL - %s\n' "$1"; }
expect_hit() {  # <label> <regex> <cmd...>
  local label=$1 pat=$2; shift 2
  local out
  out=$("$@" 2>&1) || true
  if printf '%s\n' "$out" | grep -qE "$pat"; then ok "$label"; else
    bad "$label (no match for /$pat/)"; printf '%s\n' "$out" | sed 's/^/      /' | head -6
  fi
}
expect_rc() {  # <want> <label> <cmd...>
  local want=$1 label=$2; shift 2
  local out rc
  out=$("$@" 2>&1); rc=$?
  if [ "$rc" = "$want" ]; then ok "$label"; else
    bad "$label (rc=$rc want $want)"; printf '%s\n' "$out" | sed 's/^/      /' | head -6
  fi
}

# mutate.py <src> <dst> <kind>
cat > "$TMP/mutate.py" <<'PY'
import io, os, subprocess, sys, tarfile, tempfile
from pathlib import Path

src, dst, kind = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3]

def _inner(td, name):
    p = td / name
    if name.endswith(".zst"):
        return subprocess.run(["zstd", "-dc", "--", str(p)], check=True, capture_output=True).stdout
    return p.read_bytes()

def _members(data):
    with tarfile.open(fileobj=io.BytesIO(data)) as t:
        out = []
        for m in t.getmembers():
            f = t.extractfile(m) if m.isfile() else None
            out.append((m, f.read() if f else b""))
        return out

def _pack(members):
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w") as t:
        for m, data in members:
            t.addfile(m, io.BytesIO(data) if m.isfile() else None)
    return buf.getvalue()

with tempfile.TemporaryDirectory() as tds:
    td = Path(tds)
    with tarfile.open(src) as t:
        t.extractall(td, filter="data")
    prefix = next(p.name for p in td.iterdir() if p.is_dir())
    cdir = td / prefix
    meta = next(p.name for p in cdir.iterdir() if p.name.startswith("metadata.tar"))
    image = next(p.name for p in cdir.iterdir() if p.name.startswith("image.tar"))
    mdata = _inner(cdir, meta)
    idata = _inner(cdir, image)

    if kind == "metadata-key":
        mem = _members(mdata)
        mem = [(m, b"9\n") for (m, d) in mem if m.name == "metadata/EAPI"] or mem
        # simpler: drop EAPI
        mem = [(m, d) for (m, d) in _members(mdata) if m.name != "metadata/EAPI"]
        mdata = _pack(mem)
    elif kind in ("payload", "mode", "remove"):
        mem = _members(idata)
        idx = next(i for i, (m, d) in enumerate(mem) if m.isfile() and m.size > 0)
        m, d = mem[idx]
        if kind == "payload":
            m2 = tarfile.TarInfo(m.name); m2.mode = m.mode; m2.uid = m.uid; m2.gid = m.gid
            m2.mtime = m.mtime; m2.type = m.type
            mem[idx] = (m2, d[:1] + bytes([d[1] ^ 0xFF]) + d[2:] if len(d) > 1 else b"X")
        elif kind == "mode":
            m2 = tarfile.TarInfo(m.name); m2.mode = (m.mode & 0o7777) ^ 0o100
            m2.uid = m.uid; m2.gid = m.gid; m2.mtime = m.mtime; m2.type = m.type
            mem[idx] = (m2, d)
        else:
            del mem[idx]
        idata = _pack(mem)
    else:
        raise SystemExit(f"unknown kind {kind}")

    out = td / "out"; out.mkdir()
    (out / prefix).mkdir()
    for name, data in ((meta, mdata), (image, idata)):
        p = out / prefix / name
        if name.endswith(".zst"):
            p.write_bytes(subprocess.run(["zstd", "-q", "-c"], input=data, check=True, capture_output=True).stdout)
        else:
            p.write_bytes(data)
    for extra in ("gpkg-1", "Manifest"):
        s = cdir / extra
        if s.exists():
            (out / prefix / extra).write_bytes(s.read_bytes())
    with tarfile.open(dst, "w") as t:
        for p in sorted((out / prefix).iterdir()):
            t.add(p, arcname=f"{prefix}/{p.name}")
PY

for kind in metadata-key payload mode remove; do
  python3 "$TMP/mutate.py" "$SB" "$TMP/mut-$kind.gpkg.tar" "$kind" || { echo "mutation $kind failed" >&2; exit 2; }
done
head -c 512 "$SB" > "$TMP/trunc.gpkg.tar"

# --- positive controls --------------------------------------------------
expect_rc 0 "strict: two porttest instances are hard-clean" \
  "$DIFF" --mode strict "$SA" "$SB"
expect_rc 0 "tolerant: two real portage instances are hard-clean" \
  "$DIFF" --mode payload-tolerant "$TA" "$TB"

# --- mutations ----------------------------------------------------------
expect_hit "missing metadata key -> metadata" '^\[metadata\]' \
  "$DIFF" --mode strict "$SA" "$TMP/mut-metadata-key.gpkg.tar"
expect_hit "changed payload byte -> hard in strict" '^\[image:paths\]' \
  "$DIFF" --mode strict "$SA" "$TMP/mut-payload.gpkg.tar"
expect_hit "changed payload byte -> tolerated payload" '^\[payload\]' \
  "$DIFF" --mode payload-tolerant "$SA" "$TMP/mut-payload.gpkg.tar"
expect_rc 0 "changed payload byte tolerated -> rc 0" \
  "$DIFF" --mode payload-tolerant "$SA" "$TMP/mut-payload.gpkg.tar"
expect_hit "changed mode -> image:paths hard (both modes)" '^\[image:paths\]' \
  "$DIFF" --mode payload-tolerant "$SA" "$TMP/mut-mode.gpkg.tar"
expect_hit "removed image path -> image:paths hard" '^\[image:paths\]' \
  "$DIFF" --mode strict "$SA" "$TMP/mut-remove.gpkg.tar"
expect_rc 2 "truncated archive -> usage/IO rc 2" \
  "$DIFF" --mode strict "$SA" "$TMP/trunc.gpkg.tar"

echo
echo "test-gpkg-diff: $PASS passed, $FAIL failed"
[ "$FAIL" = 0 ]
