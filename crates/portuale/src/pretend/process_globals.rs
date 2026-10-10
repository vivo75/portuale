//! Set the resolver's process-wide options from the parsed command line.

/// The values [`apply_process_globals`] reads from its caller.
pub(crate) struct ApplyProcessGlobalsArgs {
    pub(crate) package_moves: bool,
    pub(crate) useoldpkg_atoms: Vec<String>,
    pub(crate) binpkg_changed_deps_override: Option<bool>,
    pub(crate) use_ebuild_visibility: bool,
    pub(crate) debug: bool,
    pub(crate) pretend: bool,
}

/// Set the resolver's process-wide options from the parsed command line.
pub(crate) fn apply_process_globals(args: ApplyProcessGlobalsArgs) -> Vec<String> {
    let ApplyProcessGlobalsArgs {
        package_moves,
        useoldpkg_atoms,
        binpkg_changed_deps_override,
        use_ebuild_visibility,
        debug,
        pretend,
    } = args;
    // `--package-moves` (real `y_or_n`, default "y"): must be applied
    // before anything reads `profiles/updates/` -- see
    // `portage_repo::set_package_moves_enabled`. Harmless for every
    // action (a no-op when `y`).
    portage_repo::set_package_moves_enabled(package_moves);

    // `--useoldpkg-atoms ATOMS`: a process-global read during candidate
    // selection (see `portage_repo::set_useoldpkg_atoms`). Set once here,
    // before any `resolve_pretend_graph` call; empty is a strict no-op.
    // A clone is kept for the slot-collision `need_rebuild` trailer,
    // which re-checks the same atoms at render time (real reads
    // `frozen_config.useoldpkg_atoms` there too).
    let useoldpkg_atoms_render = useoldpkg_atoms.clone();
    portage_repo::set_useoldpkg_atoms(useoldpkg_atoms);

    // `--binpkg-changed-deps=y|n` / `--use-ebuild-visibility=y|n`:
    // resolver-time overrides of automatic defaults, applied as
    // process-globals (see those setters' own doc comments). `None` /
    // `false` (the defaults when the flag is absent) are strict no-ops.
    portage_repo::set_binpkg_changed_deps_override(binpkg_changed_deps_override);
    portage_repo::set_use_ebuild_visibility(use_ebuild_visibility);

    // `--debug`/`-d` under `--pretend`: besides `PORTAGE_DEBUG=1` in the
    // phase env (a no-op when nothing is built), real also turns on the
    // full resolver trace (`initialize_logger(logging.DEBUG)`). Portuale
    // emits the same shapes -- see `portage_repo::resolver_trace`. Gated
    // to `--pretend` only (portuale's build path doesn't route through
    // the traced resolver the way real's does). Process-global, set once
    // before any `resolve_pretend_graph` call.
    portage_repo::set_resolver_debug(debug && pretend);
    useoldpkg_atoms_render
}
