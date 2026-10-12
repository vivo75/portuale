set -u
F='news\|cgroup\|unshare\|^$\|eselect\|Gentoo Handbook\|MASKED PACKAGES'
printf '[oracle]\nlocation = /var/db/repos/oracle\n' > /etc/portage/repos.conf/oracle.conf
L='*/foo
dev-libs/fo*
dev-libs/foo::oracle
=dev-libs/foo-1-2
=dev-libs/foo-1-02
dev-libs/foo-1
!dev-libs/foo
dev-libs/foo[x]'
mkdir -p /etc/portage/profile /etc/portage/env; echo 'X=1' > /etc/portage/env/foo.conf
rm -rf /etc/portage/package.mask /etc/portage/package.use /etc/portage/package.accept_keywords /etc/portage/package.env /etc/portage/package.license
for f in package.mask package.unmask profile/package.mask profile/packages; do echo "$L" > /etc/portage/$f; done
for f in package.use package.accept_keywords profile/package.use.force profile/package.use; do echo "$L" | sed 's/$/ x/' > /etc/portage/$f; done
echo "$L" | sed 's/$/ MIT/' > /etc/portage/package.license
echo "$L" | grep -v '\[' | sed 's/$/ foo.conf/' > /etc/portage/package.env
echo "#### user config (package.env without the [x] line)"
echo "=== ARGV: emerge -pv --ignore-default-opts dev-libs/foo"
emerge -pv --ignore-default-opts dev-libs/foo 2>&1 | grep -v "$F" | sort
echo "=== python: settings.packages / user profile node"
python3 -c '
import portage
s=portage.settings
print("profile nodes:", [(p.location, p.eapi, p.allow_build_id, p.user_config) for p in s._locations_manager.profiles_complex][-2:])
print("packages:", [str(x) for x in s.packages if "foo" in str(x)])
' 2>&1 | grep -v '^---'
rm -rf /etc/portage/profile /etc/portage/package.*
echo "dev-libs/foo[x] foo.conf" > /etc/portage/package.env
echo "#### package.env with use-dep atom"
echo "=== ARGV: emerge -pv --ignore-default-opts dev-libs/foo"
emerge -pv --ignore-default-opts dev-libs/foo 2>&1 | grep -v "$F" | grep -v '^  ' | tail -4
rm -f /etc/portage/package.env
for ex in '>=dev-libs/foo-1' 'dev-libs/foo::oracle' '=dev-libs/foo-1-2' 'dev-libs/foo[x]' '!dev-libs/foo' 'dev-libs/foo:0/1' '*/*:0'; do
  echo "=== ARGV: emerge -pv --ignore-default-opts --exclude '$ex' dev-libs/foo"
  emerge -pv --ignore-default-opts --exclude "$ex" dev-libs/foo 2>&1 | grep -v "$F" | tail -2
done
echo "=== ARGV: emerge -pv --ignore-default-opts --usepkg-exclude 'dev-libs/foo::oracle' dev-libs/foo"
emerge -pv --ignore-default-opts --usepkg-exclude 'dev-libs/foo::oracle' dev-libs/foo 2>&1 | grep -v "$F" | tail -2
