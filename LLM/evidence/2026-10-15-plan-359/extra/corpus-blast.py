import json,re,sys
for fn in ('contract.json','expanded.json'):
    d=json.load(open(fn)); blobs=d['blobs']; cases=d['cases']
    n362=[];n361=[]
    for name,c in cases.items():
        a=c['args']; s=' '.join(a)
        short=[x for x in a if re.match(r'^-[a-zA-Z]+$',x)]
        sf=''.join(x[1:] for x in short)
        upd=any(x in a for x in ('--update','--newuse','--changed-use','--changed-deps','--changed-slot')) or any(ch in sf for ch in 'uNU')
        deep=any(x.startswith('--deep') for x in a) or 'D' in sf or '--emptytree' in a or 'e' in sf
        atoms=[x for x in a if '/' in x and not x.startswith('-')]
        out=blobs.get(c['stdout'],'')
        if upd and not deep:
            rows=[l for l in out.splitlines() if l.startswith('[ebuild') or l.startswith('[binary')]
            for r in rows:
                flags=r[8:15] if r.startswith('[ebuild') else r[8:15]
                m=re.search(r'\] (\S+?)-\d',r)
                cp=m.group(1) if m else ''
                isarg=any(cp in at for at in atoms)
                if not isarg and re.search(r'[UDR]|NS',flags):
                    n362.append(name);break
        if ('--usepkg=n' in a or ('--usepkg' in a and a[a.index('--usepkg')+1:a.index('--usepkg')+2]==['n'])) and not '--with-bdeps' in s:
            n361.append(name)
    print(fn, 'cases',len(cases),'#362-candidates',len(n362),'#361-candidates',len(n361))
    for x in n362[:40]: print('  362:',x)
    for x in n361[:10]: print('  361:',x)
