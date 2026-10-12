#!/bin/bash
# S0 capture driver (runs INSIDE localhost/test-portuale:latest).
# Usage: PM=portage|portuale bash /s0/s0-capture.sh
# Env in: /s0 (fixtures+scripts, ro except /s0/out), /pm (portuale bindir, ro)
# Out: /s0/out/<pm>/<cell>.{argv,rc,stdout,stderr}
set -u
PM=${PM:-portage}
OUT=/s0/out/$PM
S0W=/s0/out/work
mkdir -p "$OUT" "$S0W"

if [ "$PM" = portuale ]; then
  EMERGE=(/pm/portuale emerge)
else
  EMERGE=(emerge)
fi
BASE_OPTS=(--ignore-default-opts --getbinpkg=n --usepkg=n)
GPROF=/var/db/repos/gentoo/profiles/default/linux/amd64/23.0/systemd

run() { # run <cell> <cfgdir> [--root R] -- <args...>
  local cell=$1 cfg=$2; shift 2
  local root=$cfg
  if [ "${1:-}" = --root ]; then root=$2; shift 2; fi
  [ "${1:-}" = -- ] && shift
  printf '%s\n' "${EMERGE[*]} $*" > "$OUT/$cell.argv"
  PORTAGE_CONFIGROOT=$cfg ROOT=$root "${EMERGE[@]}" "$@" \
    >"$OUT/$cell.stdout" 2>"$OUT/$cell.stderr"
  printf '%s\n' "$?" > "$OUT/$cell.rc"
}

mkcfg() { # mkcfg <name> <t341repo|NONE> <profile-target>
  local name=$1 repo=$2 prof=$3
  local cfg=$S0W/cfg-$name
  rm -rf "$cfg"; mkdir -p "$cfg/etc/portage"
  {
    echo "[DEFAULT]"
    echo "main-repo = gentoo"
    echo
    echo "[gentoo]"
    echo "location = /var/db/repos/gentoo"
    if [ "$repo" != NONE ]; then
      echo
      echo "[t341]"
      echo "location = $repo"
    fi
  } > "$cfg/etc/portage/repos.conf"
  echo 'PKGDIR="/var/tmp/s0/pkgdir"' > "$cfg/etc/portage/make.conf"
  ln -sfn "$prof" "$cfg/etc/portage/make.profile"
  printf '%s\n' "$cfg"
}

T341=/s0/repo-341
T341F=/s0/repo-341-formats
U8='dev-libs/t341-dep::t341
=dev-libs/t341-dep-1-2
*/t341-dep
dev-libs/t341-dep:=
dev-libs/t341-dep:0
dev-libs/t341-dep[x(+)]
dev-libs/t341-dep-1
dev-libs/t341-dep'

if [ "$PM" = portage ]; then
  # ---- scratch repo variants (container-local, not fixtures) ----
  rm -rf $S0W/repo-nokey $S0W/repo-eapi10
  cp -a $T341F $S0W/repo-nokey
  printf 'masters =\nthin-manifests = true\nprofile-formats = portage-2 profile-repo-deps build-id\nprofile_eapi_when_unspecified = 5\n' > $S0W/repo-nokey/metadata/layout.conf
  cp -a $T341 $S0W/repo-eapi10
  echo 10 > $S0W/repo-eapi10/profiles/t341/noeapi/eapi
fi

# ============ A. ebuild cells ============
A=$(mkcfg cfgA $T341 "$GPROF")
for p in badrepo badbid badcond badslot built token twokeys good eapi7 mixcat twobuilt emptygrp glued xor qq condfirst; do
  run "A-ebuild-t341-$p" "$A" -- -pv "${BASE_OPTS[@]}" "app-misc/t341-$p"
done
run "A-ebuild-t341-dep" "$A" -- -pv "${BASE_OPTS[@]}" dev-libs/t341-dep

