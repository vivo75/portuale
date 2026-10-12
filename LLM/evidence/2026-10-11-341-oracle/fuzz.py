import random, sys, json
from portage.dep import Atom
from portage.exception import InvalidAtom
random.seed(int(sys.argv[1]))
parts = ["!","!!","=",">=","<","~","dev-libs","a","_b","+c","-d","/","foo","-1","-1.0","-r1","-2","-02","*","-1*",":",":0",":=",":*",":0/1","/1","=","::gentoo","::x-y","[","]","bar","bar(+)","-bar","!bar?","bar=","bar?",",",".","-","1","_p1","_alpha","a1"]
res=[]
for i in range(60000):
    s = "".join(random.choice(parts) for _ in range(random.randint(2,9)))
    row=[]
    for kw in ({}, {"eapi":"8"}, {"allow_wildcard":True,"allow_repo":True}):
        try:
            a=Atom(s,**kw); row.append(("OK",a.cp,a.cpv,a.version,a.operator,a.slot,a.sub_slot,a.slot_operator,a.repo,a.build_id,str(a.use) if a.use else None, (a.blocker.overlap.forbid if a.blocker else None)))
        except InvalidAtom as e: row.append(("ERR",str(e)))
        except Exception as e: row.append(("EXC",type(e).__name__))
    res.append((s,row))
json.dump(res, open(sys.argv[2],"w"))
