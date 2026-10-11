# crates/portage-repo/src/tests/pass_helpers.rs
Classification-level pins for onlydeps IDEPEND handling and `_minimize_children` collapsing.

## fn onlydeps_ideps_atom_shared_with_buildtime_key_is_not_buildtime_hard
- Backlog #194 (review round 1): under `--onlydeps --onlydeps-with-rdeps=n --onlydeps-with-ideps=y` on an EAPI-8 root, Portage keeps walking `IDEPEND` (depgraph.py:4186-4193; oracle cell `[B, E, F]`), so an atom present in both a build-time key and `IDEPEND` is a runtime edge, not `buildtime_hard` (Portage `DepPriority`: an unbreakable merge-order edge).
- Pinned at the classification level: the key selection still carries `IDEPEND`, and one shared atom enqueued with those sets lands soft.

## fn eapi7_cache_entry_carrying_idepend_is_gated_at_read_time
- Backlog #194 (review round 1): Portage's `egencache` could never emit such an entry (Portage's `bin/ebuild.sh` `depend` phase unsets `IDEPEND` below EAPI 8), so no fixture pins this path -- the adversarial entry is fed to the gate directly.

## fn minimize_children_collapses_like_portage
- Backlog #236 B: Portage `_minimize_children` (_emerge/depgraph.py:4751-4854) on its two observed shapes.
- Shape (1), an installed parent's dynamic deps (the r25 shape, Portage's own probe in `LLM/evidence/2026-09-29-236/playground/r25-debug.log`): the live `minlib:=` picks the tree's `2.0` (`0/2`), the appended built `>=minlib-1.0:0/1=` the installed `1.0`; installed-first elimination drops `2.0` and both atoms bind the installed instance.
- Shape (2), an ebuild parent's version range (the fixture's `libgit2-glib`, `libgit2-glib.log` there): `<minlib-3` picks `2.0`, `>=minlib-1.5` the tree's `3.0`; ascending elimination keeps `2.0` (the only match of `<3`) and drops `3.0`.
- A single selection, or atoms whose selections match nothing else, pass through unchanged.
- The (3) arm is backlog #107's mesa shape: both atoms carry use-deps, the installed instance's recorded `USE` satisfies `[abi]`, so Portage's `findAtomForPackage(pkg, modified_use=_pkg_use_enabled(pkg))` matches it and the tree pick is eliminated.
- The disjoint arm: each atom matches only its own pick (`foo` is in no candidate's IUSE and not in the installed instance's IUSE either).

## fn minimize_children_matches_use_deps_against_candidate_use
- Backlog #266: Portage `findAtomForPackage(pkg, modified_use=self._pkg_use_enabled(pkg))` evaluates a use-dep atom against a not-yet-built package's own candidate effective USE. Grounded on Portage 3.0.82.2's ResolverPlayground (`LLM/evidence/2026-10-01-266/r266_pg.py`).
- `>=minlib2-1[bar]` alongside `<minlib2-3` collapses onto the lower pick when that pick builds `bar`, and stays split when it does not.
