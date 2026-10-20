//! The per-run resolver context: the BFS request plus the derived seed sets.

use crate::*;

/// The top-level atom texts, plus the set argument each came from.
pub(crate) struct TopLevel<'a> {
    pub(crate) atoms: HashSet<&'a str>,
    pub(crate) sets: &'a HashMap<String, String>,
}

impl<'a> std::ops::Deref for TopLevel<'a> {
    type Target = HashSet<&'a str>;
    fn deref(&self) -> &Self::Target {
        &self.atoms
    }
}

static NO_SETS: std::sync::LazyLock<HashMap<String, String>> =
    std::sync::LazyLock::new(HashMap::new);

/// Plain atoms with no set argument behind them.
impl<'a> FromIterator<&'a str> for TopLevel<'a> {
    fn from_iter<I: IntoIterator<Item = &'a str>>(iter: I) -> Self {
        Self {
            atoms: iter.into_iter().collect(),
            sets: &NO_SETS,
        }
    }
}

impl TopLevel<'_> {
    /// How a chain names this argument: `@set` for a set member, else the atom.
    pub(crate) fn label<'b>(&'b self, atom: &'b str) -> &'b str {
        argument_label(self.sets, atom)
    }
}

/// `@set` for a set member, else the atom as given.
pub(crate) fn argument_label<'b>(sets: &'b HashMap<String, String>, atom: &'b str) -> &'b str {
    sets.get(atom).map_or(atom, String::as_str)
}

/// Breadth-first walk over `atoms` and their dependencies. One `GraphEntry`
/// per distinct category/package/slot, in discovery order; different slots
/// of one package are independent entries, never conflicts by themselves.
// Portage: depgraph.py (dependency walk, slot coexistence)
pub(crate) struct ResolveCtx<'a> {
    pub(crate) root: &'a Path,
    pub(crate) atoms: &'a [String],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) newuse: bool,
    pub(crate) changed_use: bool,
    pub(crate) nodeps: bool,
    pub(crate) onlydeps: bool,
    pub(crate) onlydeps_with_rdeps: bool,
    pub(crate) onlydeps_with_ideps: bool,
    pub(crate) update: bool,
    pub(crate) deep: Deep,
    pub(crate) excluded: &'a [String],
    pub(crate) with_bdeps: bool,
    pub(crate) changed_deps: bool,
    pub(crate) changed_slot: bool,
    pub(crate) with_test_deps: bool,
    pub(crate) changed_deps_report: bool,
    pub(crate) selective: bool,
    /// Only when on does the loop re-drive the walk after an autounmask
    /// change grows the accumulator.
    pub(crate) autounmask_backtrack_enabled: bool,
    pub(crate) usepkg: bool,
    pub(crate) usepkgonly: bool,
    pub(crate) binpkg_respect_use: bool,
    pub(crate) usepkg_exclude: &'a [String],
    pub(crate) usepkg_include: &'a [String],
    pub(crate) rebuilt_binaries: bool,
    pub(crate) rebuilt_binaries_timestamp: Option<u64>,
    pub(crate) newrepo: bool,
    pub(crate) buildpkgonly: bool,
    pub(crate) root_deps_running_root: Option<&'a Path>,
    pub(crate) distdir: &'a Path,
    pub(crate) empty: bool,
    pub(crate) getbinpkg: bool,
    pub(crate) ignore_built_slot_operator_deps: bool,
    pub(crate) backtrack_max: u32,
    pub(crate) reinstall_atoms: &'a [String],
    pub(crate) rebuild_if_new_slot: bool,
    pub(crate) rebuild_if_unbuilt: bool,
    pub(crate) rebuild_if_new_rev: bool,
    pub(crate) rebuild_if_new_ver: bool,
    pub(crate) rebuild_exclude: &'a [String],
    pub(crate) rebuild_ignore: &'a [String],
    pub(crate) dynamic_deps: bool,
    pub(crate) implicit_system_deps: bool,
    pub(crate) complete: bool,
    /// The CLI's `@world`/`@selected`-expanded atoms; a member selecting no
    /// package lands in the missing-args report instead of failing the walk.
    pub(crate) set_args: HashSet<String>,
    pub(crate) repos: Vec<RepoConfig>,
    /// Installed `(cat, pkg)` closure reachable from the required sets in
    /// complete mode; empty means "not complete mode" and disables the
    /// slot-operator and reverse-dependency scans.
    pub(crate) slot_op_reachable: HashSet<(String, String)>,
    /// Like `slot_op_reachable` but without the requested atoms, so the
    /// reverse scan can tell a world-reached consumer from a merely
    /// requested one.
    pub(crate) world_reachable: HashSet<(String, String)>,
    /// Forward-installed closure of the required-set seeds plus args;
    /// computed on every pass, read by blocker classification.
    pub(crate) blocker_retry_closure: HashSet<(String, String)>,
    /// Phase-1 merge set; in the complete pass only these cps may newly merge.
    pub(crate) complete_locked_merges: HashSet<(String, String)>,
    /// Phase-1 merge picks for the complete pass to reuse instead of
    /// re-resolving from installed (see `ResolveRequest::complete_reuse`).
    pub(crate) complete_reuse: HashMap<(String, String), PretendOutcome>,
    /// Top-level atom texts, to tell a top-level `NoVisibleCandidate`
    /// (fatal) from a dependency's (reported).
    pub(crate) top_level: TopLevel<'a>,
    /// `(cat, pkg)` of every directly-requested atom; the missing-dependency
    /// backtrack never masks one of these.
    pub(crate) top_level_cps: HashSet<(String, String)>,
    /// The local `$PKGDIR` binary index, built once for the whole walk.
    pub(crate) local_binpkg: std::sync::Arc<BinaryIndex>,
}

