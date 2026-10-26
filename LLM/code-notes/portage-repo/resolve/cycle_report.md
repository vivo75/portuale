# crates/portage-repo/src/resolve/cycle_report.rs
What the circular-dependency report says about each cycle edge: the USE flags responsible for it, test dependencies, masked `||` alternatives and the search limit for USE changes. Backlog #506 (plan 02.442 S7), ported from Portage 3.0.83 `lib/_emerge/resolver/circular_dependency.py` (commits `4e1545f8a`, `ab7a4d4fc`, `a952df887`, `599c29020`, `e0689630f`; `git -C 3rdparty/portage log portage-3.0.82.2..portage-3.0.83`). Oracle: host Portage 3.0.83-r1 on the staged fixture repo and the bed `l0-fixture-oracle-506.txt` run, `LLM/evidence/2026-10-23-506/`.

## fn circular_max_use_flags
- Portage `_get_max_affecting_use`: `settings.get("PORTAGE_CIRCULAR_MAX_USE_FLAGS")` (environment over make.conf/profile here, `env_over_config_scalar`); an empty value is the default silently; `int(value)` below 1 or unparsable is the default plus the two-line `!!! Invalid ...` warning, which the display prints. `python_int` mirrors Python's base-10 `int(str)` (whitespace, sign, single underscores between digits).
- Portage prints the warning when the handler is built, before `display(handler.merge_list)`; portuale prints it at the start of the stderr block, after the re-displayed merge list on stdout. The streams hold the same bytes; only an interleaved terminal shows the order (residue #517).

## struct CycleParent
- The depending package of an edge: its md5-cache metadata and the USE in effect (`effective_use_flags` plus the autounmask USE overlay, Portage `_pkg_use_enabled`). The same derivation `circular_dep_search` uses.
- `dep_string`: Portage `_dep_string` -- `BDEPEND DEPEND` (`Package._buildtime_keys`) for a `buildtime*` label, `RDEPEND` for `runtime*`, `PDEPEND` for `runtime_post`, nothing otherwise. Read from the edge label `cycle_edge_labels` already renders (`priorities[-1]`).

## fn parent_atom
- Portage `_parent_atom` takes the atom recorded in `_parent_atoms` for the (parent, child) pair. Here: the first non-blocker token of the dep string, reduced with the parent's USE, whose `cat/pkg` is the child's. Comparison everywhere is by the unevaluated token (Portage compares `unevaluated_atom`), so `dev-lang/cddgou[cgo?]` matches itself in the `||` tree.

## fn edge_affecting_use
- Portage `_affecting_use`: `extract_affecting_use(dep, atom)`, then a flag is responsible when toggling it alone removes the atom; when no single flag does but toggling them all does (`doc? ( B ) nls? ( B )` with both on), each flag that keeps the atom alive on its own is responsible. An unparsable dep string counts as still depending (`_depends_on`'s `InvalidDependString` arm).
- Returned sorted by name with the enabled state, which `edge_use_tail` renders red (`flag`) or blue (`-flag`), Portage `_edge_description` with `sorted(affecting_use)`.

## fn cycle_report_facts
- One pass over the edges `cycle[i] -> cycle[i + 1]`: `edge_use[i]`, the `test` parents (Portage `_find_test_dep_parents`: an edge whose responsible flags include `test`) and the masked alternatives, sorted the way `_show_circular_deps` sorts them (`key=lambda x: x.cpv`, member then alternative).

## fn masked_alternatives_for_edge
- Portage `_find_masked_alternatives` / `_iter_any_of_alternatives`: reduce the edge's dep string with the parent's USE keeping `||` groups (`use_reduce_structured`, re-parsed into a small tree), then for every `||` group with a choice holding the parent atom take the atoms of the other choices (nested groups walked too). The reduce-then-parse step is the shared `reduced_dep_tree`, also used by the reverse-dep pin scan. A non-blocker alternative is skipped when an installed or a visible ebuild matches it; otherwise the highest ebuild whose masks are all liftable by configuration is listed.
- `maskable_by_config` (Portage `_maskable_by_config`, every `_MaskReason` has an `unmask_hint`): `package.mask`, a licence, `~arch` and `missing` keyword. Not liftable: `PROPERTIES`, `RESTRICT`, `EAPI`, `invalid`, `CHOST`.
- Cuts: binary packages are not candidates (Portage `_iter_match_pkgs_any` also yields binaries under `--usepkg`); installed packages are checked by version match only. No captured shape needs either (residue #517).

## struct CycleReport / fn build_cycle_report
- Backlog #507 (plan 02.442 S8): the data step of the report, rendered today by `--circular-deps-report=json` (`portuale/pretend/display/circular_json.rs`). Portage `circular_dependency_handler` computes `cycles`, `shortest_cycle`, `parent_solutions`, `test_dep_parents`, `masked_alternatives`, `search_truncated` and `max_affecting_use` once and both formats read them. The text renderer still reads `cycle_report_facts` and `circular_dep_search` directly with the reported cycle `circular_deps[0]` (moving it onto this struct is optional cleanup; the two must keep agreeing on the shortest cycle, residue #518).
- `cycles` is `GraphResult::report_cycles`: `elementary_cycles` (the `get_cycles(ignore_medium_soft)` port) mapped to merge-bound CPVs in enumeration order, so a ring appears once per member (Q6). The shortest cycle is the first strictly shorter one (`_find_cycles`: `len(cycle) < len(shortest_cycle)`), not `circular_deps[0]`. When `report_cycles` is empty (a graph the enumeration finds nothing in), the reported cycle stands in alone; Portage would lower the ignore priority (residue #517).
- `shortest_cycle[pos]` is the edge `cycle[pos - 1] -> cycle[pos]` (Python's `pos - 1` wrap): the first edge is the closing one. `priority` is `str(priorities[-1])`, the label `cycle_edge_labels` already renders; `affecting_use` is `facts.edge_use` without the enabled state.
- `solutions`: `circular_dep_search` suggestions grouped by package (`parent_solutions` is keyed by `changed_parent`, sorted by cpv); the changes of one solution are sorted by flag and the solutions of a package by their change lists, because Portage's order is hash order (Q6). `masked_alternatives` groups `facts.masked_alternatives` by member. Oracle: `LLM/evidence/2026-10-23-507/`.

## no gate (#571, deliberate)
- Cycle edges are merge-bound (listed by construction); `CycleParent::load` and the masked-alternative enumeration stay raw. Verified: no call in this file changed.
