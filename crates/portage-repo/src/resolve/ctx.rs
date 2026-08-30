//! The per-run resolver context: the BFS request plus the derived seed sets.

use crate::*;

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
    /// Top-level atom texts, to tell a top-level `NoVisibleCandidate`
    /// (fatal) from a dependency's (reported).
    pub(crate) top_level: HashSet<&'a str>,
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
}
