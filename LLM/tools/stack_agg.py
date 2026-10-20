import re,collections,sys
S=open(sys.argv[1]).read().split('=== ')[1:]
inc=collections.Counter(); child=collections.Counter(); n=len(S)
def clean(f): return re.sub(r'\s*\(\.llvm\.\d+\)|::\{closure#?\d*\}|<[^<>]*>','',re.sub(r'^#\d+\s+0x[0-9a-f]+ ','',f))
for s in S:
    fr=[clean(l) for l in s.splitlines() if l.startswith('#')]
    seen=set()
    for f in fr:
        if re.match(r'(portage_|portuale)',f) and f not in seen: seen.add(f); inc[f]+=1
    # callee-of-run_pass / backtracking_resolve/ plan_resolve attribution
    for anchor in ('portage_repo::resolve::pass::run_pass','portuale::pretend::resolve_plan::plan_resolve','portage_repo::resolve::driver::backtracking_resolve'):
        if anchor in fr:
            i=fr.index(anchor)
            if i>0:
                child[(anchor.split('::')[-1],fr[i-1][:100])]+=1
            else: child[(anchor.split('::')[-1],'(self)')]+=1
print('samples',n)
for k,v in inc.most_common(int(sys.argv[2]) if len(sys.argv)>2 else 30): print(f'{v:4d} {100*v/n:4.0f}% {k[:110]}')
print('--- direct callee of anchors')
for k,v in child.most_common(25): print(f'{v:4d} {k}')
