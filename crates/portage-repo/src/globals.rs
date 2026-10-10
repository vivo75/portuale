//! Process-wide switches the CLI sets once per run.

use crate::*;

/// Every configured repo's `profiles/updates/` directives, concatenated in
/// `find_repos` (real `portdbapi.getRepositories`) order, parsed once and
/// memoised. Reads `$PORTAGE_CONFIGROOT` directly -- the same
/// process-global, env-driven pattern `portuale::color`'s `color.map`
/// override table uses, so no threading through the resolver's ~35-arg
/// signature. An unreadable `repos.conf` yields no updates.
/// `--package-moves` (real `y_or_n`, `main.py:936`, default `y`): whether
/// `profiles/updates/` `move`/`slotmove` directives are applied at all.
/// Real portage gates the disk-rewriting `_global_updates` pass on
/// `--package-moves != "n"` (`actions.py:3675`); portuale applies moves
/// only at read time, so `=n` simply turns every `apply_updates_to_*`
/// call into a no-op (the resolver then sees the pre-move `cat/pkg`
/// names throughout). A process-global (env-free, like `color`'s
/// override map) so it needn't thread through the resolver signature;
/// the CLI layer sets it once before resolution.
pub(crate) static PACKAGE_MOVES_ENABLED: AtomicBool = AtomicBool::new(true);

/// Set by `pretend.rs` from `--package-moves[=y|n]` before any
/// resolution. Default (never called) is `true`.
pub fn set_package_moves_enabled(enabled: bool) {
    PACKAGE_MOVES_ENABLED.store(enabled, AtomicOrdering::Relaxed);
}

/// `--useoldpkg-atoms ATOMS` (real `main.py:713`, `action: "append"` ->
/// `depgraph.py:370-371`: `WildcardPackageSet(atoms)`). For a package
/// matching one of these atoms, real portage's `_select_pkg` collects
/// the *built* (binary) candidates into `matched_oldpkg`
/// (`depgraph.py:7936`/`8337`) and, when that list is non-empty,
/// restricts `visible_matches` -- and therefore the best-version pick --
/// to them: "prefer matching binary packages over newer unbuilt
/// packages" (`--help`). Only reachable when `--usepkg`/`--getbinpkg`
/// puts binary candidates in the pool at all. A process-global set once
/// by the CLI layer -- the same env-free pattern `--package-moves` /
/// `--color`'s override map use, so it needn't thread through
/// `resolve_pretend`'s / `resolve_pretend_graph`'s already-huge
/// signatures. Same two-tier `matches_config_entry` wildcard matcher
/// `--reinstall-atoms` / `--exclude` already use.
pub(crate) static USEOLDPKG_ATOMS: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// Set by `pretend.rs` from `--useoldpkg-atoms ATOMS` before any
/// resolution. Default (never called) is empty -- a strict no-op.
pub fn set_useoldpkg_atoms(atoms: Vec<String>) {
    *USEOLDPKG_ATOMS.write().unwrap() = atoms;
}

/// Whether `cat/pkg-version` matches an atom in the current
/// `--useoldpkg-atoms` set (real `useoldpkg_atoms.findAtomForPackage`).
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

/// Explicit `--binpkg-changed-deps=y|n` override (real `create_depgraph_
/// params.py:196-203`). `None` = not given, so the auto default applies
/// (`binpkg_changed_deps` enabled unless `--usepkgonly`); `Some(true)` =
/// `=y` (force on, even under `--usepkgonly`); `Some(false)` = `=n`
/// (force off, even for an ordinary `--getbinpkg`). A process-global,
/// the same env-free "CLI sets it once before resolution" pattern
/// `--useoldpkg-atoms` / `--package-moves` use.
pub(crate) static BINPKG_CHANGED_DEPS_OVERRIDE: RwLock<Option<bool>> = RwLock::new(None);

/// Set by `pretend.rs` from `--binpkg-changed-deps[=y|n]` before any
/// resolution. Default (never called) is `None` -- the auto behavior.
pub fn set_binpkg_changed_deps_override(value: Option<bool>) {
    *BINPKG_CHANGED_DEPS_OVERRIDE.write().unwrap() = value;
}

