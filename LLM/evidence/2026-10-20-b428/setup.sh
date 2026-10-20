#!/bin/bash
# #428 oracle root: the #424 root, index record with MD5+SHA1, binrepos.conf variants chosen by $LOC (explicit|default).
S=${S7:-/var/tmp/b428}
PORT=${PORT:-18428}
LOC=${LOC:-explicit}
HERE=$(cd "$(dirname "$0")" && pwd)
rm -rf $S
S7=$S PORT=$PORT bash "$HERE/../2026-10-19-b424/setup.sh"
f=$S/binhost/dev-libs/gpkgreadpkg-1.0.gpkg.tar
md5=$(md5sum $f | cut -d' ' -f1); sha1=$(sha1sum $f | cut -d' ' -f1)
sed -i "s/^SIZE: \(.*\)$/SIZE: \1\nMD5: $md5\nSHA1: $sha1/" $S/binhost/Packages
if [ "$LOC" = default ]; then
  printf '[tmpbinhost]\nsync-uri = http://127.0.0.1:%s\npriority = 1\n' $PORT > $S/etc/portage/binrepos.conf
fi
# a FETCHCOMMAND that proves it ran: logs the URI then fetches
cat > $S/fc.sh <<EOF
#!/bin/bash
echo "FETCHCOMMAND-RAN \$1 \$2" >> $S/fc.log
exec wget -q -O "\$1/\$2" "\$3"
EOF
chmod +x $S/fc.sh
