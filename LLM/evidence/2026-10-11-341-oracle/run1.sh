set -u
printf '[oracle]\nlocation = /var/db/repos/oracle\n' > /etc/portage/repos.conf/oracle.conf
for a in app-misc/badrepo app-misc/badbid app-misc/badcond app-misc/badblock app-misc/goodslot; do
  echo "=== ARGV: emerge -pv --ignore-default-opts $a"
  emerge -pv --ignore-default-opts $a 2>&1 | grep -v '^$' | head -30
done
