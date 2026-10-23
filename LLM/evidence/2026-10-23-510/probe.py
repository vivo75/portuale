"""#510 oracle: the atom constructor versus the dependency reducer.

Run with Portage's own interpreter, isolated from the working directory:
  python3 -I probe.py [<portage lib dir>]
The optional argument puts another Portage checkout first on sys.path (the
3.0.82.2 tag, extracted with `git archive`, has no compiled C parser).
Prints one line per (atom, eapi, entry point) so the two runs can be diffed.
"""

import sys

if len(sys.argv) > 1:
    sys.path.insert(0, sys.argv[1])

import portage
from portage.dep import Atom, use_reduce
from portage.exception import InvalidAtom

ATOMS = ["games-arcade/2048", "dev-util/timidity--", "cat/pkg[a(+),-a]"]
EAPIS = ["8", None]

print("VERSION %s %s" % (portage.VERSION, portage.__file__))
for s in ATOMS:
    for eapi in EAPIS:
        kw = {} if eapi is None else {"eapi": eapi}
        tag = "eapi=%s" % eapi
        try:
            a = Atom(s, **kw)
            use = a.use.tokens if a.use else None
            print("%s\t%s\tAtom\tOK\tcp=%s ver=%s use=%s" % (s, tag, a.cp, a.version, use))
        except InvalidAtom as e:
            print("%s\t%s\tAtom\tERR\t%s" % (s, tag, e))
        for label, extra in (("reduce-Atom", {"token_class": Atom}), ("reduce-str", {})):
            try:
                r = use_reduce(s, **kw, **extra)
                print("%s\t%s\t%s\tOK\t%s" % (s, tag, label, [str(x) for x in r]))
            except Exception as e:  # any reducer failure is a row
                print("%s\t%s\t%s\tEXC\t%s: %s" % (s, tag, label, type(e).__name__, e))
