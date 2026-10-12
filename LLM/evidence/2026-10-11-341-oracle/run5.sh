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
rm -rf /etc/portage/package.mask /etc/portage/package.use /etc/portage/package.accept_keywords /etc/portage/package.env /etc/portage/profile
mkdir -p /etc/portage/profile /etc/portage/sets
echo "$L" > /etc/portage/package.mask
echo "$L" | sed 's/$/ x/' > /etc/portage/package.use
echo "$L" | sed 's/$/ ~amd64/' > /etc/portage/package.accept_keywords
echo "$L" | sed 's/$/ foo.conf/' > /etc/portage/package.env
mkdir -p /etc/portage/env; echo 'X=1' > /etc/portage/env/foo.conf
echo "$L" > /etc/portage/profile/package.mask
echo "$L" | sed 's/$/ x/' > /etc/portage/profile/package.use.force
echo "#### user config /etc/portage/{package.mask,package.use,package.accept_keywords,package.env} + /etc/portage/profile/{package.mask,package.use.force} (no eapi file)"
echo "=== ARGV: emerge -pv --ignore-default-opts dev-libs/foo"
emerge -pv --ignore-default-opts dev-libs/foo 2>&1 | grep -v "$F"
rm -rf /etc/portage/package.mask /etc/portage/package.use /etc/portage/package.accept_keywords /etc/portage/package.env /etc/portage/profile
echo "#### command line"
for a in '=dev-libs/foo-1-2' '=dev-libs/foo-1-1' '*/foo' 'dev-libs/foo::oracle' '!dev-libs/foo' 'dev-libs/foo[x?]' 'foo::oracle' 'dev-libs/foo-1' '=dev-libs/foo-1-02'; do
  echo "=== ARGV: emerge -pv --ignore-default-opts '$a'"
  emerge -pv --ignore-default-opts "$a" 2>&1 | grep -v "$F" | head -8
done
for ex in '>=dev-libs/foo-1' 'dev-libs/*' 'foo' 'dev-libs/foo::oracle' 'dev-libs/foo:0' '=dev-libs/foo-1-2'; do
  echo "=== ARGV: emerge -pv --ignore-default-opts --exclude '$ex' dev-libs/foo"
  emerge -pv --ignore-default-opts --exclude "$ex" dev-libs/foo 2>&1 | grep -v "$F" | head -6
done
for ex in '=dev-libs/foo-1-2' 'dev-libs/foo::oracle' '>=dev-libs/foo-1'; do
  echo "=== ARGV: emerge -pv --ignore-default-opts --buildpkg-exclude '$ex' dev-libs/foo"
  emerge -pv --ignore-default-opts --buildpkg-exclude "$ex" dev-libs/foo 2>&1 | grep -v "$F" | head -6
done
echo "#### /etc/portage/sets/myset"
echo "$L" > /etc/portage/sets/myset
echo "=== ARGV: emerge -pv --ignore-default-opts @myset"
emerge -pv --ignore-default-opts @myset 2>&1 | grep -v "$F" | head -20
rm -f /etc/portage/sets/myset
echo "#### world file"
cp /var/lib/portage/world /tmp/world.bak 2>/dev/null; echo "$L" > /var/lib/portage/world
echo "=== ARGV: emerge -pv --ignore-default-opts @selected"
emerge -pv --ignore-default-opts @selected 2>&1 | grep -v "$F" | head -20
