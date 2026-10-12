set -u
F='news\|cgroup\|unshare\|^$\|eselect'
cp -a /mnt/ovl /var/db/repos/oracle
printf '[oracle]\nlocation = /var/db/repos/oracle\n' > /etc/portage/repos.conf/oracle.conf
O=/var/db/repos/oracle/profiles
mkdir -p $O/oprof $O/oprof0
echo 5 > $O/oprof/eapi
echo /var/db/repos/gentoo/profiles/default/linux/amd64/23.0/systemd > $O/oprof/parent
echo ../oprof > $O/oprof0/parent
PM='dev-libs/foo::oracle
=dev-libs/foo-1-2
*/foo
dev-libs/foo:=
dev-libs/foo:0
dev-libs/foo[x(+)]
dev-libs/foo-1'
echo "$PM" > $O/oprof/package.mask
echo "$PM" > $O/oprof0/package.mask
echo "$PM" | sed 's/$/ x/' > $O/oprof/package.use
ln -sfn $O/oprof0 /etc/portage/make.profile
echo "#### profile: oprof (eapi 5) <- oprof0 (no eapi file); layout.conf without profile-formats"
echo "=== ARGV: emerge -pv --ignore-default-opts dev-libs/foo"
emerge -pv --ignore-default-opts dev-libs/foo 2>&1 | grep -v "$F"
printf 'masters = gentoo\nthin-manifests = true\nprofile-formats = portage-2 profile-repo-deps build-id\n' > /var/db/repos/oracle/metadata/layout.conf
echo "#### same, layout.conf profile-formats = portage-2 profile-repo-deps build-id"
echo "=== ARGV: emerge -pv --ignore-default-opts dev-libs/foo"
emerge -pv --ignore-default-opts dev-libs/foo 2>&1 | grep -v "$F"