# ============ B. profile cells ============
B1=$(mkcfg cfgB1 $T341 "$T341/profiles/t341/noeapi")
run "B-prof-eapi5-noeapi" "$B1" -- -pv "${BASE_OPTS[@]}" dev-libs/t341-dep
B2=$(mkcfg cfgB2 $T341F "$T341F/profiles/t341/noeapi")
run "B-prof-formats" "$B2" -- -pv "${BASE_OPTS[@]}" dev-libs/t341-dep
if [ "$PM" = portage ]; then
  B3=$(mkcfg cfgB3 $S0W/repo-nokey "$S0W/repo-nokey/profiles/t341/noeapi")
  run "B-prof-nokey" "$B3" -- -pv "${BASE_OPTS[@]}" dev-libs/t341-dep
  B4=$(mkcfg cfgB4 $S0W/repo-eapi10 "$S0W/repo-eapi10/profiles/t341/noeapi")
  run "B-prof-eapi10" "$B4" -- -pv "${BASE_OPTS[@]}" dev-libs/t341-dep
else
  # Portuale run: reuse the scratch variants left by the portage run (shared /s0/out/work).
  B3=$(mkcfg cfgB3 $S0W/repo-nokey "$S0W/repo-nokey/profiles/t341/noeapi")
  run "B-prof-nokey" "$B3" -- -pv "${BASE_OPTS[@]}" dev-libs/t341-dep
  B4=$(mkcfg cfgB4 $S0W/repo-eapi10 "$S0W/repo-eapi10/profiles/t341/noeapi")
  run "B-prof-eapi10" "$B4" -- -pv "${BASE_OPTS[@]}" dev-libs/t341-dep
fi

# ============ C. user config ============
C=$(mkcfg cfgC $T341 "$GPROF")
printf '%s\n' "$U8" > "$C/etc/portage/package.mask"
printf '%s\n' "$U8" > "$C/etc/portage/package.unmask"
printf '%s\n' "$U8" > "$C/etc/portage/package.use"
printf '%s\n' "$U8" > "$C/etc/portage/package.accept_keywords"
printf '%s\n' "$U8" > "$C/etc/portage/package.license"
printf '%s\n' "$U8" > "$C/etc/portage/package.env"
printf '%s\n' "$U8" > "$C/etc/portage/package.properties"
mkdir -p "$C/etc/portage/profile"
printf '%s\n' "$U8" > "$C/etc/portage/profile/package.mask"
printf '%s\n' "$U8" > "$C/etc/portage/profile/package.use"
printf '%s\n' "$U8" > "$C/etc/portage/profile/packages"
run "C-userconfig" "$C" -- -pv "${BASE_OPTS[@]}" dev-libs/t341-dep
# crash cell: package.env with a use-dep + env file (S6 documents the cut; S0 records it)
Cx=$(mkcfg cfgCx $T341 "$GPROF")
printf 'dev-libs/t341-dep[x] t341.conf\n' > "$Cx/etc/portage/package.env"
printf 'dev-libs/t341-dep t341.conf\n' >> "$Cx/etc/portage/package.env"
run "C-userenv-crash" "$Cx" -- -pv "${BASE_OPTS[@]}" dev-libs/t341-dep

