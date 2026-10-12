from portage.dep import Atom
from portage.exception import InvalidAtom
D='/tmp/claude-1000/-home-vivo-repo-PORTUALE-portuale/c7644c26-986a-4eef-925a-e6d77994be68/scratchpad/oracle341/'
modes=[("None",{}),("e8",{"eapi":"8"}),("e5",{"eapi":"5"}),("wild",{"allow_wildcard":True,"allow_repo":True})]
def ok(s,kw):
    try: Atom(s,**kw); return "OK"
    except InvalidAtom: return "ERR"
for line in open(D+'rs_pairs.txt'):
    s, rest = line.rstrip("\n").split("\t",1)
    rs = "ERR" if rest.startswith("INVALID") else "OK"
    p = {n:ok(s,kw) for n,kw in modes}
    diff = [n for n in p if p[n]!=rs]
    if diff and p["e8"]!=rs or p["None"]!=rs:
        print("%-38s portuale=%s  Portage: %s" % (repr(s), rs, " ".join("%s=%s"%(n,p[n]) for n in p)))
