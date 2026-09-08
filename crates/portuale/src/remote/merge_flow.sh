OLD_PF=""; OLD_COUNTER=-1
if [ "$STATELESS" = 1 ]; then :; else
for d in "$VDBROOT"/"$PKG"-*/; do

  [ -d "$d" ] || continue
  cpf=${d%/}; cpf=${cpf##*/}
  [ "$cpf" = "$NEWPF" ] && continue
  rest=${cpf#"$PKG"-}
  case "$rest" in [0-9]*) ;; *) continue;; esac
  slot=$(cat "$d/SLOT" 2>/dev/null | cut -d/ -f1)
  [ "$slot" = "$MAINS" ] || continue
  c=$(cat "$d/COUNTER" 2>/dev/null | tr -d ' \t\n'); case "$c" in ''|*[!0-9]*) c=-1;; esac
  if [ "$c" -gt "$OLD_COUNTER" ]; then OLD_COUNTER=$c; OLD_PF=$cpf; fi
done
fi
if [ -n "$OLD_PF" ]; then OLDVDB="$VDBROOT/$OLD_PF"; else OLDVDB=""; fi
if [ "$STATELESS" = 1 ]; then
  echo "MERGE_PRERM=skip:stateless-no-vdb"
elif [ -n "$OLD_PF" ]; then
  if run_old_hook "$OLDVDB" prerm; then
    echo "MERGE_PRERM=ok $OLD_PF"
  else
    rc=$?
    if [ "$rc" = 2 ]; then echo "MERGE_PRERM=skip $OLD_PF"; else echo "MERGE_PRERM=warn $OLD_PF"; fi
  fi
else
  echo "MERGE_PRERM=skip:none-installed"
fi
: > "$REPLACED_OWN"; : > "$OTHERS_OWN"
if [ "$STATELESS" != 1 ]; then
for c in "$VDB"/*/*/CONTENTS; do
  [ -f "$c" ] || continue
  pfdir=${c%/*}; pfdir=${pfdir##*/}
  awk '$1=="obj"||$1=="sym"{print $2}' "$c" > "$UNIT/scan.list"
  if [ "$pfdir" = "$NEWPF" ] || { [ -n "$OLD_PF" ] && [ "$pfdir" = "$OLD_PF" ]; }; then
    cat "$UNIT/scan.list" >> "$REPLACED_OWN"
  else
    cat "$UNIT/scan.list" >> "$OTHERS_OWN"
  fi
