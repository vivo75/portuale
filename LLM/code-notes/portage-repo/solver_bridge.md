# crates/portage-repo/src/solver_bridge.rs
Alternate `--solver=` backends: PubGrub and resolvo over the same repo facts.

## module
- Portuale-only (Portage's `emerge` has no `--solver`). Both backends drive lu-zero's `portage-atom-pubgrub` / `portage-atom-resolvo` bridges; see `docs/history/solver-backends-analysis.md` and the `3rdparty/portage-cli` checkout.
- The feeding direction is the bridges' documented boundary ("a solver over facts"): portuale computes all policy (per-version resolved USE via [`super::effective_use_flags`], target atoms, installed set) and hands it in, target atoms, installed set in; the engines only pick versions.
- Performance shape (measured: the engines take ~20ms; the adapter must not dominate): md5 reads, dep-string parses and (dominant, ~1ms each) `effective_use_flags` runs scale with the request's closure, not the tree, the same complexity shape as the backtracking walk's own BFS. The closure over-approximates (both sides of every `flag?()` conditional followed, USE unevaluated), so reachability never drops a selectable package; `all_packages` exposes exactly the closure, keeping dropped-dependency filtering sound. The PubGrub provider uses `new_for_targets` (reachable conversion, not whole-tree); the resolvo pool is populated from closure records only (its construction is inherently whole-input).
- Deliberate v1 cuts (each its own slice): no autounmask relaxation levels (the pool is visibility-filtered with the walk's own `is_visible`, so an invisible version is never offered -- but unlike the walk, a candidate that would need a `--autounmask*` flip (a `~arch` keyword, a license, a `package.mask`) simply fails to resolve rather than producing the flip suggestion); notices wired where the bridge result admits them (blockers via the walk's `resolve_blockers` (H.15c), ABI rebuilds via `slot_operator_rebuild_entries` (H.15), circular deps via `find_hard_cycles` (J), USE display with Portage's forced/masked `( )` markers (J)); merge order is the walk's `topological_merge_order` over Portage's `deps` edges rebuilt from raw dep strings plus resolved USE (H.15b).
- USE-dep / `::repo` / slot-operator target forms resolve the way the bridges resolve them, which may differ from the walk.
- `oldbest` carries the resolved candidate's own sub-slot/repo (an approximation of the installed one's); new-slot `New` entries list installed versions with empty sub-slot/repo.

## struct LoadedVersion
- Parsed dep trees per class (DEPEND, RDEPEND, BDEPEND, PDEPEND, IDEPEND in that order) plus identity back into `GraphEntry` fields.

## fn referenced_cpns
- Blockers included.

## struct LazyRepo
- `Clone` shares nothing but copies the memoized maps (cheap: the closure is fully loaded at `build` time): the provider takes one copy while the `Plan -> GraphResult` mapping keeps the other, so USE computed during the solve stays cached provider-side and only the selected versions re-resolve mapping-side.

## impl LazyRepo::build
- Installed CPNs load for favored-version registration and upgrade comparison, but their dep cones are not expanded; nothing explores them unless the target cone reaches them, in which case the target expansion loads them anyway. Loads are md5 read + parse, no USE, per visited `cat/pkg`.

## impl LazyRepo::load_versions
- The walk resolves from a visibility-filtered pool; the bridges get the same pool (keyword/license/mask/PROPERTIES/RESTRICT acceptance via the walk's own `is_visible`).

## impl LazyRepo::use_of
- History: parked-bridge memo keeps its own copy (cold path; one clone per miss, as before #103).

## fn closure_seeds
- Same construction as `ResolveCtx::top_level_cps`.

## fn newest_installed
- Uncomparable versions keep the first (stable, deterministic).

## fn graph_result_from_order
- Shared `Plan -> GraphResult` mapping: ordered `(cp, version)` selections plus `(to_cpv -> [(from_cat, from_pkg)])` parent edges become entries; each entry's `deps` come from its raw dep strings plus resolved USE (H.15b) and the whole list is re-sorted through the walk's own `topological_merge_order`. USE display resolves on demand through `repo` (memoized).
- USE display (`refresh_entry_use_display`): the bridge passed an empty set, so Portage `pkg_use_display`'s `( )` wraps never rendered. Same call, same `cat/pkg-ver:slot/sub::repo` candidate spelling.
- Blocker reporting (H.15c): every collected blocker atom is matched against the installed db plus this plan's own resolved entries via the shared walk-path `resolve_blockers` (same USE-aware `[use]` gating, same `match_from_list` semantics), and each conflict is filed on its owner entry, mirroring the walk, which resolves blockers after the graph settles and before the merge-order sort.
- ABI rebuilds: an installed consumer whose built `cat/pkg:S/SS=` dep no longer matches how this plan leaves that slot is scheduled for a reinstall, via the walk's own `slot_operator_rebuild_entries` fixpoint; entries are extended before the merge-order sort so rebuilds land dependency-first like every other entry.
- `outcome` is always `Complete`: a solved engine plan admits no same-slot divergence and no relaxation loop ran, so it cannot abort either (no `_create_graph` 0-return, no `_serialize_tasks` give-up, no backtrack loop to exhaust). See `ResolveOutcome`'s doc comment.
- `autounmask_no_clean_tail` stays false: no relaxation loop ran whose flips a "terminated early" gate could hinge on (backlog #217: no changes, so `autounmask_backtrack_disabled()` is False regardless).
- The empty virtual-cycle field (backlog #193): the engine backends have no `_virt_deps_visible` probe walk, and the bridge outcome is always `Complete` by design -- a virtual cycle is one more abort shape the bridges do not do, like slot conflicts.
- Merge-order fidelity (H.15b): the walk path builds every entry's `deps` (Portage `_add_pkg_dep_string` order/priorities) and sorts the whole list through `topological_merge_order` (Portage `_serialize_tasks`); bridge entries used to carry no edges at all, so engine install order leaked straight into the display. Same helper over the raw dep strings (`raw_deps`: DEPEND, RDEPEND, BDEPEND, PDEPEND, IDEPEND) plus this version's resolved USE.
- Blocker atoms met in plan entries' dep strings are resolved after the loop by the shared walk-path helper.
- Missing records cannot happen (the order came from the plan over these same facts).
- Bridge plans are ebuild (source) candidates, hence the ebuild key list and `built = false`, exactly the walk's own call.
- #143 S1: `raw_deps` is key-ordered (DEPEND/RDEPEND/BDEPEND/PDEPEND/IDEPEND), recovering Portage's `blocker.priority.buildtime` provenance.
- The bridge path never runs under `--buildpkgonly` (`ResolveRequest` carries no such flag; the parked engines are not the `--buildpkgonly` CLI path). No scan-collected owners of its own; orphans still arise from walk-pending owners without entries.
- ABI rebuilds (H.15, last slice): the walk's own gate (`rebuild_if_new_slot`, not `ignore_built_slot_operator_deps`) and complete-mode reachability (empty outside complete mode suppresses the scan, like a plain non-complete walk). The `(provider, consumer)` pairs feed `_show_abi_rebuild_info`.
- #211 I1: same construction as `ResolveCtx::top_level_cps`, so both solvers agree. #268: raw request atoms behind `top_level_cps`, for the greedy-eligibility half of the #252 gate.
- Same merge-order sort the walk path applies (Portage's `mylist` is dependency-first): `serialize_merge_order` re-sorts over the `deps` edges plus the `required_by` fallback, with the same top-level atoms, profile config, root, and `--implicit-system-deps` bias.
- The bridge engines pick `||` branches natively (PubGrub/resolvo unit propagation, not `dep_zapdeps` bins); an empty map keeps the suppression's re-derivation exactly as before (backlog #221).
- Circular-dep notice (J): a build-time edge no installed package satisfies is hard (`best_installed_for_atom`, the walk's own gate), anything else soft; blockers never reach the map on either side. `find_hard_cycles` also reads the entries' own `deps` for the #228 slot-operator arm, so the bridge inherits it with no extra map state.
- `slot_conflicts` and the `autounmask_*` lists stay empty on purpose (a candidate needing a flip fails instead; the remaining cut). A bridge failure is one `Error::Detail` line (engine-native text), not a Portage `depgraph.py` notice.
- The abort path is a property of the BFS walk + backtrack loop in `lib.rs`, not the bridge; see `ResolveOutcome`'s doc comment.
- Backlog #221 is a walk-path rule (cycle map). Backlog #217: no changes, so `autounmask_backtrack_disabled()` is False regardless. Backlog #193: a virtual cycle is one more abort shape the bridges do not do, like slot conflicts.

## fn target_slot
- Reads `SlotDep`'s `Display`: `0`, `0/1.2`, `0=`, `:=`, `:*`.

## fn resolve_pubgrub
- `new` converts every CPN; `new_for_targets` converts the seeds' closure with the same `with_bdeps = false` the default path uses, so behaviour is unchanged, only the converted set shrinks. The copy is O(closure), no recompute.
- Same expansion the bridge's own `Solver` impl uses (`to_portage_targets`): an unslotted target node carries no versions when every version is slotted.

## fn resolve_resolvo
- USE resolved per version (memoized, so each runs once). Per-version USE rides in each `PackageMetadata::use_flags`; the provider-level config stays default (no globally-forced flags).
- The explicit copy of the closure ids stands: `iter()` is `slice::Iter` (items are `&SolvableId`), so `to_vec` would collect references.
- Engine-native failure text (the H.15a half of the solver V1 depth cuts): resolvo's `Conflict::display_user_friendly` ("The following packages are incompatible" plus conflict chains, named through the portage provider's solvable display) instead of the raw `UnsolvableOrCancelled` debug dump (`Unsolvable(Conflict { clauses: ... })`), which names no package at all. Mirrors the pubgrub backend, whose `format_solve_error` renders its `NoSolution` derivation tree. A cancelled solve (not reachable through this synchronous call path) stays a plain message.

## fn pg_trace
- S0 Step 5 (#34): gated pubgrub-bridge trace for the §0.2 chain walk. Gate is `PORTUALE_PG_TRACE=1` (env, not the `resolver_debug` global, so unit tests can drive it without the CLI); `pg_trace(out, args)` writes one line, stderr in production, `Vec<u8>` in tests. Unset: every call site no-ops (the `versions_for` fan-out and `desired_use` flag sort sit behind an extra `pg_trace_enabled` check, so not even the formatting work runs).
- Lines: `pg-target <cp> slot=<s>` (`s` = named slot, `*` = whole slotted-node fan-out, `-` = unslotted fallback); `pg-use <cpv> source=config <flags>` (always `config` today — installed packages still expand with the config USE; vdb USE is S3); `pg-edge <parent-cpv> -> <child-cp> (<CLASS>)` (per referenced CPN per dep class in `versions_for`); `pg-installed <cpv> slot=<s> policy=Favor` (policy is hardcoded `Favor` until S2's kept/rebuild set); `pg-closure <cp> expanded|shallow` (`LazyRepo::build`, one line per first insert).
- `LazyRepo::build` is shared with resolvo, so a resolvo run with the env var set also emits `pg-closure` lines; the resolvo resolve path itself (`resolve_resolvo`, `versions_for`-equivalent pool fill) is untouched, and with the var unset both paths behave exactly as before.
- Test: `pg_trace_emits_the_installed_line` (throwaway vdb, one package, asserts the line then asserts silence without the var).

## mod tests
- Behaviour is pinned by fixture-driven tests elsewhere; here only the wiring (no panic per kind).
- The bridge test helper resolves without `--onlydeps` (the parked engines do not implement `--onlydeps-with-*` root filtering, nor `--nodeps`).
- The fixture profile's ACCEPT_KEYWORDS comes from arch/amd64/make.defaults (`ACCEPT_KEYWORDS="${ARCH}"` -> amd64).
- Visibility test: `dev-libs/maskedpkg` is `~amd64`-only (invisible under `ACCEPT_KEYWORDS=amd64`) so `load_versions` offers it nothing, while `dev-libs/newpkg` (`amd64`) keeps its one visible version.
- Closure test: `dev-libs/newpkg` pulls nothing else (only its own CPN plus installed CPNs, if any resolve); the diamond target pulls its whole dependency cone.
- Resolvo failure test: names the conflicted package; no `ClauseId`/`Unsolvable(` internals leak.
- Blocker test: weak in-graph block (`weakblockerpkg` vs `blockerpartnerpkg`) and strong block against installed (`blockerpkg` vs `samepkg-1.0`), the walk's own shapes. #68 S2 cell e (merge-bound match, merging parent -> unresolved). #68 S3: the installed RDEPEND fixtures are not walked graph nodes here, so Portage uninstalls it (`b`, rc 0); pre-S3 this was `true` from the vdb reverse scan. #72 B3: resolved by removing the installed match.
- ABI test: throwaway vdb holds `slotbindtarget-1.0` at `SLOT=2` plus a stale (`:2/2=`) and a fresh (`:2/9=`) consumer, both in the seed set; resolving the target (whose `-2.0` ebuild is `SLOT=2/9`) must reinstall only the stale consumer. The fresh `:2/9=` consumer is already bound correctly and never rebuilt.
- Merge-order test: `dev-libs/diamond`'s entry must name both `shared-a` and `shared-b`.
- Forced/masked test: the bridge used to pass an empty forced set, so Portage `pkg_use_display`'s markers never rendered. Fixture `dev-libs/pkgusemaskforcepkg` (walk-path contract pins `USE="(forceflag) (-maskflag) -specflag"`); `forced_or_masked_flags` reads the per-level stack, not the flat fields.
- Circular test: `dev-libs/hardcyclea` <-> `dev-libs/hardcycleb`, both unbuilt DEPEND edges. PubGrub linearises the cyclic closure so the notice lands; resolvo's `install_order` still cannot linearise any cycle (Tier-4 `--solver=resolvo` item, `scope-backlog.md` §J) and errors before the mapping runs.