impl<'a> ResolveCtx<'a> {
    /// The running root this walk resolves build-time groups against, if it
    /// differs from the target root.
    pub(crate) fn cross_root(&self) -> Option<&'a Path> {
        self.root_deps_running_root
            .filter(|r| r.as_os_str() != self.root.as_os_str())
    }

    pub(crate) fn new(req: &'a ResolveRequest) -> Result<Self, Error> {
        let _span = portage_util::profile::span("ResolveCtx::new");
        // Complete mode forces a deep walk only for `--changed-deps-report`;
        // forcing it always made large upgrades take minutes for a merge
        // list identical to the non-deep result.
        let deep = if req.empty
            || (req.complete && req.deep == Deep::NotRequested && req.changed_deps_report)
        {
            Deep::Unlimited
        } else {
            req.deep
        };
        // Requested atoms seed the required-set walk too; an empty seed set
        // still means "not complete mode".
        // One index and one set of parsed `*DEPEND` edges serve all three walks.
        let installed_graph = InstalledReachability::new(&req.root);
        let slot_op_reachable: HashSet<(String, String)> =
            if req.config.complete_seed_atoms.is_empty() {
                HashSet::new()
            } else {
                let mut seeds = req.config.complete_seed_atoms.clone();
                seeds.extend(req.atoms.iter().cloned());
                seeds.sort();
                seeds.dedup();
                installed_graph.reachable(&seeds, &[])
            };
        // Without the requested atoms, so the reverse scan can tell a
        // world-reached consumer from a merely requested one.
        let world_reachable: HashSet<(String, String)> =
            if req.config.complete_seed_atoms.is_empty() {
                HashSet::new()
            } else {
                installed_graph.reachable(&req.config.complete_seed_atoms, &[])
            };
        // The blocker classification reads this closure on every pass.
        let blocker_retry_closure: HashSet<(String, String)> = {
            let mut seeds = req.config.blocker_retry_seed_atoms.clone();
            seeds.extend(req.atoms.iter().cloned());
            seeds.sort();
            seeds.dedup();
            let system = req.config.system_packages.clone();
            installed_graph.reachable(&seeds, &system)
        };
        Ok(ResolveCtx {
            root: &req.root,
            atoms: &req.atoms,
            config: &req.config,
            newuse: req.newuse,
            changed_use: req.changed_use,
            nodeps: req.nodeps,
            onlydeps: req.onlydeps,
            onlydeps_with_rdeps: req.onlydeps_with_rdeps,
            onlydeps_with_ideps: req.onlydeps_with_ideps,
            update: req.update,
            deep,
            excluded: &req.excluded,
            with_bdeps: req.with_bdeps,
            changed_deps: req.changed_deps,
            changed_slot: req.changed_slot,
            with_test_deps: req.with_test_deps,
            changed_deps_report: req.changed_deps_report,
            selective: req.selective,
            autounmask_backtrack_enabled: req.config.autounmask_backtrack,
            usepkg: req.usepkg,
            usepkgonly: req.usepkgonly,
            binpkg_respect_use: req.binpkg_respect_use,
            usepkg_exclude: &req.usepkg_exclude,
            usepkg_include: &req.usepkg_include,
            rebuilt_binaries: req.rebuilt_binaries,
            rebuilt_binaries_timestamp: req.rebuilt_binaries_timestamp,
            newrepo: req.newrepo,
            buildpkgonly: req.buildpkgonly,
            root_deps_running_root: req.root_deps_running_root.as_deref(),
            distdir: &req.distdir,
            empty: req.empty,
            getbinpkg: req.getbinpkg,
            ignore_built_slot_operator_deps: req.ignore_built_slot_operator_deps,
            backtrack_max: req.backtrack_max,
            reinstall_atoms: &req.reinstall_atoms,
            rebuild_if_new_slot: req.rebuild_if_new_slot,
            rebuild_if_unbuilt: req.rebuild_if_unbuilt,
            rebuild_if_new_rev: req.rebuild_if_new_rev,
            rebuild_if_new_ver: req.rebuild_if_new_ver,
            rebuild_exclude: &req.rebuild_exclude,
            rebuild_ignore: &req.rebuild_ignore,
            dynamic_deps: req.dynamic_deps,
            implicit_system_deps: req.implicit_system_deps,
            complete: req.complete,
            set_args: req.set_args.clone(),
            repos: find_repos(&req.config_root)?,
            slot_op_reachable,
            world_reachable,
            blocker_retry_closure,
            complete_locked_merges: req
                .config
                .complete_locked_merges
                .iter()
                .filter_map(|s| {
                    s.split_once('/')
                        .map(|(c, p)| (c.to_string(), p.to_string()))
                })
                .collect(),
            complete_reuse: req.complete_reuse.clone(),
            top_level: TopLevel {
                atoms: req.atoms.iter().map(|s| s.as_str()).collect(),
                sets: &req.argument_sets,
            },
            top_level_cps: req
                .atoms
                .iter()
                .filter_map(|a| portage_dep::parse_atom(a))
                .map(|a| (a.category, a.package))
                .collect(),
            local_binpkg: build_local_binpkg_index(&req.config),
        })
    }
}
