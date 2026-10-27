#!/bin/bash
# mkcfg.sh <root-with-repo/> <cfgdir> : Portage config root whose repos.conf points at absolute paths in <root>
set -eu
D=$1; cfg=$2
rm -rf "$cfg"; mkdir -p "$cfg/etc"; cp -a "$D/etc/portage" "$cfg/etc/portage"
rm -f "$cfg/etc/portage/make.profile"; ln -s "$D/repo/profiles/default" "$cfg/etc/portage/make.profile"
python3 - "$D" "$cfg" <<'P'
import sys,re,os
D,cfg=sys.argv[1:]
p=cfg+"/etc/portage/repos.conf"
for f in os.listdir(p):
    fp=p+"/"+f; t=open(fp).read()
    t=re.sub(r"^(location\s*=\s*)(?!/)(\S+)", lambda m: m.group(1)+D+"/"+m.group(2), t, flags=re.M)
    open(fp,"w").write(t)
P
