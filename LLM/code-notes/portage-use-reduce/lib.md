# crates/portage-use-reduce/src/lib.rs
Port of Portage's `portage.dep.use_reduce`, restricted to `flat=True` mode.

## module
- Origin: the depgraph/config-resolution slice after atom matching (see LLM/agent-context.md).
- Unlike the atom port, NOT a narrowed grammar: flat mode's tokenizer and bracket/use-conditional handling is fully self-contained in Portage. Out-of-scope optional parameters the harness never exercises: `masklist`, `excludeall` (rare flag-override sets), `is_src_uri`/the `->` SRC_URI arrow token (fetch-restriction syntax), `opconvert` (the operator-into-argument-list shape, which nothing in portuale needs).
- S4 (#341) wires the two remaining parameters through `use_reduce_flat_validated`: `token_class=Atom` arrives as the per-atom-token hook, `is_valid_flag` as the flag closure checked in `is_active_validated` (Portage dep:700-715). `use_reduce_flat` itself stays atom-agnostic (tokens stay opaque strings, matching how config.py itself calls use_reduce for RESTRICT/PROPERTIES/IUSE-like values).
- Both entry points share `walk_flat` (one stack/`need_bracket` machine); only the conditional test and the plain-token sink differ (push vs. hook).
- `flat=True` is a heavily-used Portage invocation mode: `lib/portage/package/ebuild/config.py`, `_emerge/resolver/`, and `_emirrordist` all call `use_reduce(..., flat=True)`.
- `subset` is the `--with-test-deps` follow-up: `select_subset` needs the full nested `paren_reduce` structure (not `flat=True`'s eagerly-flattened one) to know where a `"||"` group's boundaries are before filtering.
- `use_reduce_structured` is the `--changed-deps` structured-comparison follow-up: ports `flat=False`/`opconvert=False` (`_use_reduce_cached`), every redundant-bracket optimization (`is_single`/`special_append`/`ends_in_any_of_dep`/`last_any_of_operator_level`).
- `use_reduce_flat_disjunctive` is the `"||"`-group resolution follow-up: portuale's cut of Portage `dep_zapdeps`'s `choice_bins`, enough to make `|| ( wine-vanilla wine-staging ... )` pick the installed `wine-staging` rather than the first-listed one. The finer bins (`in_graph`/`any_slot`/`unsat_use_*`/`other_*`) and full backtracking still aren't ported.
- Tokens stay opaque strings throughout by architecture; portage-repo supplies the installed/visibility-checking closure.
- Unlike atom.rs, this is NOT a narrowed grammar: flat mode's tokenizer and bracket/use-conditional handling (the part of use_reduce this ports) is fully self-contained in the Portage implementation and is ported as-is, error behavior included. What's out of scope is a set of *optional parameters* the harness simply never exercises: `masklist`, `excludeall` (rare flag-override sets), `is_src_uri`/the "->" SRC_URI arrow token (fetch-restriction syntax), `opconvert` (the operator-into-argument-list shape, which nothing in portuale needs), and `token_class`/`is_valid_flag` (tokens stay opaque strings here, matching how config.py itself calls use_reduce for RESTRICT/PROPERTIES/IUSE-like values -- see the grep in LLM/what-this-proves.md).
- Portage `other_installed_any_slot` (bin 7): no atom's full match exists at all (not even ignoring `[use]`), but some atom's bare `cat/pkg` is installed in some slot (bug 522652's fuzzy match). Second-pass-only.
- Portage `preferred_in_graph` / `preferred_installed` / `preferred_any_slot` (bin 0): every non-blocker atom's `cat/pkg` is already installed (Portage `all_installed` over `Atom(atom.cp)`) or already in the graph, and USE-deps are satisfiable -- what makes `virtual/wine`'s `|| ( wine-vanilla wine-staging … )` pick the installed `wine-staging` instead of failing REQUIRED_USE on the first-listed `wine-vanilla`. The top bin; once found, `resolve_disjunctions` breaks early. Selectable.

## enum MatchMode
- `Normal`: conditionals active iff the flag is in `uselist` (negated for `!flag?`). `All` = Portage's `matchall=True`; `None` = Portage's `matchnone=True`.

## enum Error
- Distinct variants mirror Portage's messages; the contract suite pins these strings.
- S4 (#341) adds the `token_class`-path errors: `InvalidAtom` (`Invalid atom (…), token N`), `MissingWhitespace` (`missing_white_space_check`, dep:788-796 — checked *after* the atom fails), and `Categorized` (a hook/`is_valid_flag` failure with `IUSE.missing`/`EAPI.incompatible`, rendering as the bare message like Portage's `InvalidDependString` with `errors=(e,)`).
- S4 also fixes the empty-group token: Portage reports the `(` token 1-based (`x? ( )` → token 2, `( )` → token 1), which the old `pos + 2` missed by one. The differential never caught it (it compares ERROR-vs-output, not messages).

## fn use_reduce_flat_validated
- One dep string like `use_reduce(matchall=True, flat=True, token_class=Atom, is_valid_flag=…)`: the flat walk with a per-atom-token hook (`hook(token, pos)`, 0-based position; the caller keeps whatever it parsed). `^^`/`??` need no special case: `^^` is a plain token (the hook rejects it: `Invalid atom (^^)`), `??` is a conditional on flag `?` (checked at its `)`). A hook syntax rejection first runs `missing_whitespace_check`, else wraps as `Invalid atom (…), token N`; categorized rejections stay bare (see `map_hook_error`).

## tests: probe atoms through the reducer (#527)
- `validated_walk_passes_probe_atoms_to_the_hook_whole`: `games-arcade/2048 dev-util/timidity-- cat/pkg[a(+),-a]` reach the hook as three whole tokens; the hook's rejection of the third renders `Invalid atom (Invalid use dep: '-a'), token 3`. Oracle: `LLM/evidence/2026-10-23-510/host-3.0.83.txt` (`reduce-Atom` rows) and the meson-built 3.0.82.2 capture (same verdicts).
- `string_path_returns_probe_atoms_unvalidated`: `use_reduce_flat` returns all three unchanged, the deliberate Q8/#526 mimicry (Portage's `use_reduce` without `token_class` does the same).

## fn is_active_validated
- Portage `use_reduce`'s `is_active` with `is_valid_flag` (dep:700-715): validity first (the `IUSE.missing` error at the matching `)`), then matchall (every valid conditional active). No `useflag_re` check — Portage checks one or the other.

## fn is_active
- Exported for portage-repo's LICENSE-grammar parser (PMS 7.3.2): it needs the `||`-group structure `use_reduce_flat` deliberately discards (hence a separate parser), but its USE-conditional semantics are identical, so this piece is shared rather than reimplemented.

## fn use_reduce_flat
- `tokens` is equivalent to Python's `depstr.split()`, Portage's own first step. `"("` always satisfies a pending `"||"`/`"flag?"` requirement regardless of `need_bracket`. `is_src_uri` is always false here (SRC_URI arrows out of scope), matching Portage with `is_src_uri=False`.

## fn use_reduce_flat (#526 decision)
- Keeps Portage's loose string path: no atom parse, so `cat/pkg[a(+),-a]` passes through. Decision recorded in `LLM/portage-quirks.md` Q8: mimic, because every validation path in `emerge`/`mrg` goes through `portage-repo`'s `validate_dep_string` (Atom hook + valid-flag closure), and the flag-only reductions this entry point serves are equally loose in Portage. Pinned by `string_path_returns_probe_atoms_unvalidated`.

## enum DepNode
- `['foobar', 'foo?', ['bar', 'baz']]` shape. No redundant-bracket-collapsing at parse time: representation-only, and `select_subset` (the only consumer) doesn't care.

## fn select_subset
- `disjunction` members each become their own list in the result rather than spliced flat, mirroring Portage's "one result entry per alternative" shape. `selected` inherits into every descendant, so a plain atom nested two levels under `test?` is still kept. `subset` only decides which already-active branch's atoms reach the output, never which branches are active.
- A bare `"( ... )"` grouping follows Portage `select_subset`'s `AttributeError` ("token has no `.endswith`") branch.

## fn serialize_dep_tree
- This crate's `paren_enclose` equivalent.

## const EMPTY_ANY_OF
- Kept verbatim so structured comparison against Portage agrees. Also the `eapi=None` default Portage's `dep_check` uses for installed packages.
- The placeholder atom Portage `use_reduce` (EAPI 7+, `empty_groups_always_true` false, which is also the `eapi=None` default) substitutes for a `|| ( )` group that reduced to nothing -- kept verbatim so a structured comparison against the Portage function agrees.

## fn ends_in_any_of_dep / last_any_of_operator_level / special_append
- Ported as-is from Portage. `last_any_of_operator_level`: a non-operator token breaks the chain first (returns -1). `special_append`: `l == ["||", Group]` splices the group's contents.

## fn use_reduce_structured
- Stays atom-agnostic: no `token_class=Atom` conversion, no `evaluate_conditionals` on atoms' USE-deps; the caller does both as a per-atom post-pass. Output shape matches `serialize_dep_tree` (also used by the subset/disjunctive paths).
- Portage `portage.dep.use_reduce`'s own `flat=False`, `opconvert=False` mode (`_use_reduce_cached`, `lib/portage/dep/__init__.py`): the nested stack reducer with every redundant-bracket optimization (`is_single`/`special_append`/`ends_in_any_of_dep`/ `last_any_of_operator_level`) ported as-is, so two dependency strings that Portage `use_reduce` would collapse to the same structure collapse to the same structure here too. Conditionals are evaluated against `uselist`/`mode` exactly like the flat path; an inactive `flag? ( )` group is dropped, an emptied `|| ( )` becomes the Portage `__const__/empty-any-of` placeholder atom. Stays token/atom-agnostic (no `token_class=Atom` conversion, no `evaluate_conditionals` on the atom's own USE-deps -- the caller does both as a per-atom post-pass, keeping this crate's established "tokens are opaque strings" architecture). Returns the reduced tree re-serialized to a flat token stream (`(`/`)` around every `Group`), the same shape `serialize_dep_tree` already produces for the subset/disjunctive paths.

## fn use_reduce_flat_subset
- Grounded against Portage's `--with-test-deps` call site in `depgraph.py`: `use_reduce(dep_string, uselist=use_enabled | {"test"}, ..., subset={"test"})`.
- Pipeline: `build_dep_tree` (= `paren_reduce`) -> `select_subset` -> `serialize_dep_tree` (= `paren_enclose`) -> unmodified `use_reduce_flat`.
- Portage `use_reduce`'s own `subset` parameter (`lib/portage/dep/__init__.py`): Portage handles this as a genuine two-pass operation even when `flat=True` is also given -- `subset` filtering (`select_subset`, over the full nested `paren_reduce` structure) runs *first*, producing an already-filtered dependency string, which is *then* reduced normally (`flat=True`'s own bracket/conditional handling). This mirrors that exactly: `build_dep_tree` (this crate's own `paren_reduce` equivalent) -> `select_subset` -> `serialize_dep_tree` (this crate's own `paren_enclose` equivalent) -> the ordinary `use_reduce_flat`, completely unmodified. Grounded against Portage's own `--with-test-deps` call site in `depgraph.py`: `use_reduce(dep_string, uselist=use_enabled | {"test"}, ..., subset={"test"})` extracts exactly the *additional* deps a `test?` conditional (anywhere in the string, at any nesting depth) contributes once `"test"` is forced on, discarding every unconditional dep and every dep gated only by some *other* conditional -- see `select_subset`'s own doc comment for the exact walk.

## fn next_alternative
- Portage's dependency-specification grammar allows a bare conditional directly as a `"||"` alternative, not just atoms/groups; hence the `flag?` marker plus its `Group` are consumed as one unit.

## enum AltPreference
- Port of `dep_zapdeps`'s `choice_bins` (`lib/portage/dep/dep_check.py`), bins in `dep_check.py` soft lines 392-402, where `preferred_in_graph` / `preferred_installed` / `preferred_any_slot` alias one list. Selection loop in soft lines 812-816 (quoted verbatim in the code).
- Bins 5-8 (`other_*`): some atom only available via a masked/force path; Portage still returns them on the `allow_masked` second pass. Ported as of slice 2/4 of backlog #22 (`docs/history/022-agent-task-22-zapdeps.fable.md`).
- `Unsatisfiable` has no Portage equivalent: Portage's classification always lands an atom in some bin; only portuale's circular-self-dep guard produces it. See `portage-repo::disjunction_preference`.
- `OtherInstalledAnySlot`: bug 522652's fuzzy match. `OtherInstalled`: typically masked or keyword-rejected (`atom.without_use` tree match fails). `Available` is what makes `|| ( foo[a] foo[b] )` prefer the USE-satisfiable `foo[b]`. `Installed` is what makes `virtual/wine` pick installed `wine-staging` instead of failing REQUIRED_USE on first-listed `wine-vanilla`; once found, `resolve_disjunctions` breaks early (pre-slice-2 it broke early on the first best-rank hit, making tie-breaks unreachable).
- `tie_break` design rule 2 (`docs/history/022-agent-task-22-zapdeps.fable.md`): portage-use-reduce stays atom-agnostic and hands the caller the tied alternatives' flattened atom lists; the caller owns candidate lookup, `cp`/version parsing and `vercmp`. In-bin ordering: prefer an upgrade via `vercmp` over the intersecting `cp_map`, or `all_installed_slots`, or `all_in_graph`, unless sacrificing an upgrade (`dep_check.py` soft 738-802). A trivial `&mut |_| 0` reproduces pre-slice-2 behaviour (portage-repo's pre-slice-2 shim at its two `--root-deps` call sites).
- Portage fills nine lists and picks the first atom of the first *selectable* choice across them, in bin order (`dep_check.py:812-816`):
- Every bin's choices have `all_available = True` except bins 5-8 (`other_*`, reached when some atom is only available via a masked / force path), which Portage still *returns* on its `allow_masked` second pass -- ported as of slice 2/4 of backlog #22 (`docs/history/022-agent-task-22-zapdeps.fable.md`): `resolve_disjunctions` runs a first pass considering only [`is_selectable`](AltPreference::is_selectable) ranks (Portage's `allow_masked=False`), and only when THAT finds nothing at all does a second pass consider the `other_*` bins too (Portage's `allow_masked=True`) -- see `resolve_disjunctions`'s own doc comment. One exception: portuale's own bolt-on circular-self-dep guard (no Portage equivalent) stays hard-`Unsatisfiable` in both passes -- see `portage-repo::disjunction_preference`'s own doc comment.

## fn use_reduce_flat_disjunctive / type TieBreak
- Portage `_add_pkg_dep_string`'s `"||"` resolution. The never-silently-wrong invariant: a dependency portuale can't currently resolve is never silently dropped (established by `resolve_pretend_graph`, portage-repo, for the unconditional-flatten v1 this replaces).
- EAPI detail: `|| ( )` drops when `empty_groups_always_true` (EAPI <= 6; `_get_eapi_attrs(eapi).empty_groups_always_true`) and becomes the placeholder otherwise (EAPI 7+, `dep/__init__.py:864-869`, including the `eapi=None` default for installed packages); the placeholder fails downstream lookup like Portage's.

## fn resolve_disjunctions
- Ordering mirrors Portage `_create_graph` draining the plain `dep_stack` before `_dep_disjunctive_stack` (`depgraph.py:3257-3268`): single flatten can't interleave two stacks, but emits plain results first, then `||`-chosen ones.
- Backlog #238: Portage `use_reduce` drops an alternative an inactive conditional emptied before `dep_zapdeps` ranks the group. Before the fix the emptied alternative was probed with an empty slice and came back vacuously satisfiable, silently dropping the whole group.
- Portage's `allow_masked = True` pass: still never picks portuale's own circular-self-dep guard (no Portage equivalent -- see `AltPreference::Unsatisfiable`'s own doc comment), only a genuine `other_*` bin.

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

## type TieBreak
- Falls back to keeping the *whole* `"||"` group exactly as `use_reduce_flat` would have flattened it (literal `"||"` marker, every alternative's own atoms, no selection at all) whenever *no* alternative is currently satisfiable -- so a dependency portuale can't currently resolve is never silently dropped, preserving the exact "never silently wrong about whether a dependency exists" invariant `resolve_pretend_graph`'s own doc comment (portage-repo) already established for the unconditional-flatten v1 this replaces. Portage's `other_*` bins and the `allow_masked` second pass still aren't ported (`AltPreference::is_selectable`'s own doc comment); everything else -- the fine `unsat_use_*` bins and, as of slice 2, in-bin upgrade-preference ordering via `tie_break` below -- is.

## fn disjunctive_picks_the_first_satisfiable_alternative
- "b" is the only satisfiable alternative -- "a" and "c" must both be dropped entirely, not just deprioritized.

## fn disjunctive_falls_back_to_every_alternative_when_none_satisfiable
- Nothing is satisfiable -- matches plain use_reduce_flat's own "flatten everything" output exactly, so nothing regresses when portuale can't currently resolve any alternative.

## fn disjunctive_prefers_an_installed_alternative_over_an_earlier_available_one
- Portage `dep_zapdeps`'s `preferred_installed` (choice bin 0) beats `preferred_non_installed` (bin 1): of `|| ( a b )` where `a` would need a merge and `b` is already installed, `b` wins even though `a` is listed first. (This is the `virtual/wine` -> `wine-staging` case.)

## fn disjunctive_defers_a_tie_to_tie_break_and_keeps_first_when_it_says_zero
- Both `a` and `b` rank `Available` (a Portage tie) -- `tie_break` decides, not "first satisfiable". A trivial `&mut |_| 0` (portage-repo's own pre-slice-2 shim at its two `--root-deps` call sites) reproduces the old first-listed-wins behaviour.

## fn disjunctive_never_calls_tie_break_for_a_single_alternative_at_the_best_rank
- "b" alone ranks Installed (beats "a"'s Available) -- there is no tie, so tie_break must not be consulted at all.
