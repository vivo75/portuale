"""usage: compare.py OUTDIR [--roots DIR] [ROOT...]   |   compare.py --plans RESOLVER.json FAST.json
Facts from a probe.sh run: per root, per side (fast, df = default solver, pg = pubgrub), the normalised
  refusals  {(consumer cp, atom, provider)}  from "stale-bindings"  (resolver cpv reduced with split_cpv; fast provider may be null)
  reinstalls  the cps whose entries say outcome "reinstall", RESTRICTED to the installed packages whose VDB entry records a
              slot-operator binding with a sub-slot (review D3: the fast plan answers -uD, the resolver -uDN, so unrelated
              --newuse reinstalls are not "stale consumers")
  provider versions  the final version each side gives every provider cp a recorded binding names (G3: a version-choice
              difference is reported before, and apart from, a rule difference)
Comparison key (review G2): the consumer cp. Atom and provider are compared per common consumer and reported separately;
a null provider compares equal to nothing.
"""
import json, os, re, sys

def split_cpv(cpv):
    m = re.match(r'^(.*)-(\d.*)$', cpv)
    return m.group(1), m.group(2)

def side(outdir, rn, label):
    p = f'{outdir}/{rn}.{label}.out'
    rc = int(open(f'{outdir}/{rn}.{label}.rc').read())
    try:
        doc = json.loads(open(p).read().strip().splitlines()[-1]) if rc == 0 else None
    except Exception:
        doc = None
    return rc, doc

def recorded_bindings(root):
    """cp -> set of provider cps it records a `:slot/sub=` binding on (any dependency key), from the VDB."""
    out = {}
    base = f'{root}/var/db/pkg'
    for cat in sorted(os.listdir(base)):
        for pf in sorted(os.listdir(f'{base}/{cat}')):
            provs = set()
            for k in ('DEPEND', 'RDEPEND', 'PDEPEND', 'BDEPEND', 'IDEPEND'):
                f = f'{base}/{cat}/{pf}/{k}'
                if os.path.exists(f):
                    for m in re.finditer(r'([\w+-]+/[\w+.-]+):[^\s/]+/[^\s=]+=', open(f).read()):
                        provs.add(re.sub(r'-\d[\w.]*$', '', m.group(1)))
            if provs:
                out[f'{cat}/{split_cpv(pf)[0].split("/")[-1]}'] = provs
    return out

def installed_versions(root):
    base = f'{root}/var/db/pkg'
    return {f'{cat}/{split_cpv(pf)[0]}': split_cpv(pf)[1] for cat in os.listdir(base) for pf in os.listdir(f'{base}/{cat}')}

def normalise(doc, kind, rec):
    refusals = set()
    for r in (doc or {}).get('stale-bindings', []):
        c = r['consumer']
        cp = split_cpv(c)[0] if kind != 'fast' else c
        prov = r.get('provider')
        refusals.add((cp, r['atom'], prov))
    reinst = {f"{e['category']}/{e['package']}" for e in (doc or {}).get('entries', []) if e.get('outcome') == 'reinstall'}
    final = {f"{e['category']}/{e['package']}": e['version'] for e in (doc or {}).get('entries', [])}
    return refusals, (reinst if rec is None else reinst & set(rec)), final

def facts(outdir, rn, roots):
    rec = recorded_bindings(f'{roots}/{rn}')
    provs = {p for s in rec.values() for p in s}
    have = installed_versions(f'{roots}/{rn}')
    res = {}
    for kind in ('fast', 'df', 'pg'):
        rc, doc = side(outdir, rn, kind)
        r, i, f = normalise(doc, kind, rec)
        res[kind] = dict(rc=rc, refusals=r, reinstalls=i, versions={p: f.get(p, have.get(p)) for p in sorted(provs)},
                         pg=len(re.findall(r'^pg-', open(f'{outdir}/{rn}.{kind}.err').read(), re.M)))
    return res

def plans(resolver_json, fast_json):
    """Real-tree mode: two plan documents, no VDB restriction (every reinstall counts, so the resolver's --newuse ones show)."""
    docs = {}
    for k, p in (('resolver', resolver_json), ('fast', fast_json)):
        text = [l for l in open(p).read().splitlines() if l.startswith('{')]
        docs[k] = json.loads(text[-1])
    f = normalise(docs['fast'], 'fast', None)
    r = normalise(docs['resolver'], 'df', None)
    print(f'fast refusals {sorted(f[0], key=str)}'); print(f'resolver refusals {sorted(r[0], key=str)}')
    print(f'consumers only fast {sorted({c for c, _, _ in f[0]} - {c for c, _, _ in r[0]})}; '
          f'only resolver {sorted({c for c, _, _ in r[0]} - {c for c, _, _ in f[0]})}')

def main():
    a = sys.argv[1:]
    if a and a[0] == '--plans':
        return plans(a[1], a[2])
    outdir = a.pop(0)
    roots = '/var/tmp/766roots'
    if a and a[0] == '--roots':
        roots = a[1]; a = a[2:]
    names = a or sorted({f.split('.')[0] for f in os.listdir(outdir) if f.endswith('.rc')})
    for rn in names:
        f = facts(outdir, rn, roots)
        have = installed_versions(f'{roots}/{rn}')
        print(f'== {rn}')
        for k in ('fast', 'df', 'pg'):
            s = f[k]
            print(f'  {k}: rc={s["rc"]} pg_lines={s["pg"]}')
            print(f'    provider versions {dict((p, v) for p, v in s["versions"].items() if v != have.get(p))}')
            print(f'    refusals {sorted(s["refusals"], key=str)}')
            print(f'    reinstalls {sorted(s["reinstalls"])}')
        fd, dd, pg = f['fast'], f['df'], f['pg']
        vdiff = {p for p in fd['versions'] if fd['versions'][p] != dd['versions'].get(p)}
        print(f'  provider-version differences fast vs df: {sorted(vdiff)}')
        fc = {c for c, _, _ in fd['refusals']}; dc = {c for c, _, _ in dd['refusals']}
        print(f'  consumers only fast {sorted(fc - dc)}; only resolver {sorted(dc - fc)}; both {sorted(fc & dc)}')
        for c in sorted(fc & dc):
            fa = sorted(x for x in fd['refusals'] if x[0] == c); da = sorted(x for x in dd['refusals'] if x[0] == c)
            print(f'    {c}: atom/provider same={fa == da}  fast={fa} resolver={da}')
        print(f'  reinstalls only fast {sorted(fd["reinstalls"] - dd["reinstalls"])}; only resolver {sorted(dd["reinstalls"] - fd["reinstalls"])}; both {sorted(fd["reinstalls"] & dd["reinstalls"])}')
        print(f'  df == pg: refusals {dd["refusals"] == pg["refusals"]} reinstalls {dd["reinstalls"] == pg["reinstalls"]}')

if __name__ == '__main__':
    main()
