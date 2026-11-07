import sys, os, shutil
# mkroot.py <root> <world-atom|-> cat/pkg-ver:slot:RDEPEND ...
root=sys.argv[1]; world=sys.argv[2]
shutil.rmtree(root, ignore_errors=True)
os.makedirs(f"{root}/var/lib/portage")
open(f"{root}/var/lib/portage/world","w").write("" if world=="-" else world+"\n")
for spec in sys.argv[3:]:
    cpv, slot, rdep = spec.split("@")
    cat, pf = cpv.split("/")
    d=f"{root}/var/db/pkg/{cat}/{pf}"; os.makedirs(d)
    files={"CATEGORY":cat,"SLOT":slot,"repository":"testrepo","EAPI":"8"}
    if rdep: files["RDEPEND"]=rdep
    for k,v in files.items(): open(f"{d}/{k}","w").write(v+"\n")
