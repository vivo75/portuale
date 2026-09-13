#!/bin/bash
# gpkg-structure.sh -- structural validation of Gentoo gpkg binary
# packages (`.gpkg.tar`), for the L2 test bed.
#
#   gpkg-structure.sh <archive.gpkg.tar>...
#   gpkg-structure.sh --dir <PKGDIR> [--packages]
#
# Format-level checks, not byte-level: container member set/shape, inner
# tar roots, the metadata key set real Portage always writes, the
# embedded Manifest record set, and (with --packages) the
# `<PKGDIR>/Packages` index entry for each archive. Comparing two builds
# is gpkg-diff.sh's job, not this one's.
#
# Exit: 0 clean, 1 findings, 2 usage/IO. One line per finding:
#   [CATEGORY] <archive-or-PKGDIR>: <detail>
# Categories: OUTER, INNER, METADATA, MANIFEST, INDEX, MULTI, IO.
#
# Format ground truth (verified on the L1 pkgcache, 2026-09-13):
#   <cat>/<pn>/<pf>-<BUILD_ID>.gpkg.tar        (multi-instance), or the
#   legacy flat <cat>/<pf>.gpkg.tar; members:
#     <prefix>/gpkg-1                 (0-byte format marker)
#     <prefix>/metadata.tar[.<comp>]  (flat members `metadata/<KEY>`)
#     <prefix>/image.tar[.<comp>]     (members `image/...`)
#     <prefix>/Manifest               (DATA lines: SHA512 + BLAKE2B)
#     [optional *.sig sidecars]
#   Real writer: lib/portage/gpkg.py:763 (`gpkg-1`), :1002-1054
#   (image), :1297-1338 (metadata); metadata key set from
#   `__dyn_install` + `_post_src_install_write_metadata`
#   (doebuild.py:2700-3005). Portuale reader/writer:
#   rust/portuale/src/binpkg.rs.
#
# tar + zstd + md5sum + python3 only. The only Python use is the
# Packages-index stanza parse (the same `KEY: value` / blank-line format
# diff.py's allowlist work already assumes).

set -u

ME=$(basename "$0")
FINDINGS=0
ARCHIVES=0
fail() { printf '[%s] %s: %s\n' "$1" "$2" "$3"; FINDINGS=$((FINDINGS + 1)); }

# Metadata keys every real Portage gpkg carries. This is the exact
# intersection of the member sets of all 78 archives in the L1 pkgcache
# (2026-09-13) -- LICENSE/HOMEPAGE/INHERITED/*DEPEND/NEEDED* vary with
# the package and are deliberately not required. `BUILD_ID` is
# conditional on a multi-instance name (checked separately).
REQUIRED_METADATA="CATEGORY PF SLOT EAPI DESCRIPTION KEYWORDS \
DEFINED_PHASES USE FEATURES BUILD_TIME SIZE IUSE IUSE_EFFECTIVE \
repository REPO_REVISIONS CBUILD CHOST CFLAGS CXXFLAGS LDFLAGS"

infer_comp() {  # metadata.tar.zst -> zst, metadata.tar -> ""
  case $1 in *.zst) echo zst;; *.gz) echo gz;; *.bz2) echo bz2;; *.xz) echo xz;; *) echo "";; esac
}

# meta_value <metadata.tar> <KEY>  -- first line of metadata/<KEY>
meta_value() { tar -xOf "$1" "metadata/$2" 2>/dev/null | head -1 | tr -d '\n'; }

# list_inner <archive-file> <comp>  -- member names of an inner tar
list_inner() {
  local f=$1 comp=$2
  case $comp in
    "")  tar -tf "$f" ;;
    zst) zstd -dc -- "$f" | tar -tf - ;;
    *)   return 1 ;;
  esac
}

# uninner <archive-file> <comp> <dest.tar>
uninner() {
  local f=$1 comp=$2 dest=$3
  case $comp in
    "")  cp -f "$f" "$dest" ;;
    zst) zstd -dc -- "$f" > "$dest" ;;
    *)   return 1 ;;
  esac
}

