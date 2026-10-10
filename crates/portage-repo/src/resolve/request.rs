//! A resolve request: the atoms and the emerge options.

use crate::*;

/// Everything a resolver needs to turn requested atoms into a merge-ordered
/// [`GraphResult`]. Owns its inputs so a [`Resolver`] can be picked at runtime.
/// Field names and types match the historical `resolve_pretend_graph` parameter
/// list one-for-one.
#[derive(Debug, Clone)]
pub struct ResolveRequest {
    pub config_root: PathBuf,
    pub root: PathBuf,
    pub atoms: Vec<String>,
    pub config: portage_profile::Config,
    pub newuse: bool,
    pub changed_use: bool,
    pub nodeps: bool,
    /// `--onlydeps`/`-o`: merge the targets' dependencies but not the targets
    /// themselves. Suppression of the root's own merge list is display-layer
    /// (`pretend.rs`); carried here so the `--onlydeps-with-*` pair gates on it.
    // Portage: main.py boolean; pkg.onlydeps gate in _add_pkg_deps
    pub onlydeps: bool,
    /// `--onlydeps-with-rdeps <y|n>` (default enabled): include run-time deps
    /// under `--onlydeps`. `false` is exactly `"n"`.
    pub onlydeps_with_rdeps: bool,
    /// `--onlydeps-with-ideps <y|n>` (default disabled, only consulted when
    /// `--onlydeps` blanks run-time deps): include install-time deps. `true`
    /// is `"y"`/`"True"`.
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
    /// `--emptytree`/`-e`: forces `deep` on and every atom resolves to a bare
    /// `Reinstall` instead of `AlreadyInstalled`, so the whole deep tree merges.
    // Portage: create_depgraph_params.py:176-179
    pub empty: bool,
    /// `--getbinpkg`/`-g`: also consider *remote* binary candidates from
    /// `config.binrepos`. A remote-only winner renders the `g` bracket column
    /// and contributes its download `SIZE`. (The caller folds `--getbinpkgonly`
    /// into `usepkgonly` plus this flag.)
    pub getbinpkg: bool,
    /// `--ignore-built-slot-operator-deps`: skip the slot-operator auto-rebuild
    /// scan entirely. Intended only for debugging.
    // Portage: main.py:470; FakeVartree strips := from installed *DEPEND
    pub ignore_built_slot_operator_deps: bool,
    /// `--backtrack=COUNT`: max retries for the backtracking search. `0`
    /// disables backtracking (a slot conflict is reported without retry).
    /// Config-growth retries never count against this budget. Portuale's CLI
    /// default is 10; Portage's own default is 20.
    // Portage: main.py type=int; max_depth = max(1, (COUNT+1)/2)
    pub backtrack_max: u32,
    /// `--reinstall-atoms ATOMS`: an installed `cat/pkg-version` matching one
    /// of these is treated as not installed (a scoped `--emptytree`). Same
    /// matcher `excluded` uses; empty at call sites that pass no flag.
    // Portage: main.py append; depgraph.py:363-365,4547,4643,8331
    pub reinstall_atoms: Vec<String>,
    /// `--rebuild-if-new-slot` (default on): the slot-operator (`:=`)
    /// auto-rebuild pass. `false` skips it.
    pub rebuild_if_new_slot: bool,
    /// `--rebuild-if-unbuilt` / `--rebuild-if-new-rev` / `--rebuild-if-new-ver`
    /// (all off by default): rebuild an installed package whose build-time dep
    /// merges this run. The caller resolves the `unbuilt > new_rev > new_ver`
    /// precedence, so at most one is `true`.
    pub rebuild_if_unbuilt: bool,
    pub rebuild_if_new_rev: bool,
    pub rebuild_if_new_ver: bool,
    /// `--rebuild-exclude` (parent skipped) / `--rebuild-ignore` (dep never
    /// triggers): same repeatable atom-list shape as `--exclude`.
    pub rebuild_exclude: Vec<String>,
    pub rebuild_ignore: Vec<String>,
    /// `--dynamic-deps` (on by default for a source install): `true` walks an
    /// AlreadyInstalled package's deps from the current ebuild; `false`
    /// (`--dynamic-deps=n`) walks its vdb snapshot. Only reachable under `--deep`.
    pub dynamic_deps: bool,
    /// `--implicit-system-deps[=y|n]` (default `y`): only `=n` disables the
    /// @system-first merge-order re-sort, leaving discovery order.
    // Portage: depgraph._merge_order_bias
    pub implicit_system_deps: bool,
    /// `--complete-graph`: the only observable delta is the forced deep walk
    /// (installed-/graph-only selection is a documented `excluded_pkgs` cut;
    /// the auto-rebuild scans already cover every installed package).
    // Portage: create_depgraph_params.py:169-175; depgraph.py _complete_graph
    pub complete: bool,
    /// Top-level atoms expanded from a `@world`/`@selected` set argument. A
    /// member selecting no package lands in the missing-args report instead of
    /// failing the run. Empty for direct resolver callers (unit tests), which
    /// keeps the fatal top-level miss everywhere.
    // Portage: SetArg world/selected; depgraph.py:5577-5605
    pub set_args: std::collections::HashSet<String>,
    /// Portuale-only `--solver=` selection (see [`SolverKind`]).
    pub solver: SolverKind,
}

impl ResolveRequest {
    /// A request to resolve `atoms` as `emerge` does with no options:
    /// `--with-bdeps`, `--selective`, `--rebuild-if-new-slot` and
    /// `--dynamic-deps` on, `--backtrack=10`, everything else off. Set fields
    /// with struct-update syntax:
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
