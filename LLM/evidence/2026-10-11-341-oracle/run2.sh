set -u
export FEATURES="-ipc-sandbox -network-sandbox -pid-sandbox -sandbox -usersandbox -cgroup"
printf '[oracle]\nlocation = /var/db/repos/oracle\n' > /etc/portage/repos.conf/oracle.conf
echo "=== ARGV: BINPKG_FORMAT=xpak emerge -1 --buildpkg=y --quiet-build --ignore-default-opts app-misc/goodslot"
BINPKG_FORMAT=xpak PKGDIR=/var/tmp/oraclepkgs emerge -1 --buildpkg=y --quiet-build --ignore-default-opts app-misc/goodslot 2>&1 | grep -E '^>>> (Emerging|Installing|Completed)|error|Error' | head
# --- installed: rewrite vdb RDEPEND
echo 'dev-libs/foo::gentoo' > /var/db/pkg/app-misc/goodslot-1/RDEPEND
echo "=== installed pkg probe (vdb RDEPEND rewritten to dev-libs/foo::gentoo), C parser default"
python3 /pkgprobe.py installed app-misc/goodslot
echo "=== same, PORTAGE_NATIVE_DEP_PARSER=0"
PORTAGE_NATIVE_DEP_PARSER=0 python3 /pkgprobe.py installed app-misc/goodslot
echo 'dev-libs/foo[nonexistent?]' > /var/db/pkg/app-misc/goodslot-1/RDEPEND
echo "=== installed pkg probe (vdb RDEPEND = dev-libs/foo[nonexistent?])"
python3 /pkgprobe.py installed app-misc/goodslot
echo 'dev-libs/foo::gentoo' > /var/db/pkg/app-misc/goodslot-1/RDEPEND
echo "=== ARGV: emerge -pv --ignore-default-opts --usepkg=n app-misc/goodslot   (vdb RDEPEND dev-libs/foo::gentoo)"
emerge -pv --ignore-default-opts app-misc/goodslot 2>&1 | grep -v '^$' | grep -v 'news\|cgroup\|unshare'
echo "=== ARGV: emerge -pc --ignore-default-opts app-misc/goodslot"
emerge -pc --ignore-default-opts app-misc/goodslot 2>&1 | grep -v '^$' | grep -v 'news\|cgroup\|unshare' | head -20
# --- binary: rewrite xpak + Packages index
python3 - <<'PY'
import portage.xpak as xpak, glob, re
p = glob.glob('/var/tmp/oraclepkgs/app-misc/goodslot*')+glob.glob('/var/tmp/oraclepkgs/app-misc/goodslot/*')
p = [x for x in p if x.endswith('.tbz2') or x.endswith('.xpak')]
print("binpkg:", p)
t = xpak.tbz2(p[0]); d = t.get_data(); d[b'RDEPEND'] = b'dev-libs/foo::gentoo\n'; t.recompose_mem(xpak.xpak_mem(d))
s = open('/var/tmp/oraclepkgs/Packages').read()
s = re.sub(r'(CPV: app-misc/goodslot-1\n(?:[A-Z_]+: .*\n)*?)RDEPEND: .*\n', r'\1RDEPEND: dev-libs/foo::gentoo\n', s)
if 'RDEPEND: dev-libs/foo::gentoo' not in s:
    s = s.replace('CPV: app-misc/goodslot-1\n', 'CPV: app-misc/goodslot-1\nRDEPEND: dev-libs/foo::gentoo\n')
open('/var/tmp/oraclepkgs/Packages','w').write(s)
PY
grep -A30 'CPV: app-misc/goodslot-1' /var/tmp/oraclepkgs/Packages | grep -E 'CPV|RDEPEND|EAPI'
echo "=== binary pkg probe (Packages RDEPEND = dev-libs/foo::gentoo)"
PKGDIR=/var/tmp/oraclepkgs python3 /pkgprobe.py binary app-misc/goodslot
echo "=== ARGV: PKGDIR=/var/tmp/oraclepkgs emerge -pv --ignore-default-opts --usepkgonly=y app-misc/goodslot"
PKGDIR=/var/tmp/oraclepkgs emerge -pv --ignore-default-opts --usepkgonly=y app-misc/goodslot 2>&1 | grep -v '^$' | grep -v 'news\|cgroup\|unshare'