# ============ D. command line ============
D=$(mkcfg cfgD $T341 "$GPROF")
run "D-cmd-bid"      "$D" -- -pv "${BASE_OPTS[@]}" '=dev-libs/t341-dep-1-2'
run "D-cmd-wild"     "$D" -- -pv "${BASE_OPTS[@]}" '*/t341-dep'
run "D-cmd-blocker"  "$D" -- -pv "${BASE_OPTS[@]}" '!dev-libs/t341-dep'
run "D-cmd-ver"      "$D" -- -pv "${BASE_OPTS[@]}" 'dev-libs/t341-dep-1'
run "D-cmd-cond"     "$D" -- -pv "${BASE_OPTS[@]}" 'dev-libs/t341-dep[x?]'
run "D-cmd-shortrepo" "$D" -- -pv "${BASE_OPTS[@]}" 't341-dep::t341'
run "D-cmd-plain"    "$D" -- -pv "${BASE_OPTS[@]}" dev-libs/t341-dep
run "D-cmd-badbid0"  "$D" -- -pv "${BASE_OPTS[@]}" '=dev-libs/t341-dep-1-02'
# exclude family (each needs a target atom)
run "D-excl-op"      "$D" -- -pv "${BASE_OPTS[@]}" --exclude '>=dev-libs/t341-dep-1' dev-libs/t341-dep
run "D-excl-repo"    "$D" -- -pv "${BASE_OPTS[@]}" --exclude 'dev-libs/t341-dep::t341' dev-libs/t341-dep
run "D-excl-bid"     "$D" -- -pv "${BASE_OPTS[@]}" --exclude '=dev-libs/t341-dep-1-2' dev-libs/t341-dep
run "D-excl-use"     "$D" -- -pv "${BASE_OPTS[@]}" --exclude 'dev-libs/t341-dep[x]' dev-libs/t341-dep
run "D-excl-blocker" "$D" -- -pv "${BASE_OPTS[@]}" --exclude '!dev-libs/t341-dep' dev-libs/t341-dep
run "D-excl-two"     "$D" -- -pv "${BASE_OPTS[@]}" --exclude 'dev-libs/t341-dep::t341' --exclude '>=dev-libs/t341-dep-1' dev-libs/t341-dep
run "D-excl-wild-ok" "$D" -- -pv "${BASE_OPTS[@]}" --exclude 'dev-libs/t341-*' dev-libs/t341-dep
run "D-excl-short-ok" "$D" -- -pv "${BASE_OPTS[@]}" --exclude 't341-dep' dev-libs/t341-dep
run "D-excl-slot-ok" "$D" -- -pv "${BASE_OPTS[@]}" --exclude 'dev-libs/t341-dep:0/1' dev-libs/t341-dep
run "D-excl-allslot-ok" "$D" -- -pv "${BASE_OPTS[@]}" --exclude '*/*:0' dev-libs/t341-dep
run "D-usexpkg-excl-repo" "$D" -- -pv "${BASE_OPTS[@]}" --usepkg-exclude 'dev-libs/t341-dep::t341' dev-libs/t341-dep
run "D-bpkgexcl-bid-ok"   "$D" -- -pv "${BASE_OPTS[@]}" --buildpkg-exclude '=dev-libs/t341-dep-1-2' dev-libs/t341-dep
run "D-bpkgexcl-repo-ok"  "$D" -- -pv "${BASE_OPTS[@]}" --buildpkg-exclude 'dev-libs/t341-dep::t341' dev-libs/t341-dep
run "D-bpkgexcl-op-ok"    "$D" -- -pv "${BASE_OPTS[@]}" --buildpkg-exclude '>=dev-libs/t341-dep-1' dev-libs/t341-dep
run "D-bpkgexcl-blocker"  "$D" -- -pv "${BASE_OPTS[@]}" --buildpkg-exclude '!dev-libs/t341-dep' dev-libs/t341-dep

# ============ E. sets + world ============
E=$(mkcfg cfgE $T341 "$GPROF")
mkdir -p "$E/etc/portage/sets" "$E/var/lib/portage"
printf '%s\n' "$U8" > "$E/etc/portage/sets/s341"
printf '%s\n' "$U8" > "$E/var/lib/portage/world"
run "E-sets-s341"    "$E" -- -pv "${BASE_OPTS[@]}" @s341
run "E-sets-selected" "$E" -- -pv "${BASE_OPTS[@]}" @selected

