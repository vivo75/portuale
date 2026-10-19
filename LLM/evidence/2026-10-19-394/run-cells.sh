#!/bin/bash
# #394 oracle runner (host). Fresh stage per cell (global updates mutate
# the staged vdb), Portage and portuale each run against their own fresh
# stage with identical env. No PORTAGE_REPOSITORIES inlined: each config
# reads its own file (391 README).
#
# Usage: run-cells.sh <portuale-bin>   (out/ records argv + outputs)
set -u
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
BIN=${1:?portuale emerge binary}
OUT=$HERE/out
SRC=/home/vivo/repo/PORTUALE/pmtest/fixtures
mkdir -p "$OUT"
make_local_bak=$HERE/make.local.bak
cp /etc/make.local "$make_local_bak"

base_env() {
  # strip any inherited make.conf-style config vars (pmtest conftest list)
  unset USE ACCEPT_KEYWORDS USE_EXPAND USE_EXPAND_UNPREFIXED \
    USE_EXPAND_IMPLICIT USE_EXPAND_HIDDEN IUSE_IMPLICIT \
    ACCEPT_LICENSE ACCEPT_PROPERTIES ACCEPT_RESTRICT \
    PKGDIR PORTAGE_LOGDIR PORTAGE_BINHOST PORTAGE_NICENESS \
    PORTAGE_IONICE_COMMAND PORTAGE_SCHEDULING_POLICY \
    PORTAGE_SCHEDULING_PRIORITY PORTAGE_ELOG_SYSTEM PORTAGE_ELOG_CLASSES \
    PORTAGE_ELOG_MAILURI FEATURES CHOST CBUILD CTARGET \
    CFLAGS CXXFLAGS CPPFLAGS LDFLAGS FFLAGS FCFLAGS \
    MAKEOPTS EMERGE_DEFAULT_OPTS PORTAGE_RSYNC_EXTRA_OPTS \
    GENTOO_MIRRORS VIDEO_CARDS PYTHON_TARGETS PYTHON_SINGLE_TARGET \
    LINGUAS L10N CPU_FLAGS_X86 ELIBC KERNEL USERLAND ABI_X86 \
    BINPKG_FORMAT BINPKG_COMPRESS BINPKG_COMPRESS_FLAGS \
    PORTAGE_BZIP2_COMMAND PORTAGE_BUNZIP2_COMMAND \
    CONFIG_PROTECT CONFIG_PROTECT_MASK ENV_UNSET ESYSROOT \
    PORTAGE_REPOSITORIES PORTAGE_CONFIGROOT ROOT PORTAGE_RUNNING_ROOT \
    DISTDIR PORTDIR PORTAGE_BINHOST 2>/dev/null || true
}

trim() { printf '%s' "$1" | sed -e 's/^[[:space:]]*//' -e 's/^[[:space:]]*$//' -e 's/[[:space:]]*$//'; }

while IFS='|' read -r raw_name raw_args; do
  name=$(trim "$raw_name"); [ -z "$name" ] && continue
  case "$name" in \#*) continue;; esac
  args=$(trim "$raw_args")
  # fresh stage per cell, per PM
  bash "$HERE/stage-host.sh" "$SRC" /var/tmp/pt-394/st-portage "$HERE/scratch" > /dev/null
  bash "$HERE/stage-host.sh" "$SRC" /var/tmp/pt-394/st-portuale "$HERE/scratch" > /dev/null
  FX_P=/var/tmp/pt-394/st-portage/fixtures
  FX_R=/var/tmp/pt-394/st-portuale/fixtures
  # host make.local would leak EMERGE_DEFAULT_OPTS into Portage; empty it
  sudo truncate -s 0 /etc/make.local
  {
    echo "### emerge $args --color=n"
    echo "### binary: /usr/sbin/emerge ($(/usr/sbin/emerge --version 2>&1 | head -1))"
    # shellcheck disable=SC2086
    ( base_env
      export PYTHONHASHSEED=0 LC_ALL=C.UTF-8 TZ=UTC CLEAN_DELAY=0
      export PORTAGE_CONFIGROOT=$FX_P ROOT=$FX_P DISTDIR=$FX_P/distfiles SYSROOT=$FX_P
      timeout 300 /usr/sbin/emerge $args --color=n 2>&1 )
    echo "rc=$?"
  } > "$OUT/portage-$name.txt" 2>&1
  sudo cp "$make_local_bak" /etc/make.local
  {
    echo "### emerge $args"
    echo "### binary md5: $(md5sum "$BIN" | cut -d' ' -f1)"
    # shellcheck disable=SC2086
    ( base_env
      export HOME=/var/tmp/pt-394 PYTHONHASHSEED=0 LC_ALL=C.UTF-8 TZ=UTC CLEAN_DELAY=0
      export PORTAGE_CONFIGROOT=$FX_R ROOT=$FX_R PORTAGE_RUNNING_ROOT=$FX_R DISTDIR=$FX_R/distfiles ESYSROOT=$FX_R
      timeout 300 "$BIN" $args 2>&1 )
    echo "rc=$?"
  } > "$OUT/portuale-$name.txt" 2>&1
done < "$HERE/cells.txt"

sudo cp "$make_local_bak" /etc/make.local
echo "make.local md5: $(md5sum /etc/make.local | cut -d' ' -f1) (expect 3f29e0a60206a91b570347f8d737fbd2)"
echo "done $(ls "$OUT"/*.txt | wc -l) files"