/// Whether the `--binpkg-changed-deps` binary-candidate rejection is
/// active this run: the explicit override if given, else real's own
/// auto default (`myparams["binpkg_changed_deps"] = "auto"` iff
/// `--usepkgonly` not given -> `depgraph.py:7828`'s `!= "n"` -> `True`).
pub(crate) fn binpkg_changed_deps_active(usepkgonly: bool) -> bool {
    BINPKG_CHANGED_DEPS_OVERRIDE
        .read()
        .unwrap()
        .unwrap_or(!usepkgonly)
}

/// `--use-ebuild-visibility[=y|n]` (real `main.py:1103`, `depgraph.py`'s
/// own `use_ebuild_visibility = myopts.get(...) != "n"`): when set,
/// `_equiv_ebuild_visible` is enforced on a built candidate even under
/// `--usepkgonly` / a `--useoldpkg-atoms` match (both of which otherwise
/// exempt it). Real default is off. Process-global, same pattern.
pub(crate) static USE_EBUILD_VISIBILITY: AtomicBool = AtomicBool::new(false);

/// Set by `pretend.rs` from `--use-ebuild-visibility[=y|n]` before any
/// resolution. Default (never called) is `false`.
pub fn set_use_ebuild_visibility(enabled: bool) {
    USE_EBUILD_VISIBILITY.store(enabled, AtomicOrdering::Relaxed);
}

pub(crate) fn use_ebuild_visibility() -> bool {
    USE_EBUILD_VISIBILITY.load(AtomicOrdering::Relaxed)
}

/// `--debug` / `-d` (real `main.py:1235`): besides `PORTAGE_DEBUG=1` in
/// the ebuild phase env (`set -x`), real also calls
/// `initialize_logger(logging.DEBUG)`, which turns on `depgraph.py`'s
/// `writemsg_level(..., level=logging.DEBUG)` resolver trace -- the
/// `Arg:`/`Atom:` / `Parent:`/`Depstring:`/`Priority:`/`Candidates:` /
/// `Child:`/`Parent Dep:`/`Exiting...` per-package narration on stdout,
/// and the `\ndigraph:\n\n` + `debug_print()` graph dump plus the
/// per-atom `ebuild:`/`binary:`/`installed:` candidate list on stderr.
/// Portuale emits the same shapes (see `resolver_trace`), gated on this
/// flag. Process-global, env-free -- same pattern as
/// `USE_EBUILD_VISIBILITY` above -- so it needn't thread through
/// `resolve_pretend_graph`'s ~60-arg signature. The CLI layer
/// (`pretend.rs`) sets it once, on the `--pretend` path only, before any
/// resolution. Default (never called) is `false`.
pub(crate) static RESOLVER_DEBUG: AtomicBool = AtomicBool::new(false);

/// Set by `pretend.rs` from `--debug`/`-d` on the `--pretend` path,
/// before resolution.
pub fn set_resolver_debug(enabled: bool) {
    RESOLVER_DEBUG.store(enabled, AtomicOrdering::Relaxed);
}

/// Whether the `emerge --pretend --debug` resolver trace is active.
pub fn resolver_debug() -> bool {
    RESOLVER_DEBUG.load(AtomicOrdering::Relaxed)
}

/// Dependency tokens the resolver dropped because `portage_dep::parse_atom`
/// could not read them (`docs/history/second_python_copy_removal.md` §4). Real
/// `use_reduce(token_class=Atom)` raises `InvalidDependString` on such a
/// token; portuale skips it, so a grammar gap in `portage-dep` would
/// otherwise vanish silently from the graph. Every skip goes through
/// `note_unparsed_dep_token`; `emerge --pretend` reports the total on
/// stderr when `PORTUALE_REPORT_UNPARSED_DEP_TOKENS` is set, and the L0
/// bed and the contract suite assert it stays zero.
pub(crate) static UNPARSED_DEP_TOKENS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Record one dependency token dropped as unparseable (debug builds also
/// name it on stderr).
pub fn note_unparsed_dep_token(token: &str, site: &str) {
    UNPARSED_DEP_TOKENS.fetch_add(1, AtomicOrdering::Relaxed);
    if cfg!(debug_assertions) {
        eprintln!("portage-repo: dropped unparseable dependency token {token:?} ({site})");
    }
}

/// How many dependency tokens this process dropped as unparseable.
pub fn unparsed_dep_tokens() -> usize {
    UNPARSED_DEP_TOKENS.load(AtomicOrdering::Relaxed)
}
