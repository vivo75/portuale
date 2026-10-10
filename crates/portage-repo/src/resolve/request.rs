//! A resolve request: the atoms and the emerge options.

use crate::*;

/// Everything a resolver needs to turn a set of requested atoms into a
/// merge-ordered [`GraphResult`]. Owns its inputs so a [`Resolver`] can
/// be a `Box<dyn Resolver>` selected at runtime, and so the resolution
/// job is fully decoupled from the CLI layer that assembles it. Field
/// names and types match the historical `resolve_pretend_graph`
/// parameter list one-for-one.
#[derive(Debug, Clone)]
pub struct ResolveRequest {
    pub config_root: PathBuf,
    pub root: PathBuf,
    pub atoms: Vec<String>,
    pub config: portage_profile::Config,
    pub newuse: bool,
    pub changed_use: bool,
    pub nodeps: bool,
    /// `--onlydeps`/`-o` (real `main.py` boolean): merge the targets'
    /// dependencies but not the targets themselves. The resolver itself
    /// walks every dep class for the root (real selects the root node
    /// with `onlydeps=True` and still traverses its deps); the root's
    /// own merge-list suppression is display-layer (`pretend.rs`
    /// `onlydeps`/`top_level_pkgs`). Carried here so the
    /// `--onlydeps-with-*` pair below can gate on it, exactly like
    /// real's `pkg.onlydeps` gate in `_add_pkg_deps`.
    pub onlydeps: bool,
    /// `--onlydeps-with-rdeps <y|n>` (real `main.py` `true_y_or_n`,
    /// default enabled -- `man emerge.1` "This option is enabled by
    /// default"): include run-time deps under `--onlydeps`. Real
    /// `depgraph.py::_add_pkg_deps` blanks an onlydeps root's
    /// `RDEPEND` **and** `PDEPEND` when the value is `"n"`; any other
    /// value (`"y"`, `"True"`, absent) keeps them. `false` here is
    /// exactly that `"n"`.
    pub onlydeps_with_rdeps: bool,
    /// `--onlydeps-with-ideps <y|n>` (real `main.py` `true_y_or_n`,
    /// default disabled -- `man emerge.1` "This option is disabled by
    /// default", and only consulted "when `--onlydeps` and
    /// `--onlydeps-with-rdeps=n` are both specified"): include
    /// install-time deps for an onlydeps root whose run-time deps are
    /// disabled. Real blanks its `IDEPEND` when the value is `"n"` or
    /// absent (`in ("n", None)`); `true` here is `"y"`/`"True"`.
    pub onlydeps_with_ideps: bool,
    pub update: bool,
    pub deep: Deep,
    pub excluded: Vec<String>,
    pub with_bdeps: bool,
    pub changed_deps: bool,
    pub changed_slot: bool,
    pub with_test_deps: bool,
    pub changed_deps_report: bool,
    pub selective: bool,
    pub autounmask_suggest_keywords: bool,
    pub autounmask_suggest_use: bool,
    pub autounmask_suggest_license: bool,
    pub autounmask_suggest_masks: bool,
    pub usepkg: bool,
    pub usepkgonly: bool,
    pub binpkg_respect_use: bool,
    pub usepkg_exclude: Vec<String>,
    pub usepkg_include: Vec<String>,
    pub rebuilt_binaries: bool,
    pub rebuilt_binaries_timestamp: Option<u64>,
    pub newrepo: bool,
    pub buildpkgonly: bool,
    pub root_deps_running_root: Option<PathBuf>,
    pub distdir: PathBuf,
    /// `--emptytree`/`-e` (real `create_depgraph_params.py:176-179`).
    /// Forces `deep` on (`myparams["deep"] = True`) and is threaded into
    /// every `resolve_pretend` call so an already-installed atom -- top
    /// level *or* a dependency reached by the now-mandatory deep walk --
    /// resolves to a bare `Reinstall` instead of `AlreadyInstalled`. The
    /// net effect matches real `emerge -e`: the entire deep dependency
    /// tree is (re)merged. Useful for byte-for-byte comparison against
    /// real portage and for debugging resolution.
    pub empty: bool,
    /// `--getbinpkg`/`-g` (real `main.py` -- distinct from `--usepkg`/
    /// `-k`). The caller folds `--getbinpkgonly`/`-G` into `usepkgonly`
    /// (real: `--getbinpkgonly` implies binary-only) and into this flag,
    /// so `getbinpkg` here means "also consider *remote* binary-package
    /// candidates from `config.binrepos`" (`list_remote_binary_candidates`
    /// -- each binrepo's own on-disk `Packages` index). A remote-only
    /// candidate that wins resolution renders the real `g` bracket column
    /// and contributes its download `SIZE` to `Size of downloads:`.
    pub getbinpkg: bool,
    /// `--ignore-built-slot-operator-deps` (real `main.py:470`, `y_or_n`,
    /// default `"n"`). Real portage feeds this into `FakeVartree`, which
    /// then strips the slot/sub-slot `:=` operator parts out of every
    /// installed package's recorded `*DEPEND` (`_slot_operator.
    /// ignore_built_slot_operator_deps`) -- so the slot-operator
    /// auto-rebuild scan (`_slot_operator_trigger_reinstalls`) finds
    /// nothing to trigger. Same net effect here: when set, the
    /// `slot_operator_rebuild_entries` post-pass is skipped entirely.
    /// "Intended only for debugging purposes" per the real `--help`.
    pub ignore_built_slot_operator_deps: bool,
    /// `--backtrack=COUNT` (real `main.py`, `type=int`, `valid_integers`).
    /// The maximum number of retries for the backtracking search.
    /// max_depth is computed as `max(1, (COUNT + 1) / 2)`, and a total
    /// restart cap is enforced: the search stops when restarts >= COUNT.
    /// Portuale's CLI default is 10 (flag absent); real's own default is
    /// `--backtrack=20` (depth 10). `--backtrack=0` disables backtracking
    /// entirely -- no feedback node is explored, so a slot conflict is
    /// reported without any retry, exactly the pre-backtracking behavior.
    /// The one exception is backlog #209's reverse-dependency feed: real
    /// `_resolve_conflicts` runs `_complete_graph` with no backtracking
    /// gate, so satisfiable installed-consumer pins re-run the pass
    /// in-process (a feed loop,
    /// never a counted restart) instead of settling.
    /// Config-growth retries never counted against this budget (real's
    /// `_feedback_config` doesn't count toward `--backtrack=N` either).
    pub backtrack_max: u32,
    /// `--reinstall-atoms ATOMS` (real `main.py`, `action: "append"` ->
    /// `depgraph.py:363-365`: `WildcardPackageSet(atoms)`). An
    /// already-installed package whose `cat/pkg-version` matches one of
    /// these atoms is treated as if not installed -- real `depgraph.py:
    /// 4547`/`4643`/`8331` drop it from every `inst_pkgs` satisfaction
    /// list, forcing a re-merge. In portuale's model that is exactly a
    /// scoped `--emptytree`: `resolve_pretend`'s own
    /// `AlreadyInstalled` -> `Reinstall` rewrite (see `empty`), applied
    /// per matching atom right after resolution. Same two-tier
    /// `matches_config_entry` matcher `excluded` uses. Empty (`&[]`) at
    /// every call site that doesn't pass `--reinstall-atoms`.
    pub reinstall_atoms: Vec<String>,
    /// `--rebuild-if-new-slot` (real `y_or_n`, default `"y"`): the
    /// slot-operator (`:=`) auto-rebuild pass. `false` (`=n`) skips it,
    /// like `--ignore-built-slot-operator-deps` already does.
    pub rebuild_if_new_slot: bool,
    /// `--rebuild-if-unbuilt` / `--rebuild-if-new-rev` /
    /// `--rebuild-if-new-ver` (real `true_y_or_n`, all OFF by default):
    /// rebuild an installed package whose build-time dep is being merged
    /// this run -- see `rebuild_if_entries`'s own doc comment. The caller
    /// resolves the real `unbuilt > new_rev > new_ver` precedence, so at
    /// most one is `true` here.
    pub rebuild_if_unbuilt: bool,
    pub rebuild_if_new_rev: bool,
    pub rebuild_if_new_ver: bool,
    /// `--rebuild-exclude ATOMS` (parent skipped) / `--rebuild-ignore
    /// ATOMS` (dep never triggers) -- same repeatable atom-list shape as
    /// `--exclude`. `&[]` at every call site that passes neither.
    pub rebuild_exclude: Vec<String>,
    pub rebuild_ignore: Vec<String>,
    /// `--dynamic-deps` (real `create_depgraph_params.py`, ON by default
    /// for a source install): `true` walks an AlreadyInstalled package's
    /// deps from the current ebuild (portuale's own long-standing
    /// behaviour); `false` (`--dynamic-deps=n`) walks its vdb snapshot --
    /// see `enqueue_dependencies`'s own doc comment. Only reachable under
    /// `--deep`.
    pub dynamic_deps: bool,
    /// `--implicit-system-deps[=y|n]` (real `y_or_n`, default `y`): only
    /// `=n` disables, which skips `merge_order::merge_order_bias`'s
    /// @system-first / reference-count re-sort (real
    /// `depgraph._merge_order_bias`'s own `implicit_system_deps` early
    /// return) -- the `--pretend` merge list then stays in discovery order.
    pub implicit_system_deps: bool,
    /// `--complete-graph` (real `create_depgraph_params.py:169-175`:
    /// `--complete-graph` / `--rebuild-if-{unbuilt,new-rev,new-ver}` all
    /// set `myparams["complete"] = True`) plus the
    /// `--complete-graph-if-new-use` / `-if-new-ver` auto-enable, both
    /// resolved by the CLI layer (see `complete_graph_auto_enable` and
    /// `pretend.rs`'s two-pass). Real `depgraph.py::_complete_graph`
    /// (8562-8670) then does four things: loads the vdb, restricts package
    /// selection to installed-/already-graphed packages, seeds
    /// `args ∪ @system ∪ @world`, and toggles `myparams["deep"] = True`
    /// (8668-8670). In a `--pretend` that never removes or downgrades an
    /// installed package, the first three are provably inert -- a
    /// consistent installed set stays consistent, and portuale's
    /// `--rebuild-if-*` / slot-operator (`:=`) auto-rebuild passes already
    /// scan *every* installed package (`all_installed_packages`), both
    /// firing only for a dep that actually merges this run (which
    /// installed-only selection never adds). So the whole observable delta
    /// is the forced deep walk. The one case the deep walk still surfaces
    /// that real complete mode would suppress -- an already-broken
    /// installed dep shown as `[ebuild N]` -- is existing `--deep`
    /// behaviour, and complete mode's stricter installed-/graph-only
    /// selection is already a documented `excluded_pkgs` scope cut.
    pub complete: bool,
    /// Top-level atoms expanded from a `@world`/`@selected` set argument
    /// (real `SetArg` named `"world"`/`"selected"`, `depgraph.py:5577-5605`).
    /// A member that selects no package is dropped into the missing-args
    /// report (`_missing_args`) and the walk continues, where an explicit
    /// argument fails the run. Populated by the CLI layer, which owns set
    /// expansion; empty for direct resolver callers (every existing unit
    /// test), which keeps the fatal top-level miss everywhere.
    pub set_args: std::collections::HashSet<String>,
    /// Portuale-only `--solver=` selection (see [`SolverKind`]): which
    /// algorithm answers this request. `Portage` (the default) runs the
    /// backtracking walk; the other two run lu-zero's bridges over the
    /// same repo facts (see `solver_bridge.rs`).
    pub solver: SolverKind,
}

