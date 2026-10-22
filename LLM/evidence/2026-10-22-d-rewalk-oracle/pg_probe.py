import sys; sys.path.insert(0,"lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
from _emerge.depgraph import depgraph
calls=[]
oc=depgraph._create_graph; occ=depgraph._complete_graph
def cg(self,*a,**k):
    calls.append(("create_graph", dict(k), "complete" in self._dynamic_config.myparams, self._dynamic_config._complete_mode, len(self._dynamic_config._dep_stack)))
    return oc(self,*a,**k)
def ccg(self,*a,**k):
    r=occ(self,*a,**k)
    calls.append(("complete_graph->", r, "complete" in self._dynamic_config.myparams))
    return r
depgraph._create_graph=cg; depgraph._complete_graph=ccg
ebuilds={"dev-libs/lib-1":{"EAPI":"8","SLOT":"0/1"},"dev-libs/lib-2":{"EAPI":"8","SLOT":"0/2"},
 "app-misc/app-1":{"EAPI":"8","RDEPEND":"dev-libs/lib:=","DEPEND":"dev-libs/lib:="}}
installed={"dev-libs/lib-1":{"EAPI":"8","SLOT":"0/1"},
 "app-misc/app-1":{"EAPI":"8","RDEPEND":"dev-libs/lib:0/1=","DEPEND":"dev-libs/lib:0/1="}}
pg=ResolverPlayground(ebuilds=ebuilds,installed=installed,world=["app-misc/app"],debug=False)
try:
    for atoms,opts in ((["@world"],{"--update":True,"--deep":True}),(["dev-libs/lib"],{"--update":True,"--deep":True}),(["dev-libs/lib"],{"--update":True})):
        calls.clear()
        r=pg.run(atoms,options=opts)
        print(atoms,sorted(opts),"mergelist=",r.mergelist)
        for c in calls: print("   ",c)
finally:
    pg.cleanup()
