set -u
export FEATURES="-ipc-sandbox -network-sandbox -pid-sandbox -sandbox -usersandbox -cgroup"
printf '[oracle]\nlocation = /var/db/repos/oracle\n' > /etc/portage/repos.conf/oracle.conf
BINPKG_FORMAT=xpak PKGDIR=/var/tmp/oraclepkgs emerge -1 --buildpkg=y --quiet-build --ignore-default-opts app-misc/goodslot >/dev/null 2>&1
setdep(){ echo "$1" > /var/db/pkg/app-misc/goodslot-1/RDEPEND; touch -d '+1 min' /var/db/pkg/app-misc/goodslot-1 /var/db/pkg/app-misc; rm -f /var/cache/edb/vdb_metadata.pickle; }
for dep in 'dev-libs/foo::gentoo' '=dev-libs/foo-1-2' 'dev-libs/foo[nonexistent?]' 'dev-libs/foo:0*'; do
  setdep "$dep"
  echo "=== installed probe, vdb RDEPEND=$dep (native C parser)"
  python3 /pkgprobe.py installed app-misc/goodslot
  echo "=== installed probe, vdb RDEPEND=$dep (PORTAGE_NATIVE_DEP_PARSER=0)"
  PORTAGE_NATIVE_DEP_PARSER=0 python3 /pkgprobe.py installed app-misc/goodslot
done
setdep 'dev-libs/foo::gentoo'
echo "=== ARGV: emerge -pv --ignore-default-opts dev-libs/foo   (installed goodslot vdb RDEPEND=dev-libs/foo::gentoo)"
emerge -pv --ignore-default-opts dev-libs/foo 2>&1 | grep -v '^$' | grep -v 'news\|cgroup\|unshare'
echo "=== ARGV: PORTAGE_NATIVE_DEP_PARSER=0 emerge -pv --ignore-default-opts dev-libs/foo"
PORTAGE_NATIVE_DEP_PARSER=0 emerge -pv --ignore-default-opts dev-libs/foo 2>&1 | grep -v '^$' | grep -v 'news\|cgroup\|unshare'
echo "=== ARGV: emerge -pv --ignore-default-opts app-misc/goodslot"
emerge -pv --ignore-default-opts app-misc/goodslot 2>&1 | grep -v '^$' | grep -v 'news\|cgroup\|unshare'
# binary
python3 - <<'PY'
import re
s = open('/var/tmp/oraclepkgs/Packages').read()
parts = s.split('\n\n')
out=[]
for p in parts:
    if 'CPV: app-misc/goodslot-1\n' in p:
        p = re.sub(r'^RDEPEND: .*$', 'RDEPEND: dev-libs/foo::gentoo', p, flags=re.M)
    out.append(p)
open('/var/tmp/oraclepkgs/Packages','w').write('\n\n'.join(out))
import portage.xpak as xpak, glob
f = glob.glob('/var/tmp/oraclepkgs/app-misc/goodslot/*.xpak')[0]
t = xpak.tbz2(f); d = t.get_data(); d[b'RDEPEND'] = b'dev-libs/foo::gentoo\n'; t.recompose_mem(xpak.xpak_mem(d))
PY
awk 'BEGIN{RS=""} /CPV: app-misc\/goodslot-1\n/' /var/tmp/oraclepkgs/Packages | grep -E 'CPV|RDEPEND|EAPI'
echo "=== binary probe (Packages + xpak RDEPEND = dev-libs/foo::gentoo)"
PKGDIR=/var/tmp/oraclepkgs python3 /pkgprobe.py binary app-misc/goodslot
echo "=== ARGV: PKGDIR=/var/tmp/oraclepkgs emerge -pv --ignore-default-opts --usepkgonly=y app-misc/goodslot"
PKGDIR=/var/tmp/oraclepkgs emerge -pv --ignore-default-opts --usepkgonly=y app-misc/goodslot 2>&1 | grep -v '^$' | grep -v 'news\|cgroup\|unshare'
echo "=== ARGV: PKGDIR=/var/tmp/oraclepkgs emerge -pv --ignore-default-opts --usepkgonly=y --emptytree app-misc/goodslot"
PKGDIR=/var/tmp/oraclepkgs emerge -pv --ignore-default-opts --usepkgonly=y --emptytree --nodeps app-misc/goodslot 2>&1 | grep -v '^$' | grep -v 'news\|cgroup\|unshare'
