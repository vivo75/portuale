# crates/portage-repo/src/resolve/feedback/no_leaf_restart.rs
General no-leaf restart arm for multi-node hard cycles the merge-order walk strands on.

## fn step_no_leaf_restart
- Backlog #221: Portage `_serialize_tasks`' no-leaf restart (`_emerge/depgraph.py:10264-10296`, bugs 703440, 705986). A pass that settled everywhere else but stranded the merge-order walk on a dependency cycle records the cycle's edges into `circular_dependency` and re-runs the whole walk, on which `disjunction_preference` demotes the cycle-closing `||` branches (see its `circular_atom` site).
- Placed after every other feedback: Portage only reaches serialization once graph creation (slot conflicts, missing deps, autounmask growth, reverse-dep pins, the slot-operator trigger) settled. Gated on `--backtrack=0` (Portage's `_allow_backtracking` -- the `--backtrack=0` run reports the circular error exactly as before), on no unsatisfiable dependency (Portage never serializes a failed `_create_graph`), and on no surviving slot conflict or orphan blocker (Portage backtracks those first; a conflict-only fall-through still settles below).
- `unsolved_cycle`: a cycle re-using an already-recorded node means the demotion did not break it -- settle into the circular report instead of retrying, exactly like Portage's `_skip_restart`. Every retry therefore strictly grows the map, so the search terminates.
- `Circular` feedback is budget-free (the driver treats it like `Config`: no mask step, and under `--backtrack=0` the pass settles into the circular report) -- shared with backlog #216's fresh-self-loop arm above, which emits the same variant.
- Track X Slice C (#242): node-identity keys (Portage's package-object keying) -- a both-roots cp reusing an already-recorded node in the *other* root is a new edge, not an unsolved cycle.
- Portage's rotation: `circular_child = cycle[-1]` for index 0, `cycle[index - 1]` otherwise -- each node's in-cycle dependency.
- Merge-bound by construction (both trigger arms only link merge-bound entries), so the version is never empty in practice; skip belt-and-braces (M1): an empty version would build a `cat/pkg-` candidate no atom matches.
