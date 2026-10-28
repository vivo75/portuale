import sys,re
# compact Portage arm sequence from an R6 trace: R<k>=restart (k = unexplored left), F+=failed counted, F0=failed empty, B:<why>=break
for f in sys.argv[1:]:
    out=[]
    for l in open(f, errors='replace'):
        if not l.startswith('R6 '): continue
        if 'ARM restart' in l: out.append('R'+re.search(r'feedback=(\d+)',l).group(1))
        elif 'unexplored nonempty' in l: out.append('F+')
        elif 'unexplored EMPTY' in l: out.append('F0')
        elif 'ARM break' in l:
            why=[k for k in ('success=True','need_config_change=True','allow_backtracking=False','limit=True') if k in l]
            out.append('B:'+(why[0].split('=')[0] if why else '?'))
        elif 'BEST-RUN' in l: out.append('+best')
        elif 'BREAKAGE' in l: out.append('+brk')
    print(' '.join(out))
