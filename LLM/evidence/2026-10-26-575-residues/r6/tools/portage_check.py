"""Run host Portage (r6trace overlay) on every distinct args of the corpus rows tagged by the emulation,
on the scratch Portage root $S/fx (cfg $S/fx.cfg), env from the corpus row; compare to the emulated count."""
import json, lzma, os, re, subprocess, sys, concurrent.futures as cf
S='/var/tmp/claude-r6'; C='/home/vivo/repo/PORTUALE/pmtest/pytests-contract-suite/corpus/'
rows=[l.rstrip('\n').split('\t') for l in open(S+'/captures/corpus-impact.tsv')][1:]
cases={}
for f in ['contract.json.xz','expanded.json.xz']:
    d=json.load(lzma.open(C+f)); cases.update({(f,k):v for k,v in d['cases'].items()})
seen={}
for f,k,args,stored,now,emu_,tags,seq in rows:
    r=cases[(f,k)]; key=(tuple(r['args']), tuple(sorted(r['env'].items())))
    seen.setdefault(key, []).append((f,k,now,emu_,tags))
os.makedirs(S+'/pchk', exist_ok=True)
def run(item):
    (args,envt),lst=item
    env={a:v for a,v in os.environ.items() if a!='EMERGE_DEFAULT_OPTS'}
    for a,v in envt: env[a]=v.replace('<FIXTURES>',S+'/fx')
    env['PORTAGE_CONFIGROOT']=S+'/fx.cfg'; env['PORTAGE_TMPDIR']=S+'/ptmp'; env['PYTHONHASHSEED']='0'
    a2=[x.replace('<FIXTURES>',S+'/fx') for x in args]
    p=subprocess.run(['python3','-I',S+'/tools/r6trace.py','--ignore-default-opts',*a2],env=env,capture_output=True,stdin=subprocess.DEVNULL,timeout=900)
    txt=p.stdout.decode('utf-8','replace')+p.stderr.decode('utf-8','replace')
    fn=S+'/pchk/'+re.sub(r'[^A-Za-z0-9=._-]+','_',' '.join(args))[:150]+'.txt'; open(fn,'w').write(txt)
    m=re.search(r'backtrack: (\d+)/(\d+)',txt)
    return dict(args=args,env=dict(envt),portage=m.group(0) if m else None,rc=p.returncode,rows=lst,capture=fn)
with cf.ThreadPoolExecutor(6) as ex, open(S+'/captures/portage-check.jsonl','w') as o:
    for r in ex.map(run, list(seen.items())): o.write(json.dumps(r)+'\n'); o.flush()
