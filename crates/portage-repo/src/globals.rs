//! Process-wide switches the CLI sets once per run.

use crate::*;

/// `--package-moves`: whether `profiles/updates/` `move`/`slotmove` directives apply (default on).
/// Process-global so it needn't thread through the resolver.
// Portage: main.py, actions.py
pub(crate) static PACKAGE_MOVES_ENABLED: AtomicBool = AtomicBool::new(true);

/// `--useoldpkg-atoms ATOMS`: for a matching package, prefer binary candidates over newer
/// unbuilt ones (only with `--usepkg`/`--getbinpkg`). Process-global, set once by the CLI.
// Portage: main.py, depgraph.py
pub(crate) static USEOLDPKG_ATOMS: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// Whether `cat/pkg-version` matches an atom in the `--useoldpkg-atoms` set.
// Portage: useoldpkg_atoms.findAtomForPackage
pub(crate) fn useoldpkg_atom_matches(category: &str, package: &str, version: &str) -> bool {
    let atoms = USEOLDPKG_ATOMS.read().unwrap();
    if atoms.is_empty() {
        return false;
    }
    let cpv = format!("{category}/{package}-{version}");
    atoms
        .iter()
        .any(|a| matches_config_entry(a, &cpv, category, package))
}

/// Explicit `--binpkg-changed-deps=y|n` override (`None` = auto: on unless `--usepkgonly`).
/// Process-global, set once by the CLI.
// Portage: create_depgraph_params.py
pub(crate) static BINPKG_CHANGED_DEPS_OVERRIDE: RwLock<Option<bool>> = RwLock::new(None);

/// Whether the `--binpkg-changed-deps` binary-candidate rejection is active this
/// run: the explicit override if given, else on unless `--usepkgonly`.
// Portage: depgraph.py
pub(crate) fn binpkg_changed_deps_active(usepkgonly: bool) -> bool {
    BINPKG_CHANGED_DEPS_OVERRIDE
        .read()
        .unwrap()
        .unwrap_or(!usepkgonly)
}

/// `--use-ebuild-visibility`: also enforce ebuild visibility on a built candidate under
/// `--usepkgonly` or a `--useoldpkg-atoms` match (both otherwise exempt it). Off by default.
// Portage: main.py, depgraph.py
pub(crate) static USE_EBUILD_VISIBILITY: AtomicBool = AtomicBool::new(false);

pub(crate) fn use_ebuild_visibility() -> bool {
    USE_EBUILD_VISIBILITY.load(AtomicOrdering::Relaxed)
}

/// `--debug`: emit the per-package resolver trace plus the graph dump (see
/// `resolver_trace`). Process-global, set once by the CLI on the `--pretend` path.
// Portage: main.py, depgraph.py
pub(crate) static RESOLVER_DEBUG: AtomicBool = AtomicBool::new(false);

/// Set by `pretend.rs` from `--debug`/`-d` on the `--pretend` path.
pub fn set_resolver_debug(enabled: bool) {
    RESOLVER_DEBUG.store(enabled, AtomicOrdering::Relaxed);
}
