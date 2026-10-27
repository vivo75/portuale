# emerge with one depgraph method neutralised (R5 S0 discriminating probe).
# PATCH=trigger  -> _slot_operator_trigger_reinstalls is a no-op
# PATCH=abi      -> _slot_conflict_backtrack_abi returns False
# PATCH=wantupd  -> wrap _add_pkg_deps? (unused)
import os, sys
from _emerge import depgraph as D
p = os.environ.get("PATCH", "")
if "trigger" in p:
    D.depgraph._slot_operator_trigger_reinstalls = lambda self: None
if "abi" in p:
    D.depgraph._slot_conflict_backtrack_abi = lambda self, pkg, slot_nodes, conflict_atoms: False
from _emerge.main import emerge_main
sys.argv[0] = "emerge"
sys.exit(emerge_main())
