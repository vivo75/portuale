# crates/portage-repo/src/resolve/request.rs
A resolve request: the atoms and the emerge options.

## struct ResolveRequest
- Field names and types match the historical `resolve_pretend_graph` parameter list one-for-one. Owns its inputs so a `Box<dyn Resolver>` can be selected at runtime and the job stays decoupled from the CLI layer.

## field onlydeps
- Portage's `main.py` boolean; the resolver walks every dep class for the root (Portage selects the root node with `onlydeps=True` and still traverses deps). Portage's `pkg.onlydeps` gate in `_add_pkg_deps`.

## field onlydeps_with_rdeps
- Portage `main.py` `true_y_or_n`, default enabled (`man emerge.1` "enabled by default"). Portage's `depgraph.py::_add_pkg_deps` blanks an onlydeps root's `RDEPEND` **and** `PDEPEND` on `"n"`; any other value (`"y"`, `"True"`, absent) keeps them.

## field onlydeps_with_ideps
- Portage `main.py` `true_y_or_n`, default disabled (`man emerge.1` "disabled by default", only consulted "when `--onlydeps` and `--onlydeps-with-rdeps=n` are both specified"). Portage blanks `IDEPEND` on `"n"` or absent (`in ("n", None)`).

## field empty
- Portage `create_depgraph_params.py:176-179` (`myparams["empty"] = True`, `depgraph.py:7889`: installed packages are not selected, so every atom in the deep tree resolves to a merge). Portuale's pool is tree-only, so `empty` forces the "would be `AlreadyInstalled`" fallback to a bare `Reinstall` (Portage `output.py` `attr_display.replace` still set from `vardb.cpv_exists`, no `[oldver]`, no reason -- portuale's reasonless `[ebuild R]`). Useful for byte-for-byte comparison against Portage and debugging.