impl ResolveRequest {
    /// A request to resolve `atoms` as `emerge` does with no options:
    /// `--with-bdeps`, `--selective`, `--rebuild-if-new-slot` and
    /// `--dynamic-deps` on, `--backtrack=10` (portuale's default; Portage's
    /// is 20), everything else off. Set fields with struct-update syntax:
    ///
    /// ```ignore
    /// ResolveRequest { update: true, deep: Deep::Unlimited, ..ResolveRequest::new(..) }
    /// ```
    pub fn new(
        config_root: &Path,
        root: &Path,
        atoms: &[String],
        config: &portage_profile::Config,
        distdir: &Path,
    ) -> Self {
        Self {
            config_root: config_root.to_path_buf(),
            root: root.to_path_buf(),
            atoms: atoms.to_vec(),
            config: config.clone(),
            newuse: false,
            changed_use: false,
            nodeps: false,
            onlydeps: false,
            onlydeps_with_rdeps: true,
            onlydeps_with_ideps: false,
            update: false,
            deep: Deep::NotRequested,
            excluded: Vec::new(),
            with_bdeps: true,
            changed_deps: false,
            changed_slot: false,
            with_test_deps: false,
            changed_deps_report: false,
            selective: true,
            autounmask_suggest_keywords: false,
            autounmask_suggest_use: false,
            autounmask_suggest_license: false,
            autounmask_suggest_masks: false,
            usepkg: false,
            usepkgonly: false,
            binpkg_respect_use: false,
            usepkg_exclude: Vec::new(),
            usepkg_include: Vec::new(),
            rebuilt_binaries: false,
            rebuilt_binaries_timestamp: None,
            newrepo: false,
            buildpkgonly: false,
            root_deps_running_root: None,
            distdir: distdir.to_path_buf(),
            empty: false,
            getbinpkg: false,
            ignore_built_slot_operator_deps: false,
            backtrack_max: 10,
            reinstall_atoms: Vec::new(),
            rebuild_if_new_slot: true,
            rebuild_if_unbuilt: false,
            rebuild_if_new_rev: false,
            rebuild_if_new_ver: false,
            rebuild_exclude: Vec::new(),
            rebuild_ignore: Vec::new(),
            dynamic_deps: true,
            implicit_system_deps: true,
            complete: false,
            set_args: std::collections::HashSet::new(),
            solver: SolverKind::Portage,
        }
    }

    /// Resolves this request with the [`active_resolver`].
    pub fn resolve(&self) -> Result<GraphResult, Error> {
        active_resolver().resolve(self)
    }
}
