# crates/portage-required-use/src/lib.rs
Port of Portage's `portage.dep.check_required_use`: the REQUIRED_USE evaluator.

## module
- Origin: the REQUIRED_USE (PMS 7.3.4/8.2) slice from LLM/agent-context.md's follow-up work. Ports `is_active`/`is_satisfied` plus the tokenizer/group-tree builder around them (`lib/portage/dep/__init__.py`).
- Grammar (PMS 8.2, a "specification style variable"): leaf `flag`/`!flag`; bare all-of, `||`, `^^`, `??`, `[!]flag? ( item+ )`.
- `??` is EAPI-gated per PMS table 8.5 (`eapi >= 5` per `lib/portage/eapi.py`'s `required_use_at_most_one_of`); always recognized here, matching the EAPI 5+ profile floor and the established "no EAPI parametrization" precedent elsewhere.
- `check_required_use` evaluates directly with a recursive descent (`parse_items`); Portage instead builds a `_RequiredUseBranch` tree and returns it (its `__bool__` is the verdict). The two agree on the verdict for every input, verified via the `required-use-harness`/`required_use_harness.py` pair driven by `test_required_use_contract.py` (the same wraps-Portage pattern `use-reduce-harness` established). NOTE: the Python mirror no longer exists (see AGENTS.md rule 4).
- `unsatisfied_reduced` ports the tree build + the `)` handler's node surgery + `tounicode`: Portage's `depgraph.py::_show_unsatisfied_dep` renders `tree.tounicode()` for its "The following REQUIRED_USE flag constraints are unsatisfied:" line. Verified against Portage via the harness `reduce` op.
- Simplification: `empty_groups_always_true` (`lib/portage/eapi.py`, `eapi <= Eapi("6")`) is never applied -- PMS's own formal grammar requires "one or more" items but Portage's `check_required_use` is more lenient than its own spec here. Empty groups fall through to ordinary per-operator evaluation on an empty list. The only divergence: a literal degenerate `|| ( )` or `^^ ( )` (which no real-world ebuild has reason to write) would evaluate differently under EAPI <= 6.
- A referenced flag missing from IUSE is an error (`is_active`'s `iuse_match` check, `InvalidDependString`), ported as `Err(Error)` and propagated by `portage-repo` as a fatal error for the whole `--pretend` run, matching depgraph's REQUIRED_USE-violation severity.

## enum Error
- Distinct variants mirror `lib/portage/package/ebuild/dochat.py` / `is_active` messages.

## fn parse_items
- Returns `is_satisfied`'s `argument` list for whichever group (or the implicit top-level all-of) contains the items.

## fn parse_items (use-conditional arm)
- `cond` is "flag" or "!flag", passed to `is_active` like a leaf token (`is_active(op[:-1])` in Portage).

## fn build_required_use_tree
- Faithful port of the tree-building half of Portage's `check_required_use`, `)` handler node surgery included: bare all-of groups collapse (single-child operator groups replaced by their child), empty operator groups dropped, inactive use-conditionals dropped as vacuously satisfied.
- Single-child surgery detail: when the only child is itself a branch, it is re-parented; a re-parented operator-less node under an operator-less parent is spliced in turn.

## fn build_required_use_tree (stack)
- A dangling operator inside a closed group counts as malformed (mapped to `false` in the level list, then rejected by the bracket checks).

## tests: nested_groups
- `foo? ( || ( a ^^ ( b c ) ) )` with foo+a on: inner `^^` unsatisfied (0 true), outer `||` sees [true, false] so true.

## tests: reduced_keeps_only_the_unsatisfied_conditional
- The libsdl2 / wine-vanilla shape: a big implicit-AND of `flag? ( ... )` conditionals, only one unsatisfied.

## tests: reduced_inside_an_operator_keeps_all_children
- Portage's "complex_nesting": under a `||`/`^^`/`??`, `tounicode` keeps every child so the operator still reads correctly.
