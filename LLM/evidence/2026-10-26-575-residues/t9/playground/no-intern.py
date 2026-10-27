"""S0b counterfactual: run Portage 3.0.83 with Atom interning switched off
(portage/dep/__init__.py:130/992 `_intern_atom`, :1897-1949 `_atom_ctor_cache`),
to test whether the =5.42* skip rows of the 5.44-first installed virtuals come
from object identity at depgraph.py:6122-6133/6164-6167. Not Portage's real
behaviour -- a discriminating probe only."""
import sys, portage.dep as pd
pd._intern_atom = lambda atom: atom
pd._atom_ctor_key = lambda *a, **k: None
from _emerge.main import emerge_main
sys.exit(emerge_main(sys.argv[1:]))
