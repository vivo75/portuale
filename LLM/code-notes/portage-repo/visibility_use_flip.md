# crates/portage-repo/src/visibility_use_flip.rs
USE-flip suggestions for autounmask.

## fn flag_is_settable
- Portage `pkg.use.mask`/`pkg.use.force` (global `use.mask`/`use.force` folded in) always override `package.use` regardless of what it says (Portage `_wrapped_select_pkg_highest_available_imp`'s `can_adjust_use` hard block). Rather than re-deriving the matching logic separately (a second copy of what `effective_use_flags` already does), this recomputes with a synthetic maximally-specific entry.
- The synthetic atom is a plain `=category/package-version` atom -- not `candidate_str` itself, which carries a `:slot/subslot::repo` suffix `match_from_list` can't parse back as an atom pattern (it expects an atom on the left, a candidate string on the right, not a fully-qualified candidate string used as both).
- The synthetic entry stands in for the hypothetical autounmask change itself.

## fn suggested_use_flip
- Portage "flag not in IUSE" unfixability -- no `package.use` entry could address it. Portage's "only suggest a change that would actually fix it" spirit; Portage `can_adjust_use`'s whole-atom rejection.
- Conditional forms (`flag?`/`!flag?`/`flag=`/`!flag=`) are evaluated away into concrete Enabled/Disabled (or dropped) before an atom is ever queued -- see `enqueue_flat_deps`'s doc. Portage's separate opt?-driven suggestion mechanism (a parent's own flag, not the candidate's) is a different code path entirely (this module's parent-side mechanism below).
- A `(+)`/`(-)` default stands in for a missing flag's state (Portage `_use_dep`'s "missing" handling, same as `use_deps_satisfied`): `[flag(+)]` is already satisfied when the requirement is "enabled", `[flag(-)]` when "disabled". Only a genuinely unsatisfiable missing flag (no default, or a contradicting one) makes the whole atom unfixable.

## fn suggested_use_candidate
- `None` when `use_deps` is empty/absent (nothing to suggest fixing), `category/package` isn't listable at all, or no candidate qualifies. Shared by the same two call sites as `suggested_keyword_candidate` (a top-level atom's fatal `NoVisibleCandidate` and a dependency's own; see `GraphEntry::use_suggestion`'s doc).