# ============ F. binary (Portage builds+rewrites; portuale reuses the index) ====
if [ "$PM" = portage ]; then
  export FEATURES="-ipc-sandbox -network-sandbox -pid-sandbox -sandbox -usersandbox -cgroup"
  FB=$(mkcfg cfgFB $T341 "$GPROF")
  mkdir -p $S0W/pkgdir
  PORTAGE_CONFIGROOT=$FB ROOT=$FB BINPKG_FORMAT=xpak PKGDIR=$S0W/pkgdir \
    emerge --buildpkg=y --oneshot --quiet-build --ignore-default-opts app-misc/t341-good \
    >"$OUT/F-build.stdout" 2>"$OUT/F-build.stderr"; printf '%s\n' "$?" > "$OUT/F-build.rc"
  printf 'PORTAGE_CONFIGROOT=%s ROOT=%s BINPKG_FORMAT=xpak PKGDIR=%s emerge --buildpkg=y --oneshot --quiet-build --ignore-default-opts app-misc/t341-good\n' "$FB" "$FB" "$S0W/pkgdir" > "$OUT/F-build.argv"
  python3 - "$S0W/pkgdir" <<'PY'
import re, sys, glob
pkgdir = sys.argv[1]
s = open(pkgdir + '/Packages').read()
parts = s.split('\n\n')
out = []
for p in parts:
    if 'CPV: app-misc/t341-good-1\n' in p:
        p = re.sub(r'^RDEPEND: .*$', 'RDEPEND: dev-libs/t341-dep::gentoo', p, flags=re.M)
    out.append(p)
open(pkgdir + '/Packages', 'w').write('\n\n'.join(out))
import portage.xpak as xpak
f = glob.glob(pkgdir + '/app-misc/t341-good/*.xpak')[0]
t = xpak.tbz2(f); d = t.get_data(); d[b'RDEPEND'] = b'dev-libs/t341-dep::gentoo\n'
t.recompose_mem(xpak.xpak_mem(d))
print("rewrote", f)
PY
  grep -E 'CPV|RDEPEND|EAPI' "$S0W/pkgdir/Packages" | head -6
  FB2=$(mkcfg cfgFB2 $T341 "$GPROF")
  mkdir -p $S0W/pkgdir2; cp -a $S0W/pkgdir/. $S0W/pkgdir2/
  printf '%s\n' "PKGDIR=$S0W/pkgdir" >> "$FB2/etc/portage/make.conf"
  run "F-binary-rewritten" "$FB2" -- -pv --ignore-default-opts --usepkgonly=y app-misc/t341-good
fi
if [ "$PM" = portuale ] && [ -d "$S0W/pkgdir" ]; then
  # Reuse the Portage-built, RDEPEND-rewritten PKGDIR from the portage run.
  FB2=$(mkcfg cfgFB2 $T341 "$GPROF")
  printf '%s\n' "PKGDIR=$S0W/pkgdir" >> "$FB2/etc/portage/make.conf"
  run "F-binary-rewritten" "$FB2" -- -pv --ignore-default-opts --usepkgonly=y app-misc/t341-good
fi

