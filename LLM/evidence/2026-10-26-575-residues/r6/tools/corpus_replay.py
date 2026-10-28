"""Replay every corpus row (contract + expanded) whose stored output has `backtrack: N/M` and whose
env is the plain fixtures root, against a seeded scratch copy ($S/cfx), with PORTUALE_BT_TRACE=1
PORTUALE_PROFILE=1; apply emu.seg_delta (rule A) to the last resolve segment; flag
--autounmask-backtrack=y rows for rule B. Output: captures/corpus-replay.jsonl"""
import lzma, json, re, os, sys, subprocess, concurrent.futures as cf
sys.path.insert(0, '/var/tmp/claude-r6/tools'); import emu
S='/var/tmp/claude-r6'; FX=S+'/cfx'; BIN='/home/vivo/repo/PORTUALE/portuale/target/release/portuale'
C='/home/vivo/repo/PORTUALE/pmtest/pytests-contract-suite/corpus/'
os.makedirs(S+'/ctr', exist_ok=True)
jobs=[]
for f in ['contract.json.xz','expanded.json.xz']:
    d=json.load(lzma.open(C+f)); B=d['blobs']
    for k,r in d['cases'].items():
        txt=B.get(r['stdout'],'')+B.get(r['stderr'],'')
        m=re.search(r'backtrack: (\d+)/(\d+)',txt)
        if not m or r['env'].get('ROOT')!='<FIXTURES>' or r['env'].get('PORTAGE_CONFIGROOT')!='<FIXTURES>': continue
        if any('<TMP>' in a or '<REPO>' in a for a in r['args']) or any('<TMP>' in v for v in r['env'].values()): continue
        pret=any(a=='--pretend' or re.match(r'^-[a-zA-Z]*p[a-zA-Z]*$',a) for a in r['args'])
        if not pret: continue
        jobs.append((f,k,r['args'],r['env'],m.group(0)))
print(len(jobs), file=sys.stderr)
def run(j):
    f,k,args,env0,stored=j
    env={k2:v for k2,v in os.environ.items() if k2!='EMERGE_DEFAULT_OPTS'}
    env.update({a:v.replace('<FIXTURES>',FX) for a,v in env0.items()})
    env.update(PORTUALE_BT_TRACE='1', PORTUALE_PROFILE='1', PORTAGE_TMPDIR=S+'/ptmp')
    p=subprocess.run(['emerge',*[a.replace('<FIXTURES>',FX) for a in args]], executable=BIN, env=env,
                     capture_output=True, stdin=subprocess.DEVNULL, timeout=600)
    out=p.stdout.decode('utf-8','replace'); err=p.stderr.decode('utf-8','replace')
    m=re.search(r'backtrack: (\d+)/(\d+)', out+err)
    tf=f"{S}/ctr/{abs(hash((f,k)))}.err"; open(tf,'w').write(err)
    segs=emu.parse(tf); d=emu.seg_delta(segs[-1]) if segs else 0
    seq=' | '.join(' '.join(emu.short.get(e['bt'],'X') if e['bt'] else 'X' for e in s) for s in segs)
    return dict(file=f,key=k,args=args,stored=stored,now=m.group(0) if m else None,rc=p.returncode,deltaA=d,
                aby=any(a.startswith('--autounmask-backtrack=y') or a=='--autounmask-backtrack' or a=='--autounmask-continue' for a in args),
                cf=sum(1 for s in segs for e in s if e['bt']=='Feedback:Config'), seq=seq, trace=tf)
with cf.ThreadPoolExecutor(12) as ex, open(S+'/captures/corpus-replay.jsonl','w') as o:
    for r in ex.map(run, jobs):
        o.write(json.dumps(r)+'\n')
