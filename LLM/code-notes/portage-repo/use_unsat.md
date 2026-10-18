# crates/portage-repo/src/use_unsat.rs
Reports for dependencies whose USE requirements no candidate meets.

## fn use_unsat_parent_row
- Portage `_show_unsatisfied_dep`'s parent-conditional row (`depgraph.py:6768-6858`): when a `[use]`-dep is `opt?`-conditional on the requirer and the child's violation is entirely conditional, Portage appends the *requirer* to `missing_use_reasons` with its own `Change USE:` flip, and the display shows it after the latest child row. The involved flags are Portage `violated_conditionals`' parent-side partition (the same narrowing `suggested_parent_use_candidate` uses, via `viable_parent_flip_targets`: only parent-active violated conditionals of a version-matching visible child that passes the untouchable-child `continue`), gated on the flip being settable and not newly violating the parent's own REQUIRED_USE (Portage's warning text is reproduced when it would).
- Merge-bound parents resolve through `parent_use_state`; installed ones (the live `qtdeclarative` row, the r135 fixture) through the vdb-backed twin.

## fn use_unsat_candidates_for_atom
- Portage `_show_unsatisfied_dep`'s "no ebuilds built with USE flags to satisfy" scan (`depgraph.py:6619` collects `missing_use`, `:6714-6873` turns it into `unmasked_use_reasons` / `unmasked_iuse_reasons`).
- `required` is Portage `use.required` -- every no-default flag, conditionals included (backlog #265(a)).
- `None` when the atom carries no use-deps or no visible candidate misses on USE -- Portage's plain masked / "no ebuilds to satisfy" paths own those.
- Display order mirrors Portage exactly: "Only show the latest version" (`:6876-6890`); all `Missing IUSE:` candidates print (`:6896-6907`).
- Documented narrowings (same family as `masked_candidates_for_atom`'s): Portage's `Missing IUSE:` fallback can be suppressed by a masked candidate that has the flag (`:6897-6907`).
- `db.match(atom.without_use)`: version/slot filter, USE and visibility ignored.
- `required_of`: Portage `atom.unevaluated_atom.use.required` -- `no_default`, i.e. every token *without* a `(+)`/`(-)` default marker, conditionals (`flag?`, `flag=`, `!flag?`, …) included, in declaration order here (`lib/portage/dep/__init__.py:1363` `self.required = frozenset(no_default)`; backlog #265(a)). A default-marked token is not a `required` flag in Portage's parser (it has a fallback when missing from IUSE).
- `enabled`/`disabled`: Portage `atom.use.enabled` / `.disabled` -- the evaluated atom's own unconditional deps.
- Rows: Portage `unmasked_use_reasons` / `unmasked_iuse_reasons` (masked instances never reach them); `pkg.iuse.get_missing_iuse(required_flags)`; `need_enable`/`need_disable` (`:6729-6730`); the untouchable-child gate (`:6732-6736`, also staying out of `missing_use_adjustable`); "Only show the latest version" (`:6877-6887`).
- #346 S2: the `is_stable` input is the effective KEYWORDS (`effective_keywords`), matching Portage `isStable` over `getKeywords`.
