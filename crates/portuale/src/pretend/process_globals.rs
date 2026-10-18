//! Set the resolver's process-wide options from the parsed command line.

use crate::pretend::*;

/// Set the resolver's process-wide options from the parsed command line.
pub(crate) fn apply_process_globals(opts: &EmergeOpts<'_>) {
    let &EmergeOpts {
        package_moves,
        ref useoldpkg_atoms,
        binpkg_changed_deps_override,
        use_ebuild_visibility,
        ebuild_db_dropped,
        debug,
        pretend,
        ..
    } = opts;
    // `--package-moves` must be applied before anything reads
    // `profiles/updates/`. Harmless for every action (a no-op when `y`).
    portage_repo::set_package_moves_enabled(package_moves);

    // `--useoldpkg-atoms`: process-global read during candidate selection.
    // Set once here, before any resolve; empty is a strict no-op. The opts
    // copy stays for the slot-collision trailer, re-checked at render time.
    portage_repo::set_useoldpkg_atoms(useoldpkg_atoms.clone());

    // `--binpkg-changed-deps` / `--use-ebuild-visibility`: resolver-time
    // overrides as process-globals. Absent (the defaults) is a strict no-op.
    portage_repo::set_binpkg_changed_deps_override(binpkg_changed_deps_override);
    portage_repo::set_use_ebuild_visibility(use_ebuild_visibility);
    portage_repo::set_ebuild_db_dropped(ebuild_db_dropped);
    portage_repo::set_display_root(
        &portage_repo::root_from_env(),
        &portage_repo::running_root_from_env(),
    );

    // `--debug` under `--pretend` also turns on the resolver trace.
    // Gated to `--pretend` only; portuale's build path never routes
    // through the traced resolver. Set once before any resolve.
    portage_repo::set_resolver_debug(debug && pretend);
}
