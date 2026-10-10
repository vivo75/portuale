# crates/portage-use-reduce/src/lib.rs
Port of Portage's `portage.dep.use_reduce`, restricted to `flat=True` mode.

## module
- Origin: the depgraph/config-resolution slice after atom matching (see LLM/agent-context.md).
- Unlike the atom port, NOT a narrowed grammar: flat mode's tokenizer and bracket/use-conditional handling is fully self-contained in Portage. Out-of-scope optional parameters the harness never exercises: `masklist`, `excludeall` (rare flag-override sets), `is_src_uri`/the `->` SRC_URI arrow token (fetch-restriction syntax), `opconvert` (the operator-into-argument-list shape, which nothing in portuale needs). `token_class`/`is_valid_flag` stay out because tokens stay opaque strings, matching how config.py itself calls use_reduce for RESTRICT/PROPERTIES/IUSE-like values.
- `flat=True` is a heavily-used Portage invocation mode: `lib/portage/package/ebuild/config.py`, `_emerge/resolver/`, and `_emirrordist` all call `use_reduce(..., flat=True)`.
- `subset` is the `--with-test-deps` follow-up: `select_subset` needs the full nested `paren_reduce` structure (not `flat=True`'s eagerly-flattened one) to know where a `"||"` group's boundaries are before filtering.
- `use_reduce_structured` is the `--changed-deps` structured-comparison follow-up: ports `flat=False`/`opconvert=False` (`_use_reduce_cached`), every redundant-bracket optimization (`is_single`/`special_append`/`ends_in_any_of_dep`/`last_any_of_operator_level`).
- `use_reduce_flat_disjunctive` is the `"||"`-group resolution follow-up: portuale's cut of Portage `dep_zapdeps`'s `choice_bins`, enough to make `|| ( wine-vanilla wine-staging ... )` pick the installed `wine-staging` rather than the first-listed one. The finer bins (`in_graph`/`any_slot`/`unsat_use_*`/`other_*`) and full backtracking still aren't ported.
- Tokens stay opaque strings throughout by architecture; portage-repo supplies the installed/visibility-checking closure.

## enum MatchMode
- `Normal`: conditionals active iff the flag is in `uselist` (negated for `!flag?`). `All` = Portage's `matchall=True`; `None` = Portage's `matchnone=True`.

## enum Error
- Distinct variants mirror Portage's messages; the contract suite pins these strings.

## fn is_active
- Exported for portage-repo's LICENSE-grammar parser (PMS 7.3.2): it needs the `||`-group structure `use_reduce_flat` deliberately discards (hence a separate parser), but its USE-conditional semantics are identical, so this piece is shared rather than reimplemented.

## fn use_reduce_flat
- `tokens` is equivalent to Python's `depstr.split()`, Portage's own first step. `"("` always satisfies a pending `"||"`/`"flag?"` requirement regardless of `need_bracket`. `is_src_uri` is always false here (SRC_URI arrows out of scope), matching Portage with `is_src_uri=False`.

## enum DepNode
- `['foobar', 'foo?', ['bar', 'baz']]` shape. No redundant-bracket-collapsing at parse time: representation-only, and `select_subset` (the only consumer) doesn't care.

## fn select_subset
- `disjunction` members each become their own list in the result rather than spliced flat, mirroring Portage's "one result entry per alternative" shape. `selected` inherits into every descendant, so a plain atom nested two levels under `test?` is still kept. `subset` only decides which already-active branch's atoms reach the output, never which branches are active.
- A bare `"( ... )"` grouping follows Portage `select_subset`'s `AttributeError` ("token has no `.endswith`") branch.

## fn serialize_dep_tree
- This crate's `paren_enclose` equivalent.

## const EMPTY_ANY_OF
- Kept verbatim so structured comparison against Portage agrees. Also the `eapi=None` default Portage's `dep_check` uses for installed packages.

## fn ends_in_any_of_dep / last_any_of_operator_level / special_append
- Ported as-is from Portage. `last_any_of_operator_level`: a non-operator token breaks the chain first (returns -1). `special_append`: `l == ["||", Group]` splices the group's contents.

## fn use_reduce_structured
- Stays atom-agnostic: no `token_class=Atom` conversion, no `evaluate_conditionals` on atoms' USE-deps; the caller does both as a per-atom post-pass. Output shape matches `serialize_dep_tree` (also used by the subset/disjunctive paths).

## fn use_reduce_flat_subset
- Grounded against Portage's `--with-test-deps` call site in `depgraph.py`: `use_reduce(dep_string, uselist=use_enabled | {"test"}, ..., subset={"test"})`.
- Pipeline: `build_dep_tree` (= `paren_reduce`) -> `select_subset` -> `serialize_dep_tree` (= `paren_enclose`) -> unmodified `use_reduce_flat`.

## fn next_alternative
- Portage's dependency-specification grammar allows a bare conditional directly as a `"||"` alternative, not just atoms/groups; hence the `flag?` marker plus its `Group` are consumed as one unit.

## enum AltPreference
- Port of `dep_zapdeps`'s `choice_bins` (`lib/portage/dep/dep_check.py`), bins in `dep_check.py` soft lines 392-402, where `preferred_in_graph` / `preferred_installed` / `preferred_any_slot` alias one list. Selection loop in soft lines 812-816 (quoted verbatim in the code).
- Bins 5-8 (`other_*`): some atom only available via a masked/force path; Portage still returns them on the `allow_masked` second pass. Ported as of slice 2/4 of backlog #22 (`docs/history/022-agent-task-22-zapdeps.fable.md`).
- `Unsatisfiable` has no Portage equivalent: Portage's classification always lands an atom in some bin; only portuale's circular-self-dep guard produces it. See `portage-repo::disjunction_preference`.
- `OtherInstalledAnySlot`: bug 522652's fuzzy match. `OtherInstalled`: typically masked or keyword-rejected (`atom.without_use` tree match fails). `Available` is what makes `|| ( foo[a] foo[b] )` prefer the USE-satisfiable `foo[b]`. `Installed` is what makes `virtual/wine` pick installed `wine-staging` instead of failing REQUIRED_USE on first-listed `wine-vanilla`; once found, `resolve_disjunctions` breaks early (pre-slice-2 it broke early on the first best-rank hit, making tie-breaks unreachable).
- `tie_break` design rule 2 (`docs/history/022-agent-task-22-zapdeps.fable.md`): portage-use-reduce stays atom-agnostic and hands the caller the tied alternatives' flattened atom lists; the caller owns candidate lookup, `cp`/version parsing and `vercmp`. In-bin ordering: prefer an upgrade via `vercmp` over the intersecting `cp_map`, or `all_installed_slots`, or `all_in_graph`, unless sacrificing an upgrade (`dep_check.py` soft 738-802). A trivial `&mut |_| 0` reproduces pre-slice-2 behaviour (portage-repo's pre-slice-2 shim at its two `--root-deps` call sites).

