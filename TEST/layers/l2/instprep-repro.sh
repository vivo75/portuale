#!/bin/bash
# #38 S4 -- in-container repro for the merge-time `instprep` phase.
#
# Real runs `__dyn_instprep` from `dblink.treewalk()` on every merge
# (`vartree.py:4440-4450`); it strips/compresses the image iff the
# `binpkg-dostrip`/`binpkg-docompress` token is ABSENT. With both tokens
# removed, `install_qa_check` skips the transforms, so the only place they
# can happen is instprep.
#
# Usage (from TEST/run/l2-instprep-repro.sh):
#   podman run ... IMAGE /TEST/layers/l2/instprep-repro.sh <portage|portuale> <outdir>
#
# Two cells, one fresh container per PM:
#   src  -- `emerge -1 porttest/{docs,setuid}` (source merge)
#   bin  -- `emerge -B` the same atoms (archives carry unstripped,
#           uncompressed images), unmerge, then `emerge -1K` them

set -u
PM=${1:?portage|portuale}
OUT=${2:?output dir}
case $PM in
  portage)  EM=/usr/sbin/emerge ;;
  portuale) EM=/usr/local/bin/emerge ;;
  *) echo "PM must be portage or portuale" >&2; exit 2 ;;
esac
ATOMS=(porttest/docs porttest/setuid)

export PORTAGE_CONFIGROOT=/ ROOT=/
export LC_ALL=C.UTF-8 TZ=UTC
export EMERGE_DEFAULT_OPTS=""
export PKGDIR=/var/cache/binpkgs-instprep
export FEATURES="-binpkg-dostrip -binpkg-docompress -buildpkg -cgroup -ccache -distcc -sign xattr filecaps"
export BINPKG_FORMAT="gpkg"
umask 022
mkdir -p "$OUT" "$PKGDIR"

log() { printf '[instprep-repro %s] %s\n' "$PM" "$*"; }

rm -rf /var/db/repos/porttest
cp -a /porttest-overlay /var/db/repos/porttest
cat > /etc/portage/repos.conf/porttest.conf <<-EOF
	[porttest]
	location = /var/db/repos/porttest
	masters = gentoo
	auto-sync = no
	EOF

# same base on both sides (real's behaviour is the pin's)
if [ "${L2_SKIP_PORTAGE_UPGRADE:-0}" != 1 ]; then
  PIN=${L2_PORTAGE_PIN:-3.0.82.2}
  cur=$(/usr/sbin/emerge --version 2>/dev/null | sed -n 's/^Portage \([0-9.]*\).*/\1/p')
  if [ "$cur" != "$PIN" ]; then
    log "upgrading portage $cur -> $PIN"
    FEATURES="-cgroup" ACCEPT_KEYWORDS="~amd64" /usr/sbin/emerge -q --usepkg=n "=sys-apps/portage-$PIN" \
      || { log "!!! portage upgrade failed"; exit 1; }
  fi
fi

snapshot() {  # <cell>
  local f=$OUT/$1.txt
  {
    echo "## docs"
    (cd /usr/share && find doc/docs-1.0 man/man1/pt.1* info/pt.info* -print 2>/dev/null | LC_ALL=C sort)
    echo "## setuid"
    for b in /usr/bin/pt-setuid /usr/bin/pt-setgid /usr/bin/pt-sticky; do
      printf '%s %s ' "$b" "$(stat -c %s "$b" 2>/dev/null)"
      file -b "$b" 2>/dev/null | grep -o 'not stripped\|stripped' || echo missing
    done
    echo "## debug"
    find /usr/lib/debug -path '*pt-*' -print 2>/dev/null | LC_ALL=C sort
    echo "## CONTENTS"
    # All three setuid binaries are byte-identical and share one
    # build-id, so which name `estrip`'s `___parallel` traversal links
    # the shared `.build-id/e0/2277…` lines to is nondeterministic in
    # real too (the #38 S0/S2 race): match every `pt-*` target and drop
    # the target text (`awk` keeps `$1 $2`), so the link *paths* still
    # compare.
    for p in docs setuid; do
      grep -h -E 'BIG\.txt|pt-setuid|pt-setgid|pt-sticky' \
        /var/db/pkg/porttest/$p-1.0/CONTENTS 2>/dev/null \
        | awk '{print $1, $2}'
    done | LC_ALL=C sort
  } > "$f"
  cat "$f"
}

log "cell src: emerge -1 ${ATOMS[*]}"
"$EM" --oneshot --color=n "${ATOMS[@]}" > "$OUT/src.log" 2>&1 || { log "!!! src merge failed"; tail -30 "$OUT/src.log"; }
snapshot src

log "cell bin: emerge -B, unmerge, emerge -1K"
# No builddir wipe: real `_emerge/EbuildBuild._start_pre_clean` runs the
# `clean` phase before every build, and portuale does the same since
# backlog #42 (and post-cleans after a successful merge), so the -B build
# starts from the state the pre-clean leaves in either PM. Before #42
# this cell needed `rm -rf /var/tmp/portage/porttest` to emulate it.
"$EM" --buildpkgonly --oneshot --color=n "${ATOMS[@]}" > "$OUT/bin-build.log" 2>&1 \
  || { log "!!! -B failed"; tail -30 "$OUT/bin-build.log"; }
/usr/sbin/emerge --unmerge --color=n "${ATOMS[@]}" > "$OUT/bin-unmerge.log" 2>&1
rm -rf /usr/lib/debug/usr/bin/pt-* /usr/share/doc/docs-1.0
"$EM" --oneshot --usepkgonly --color=n "${ATOMS[@]}" > "$OUT/bin-merge.log" 2>&1 \
  || { log "!!! -K merge failed"; tail -30 "$OUT/bin-merge.log"; }
echo "## archive image (setuid)" > "$OUT/bin-archive.txt"
a=$(find "$PKGDIR" -name 'setuid-*.gpkg.tar' | head -1)
[ -n "$a" ] && tar -xOf "$a" "$(tar -tf "$a" | grep image.tar)" | zstd -dc 2>/dev/null | tar -tv 2>/dev/null \
  | grep pt-setuid >> "$OUT/bin-archive.txt"
cat "$OUT/bin-archive.txt"
snapshot bin
