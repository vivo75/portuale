"""Emulate rule (A) on a portuale PORTUALE_PROFILE=1 + PORTUALE_BT_TRACE=1 stderr.
Prints: printed-now, emulated, delta, and the per-segment sequence.
(A): the loop's last pass was a failed walk (driver Err arm) that did not advance `passes`
(empty stack or the cap) -> Portage counts that pass's predecessor too: +1.
Detection per resolve segment (a segment starts at `run_pass first=true`): the segment's last
collect_feedback event j is the post-loop best-run probe and the event before it is a no-BT pass
that is not the root, and j's incoming masks carry no mask-step entry (only `[binary]` or none)."""
import sys, re
def parse(path):
    segs=[]; cur=None; last=None
    for l in open(path, errors='replace'):
        if l.startswith('[profile] run_pass'):
            if 'first=true' in l: cur=[]; segs.append(cur)
            if cur is None: cur=[]; segs.append(cur)
            last={'bt':None,'masks':None}; cur.append(last); continue
        m=re.match(r'BT try=\d+ kind=(\S+)', l)
        if m and last is not None and last['bt'] is None: last['bt']=m.group(1); continue
        m=re.match(r'BT   masks-in (\[.*\])', l)
        if m and last is not None and last['masks'] is None: last['masks']=m.group(1)
    return segs
def terminal(masks):
    negs=re.findall(r'"([^"]*)"', masks or '[]')
    return all(n.endswith('[binary]') for n in negs)
def seg_delta(seg, printed=None, mx=None):
    """(A) last loop pass a no-BT failed walk -> +1; (C) last loop pass a Feedback whose children were
    all rejected (exhaustion right after it; not the cap break: printed < max) -> +1."""
    bts=[i for i,e in enumerate(seg) if e['bt']]
    if not bts: return 0
    j=bts[-1]
    if not terminal(seg[j]['masks']): return 0
    if j>=2 and seg[j-1]['bt'] is None: return 1
    if (j>=1 and seg[j-1]['bt'] and seg[j-1]['bt'].startswith('Feedback:') and seg[j]['bt']!='Settle'
            and printed is not None and mx is not None and printed < mx): return 1
    return 0
short={'Feedback:SlotConflict':'SC','Feedback:MissingDep':'MD','Feedback:Config':'CF','Feedback:RevDep':'RD','Feedback:Circular':'CI','Settle':'OK','DeadEnd':'DE'}
if __name__=='__main__':
    for f in sys.argv[1:]:
        segs=parse(f)
        txt=open(f.replace('.err','.out'),errors='replace').read()+open(f,errors='replace').read() if f.endswith('.err') else ''
        m=re.search(r'backtrack: (\d+)/(\d+)',txt); pr,mx=(int(m.group(1)),int(m.group(2))) if m else (None,None)
        d=[seg_delta(s,pr,mx) for s in segs]
        print(f, d, ' | '.join(' '.join(short.get(e['bt'],'X') if e['bt'] else 'X' for e in s) for s in segs))