## field getbinpkg
- Portage `main.py` (distinct from `--usepkg`/`-k`); `--getbinpkgonly`/`-G` implies binary-only. Means "also consider remote binary candidates from `config.binrepos`" (`list_remote_binary_candidates`, each binrepo's on-disk `Packages` index).

## field ignore_built_slot_operator_deps
- Portage `main.py:470` (`y_or_n`, default `"n"`), fed into `FakeVartree` which strips the `:=` parts out of every installed package's recorded `*DEPEND` (`_slot_operator.ignore_built_slot_operator_deps`), so `_slot_operator_trigger_reinstalls` finds nothing. "Intended only for debugging purposes" per Portage's `--help`.

## field backtrack_max
- Portage `main.py` (`type=int`, `valid_integers`); `max_depth = max(1, (COUNT + 1) / 2)`, restart cap at `restarts >= COUNT`. Portage's own default is `--backtrack=20` (depth 10). The one exception is backlog #209's reverse-dependency feed: Portage `_resolve_conflicts` runs `_complete_graph` with no backtracking gate, so satisfiable installed-consumer pins re-run in-process (a feed loop, never a counted restart). Config-growth retries never counted (Portage `_feedback_config` doesn't count toward `--backtrack=N` either).

## field reinstall_atoms
- Portage `main.py` (`action: "append"` -> `depgraph.py:363-365` `WildcardPackageSet(atoms)`); Portage `depgraph.py:4547`/`4643`/`8331` drop matching installed packages from every `inst_pkgs` list, forcing a re-merge.

## field rebuild_if_new_slot
- Portage `y_or_n`, default `"y"`.

## field rebuild_if_unbuilt / rebuild_if_new_rev / rebuild_if_new_ver
- Portage `true_y_or_n`, all OFF by default; see `rebuild_if_entries`.

## field rebuild_exclude / rebuild_ignore
- Same repeatable atom-list shape as `--exclude`.

## field dynamic_deps
- Portage `create_depgraph_params.py`, ON by default for a source install; see `enqueue_dependencies`.

## field implicit_system_deps
- Portage `y_or_n`, default `y`; `merge_order::merge_order_bias`'s `@system`-first / reference-count re-sort vs Portage `depgraph._merge_order_bias`'s early return.

## field complete
- Portage `create_depgraph_params.py:169-175` (`--complete-graph` / `--rebuild-if-*` set `myparams["complete"] = True`) plus the `--complete-graph-if-new-use` / `-if-new-ver` auto-enable resolved by the CLI layer (`complete_graph_auto_enable`, `pretend.rs` two-pass). Portage `depgraph.py::_complete_graph` (8562-8670): loads the vdb, restricts selection to installed-/already-graphed packages, seeds `args ∪ @system ∪ @world`, toggles `myparams["deep"] = True` (8668-8670). In a `--pretend` that never removes or downgrades, the first three are provably inert (consistent installed set stays consistent; `--rebuild-if-*` / `:=` scans already cover every installed package and fire only for a dep merging this run). The deep walk's one extra surface -- an already-broken installed dep shown as `[ebuild N]` -- is existing `--deep` behaviour.

## field set_args
- Portage `SetArg` `"world"`/`"selected"` (`depgraph.py:5577-5605`); dropped into `_missing_args`, walk continues. Populated by the CLI layer which owns set expansion.

## field solver
- `Portage` (default) runs the backtracking walk; the other two run lu-zero's bridges over the same repo facts (see `solver_bridge.rs`).

## fn ResolveRequest::new
- Portuale's default `--backtrack=10` (Portage's is 20).

## fn ResolveRequest::resolve
- Resolves with the `active_resolver`.

## field ResolveRequest::onlydeps
- The resolver itself walks every dep class for the root (Portage selects the root node with `onlydeps=True` and still traverses its deps); the root's own merge-list suppression is display-layer (`pretend.rs` `onlydeps`/`top_level_pkgs`).

## field ResolveRequest::onlydeps_with_ideps
- `--onlydeps-with-ideps <y|n>` (Portage `main.py` `true_y_or_n`, default disabled -- `man emerge.1` "This option is disabled by default", and only consulted "when `--onlydeps` and `--onlydeps-with-rdeps=n` are both specified"): include install-time deps for an onlydeps root whose run-time deps are disabled.

## field ResolveRequest::empty
- The net effect matches Portage `emerge -e`: the entire deep dependency tree is (re)merged.
- Original comment citations: Portage: create_depgraph_params.py:176-179

## field ResolveRequest::getbinpkg
- A remote-only candidate that wins resolution renders the Portage `g` bracket column and contributes its download `SIZE` to `Size of downloads:`.

## field ResolveRequest::ignore_built_slot_operator_deps
- Same net effect here: when set, the `slot_operator_rebuild_entries` post-pass is skipped entirely.
- Original comment citations: Portage: main.py:470; FakeVartree strips := from installed *DEPEND

## field ResolveRequest::backtrack_max
- `--backtrack=0` disables backtracking entirely -- no feedback node is explored, so a slot conflict is reported without any retry, exactly the pre-backtracking behavior.

## field ResolveRequest::reinstall_atoms
- Same two-tier `matches_config_entry` matcher `excluded` uses.
- Empty (`&[]`) at every call site that doesn't pass `--reinstall-atoms`.
- Original comment citations: Portage: main.py append; depgraph.py:363-365,4547,4643,8331

## field ResolveRequest::rebuild_exclude
- `--rebuild-exclude ATOMS` (parent skipped) / `--rebuild-ignore ATOMS` (dep never triggers) -- same repeatable atom-list shape as `--exclude`.

## field ResolveRequest::complete
- `--complete-graph` (Portage `create_depgraph_params.py:169-175`: `--complete-graph` / `--rebuild-if-{unbuilt,new-rev,new-ver}` all set `myparams["complete"] = True`) plus the `--complete-graph-if-new-use` / `-if-new-ver` auto-enable, both resolved by the CLI layer (see `complete_graph_auto_enable` and `pretend.rs`'s two-pass).
- In a `--pretend` that never removes or downgrades an installed package, the first three are provably inert -- a consistent installed set stays consistent, and portuale's `--rebuild-if-*` / slot-operator (`:=`) auto-rebuild passes already scan *every* installed package (`all_installed_packages`), both firing only for a dep that actually merges this run (which installed-only selection never adds).
- Original comment citations: Portage: create_depgraph_params.py:169-175; depgraph.py _complete_graph

## field ResolveRequest::set_args
- Original comment citations: Portage: SetArg world/selected; depgraph.py:5577-5605

## field ResolveRequest::complete_reuse
- Backlog #394: phase-1 merge picks (`cat/pkg` to outcome) for the complete walk to reuse (see `resolve_initial_outcome`). Empty on every non-complete pass. Carried as request state, not `Config`, so the phase layer can pass a different map per pass.
- The phase layer fills only `New`/`Upgrade`/`Downgrade` picks: every `Reinstall` re-derives through the complete walk's own machinery (slot-operator scan + eliminate, the `--rebuild-if-*` restart step, the `--reinstall-atoms`/USE/argument flips), so a reused record can neither freeze a dropped `r` nor skip the eliminate latch (oracle `soc` cell: top-level consumer stays a plain argument `R`).

## field categories
- The run-shared `Categories` (see `categories.md`), when the caller built one: every walk of the resolve gates on it and shares its once-per-cp warning record. `None` (unit tests, other binaries) loads a fresh one per walk. `Rc` because the request is owned and lifetime-free while the sharing must survive `req.clone()` across the best-run re-pass; the resolver is single-threaded (`Rc` already threads the pool memo).

## field verbose
- `--verbose`: the ignored-user-patches block also names the triggering patch files (#509 oracle shape 11).

## fields complete_if_new_use, complete_if_new_ver
- #580 10b: `--complete-graph-if-new-use` / `--complete-graph-if-new-ver` (default on), carried so the resolver can run Portage's complete-mode auto-detection itself (`ResolveCtx::portage_complete`). The CLI used them only to decide a second resolve.
