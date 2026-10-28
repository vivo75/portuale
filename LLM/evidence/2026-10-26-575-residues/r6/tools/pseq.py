import sys,re
# portuale pass sequence from PORTUALE_PROFILE=1 + PORTUALE_BT_TRACE=1 stderr:
# each run_pass span is one pass; the BT try line after it (if any) gives collect_feedback's kind.
# A pass with no BT line = a walk that returned Err (or a post-loop re-run that skips collect_feedback).
for f in sys.argv[1:]:
    ev=[]
    for l in open(f, errors='replace'):
        if l.startswith('[profile] run_pass'): ev.append(['P', 'first' if 'first=true' in l else ''])
        m=re.match(r'BT try=\d+ kind=(\S+)', l)
        if m and ev and ev[-1][0]=='P' and len(ev[-1])==2: ev[-1].append(m.group(1))
        elif m: ev.append(['?', '', m.group(1)])
    short={'Feedback:SlotConflict':'SC','Feedback:MissingDep':'MD','Feedback:Config':'CF','Feedback:RevDep':'RD','Feedback:Circular':'CI','Settle':'OK','DeadEnd':'DE'}
    print(' '.join(short.get(e[2], e[2]) if len(e)>2 else 'ERR' for e in ev))
