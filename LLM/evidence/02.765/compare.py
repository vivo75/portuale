import re, sys
def load(p):
    d = {}
    for b in open(p).read().split('\n\n'):
        if not b.strip():
            continue
        l = b.split('\n')
        m = re.match(r'argv: \[(\w+)/(df|pg)\]( TRACE)? emerge\s+(?:--solver=pubgrub )?(.*)', l[0])
        rn, s, tr, a = m.groups()
        d[(rn, s, a, bool(tr))] = ('\n'.join(l[2:]), re.search(r'stdout=(\w+)', l[1]).group(1), re.search(r'note=(\d) jsonkey=(\d)', l[1]).groups())
    return d
b, a = load(sys.argv[1]), load(sys.argv[2])
assert b.keys() == a.keys()
chg = [k for k in b if b[k][1] != a[k][1]]
print("changed:", len(chg), "of", len(b), "; default-solver side changed:", [k for k in chg if k[1] == 'df'])
for k in chg:
    df = a.get((k[0], 'df', k[2], False))
    print(' ', k[0], k[2], '| note/jsonkey', b[k][2], '->', a[k][2], '| default has NOTE body equal:', df is not None and df[0] == a[k][0] and df[2] == a[k][2])
bad = []
for k in a:
    if k[1] == 'pg' and not k[3]:
        df = a[(k[0], 'df', k[2], False)]
        if a[k][2] != df[2] or a[k][0] != df[0]:
            bad.append((k, a[k][2], df[2]))
print("pg NOTE/jsonkey/body differing from the default after the change:", bad)
