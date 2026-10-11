# crates/portage-repo/src/tests/unsat_probe.rs
Dead-end pass seeding a replacement parent for an unsatisfiable built `:=` dep.

## fn slot_operator_unsatisfied_probe_seeds_a_replacement_parent
- Backlog #215 (v2 `#24f`): Portage's `_slot_operator_unsatisfied_probe` (`_emerge/depgraph.py:2817`, fired at `:3447-3458`).
- The replacement parent must be same slot, visible, unmasked per instance like Portage's `_iter_similar_available` (`:3036-3040`).
- Guards: only an owner whose own recorded want is the built atom seeds (Portage probes one dep edge, so a plain-atom co-owner never does); the atom must be built-form (a plain `:=` never qualifies); the owner must be installed; an already masked installed instance stays dead (else the restart would loop, Portage `:3447-3448`); an `--exclude`d owner refuses (Portage `:2818-2824`); with no (or a different-slot, or a fully masked) replacement the pass stays a dead end.
- The dead-end pass pulls the installed parent's own built want (`>=app-misc/unsatprov-1:0/1=`).
- The scratch repo's requested parent ebuilds each carry the satisfiable live `:=`; `unsatprov-2` is slot 2/2.
- The installed instance itself stays probed: masking only the v2 candidate still seeds through v1.
- #257: a non-installed built (binary) parent. No vdb owner, the parent is a merge-bound binary graph entry; Portage's `_slot_operator_unsatisfied_backtrack` masks it (`slot_operator_mask_built`, `_emerge/depgraph.py:2896-2903`) instead of seeding a reinstall.
- Already masked as a binary: Portage's `dep.parent not in _runtime_pkg_mask` gate (`:3447-3448`) stops the loop.