## fn implicit_iuse_set
- Portage `config.py::_get_implicit_iuse()`: the package's literal `IUSE` alone is not Portage `pkg.iuse.is_valid_flag`'s full domain. Without the fold, a `REQUIRED_USE` (or conditional use-dep) referencing an implicit flag never mentioned in IUSE (e.g. Portage `media-libs/mesa`'s `REQUIRED_USE` referencing `"x86"`) spuriously reports "not in IUSE" -- confirmed live against the installed system Portage. See `portage_profile::Config::archlist`'s doc for the full grounding.
- Portage EAPI 5+ `check_required_use` is called with `pkg.iuse.is_valid_flag`, which for EAPI 5+ is `explicit union IUSE_EFFECTIVE` -- so a `REQUIRED_USE` referencing an `elibc_*`/`kernel_*`/... implicit flag (via `USE_EXPAND_IMPLICIT`, `Config::iuse_effective`) is recognized the same way one referencing `x86` (via `archlist`) is. `valid_iuse` (used for `use_deps_satisfied`) is the narrower `declared union iuse_effective` subset of this; this pragmatic superset stays for the `REQUIRED_USE`/parent-USE-state path.

## fn parent_use_state
- `None` for `AlreadyInstalled`/`NoVisibleCandidate` outcomes -- moot anyway, since `enqueue_dependencies`'s `AlreadyInstalled` recursion path never conditional-evaluates deps at all, so `--autounmask-use`'s `opt?` mechanism never triggers for it.

## fn installed_parent_use_state
- Portage `_show_unsatisfied_dep`'s parent-conditional row for an installed requesting parent (`depgraph.py:6768-6858` reads `self._pkg_use_enabled(myparent)` and `myparent.use.mask/.force` with no installed/merge-bound distinction -- the live `qtdeclarative` row is `[installed]`). `parent_use_state` covers merge-bound parents only (its version lookup has no `AlreadyInstalled` arm). The abort block is the only consumer.
- The fixture vdb carries no `REQUIRED_USE` file; same-version ebuild metadata is what a Portage vdb entry built from that ebuild records. Without a same-version candidate neither mask/force nor the row cpv can be grounded -- Portage would still print the row from the instance, a documented narrowing.

## fn violated_parent_flags
- Portage `Atom.violated_conditionals(child_use, child_is_valid_flag, parent_use)` (`lib/portage/dep/__init__.py:1465`), narrowed to the parent-conditional arm (`lib/_emerge/depgraph.py:6779-6798`).
- Portage `Package.py:728-731` filters `pkg.use.enabled` to valid IUSE, so a profile flag the child never declares (the fixture profile's `foo` on IUSE-less `aup0d`) never counts as set.
- Per-operator truth table (`P` = parent-has, `C` = child-has, `V` = child-IUSE-valid, `E` = `(+)`-defaulted, `D` = `(-)`-defaulted; oracle `LLM/evidence/2026-09-28-g195/g195d-oracle-output.txt`, from `g195d-oracle-probe.py` over Portage 3.0.82.2; pinned cell-by-cell by `tests_195d`):
  - `flag?` (`IfParentEnabled`, `:1525-1531`): violated iff the parent has the flag, the child lacks it, and the flag is valid or `(-)`-assumed -- an invalid `(+)` flag is satisfied there (its default already assumes it enabled, so the parent having it on changes nothing).
  - `flag=` (`EqualParent`, `:1532-1549`): parent-has-child-lacks violates for a valid or `(-)` flag; parent-lacks-child-lacks violates only for an invalid `(+)` flag (the `missing_enabled` sub-branch -- a flip off is already in effect); parent-lacks-child-has always violates.
  - `flag!=` (`OppositeParent`, `:1550-1567`): mirror image -- parent-lacks-child-lacks violates for a valid or `(-)` flag; parent-has-child-lacks violates only for an invalid `(+)` flag; parent-has-child-has always violates.
  - `!flag?` (`IfParentDisabled`, `:1568-1576`): parent-has is always satisfied; otherwise child-has always violates, and parent-lacks-child-lacks violates only for an invalid `(+)` flag.
- Concrete gate: Portage's violated atom carries no `enabled`/`disabled` for a parent-lacks-child-has `flag=` (or either `flag!=` match direction -- they land in `conditional`, never the concrete sets). Portage `:1511-1524`: `flag` violates iff the child lacks it and the flag is valid or `(-)`-assumed; `-flag` violates iff the child has it, or (lacking it) the flag is invalid-`(+)`-assumed. Defaultless-invalid shapes never reach this gate: `required` poisons above, as Portage's `missing_iuse` branch does.
- Narrowings: in isolation Portage's function would report an invalid-undefaulted conditional in `conditional`; the `tests_195d` oracle table's `arm` column shows the composed outcome. The unconditional-`required` subset must sit in child IUSE for the same reason (Portage's `missing_iuse` branch -- `:6725-6727`).

## fn viable_parent_flip_targets
- Portage `_show_unsatisfied_dep`'s per-`missing_use`-package loop (`lib/_emerge/depgraph.py:6716-6858`), descending-cpv order: `db.match(atom.without_use)` plus the `masked_pkg_instances` skip (`:6775`). Mirrors Portage's return-True-on-first-collect.
- Untouchable-child `continue` (`:6732-6736`): no child flip -- and no parent flip either -- can fix it.
- `need_enable`/`need_disable` (`:6729-6730`).

## fn suggested_parent_use_candidate
- Portage `--autounmask-use`'s second, architecturally distinct mechanism (Portage `_show_unsatisfied_dep`, `lib/_emerge/depgraph.py:6756-6846`).
- Only conditionals the parent's current USE actually activates (e.g. `foo(-)?` with parent `foo` on; a `bar(-)?` the parent already has off is satisfied, not violated), never every conditional flag the atom references.
- Gated on `flag_is_settable` (reused as-is -- its logic assumes nothing child-specific). Mirrors Portage `_show_unsatisfied_dep`'s `collect_use_changes and not required_use_warning` gate.
- Candidates tried in descending version order; first viable wins (Portage returns True on the first successful collect).
- Attached to the dependency's own `GraphEntry` rather than the parent's entry, unlike Portage's `missing_use_reasons.append((myparent, ...))` -- a pragmatic simplification: portuale's `GraphEntry` model has no per-parent "reasons" list, and the dependency's entry is where the "no visible ebuild for dependency" note already lives.