## fn use_reduce_flat_disjunctive / type TieBreak
- Portage `_add_pkg_dep_string`'s `"||"` resolution. The never-silently-wrong invariant: a dependency portuale can't currently resolve is never silently dropped (established by `resolve_pretend_graph`, portage-repo, for the unconditional-flatten v1 this replaces).
- EAPI detail: `|| ( )` drops when `empty_groups_always_true` (EAPI <= 6; `_get_eapi_attrs(eapi).empty_groups_always_true`) and becomes the placeholder otherwise (EAPI 7+, `dep/__init__.py:864-869`, including the `eapi=None` default for installed packages); the placeholder fails downstream lookup like Portage's.

## fn resolve_disjunctions
- Ordering mirrors Portage `_create_graph` draining the plain `dep_stack` before `_dep_disjunctive_stack` (`depgraph.py:3257-3268`): single flatten can't interleave two stacks, but emits plain results first, then `||`-chosen ones.
- Backlog #238: Portage `use_reduce` drops an alternative an inactive conditional emptied before `dep_zapdeps` ranks the group. Before the fix the emptied alternative was probed with an empty slice and came back vacuously satisfiable, silently dropping the whole group.

## tests: subset_still_honors_a_nested_non_subset_conditional_normally
- `test? ( bar? ( dev-libs/a ) )`: "bar" not in subset, still evaluated normally (only actually-active branches contribute, exactly like without subset filtering).

## tests: subset_negated_conditional_never_contributes
- Portage's `token[:-1] in subset` check keeps the leading `!`, so `"!test?"` never matches `"test"` in the subset, even though `"!test?"` is itself active here.

## tests: subset_preserves_an_any_of_group_that_survives_filtering
- The `"||"` marker is preserved (matching `use_reduce_flat`'s flat convention) since something inside it survived.

## tests: disjunctive_skips_an_emptied_alternative_and_selects_the_survivor
- Backlog #238. Portage reduces `|| ( foo? ( dev-libs/a ) dev-libs/b )` with foo off to the bare `dev-libs/b` atom.

## tests: disjunctive_emptied_group_becomes_empty_any_of_placeholder
- Backlog #238, EAPI 7+: the upstream `test_eapi` pg1 `=dev-libs/C-2` shape. No surviving alternative, so the probe runs zero times.

## tests: disjunctive_emptied_group_is_dropped_when_empty_groups_always_true
- Backlog #238, EAPI <= 6: the upstream `test_eapi` pg1 `=dev-libs/C-1` shape; the package merges bare.

## tests: disjunctive_prefers_an_installed_alternative_over_an_earlier_available_one
- Portage `preferred_installed` (bin 0) beats `preferred_non_installed` (bin 1): the `virtual/wine` -> `wine-staging` case.

## tests: disjunctive_defers_a_tie_to_tie_break_and_keeps_first_when_it_says_zero
- Both rank `Available` (a genuine tie); `tie_break` decides, not "first satisfiable".

## tests: structured_matches_portage_use_reduce_normalization
- Every expectation captured from Portage `portage.dep.use_reduce(s, uselist=u, token_class=Atom)` (`flat=False`/`opconvert=False` default), serialized like `serialize_dep_tree` (`(`/`)` around every sublist).
