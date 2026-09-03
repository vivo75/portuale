#! /bin/bash

if [[ $(id -u) -ne 0 ]] ; then
  echo "Must be root to be happy"
  exit 99
fi

#BASEDIR=/home/vivo/tmp/gentoo
BASEDIR=$( realpath ${0%/*} )
if [[ ! -d ${BASEDIR} ]] ; then echo "failed discovery of workdir" ; exit 1 ; fi

declare -a REPOS=( gentoo buildovl )
declare -A LAST_COMMIT
LAST_COMMIT["gentoo"]="11c58b7af1df0fbc3e9f2560a82c4355231967a6"
LAST_COMMIT["buildovl"]="3b1df68114f4c486520864338b1b6a42efcaec7e"
DATESTART=2026-08-23T15:30:57Z

STAGEID=stage3-amd64-systemd
STAGETS=${DATESTART//-/}
STAGETS=${STAGETS//:/}

#PORTUALE_EXECS=/home/vivo/repo/portage/PORTING/rust/target/release

SU='sudo -su vivo'

rm -rf WORKDIR
mkdir WORKDIR
if [[ ! -d WORKDIR ]] ; then echo "failed creation of WORKDIR" ; exit 2 ; fi
pushd WORKDIR

mkdir -p TEST/{scripts,logs}

gcc -O2 -Wall -o init ../init.c
ret=$? ; if [[ ${ret} != 0 ]] ; then echo "compile init failed with err=${ret}" ; exit 3; fi

if [[ ! -e ${BASEDIR}/${STAGEID}-${STAGETS}.tar.xz ]] ; then
  # https://www.gentoo.org/downloads/amd64/#stages
  wget https://distfiles.gentoo.org/releases/amd64/autobuilds/${STAGEID}/${STAGETS}-${STAGETS}.tar.xz
  ret=$? ; if [[ ${ret} != 0 ]] ; then echo "download of stage3 failed with err=${ret}" ; exit 4; fi
fi
tar -xf ${BASEDIR}/${STAGEID}-${STAGETS}.tar.xz

# cp -a \
#   ${PORTUALE_EXECS}/{required-use-harness,atom-harness,portuale,versions-harness,use-reduce-harness} \
#   usr/local/bin/
# ret=$? ; if [[ ${ret} != 0 ]] ; then echo "copy portuale files failed with err=${ret}" ; exit 3; fi

# Clone repositories up to a known situation
pushd ./var/db/repos/
for repo in ${REPOS[@]} ; do
  mkdir ${repo}
  pushd ${repo}
  git init
  git remote add origin file://${BASEDIR}/repos/${repo}/
  # limit it with '--depth=...' or '--shallow-since=...'
  git fetch origin --shallow-since=${DATESTART} ${LAST_COMMIT[${repo}]}
  git reset --hard FETCH_HEAD
  popd # ${repo}
done
popd # ./var/db/repos/

# Make internet available
cat << 'EOF' > etc/resolv.conf 
nameserver 1.1.1.1
nameserver 8.8.8.8
search f1r.eu
options timeout:1
options edns0
options use-vc
options no-tld-query
EOF


eselect news read &> /dev/null

pushd ./etc/portage/
echo 'USE="${USE} X alsa dbus icu libproxy"' > make.USE.conf
cat <<'EOF' >> make.conf
source /etc/portage/make.USE.conf
FEATURES="cgroup distlocks ebuild-locks -fail-clean multilib-strict noinfo observability pid-sandbox pkgdir-index-trusted preserve-libs protect-owned qa-unresolved-soname-deps sandbox sign split-elog split-log splitdebug strict unknown-features-warn unmerge-logs unmerge-orphans userfetch userpriv usersandbox usersync xattr"
EOF
echo '*/* PYTHON_SINGLE_TARGET: python3_14' > package.use/PYTHON_SINGLE_TARGET
echo '*/* minizip  opengl policykit qml text wayland -webengine' > package.use/kde-apps--kdecore-meta

# Repositories
mkdir repos.conf
cat << 'EOF' > repos.conf/gentoo.conf
[DEFAULT]
main-repo = gentoo
[gentoo]
location = /var/db/repos/gentoo
sync-type = git
sync-uri = https://github.com/gentoo-mirror/gentoo.git
EOF
cat << 'EOF' > repos.conf/buildovl.conf
[buildovl]
location = /var/db/repos/buildovl
sync-type = git
sync-uri = https://github.com/vivo75/buildovl.git
priority = 10
EOF
popd # ./etc/portage/
popd # WORKDIR

# Update uids/gids to userpriv container
UIDS=$(find WORKDIR -exec stat --format %u {} + | sort -un)
for uid in ${UIDS} ; do
  find WORKDIR -uid ${uid} -exec chown --no-dereference $(( 100000 + ${uid} )) {} +
done

GIDS=$(find WORKDIR -exec stat --format %g {} + | sort -un)
for gid in ${GIDS} ; do        
  find WORKDIR -gid ${gid} -exec chgrp --no-dereference $(( 100000 + ${gid} )) {} +
done

${SU} podman rmi localhost/test-portuale
${SU} buildah unshare bash << 'EOF'
set -x

ctr=$(buildah from scratch)
mnt=$(buildah mount "$ctr")

# 3. Copia il contenuto di WORKDIR preservando uid/gid esattamente
#    (dentro unshare, 100000 è visto/gestibile come se stesso,
#    quindi cp -a può chownare correttamente i file)
cp -a WORKDIR/* "$mnt"/

buildah umount "$ctr"
#buildah config --user 1000 "$ctr"
buildah config --user 0 "$ctr"
buildah config --entrypoint '["/init"]' "$ctr"
buildah commit "$ctr" localhost/test-portuale:latest
buildah rm "$ctr"
EOF
${SU} podman images

# # L'utente effettivo del processo deve essere 1000
# podman run --rm localhost/test-portuale /usr/bin/id
# # I file copiati devono apparire di proprietà di uid 0 (root) dentro al container,
# # perché con il mapping rootless di default host-uid 100000 -> container-uid 0
# podman run --rm localhost/test-portuale sh -c 'ls -ln /TEST /usr/local/bin'

# podman run --rm \
#  -v ./TEST/scripts:/TEST/scripts \
#  -v ./TEST/logs:/TEST/logs \
#  -v /home/vivo/repo/portage/PORTING/rust/target/release:/usr/local/bin \
#  localhost/test-portuale


# sudo usermod --add-subuids 100000-1065535 --add-subgids 100000-165535 "$(whoami)"
# podman run --rm -ti --user 100000 localhost/test-portuale:latest

# #EMERGE="/usr/bin/emerge --autounmask-backtrack=y"
# EMERGE="/usr/bin/emerge"
# EMERGE="/usr/local/bin/portuale emerge"
# 
# ${EMERGE} -tp app-editors/vim
# ${EMERGE} -ptv --getbinpkg=y app-editors/vim
# USE="-X -wayland" ${EMERGE} -p --columns app-editors/vim
# ${EMERGE} -p --getbinpkg=n kde-apps/kdecore-meta
# ${EMERGE} -p =app-portage/gentoolkit-0.8.0::gentoo
# ${EMERGE} -p '>=app-portage/eix-0.36.9'