done
rm -f "$UNIT/scan.list"
fi
: > "$NEWCONTENTS"; : > "$NEWPATHS"
ROOTUID=$(id -u)
while read -r line; do
  [ -n "$line" ] || continue
  set -f; set -- $line; set +f; kind=$1
  case "$kind" in
    dir) rel=$4 ;;
    obj) rel=$4; md5=$2; mtime=$3 ;;
    sym) rel=$4; mtime=$3; target=$5 ;;
    *) mfail copy "unknown filemeta kind $kind" ;;
  esac
  src="$IMAGE/$rel"; dest="$ROOT/$rel"; apath="/$rel"
  if [ -e "$dest" ] || [ -L "$dest" ]; then
    if [ "$STATELESS" = 1 ]; then
      # No ownership proof without a client vdb: the shape rules still
      # apply, but any existing file/symlink destination outside
      # CONFIG_PROTECT divert is a fail-closed collision (merging
      # directories and the divert itself need no ownership).
      if [ -d "$dest" ] && [ ! -L "$dest" ]; then
        [ "$kind" = dir ] || mfail collision "file over directory at $apath (stateless)";
      elif [ "$kind" = dir ]; then
        mfail collision "directory over file at $apath (stateless)";
      elif [ "$(is_protected "$dest")" = yes ]; then :;
      else mfail collision "stateless refusing to overwrite $apath"; fi
    elif grep -Fxq "$apath" "$REPLACED_OWN"; then :;
    elif grep -Fxq "$apath" "$OTHERS_OWN"; then mfail collision "$apath owned by another package";
    elif [ -d "$dest" ] && [ ! -L "$dest" ]; then
      [ "$kind" = dir ] || mfail collision "file over directory at $apath";
    elif [ "$kind" = dir ]; then
      mfail collision "directory over file at $apath";
    else :; fi
  fi
  case "$kind" in
    dir)
      if [ ! -d "$dest" ]; then
        mkdir -p "$dest" || mfail copy "mkdir $apath"
        [ "$ROOTUID" = 0 ] && chown --reference "$src" "$dest" || true
        chmod --reference "$src" "$dest" || mfail copy "chmod $apath"
      fi
      echo "dir $apath" >> "$NEWCONTENTS"
      ;;
    obj)
      if [ "$(is_protected "$dest")" = yes ] && { [ -f "$dest" ] || [ -L "$dest" ]; }; then
        if cmp -s "$src" "$dest"; then :;
        else dest=$(alloc_cfg "$dest"); fi
      fi
      mkdir -p "${dest%/*}" || mfail copy "mkdir parent of $apath"
      # A symlink at dest would make `mv` treat a symlink-to-directory as
      # a destination directory (and `cp` follow it): drop the link
      # first, exactly like real movefile's `os.unlink(dest)` for a
      # symlink destination.
      [ -L "$dest" ] && rm -f "$dest"
      # Real movefile / local replace_file_atomic (portuale #96/#97):
      # materialize the new file at a temporary sibling and rename(2) it
      # over the destination. Never `cp` into the live inode -- ETXTBSY
      # protects a running executable, but an mmap'd shared library
      # opens fine, and rewriting it kills every process mapping it.
      tmp="${dest%/*}/.${dest##*/}._portage_merge_.$$"
      rm -f "$tmp"
      cp -p "$src" "$tmp" || { rm -f "$tmp"; mfail copy "$apath"; }
      [ "$ROOTUID" = 0 ] && chown --reference "$src" "$tmp" || true
      chmod --reference "$src" "$tmp" || { rm -f "$tmp"; mfail copy "chmod $apath"; }
      mv -f "$tmp" "$dest" || { rm -f "$tmp"; mfail copy "$apath"; }
      echo "obj $apath $md5 $mtime" >> "$NEWCONTENTS"
      ;;
    sym)
      if [ "$(is_protected "$dest")" = yes ] && { [ -f "$dest" ] || [ -L "$dest" ]; }; then
        cur=""; [ -L "$dest" ] && cur=$(readlink "$dest")
        [ "$cur" = "$target" ] || dest=$(alloc_cfg "$dest")
      fi
      mkdir -p "${dest%/*}" || mfail copy "mkdir parent of $apath"
      # Same atomic replacement for the link: a symlink destination is
      # unlinked first (so `mv` cannot resolve it as a directory), then
      # the new link is built at a temporary sibling and renamed over
      # the destination -- never `rm` then `ln` on the live path.
      [ -L "$dest" ] && rm -f "$dest"
      tmp="${dest%/*}/.${dest##*/}._portage_merge_.$$"
      rm -f "$tmp"
      ln -s "$target" "$tmp" || { rm -f "$tmp"; mfail copy "symlink $apath"; }
      touch -h -r "$src" "$tmp" 2>/dev/null || true
      if [ "$ROOTUID" = 0 ]; then chown -h --reference "$src" "$tmp" 2>/dev/null || true; fi
      mv -f "$tmp" "$dest" || { rm -f "$tmp"; mfail copy "$apath"; }
      echo "sym $apath -> $target $mtime" >> "$NEWCONTENTS"
      ;;
  esac
  echo "$apath" >> "$NEWPATHS"
done < "$UNIT/filemeta"
echo "MERGE_COPY=ok"
# Stateless clients keep no vdb at all (plan §7: the server owns
# installed-db state) -- files merge, but no entry, COUNTER, or
# edb-counter is recorded.
if [ "$STATELESS" = 1 ]; then
  echo "MERGE_VDB=skip:stateless-no-vdb"
else
rm -rf "$TMPVDB"; mkdir -p "$TMPVDB" || mfail vdb "mkdir tmp"
for f in "$UNIT/build-info"/*; do
  # NOTE: `cond && action || fail` misfires when cond is false -- always
  # an explicit if for fallible steps.
  if [ -f "$f" ]; then cp "$f" "$TMPVDB"/ || mfail vdb "copy build-info"; fi
done
# Backlog #171: no plain `environment` in the vdb (real writes
# `environment.bz2` only). The build-time `environment.bz2` rides the
# verbatim `build-info/*` copies above as the fallback until the
# postinst regen phase overwrites it; future unmerge hooks source that
# file (server-decompressed at ship time, never a plain copy).
printf '%s\n' "$CAT" > "$TMPVDB/CATEGORY"
printf '%s\n' "$FULLSLOT" > "$TMPVDB/SLOT"
printf '%s\n' "$REPO" > "$TMPVDB/repository"
cp "$NEWCONTENTS" "$TMPVDB/CONTENTS" || mfail vdb "write CONTENTS"
max_c=-1
for f in "$VDBROOT"/*/COUNTER "$ROOT/var/cache/edb/counter"; do
  [ -f "$f" ] || continue
  c=$(cat "$f" 2>/dev/null | tr -d ' \t\n'); case "$c" in ''|*[!0-9]*) continue;; esac
  [ "$c" -gt "$max_c" ] && max_c=$c