ARCHNAME=""
check_archive() {
  local archive=$1
  local abspath
  if ! abspath=$(realpath -e "$archive" 2>/dev/null) || [ ! -f "$abspath" ]; then
    fail IO "$archive" "not a readable file"; return
  fi
  ARCHNAME=$archive
  ARCHIVES=$((ARCHIVES + 1))

  local tmp; tmp=$(mktemp -d) || { fail IO "$archive" "mktemp failed"; return; }

  # --- 1. outer container ---------------------------------------------
  local outer
  if ! outer=$(tar -tf "$abspath" 2>&1); then
    fail OUTER "$archive" "tar cannot read the container: ${outer##*$'\n'}"
    rm -rf "$tmp"; return
  fi
  local prefix="" mixed=0
  while IFS= read -r name; do
    name=${name%/}
    [ -n "$name" ] || continue
    case $name in
      /*|..|../*|*/..|*/../*)
        fail OUTER "$archive" "unsafe member path: $name"; mixed=1; continue ;;
    esac
    local topseg=${name%%/*}
    if [ -z "$prefix" ]; then prefix=$topseg
    elif [ "$prefix" != "$topseg" ]; then
      fail OUTER "$archive" "mixed top-level dirs: $prefix vs $topseg"; mixed=1
    fi
  done <<<"$outer"
  [ "$mixed" = 0 ] || { rm -rf "$tmp"; return; }
  [ -n "$prefix" ] || { fail OUTER "$archive" "empty container"; rm -rf "$tmp"; return; }

  if ! tar -xf "$abspath" -C "$tmp" 2>/dev/null; then
    fail OUTER "$archive" "tar -xf failed"; rm -rf "$tmp"; return
  fi
  # only the <prefix>/ dir may exist at the top level of the container
  local stray
  stray=$(find "$tmp" -maxdepth 1 -mindepth 1 ! -name "$prefix" -printf '%f\n' | head -1)
  [ -z "$stray" ] || fail OUTER "$archive" "top-level member outside $prefix/: $stray"
  # real `_allocate_filename_multi` ties the on-disk name to the inner
  # `<pf>-<BUILD_ID>` prefix; a renamed copy is not a valid archive.
  local fname=${abspath##*/}; fname=${fname%.gpkg.tar}
  [ "$fname" = "$prefix" ] || fail MULTI "$archive" "filename '$fname' != inner prefix '$prefix'"
  local src="$tmp/$prefix"
  [ -d "$src" ] || { fail OUTER "$archive" "no $prefix/ directory"; rm -rf "$tmp"; return; }

  # member set
  local meta_member="" image_member="" have_marker=0 have_manifest=0
  local -a members=()
  while IFS= read -r m; do
    members+=("$m")
    case $m in
      gpkg-1) have_marker=1 ;;
      metadata.tar|metadata.tar.*) meta_member=$m ;;
      image.tar|image.tar.*) image_member=$m ;;
      Manifest) have_manifest=1 ;;
      *.sig) : ;;
      *) fail OUTER "$archive" "unexpected container member: $m" ;;
    esac
  done <<<"$(cd "$src" && find . -maxdepth 1 -mindepth 1 -printf '%f\n' | sort)"
  [ "$have_marker" = 1 ] || fail OUTER "$archive" "missing the 0-byte gpkg-1 marker"
  if [ "$have_marker" = 1 ] && [ -s "$src/gpkg-1" ]; then
    fail OUTER "$archive" "gpkg-1 is not empty ($(stat -c%s "$src/gpkg-1") bytes)"
  fi
  if [ -z "$meta_member" ]; then fail OUTER "$archive" "missing metadata.tar[.comp]"; fi
  if [ -z "$image_member" ]; then fail OUTER "$archive" "missing image.tar[.comp]"; fi
  if [ "$have_manifest" = 0 ]; then fail OUTER "$archive" "missing Manifest"; fi
  if [ -z "$meta_member" ] || [ -z "$image_member" ]; then rm -rf "$tmp"; return; fi

  local mcomp icomp
  mcomp=$(infer_comp "$meta_member"); icomp=$(infer_comp "$image_member")
  if [ "$mcomp" = gz ] || [ "$mcomp" = bz2 ] || [ "$mcomp" = xz ]; then
    fail INNER "$archive" "metadata compression .$mcomp is not a gpkg compressor"
  fi
  if [ "$icomp" = gz ] || [ "$icomp" = bz2 ] || [ "$icomp" = xz ]; then
    fail INNER "$archive" "image compression .$icomp is not a gpkg compressor"
  fi

  # --- 2. inner tars ---------------------------------------------------
  local imembers img
  if ! imembers=$(list_inner "$src/$meta_member" "$mcomp"); then
    fail INNER "$archive" "cannot read/decompress $meta_member"; rm -rf "$tmp"; return
  fi
  imembers=$(printf '%s\n' "$imembers" | sed 's:/$::' | sed '/^$/d')
  if [ -z "$imembers" ]; then fail INNER "$archive" "metadata.tar is empty"; fi
  local x
  while IFS= read -r x; do
    case $x in metadata/*) : ;; *) fail INNER "$archive" "metadata member outside metadata/: $x" ;; esac
  done <<<"$imembers"
  if ! img=$(list_inner "$src/$image_member" "$icomp"); then
    fail INNER "$archive" "cannot read/decompress $image_member"; rm -rf "$tmp"; return
  fi
  img=$(printf '%s\n' "$img" | sed 's:/$::' | sed '/^$/d')
  if [ -z "$img" ]; then fail INNER "$archive" "image.tar is empty"; fi
  while IFS= read -r x; do
    case $x in image|image/*) : ;; *) fail INNER "$archive" "image member outside image/: $x" ;; esac
  done <<<"$img"

  # --- 3. metadata key set + identity ----------------------------------
  local keys; keys=$(printf '%s\n' "$imembers" | sed -n 's:^metadata/::p' | sort)
  local key
  for key in $REQUIRED_METADATA; do
    printf '%s\n' "$keys" | grep -qxF "$key" || fail METADATA "$archive" "missing metadata/$key"
  done
  local pf
  pf=$(printf '%s\n' "$keys" | grep -x '[^/]*\.ebuild' | sed 's/\.ebuild$//' | head -1)
  if [ -z "$pf" ]; then
    fail METADATA "$archive" "no metadata/<pf>.ebuild member"
  fi

  # decompress metadata once for value reads
  local mtar="$tmp/.metadata.tar" meta_ok=0
  if uninner "$src/$meta_member" "$mcomp" "$mtar" 2>/dev/null; then meta_ok=1; fi

  if [ "$meta_ok" = 1 ] && [ -n "$pf" ]; then
    local pf_val; pf_val=$(meta_value "$mtar" PF)
    [ "$pf_val" = "$pf" ] || fail METADATA "$archive" "metadata/PF '$pf_val' != ebuild member '$pf'"
  fi
  local build_id=""
  if [ "$meta_ok" = 1 ] && printf '%s\n' "$keys" | grep -qx BUILD_ID; then
    build_id=$(meta_value "$mtar" BUILD_ID)
  fi
  if [ -n "$build_id" ]; then
    local suffix=${prefix##*-}
    [ "$suffix" = "$build_id" ] || fail MULTI "$archive" "metadata/BUILD_ID '$build_id' != name suffix '$suffix'"
  fi
  if [ -n "$pf" ]; then
    if [ "$prefix" != "$pf" ] && [ "$prefix" != "${pf}-${build_id}" ]; then
      fail MULTI "$archive" "prefix '$prefix' does not match pf '$pf' (build_id '$build_id')"
    fi
  fi

  # --- 4. Manifest -----------------------------------------------------
  if [ "$have_manifest" = 1 ]; then
    local listed=0
    local -a names=()
    while IFS= read -r line; do
      case $line in
        DATA\ *)
          set -- $line
          if [ $# -ne 7 ]; then
            fail MANIFEST "$archive" "malformed DATA line: $line"; continue
          fi
          local nm=$2 msize=$3 sha=$5 blake=$7
          local real_size; real_size=$(stat -c%s "$src/$nm" 2>/dev/null || echo missing)
          [ "$msize" = "$real_size" ] || fail MANIFEST "$archive" "size mismatch for $nm ($msize vs $real_size)"
          [ ${#sha} = 128 ] || fail MANIFEST "$archive" "SHA512 for $nm is not 128 hex chars"
          [ ${#blake} = 128 ] || fail MANIFEST "$archive" "BLAKE2B for $nm is not 128 hex chars"
          names+=("$nm"); listed=$((listed + 1)) ;;
        *) : ;;  # clear-signed wrappers / future record kinds
      esac
    done < "$src/Manifest"
    [ "$listed" -ge 3 ] || fail MANIFEST "$archive" "Manifest lists only $listed DATA members"
    local want
    for want in gpkg-1 "$meta_member" "$image_member"; do
      if [ ${#names[@]} -gt 0 ]; then
        printf '%s\n' "${names[@]}" | grep -qxF "$want" || \
          fail MANIFEST "$archive" "Manifest has no DATA record for $want"
      else
        fail MANIFEST "$archive" "Manifest has no DATA record for $want"
      fi
    done
    local f
    while IFS= read -r f; do
      [ "$f" = Manifest ] && continue
      if [ ${#names[@]} -gt 0 ]; then
        printf '%s\n' "${names[@]}" | grep -qxF "$f" || \
          fail MANIFEST "$archive" "container member $f has no Manifest DATA record"
      else
        fail MANIFEST "$archive" "container member $f has no Manifest DATA record"
      fi
    done <<<"$(cd "$src" && find . -maxdepth 1 -mindepth 1 -type f -printf '%f\n' | sort)"
  fi

  rm -rf "$tmp"
}

# --- Packages index ----------------------------------------------------
# index_fields <PKGDIR> <PATH> -> "KEY<TAB>value" for that stanza;
# exit 1 no Packages file, 2 no stanza.
index_fields() {
  python3 - "$1" "$2" <<'PY'
import sys
from pathlib import Path
pkgdir, want = Path(sys.argv[1]), sys.argv[2]
f = pkgdir / "Packages"
if not f.is_file():
    sys.exit(1)
for block in f.read_text(errors="replace").split("\n\n"):
    d = {}
    for line in block.splitlines():
        k, _, v = line.partition(": ")
        if k:
            d[k] = v
    if d.get("PATH") == want:
        for k, v in d.items():
            print(f"{k}\t{v}")
        sys.exit(0)
sys.exit(2)
PY
}

check_dir() {
  local pkgdir=$1 want_index=$2
  local a
  while IFS= read -r a; do
    check_archive "$a"
  done < <(find "$pkgdir" -name '*.gpkg.tar' | LC_ALL=C sort)
  while IFS= read -r a; do
    printf '[INNER] %s: non-gpkg binary format (only gpkg is in L2 scope)\n' "$a"
  done < <(find "$pkgdir" \( -name '*.tbz2' -o -name '*.xpak' \) | LC_ALL=C sort)

  [ "$want_index" = 1 ] || return 0
  if [ ! -f "$pkgdir/Packages" ]; then
    fail INDEX "$pkgdir" "no Packages index (real Portage under pkgdir-index-trusted cannot see any archive)"
    return 0
  fi
  while IFS= read -r a; do
    local rel=${a#"$pkgdir"/}
    local fields rc
    fields=$(index_fields "$pkgdir" "$rel"); rc=$?
    if [ "$rc" = 2 ]; then
      fail INDEX "$a" "no Packages stanza with PATH: $rel"
      continue
    fi
    [ "$rc" = 0 ] || continue
    local f_size f_md5 f_build_time f_build_id f_repo
    f_size=$(printf '%s\n' "$fields" | awk -F'\t' '$1=="SIZE"{print $2}')
    f_md5=$(printf '%s\n' "$fields" | awk -F'\t' '$1=="MD5"{print $2}')
    f_build_time=$(printf '%s\n' "$fields" | awk -F'\t' '$1=="BUILD_TIME"{print $2}')
    f_build_id=$(printf '%s\n' "$fields" | awk -F'\t' '$1=="BUILD_ID"{print $2}')
    f_repo=$(printf '%s\n' "$fields" | awk -F'\t' '$1=="REPO"{print $2}')
    [ "$f_size" = "$(stat -c%s "$a")" ] || fail INDEX "$a" "Packages SIZE '$f_size' != file size"
    [ "$f_md5" = "$(md5sum "$a" | cut -d' ' -f1)" ] || fail INDEX "$a" "Packages MD5 mismatch"
    # the index BUILD_ID/filename suffix must agree (multi-instance)
    local fname=${rel##*/}; fname=${fname%.gpkg.tar}
    case $fname in
      *-[0-9]|*-[0-9]*)
        local suffix=${fname##*-}
        [ -z "$f_build_id" ] || [ "$suffix" = "$f_build_id" ] || \
          fail INDEX "$a" "Packages BUILD_ID '$f_build_id' != filename suffix '$suffix'"
        ;;
    esac
    [ -n "$f_repo" ] || fail INDEX "$a" "Packages stanza has no REPO field"
    [ -n "$f_build_time" ] || fail INDEX "$a" "Packages stanza has no BUILD_TIME field"
  done < <(find "$pkgdir" -name '*.gpkg.tar' | LC_ALL=C sort)
}

# --- CLI ----------------------------------------------------------------
DIR=""; WANT_INDEX=0; args=()
while [ $# -gt 0 ]; do
  case $1 in
    --dir) DIR=${2:?--dir needs a PKGDIR}; shift 2 ;;
    --packages) WANT_INDEX=1; shift ;;
    -h|--help) sed -n '2,30p' "$0"; exit 0 ;;
    *) args+=("$1"); shift ;;
  esac
done

if [ -n "$DIR" ]; then
  [ -d "$DIR" ] || { echo "$ME: no such directory: $DIR" >&2; exit 2; }
  check_dir "$(realpath "$DIR")" "$WANT_INDEX"
elif [ ${#args[@]} -gt 0 ]; then
  for a in "${args[@]}"; do check_archive "$a"; done
else
  echo "usage: $ME <archive.gpkg.tar>... | --dir <PKGDIR> [--packages]" >&2
  exit 2
fi

if [ "$FINDINGS" -gt 0 ]; then
  echo "$ME: $ARCHIVES archives checked, $FINDINGS finding(s)" >&2
  exit 1
fi
exit 0
