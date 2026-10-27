#!/bin/bash
# s0e-pair.sh <name> <root> <cfg> <args...>
# Runs portuale (PORTUALE_BT_TRACE=1) and host Portage on a SCRATCH root with the
# suite's argv; writes <name>.{portuale,portage}.{out,err,rc,argv} to $OUT and prints a summary line.
# <root> is a scratch copy of pmtest/fixtures (ROOT and DISTDIR); <cfg> is its mkcfg.sh config root
# (absolute repos.conf for Portage). portuale uses <root> as its config root like the suite does.
set -u
name=$1; root=$2; cfg=$3; shift 3
OUT=${OUT:?}; BIN=/home/vivo/repo/PORTUALE/portuale/target/release/portuale
case "$root" in /var/tmp/claude-s0e/*) ;; *) echo "REFUSED root $root" >&2; exit 9;; esac
PCFG=${PCFG:-$root}   # portuale config root (the suite sets it to fixtures when ROOT differs)
TMPD=/var/tmp/claude-s0e/ptmp; mkdir -p "$TMPD"
printf 'portuale: PORTAGE_CONFIGROOT=%s ROOT=%s PORTAGE_RUNNING_ROOT=%s PORTUALE_BT_TRACE=1 portuale emerge %s\nportage: PORTAGE_CONFIGROOT=%s ROOT=%s PORTAGE_RUNNING_ROOT=%s PYTHONHASHSEED=0 emerge %s\n' \
  "$PCFG" "$root" "$root" "$*" "$cfg" "$root" "$root" "$*" > "$OUT/$name.argv"
env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=$PCFG ROOT=$root PORTAGE_RUNNING_ROOT=$root DISTDIR=$PCFG/distfiles PORTAGE_TMPDIR=$TMPD PORTUALE_BT_TRACE=1 \
  "$BIN" emerge "$@" > "$OUT/$name.portuale.out" 2> "$OUT/$name.portuale.err"; echo $? > "$OUT/$name.portuale.rc"
env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=$cfg ROOT=$root PORTAGE_RUNNING_ROOT=$root DISTDIR=$PCFG/distfiles PORTAGE_TMPDIR=$TMPD PYTHONHASHSEED=0 \
  emerge "$@" > "$OUT/$name.portage.out" 2> "$OUT/$name.portage.err"; echo $? > "$OUT/$name.portage.rc"
rows() { grep -E '^\[(ebuild|binary)' "$1" | sed -E "s# to $root/? *#  #; s# +[0-9.]+ [KMG]i?B *\$##" ; }
bt() { grep -ho 'backtrack: [0-9]*/[0-9]*' "$@" | head -1; }
heal=$(grep -c 'kind=Feedback:MissingDep' "$OUT/$name.portuale.err")
same=n; [ "$(rows "$OUT/$name.portuale.out")" = "$(rows "$OUT/$name.portage.out")" ] && same=y
echo "$name | portuale rc $(cat "$OUT/$name.portuale.rc") $(bt "$OUT/$name.portuale.out" "$OUT/$name.portuale.err") passes=$(grep -c '^BT try=' "$OUT/$name.portuale.err") missingdep-passes=$heal | portage rc $(cat "$OUT/$name.portage.rc") $(bt "$OUT/$name.portage.out" "$OUT/$name.portage.err") | rows-equal $same"