done
NEXT=$((max_c + 1))
printf '%s' "$NEXT" > "$TMPVDB/COUNTER"
mkdir -p "$ROOT/var/cache/edb" && printf '%s' "$NEXT" > "$ROOT/var/cache/edb/counter"
tmpmeta="$TMPVDB/metadata.tmp"
: > "$tmpmeta"
for f in BDEPEND BUILD_ID BUILD_TIME CHOST COUNTER DEFINED_PHASES DEPEND DESCRIPTION EAPI HOMEPAGE IDEPEND IUSE KEYWORDS LICENSE PDEPEND PROPERTIES PROVIDES RDEPEND REQUIRES RESTRICT SLOT USE repository; do
  [ -f "$TMPVDB/$f" ] || continue
  printf '%s=%s\n' "$f" "$(tr -s '[:space:]' ' ' < "$TMPVDB/$f" | sed -e 's/^ *//' -e 's/ *$//')" >> "$tmpmeta"
done
if [ -s "$tmpmeta" ]; then
  { echo "#format=1"; LC_ALL=C sort "$tmpmeta"; } > "$TMPVDB/metadata"
  printf '#dir_mtime=%s\n' "$(stat -c %Y "$TMPVDB")" >> "$TMPVDB/metadata"
  rm -f "$tmpmeta"
fi
rm -rf "$NEWVDB"
mv "$TMPVDB" "$NEWVDB" || mfail vdb "rename into place"
echo "MERGE_VDB=ok"
fi
if [ -n "$OLD_PF" ]; then
  if [ -f "$OLDVDB/CONTENTS" ]; then
    : > "$UNIT/olddirs.list"
    while read -r line; do
      [ -n "$line" ] || continue
      set -f; set -- $line; set +f; kind=$1
      case "$kind" in
        obj|sym)
          apath=$2; recorded=${line##* }
          grep -Fxq "$apath" "$NEWPATHS" && continue
          if [ ! -e "$ROOT$apath" ] && [ ! -L "$ROOT$apath" ]; then continue; fi
          if [ "$(stat -c %Y "$ROOT$apath" 2>/dev/null)" = "$recorded" ]; then
            rm -f "$ROOT$apath" || warn remove "cannot remove $apath"
          else
            warn remove "modified-kept $apath"
          fi
          ;;
        dir) echo "$2" >> "$UNIT/olddirs.list" ;;
        *) warn remove "unmerge-skip $line" ;;
      esac
    done < "$OLDVDB/CONTENTS"
    sort -r "$UNIT/olddirs.list" | while read -r d; do
      [ -n "$d" ] && rmdir "$ROOT$d" 2>/dev/null || true
    done
    rm -f "$UNIT/olddirs.list"
  else
    warn remove "no CONTENTS in $OLD_PF, files kept"
  fi
  echo "MERGE_REMOVE=ok $OLD_PF"
elif [ "$STATELESS" = 1 ]; then
  echo "MERGE_REMOVE=skip:stateless-no-vdb"
else
  echo "MERGE_REMOVE=skip:none-installed"
fi
if [ "$STATELESS" = 1 ]; then
  echo "MERGE_POSTRM=skip:stateless-no-vdb"
elif [ -n "$OLD_PF" ]; then
  if run_old_hook "$OLDVDB" postrm; then echo "MERGE_POSTRM=ok $OLD_PF"; else
    rc=$?
    if [ "$rc" = 2 ]; then echo "MERGE_POSTRM=skip $OLD_PF"; else echo "MERGE_POSTRM=warn $OLD_PF"; fi
  fi
  rm -rf "$OLDVDB"
else
  echo "MERGE_POSTRM=skip:none-installed"
fi
if command -v ldconfig >/dev/null 2>&1; then
  if ldconfig -r "$ROOT" 2>/dev/null; then echo "MERGE_ENVUPDATE=ok ldconfig"; else echo "MERGE_ENVUPDATE=skip:ldconfig-failed"; fi
else
  echo "MERGE_ENVUPDATE=skip:no-ldconfig"
fi
if [ -n "$LEDGER_FILE" ]; then
  mkdir -p "${LEDGER_FILE%/*}" 2>/dev/null || warn ledger "mkdir for $LEDGER_FILE"
  if printf '%s\n' "$LEDGER_LINE" >> "$LEDGER_FILE" 2>/dev/null; then
    tail -n 10 "$LEDGER_FILE" > "$LEDGER_FILE.tmp" 2>/dev/null && mv "$LEDGER_FILE.tmp" "$LEDGER_FILE" 2>/dev/null || warn ledger "rotate $LEDGER_FILE"
    echo "MERGE_LEDGER=ok"
  else
    warn ledger "append $LEDGER_FILE"
  fi
else
  echo "MERGE_LEDGER=skip:none-requested"
fi
echo "MERGE_DONE=ok"
echo "STATUS=merged"
