#!/usr/bin/env python3
"""List functions over N code lines (#339 acceptance check).

Usage (from the portuale root): fn_lengths.py [N]   (default 200)

Counts body lines strictly between a fn's braces that hold code (blank and
comment-only lines excluded, strings/comments lexed by rust_items.code_mask),
over crates/*/src and crates/*/tests. Same counts as the syn-based list in
LLM/plans/2026-10-10-long-functions.md (cross-checked 2026-10-11: 36 of them still over 200; see baseline).
Prints "len<TAB>file:line<TAB>name" for every fn over N, then a summary line.
"""
import sys,re,glob
sys.path.insert(0, __import__('os').path.dirname(__file__))
from rust_items import code_mask
FN=re.compile(r'\bfn\s+([A-Za-z_][A-Za-z0-9_]*)')
res=[]
for f in glob.glob('crates/*/src/**/*.rs',recursive=True)+glob.glob('crates/*/tests/**/*.rs',recursive=True):
    src=open(f,encoding='utf-8').read(); m=code_mask(src)
    lines_start=[0]
    for i,c in enumerate(src):
        if c=='\n': lines_start.append(i+1)
    import bisect
    ln=lambda p: bisect.bisect_right(lines_start,p)
    for mm in FN.finditer(src):
        p=mm.start()
        if not m[p]: continue
        # find body open brace (skip signature parens/generics); stop at ';'
        i=mm.end(); depth=0
        while i<len(src):
            if m[i]:
                c=src[i]
                if c in '(<[': depth+=1
                elif c in ')>]': depth-=1
                elif c==';' and depth<=0: i=-1;break
                elif c=='{' and depth<=0: break
            i+=1
        if i<0 or i>=len(src): continue
        d=0;j=i
        while j<len(src):
            if m[j]:
                if src[j]=='{': d+=1
                elif src[j]=='}':
                    d-=1
                    if d==0: break
            j+=1
        a,b=ln(i),ln(j)
        cnt=0
        for L in range(a+1,b):
            s=src[lines_start[L-1]:lines_start[L]] if L<len(lines_start) else ''
            seg=range(lines_start[L-1], lines_start[L]-1 if L<len(lines_start) else len(src))
            if any(m[k] and not src[k].isspace() for k in seg): cnt+=1
        res.append((cnt,f,ln(p),mm.group(1)))
res.sort(reverse=True)
N=int(sys.argv[1]) if len(sys.argv)>1 else 200
for r in res:
    if r[0]>N: print(f"{r[0]}\t{r[1]}:{r[2]}\t{r[3]}")
print(f"over{N}",sum(r[0]>N for r in res),"over150",sum(r[0]>150 for r in res),"over100",sum(r[0]>100 for r in res), "fns",len(res))
