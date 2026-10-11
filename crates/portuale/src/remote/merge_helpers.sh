mfail() { echo "MERGE_FAIL=$1 $2"; echo "STATUS=failed:$1"; exit 1; }
warn() { echo "MERGE_WARN=$1 $2"; }
env_val() {
  sed -n "s/^declare -x $2=\"\\(.*\\)\"$/\\1/p" "$1" | head -n 1
}
is_protected() {
  dest=$1; best_p=0; best_m=0
  for e in $PROTECT; do
    pp="$ROOT/${e#/}"
    if [ -d "$pp" ]; then
      case "$dest" in "$pp"|"$pp"/*)
        len=${#pp}; [ "$len" -gt "$best_p" ] && best_p=$len ;;
      esac
    else
      [ "$dest" = "$pp" ] && { len=${#pp}; [ "$len" -gt "$best_p" ] && best_p=$len; }
    fi
  done
  for e in $MASK; do
    pp="$ROOT/${e#/}"
    if [ -d "$pp" ]; then
      case "$dest" in "$pp"|"$pp"/*)
        len=${#pp}; [ "$len" -gt "$best_m" ] && best_m=$len ;;
      esac
    else
      [ "$dest" = "$pp" ] && { len=${#pp}; [ "$len" -gt "$best_m" ] && best_m=$len; }
    fi
  done
  [ "$best_p" -gt 0 ] && [ "$best_p" -gt "$best_m" ] && echo yes || echo no
}
alloc_cfg() {
  dir=${1%/*}; base=${1##*/}; n=0
  while [ -e "$dir/._cfg$(printf '%04d' $n)_$base" ]; do n=$((n + 1)); done
  echo "$dir/._cfg$(printf '%04d' $n)_$base"
}
run_old_hook() {
  vdbdir=$1; phase=$2
  set -- "$vdbdir"/*.ebuild
  [ -f "$1" ] || { echo "OLDHOOK=missing-ebuild"; return 2; }
  ebuild=$1
  OTMP="$WORKDIR/oldtmp"
  rm -rf "$OTMP"
  mkdir -p "$OTMP/temp" "$OTMP/work" "$OTMP/homedir" "$OTMP/files" "$OTMP/empty" "$OTMP/image" || return 1
  # Backlog #171: prefer the server-decompressed env the server staged
  # at `$UNIT/old-env/<pf>` (no client bzip2 needed); then the legacy
  # plain `environment` older `mrg` versions left, used only when no
  # `environment.bz2` exists; client `bzip2 -dc` only as a last resort;
  # then any plain `environment` at all (an older-`mrg` entry with both
  # files whose staging failed, on a bzip2-less client).
  pf=${vdbdir##*/}
  if [ -f "$UNIT/old-env/$pf" ]; then
    cp "$UNIT/old-env/$pf" "$OTMP/temp/environment" || return 1
  elif [ ! -f "$vdbdir/environment.bz2" ] && [ -f "$vdbdir/environment" ]; then
    cp "$vdbdir/environment" "$OTMP/temp/environment" || return 1
  elif [ -f "$vdbdir/environment.bz2" ] && command -v bzip2 >/dev/null 2>&1; then
    bzip2 -dc -- "$vdbdir/environment.bz2" > "$OTMP/temp/environment" || return 1
  elif [ -f "$vdbdir/environment" ]; then
    cp "$vdbdir/environment" "$OTMP/temp/environment" || return 1
  else
    echo "OLDHOOK=no-env-bzip2-missing"; return 2
  fi
  : > "$OTMP/temp/environment.raw"
  EAPI=$(env_val "$OTMP/temp/environment" EAPI)
  [ -n "$EAPI" ] || { echo "OLDHOOK=no-eapi"; return 2; }
  export EAPI
  for v in CATEGORY PN PV PR PVR P PF; do
    val=$(env_val "$OTMP/temp/environment" "$v")
    [ -n "$val" ] || { echo "OLDHOOK=no-$v"; return 2; }
    export "$v=$val"
  done
  export EBUILD="$ebuild" O="$vdbdir"
  export ROOT="$ROOT" EROOT="$ROOT"
  export PORTAGE_BUILDDIR="$OTMP"
  export WORKDIR="$OTMP/work"
  export D="$OTMP/image/" ED="$OTMP/image/"
  export T="$OTMP/temp" HOME="$OTMP/homedir" FILESDIR="$OTMP/files"
  export PORTAGE_BIN_PATH="$UNITBIN"
  export PORTAGE_ECLASS_LOCATIONS=""
  export PORTAGE_PYTHON="$UNITBIN/portuale-python"
  export PORTAGE_PYM_PATH=/
  unset PORTAGE_IPC_DAEMON
  export PORTAGE_COLORMAP="$COLORMAP"
  export PORTAGE_TMPDIR="$WORKDIR"
  export SANDBOX_LOG="$OTMP/temp/sandbox.log"
  export EBUILD_PHASE="$phase" EMERGE_FROM=binary
  export PATH="$UNITBIN/ebuild-helpers:$PATH"
  bash "$UNITBIN/ebuild.sh" "$phase"
  rc=$?
  echo "OLDHOOK_$phase=$rc"
  return $rc
}