# ============ G. installed (vdb rewrite; S10 evidence) ============
if [ "$PM" = portage ]; then
  export FEATURES="-ipc-sandbox -network-sandbox -pid-sandbox -sandbox -usersandbox -cgroup"
  G=$(mkcfg cfgG $T341 "$GPROF")
  GROOT=$S0W/root-inst; rm -rf "$GROOT"; mkdir -p "$GROOT"
  PORTAGE_CONFIGROOT=$G ROOT=$GROOT emerge --oneshot --quiet-build --ignore-default-opts --getbinpkg=n --usepkg=n app-misc/t341-good \
    >"$OUT/G-install.stdout" 2>"$OUT/G-install.stderr"; printf '%s\n' "$?" > "$OUT/G-install.rc"
  printf 'PORTAGE_CONFIGROOT=%s ROOT=%s emerge --oneshot ... app-misc/t341-good\n' "$G" "$GROOT" > "$OUT/G-install.argv"
  cp /s0/pkgprobe.py $S0W/pkgprobe.py
  for dep in 'dev-libs/t341-dep::gentoo' '=dev-libs/t341-dep-1-2' 'dev-libs/t341-dep[nonexistent?]' 'dev-libs/t341-dep:0*'; do
    safe=$(printf '%s' "$dep" | tr -c 'A-Za-z0-9' _)
    echo "$dep" > "$GROOT/var/db/pkg/app-misc/t341-good-1/RDEPEND"
    touch -d '+1 min' "$GROOT/var/db/pkg/app-misc/t341-good-1" "$GROOT/var/db/pkg/app-misc" 2>/dev/null || touch "$GROOT/var/db/pkg/app-misc/t341-good-1" "$GROOT/var/db/pkg/app-misc"
    rm -f /var/cache/edb/vdb_metadata.pickle
    PORTAGE_CONFIGROOT=$G ROOT=$GROOT python3 $S0W/pkgprobe.py installed app-misc/t341-good \
      >"$OUT/G-vdb-$safe.stdout" 2>"$OUT/G-vdb-$safe.stderr" || true
    printf 'vdb RDEPEND=%s; PORTAGE_CONFIGROOT=%s ROOT=%s [C] python3 pkgprobe.py installed app-misc/t341-good\n' "$dep" "$G" "$GROOT" > "$OUT/G-vdb-$safe.argv"
    PORTAGE_CONFIGROOT=$G ROOT=$GROOT PORTAGE_NATIVE_DEP_PARSER=0 python3 $S0W/pkgprobe.py installed app-misc/t341-good \
      >"$OUT/G-vdb-$safe-py.stdout" 2>"$OUT/G-vdb-$safe-py.stderr" || true
    printf 'vdb RDEPEND=%s; PORTAGE_CONFIGROOT=%s ROOT=%s [PY] PORTAGE_NATIVE_DEP_PARSER=0 python3 pkgprobe.py installed app-misc/t341-good\n' "$dep" "$G" "$GROOT" > "$OUT/G-vdb-$safe-py.argv"
  done
  echo 'dev-libs/t341-dep::gentoo' > "$GROOT/var/db/pkg/app-misc/t341-good-1/RDEPEND"
  rm -f /var/cache/edb/vdb_metadata.pickle
  run "G-inst-poisoned-emerge" "$G" --root "$GROOT" -- -pv "${BASE_OPTS[@]}" dev-libs/t341-dep
  PORTAGE_CONFIGROOT=$G ROOT=$GROOT PORTAGE_NATIVE_DEP_PARSER=0 emerge -pv "${BASE_OPTS[@]}" dev-libs/t341-dep \
    >"$OUT/G-inst-poisoned-emerge-py.stdout" 2>"$OUT/G-inst-poisoned-emerge-py.stderr"
  printf '%s\n' "$?" > "$OUT/G-inst-poisoned-emerge-py.rc"
  printf 'PORTAGE_NATIVE_DEP_PARSER=0 emerge -pv %s dev-libs/t341-dep (PORTAGE_CONFIGROOT=%s ROOT=%s, vdb RDEPEND=dev-libs/t341-dep::gentoo)\n' "${BASE_OPTS[*]}" "$G" "$GROOT" > "$OUT/G-inst-poisoned-emerge-py.argv"
fi
if [ "$PM" = portuale ] && [ -d "$S0W/root-inst/var/db/pkg/app-misc/t341-good-1" ]; then
  # Reuse the Portage-installed root with the poisoned vdb RDEPEND.
  G=$(mkcfg cfgG $T341 "$GPROF")
  run "G-inst-poisoned-emerge" "$G" --root "$S0W/root-inst" -- -pv "${BASE_OPTS[@]}" dev-libs/t341-dep
fi
echo "PM=$PM done: $(ls "$OUT" | wc -l) files"
