//! When an installed package needs a reinstall (changed deps, slot, repo, USE).

use crate::*;

/// `--changed-deps`: whether `version`'s own vdb-recorded dependency
/// strings differ from the repo's own *current* ebuild for that exact
/// version, once both are flattened against the *same* input -- the
/// installed package's own recorded `USE` (real `depgraph.py`'s own
/// `_changed_deps`: `uselist=pkg.use.enabled`, used for *both* sides of
/// the comparison), so a difference driven purely by a USE change is
/// never what this detects -- that's `--newuse`/`--changed-use`'s own
/// job, and can fire independently of (or alongside) this one. Which
/// keys are compared respects `with_bdeps` exactly like
/// `enqueue_dependencies`'s own dep-key list does (real `depgraph.py`'s
/// own `if self._dynamic_config.myparams.get("bdeps") in ("y", "auto"):
/// depvars = Package._dep_keys ... else: depvars = Package._runtime_keys`).
///
/// Real `_changed_deps` compares its per-key `use_reduce(token_class=
/// Atom)` output key by key (`built_deps != unbuilt_deps`, each a list of
/// one struct per dep key), after `strip_slots` and `strip_libc_deps`.
/// This function mirrors that key-by-key shape: `DEPEND` moving to
/// `RDEPEND` with the same overall atom set now registers as changed
/// (the pre-slice merged-into-one-string comparison missed it), and
/// `strip_slot_operator_slots` (real `strip_slots`) keeps a built
/// `cat/pkg:2=` vdb dep from spuriously differing from the ebuild's own
/// `cat/pkg:=`.
///
/// The comparison is now *structured*, matching real `_changed_deps`
/// (`built_deps != unbuilt_deps`, each a list of one
/// `use_reduce(token_class=Atom)` -- i.e. `flat=False` -- struct per dep
/// key): `portage_use_reduce::use_reduce_structured` ports real
/// `use_reduce`'s own `flat=False` bracket-optimization pass, so a
/// `||`-group reorder (`|| ( a b )` -> `|| ( b a )`) *and* -- since real
/// portage compares Python lists, which are order-sensitive everywhere --
/// a plain `RDEPEND="a b"` -> `"b a"` reorder both register as changed,
/// while redundant-bracket differences (`a b` vs `( a b )`) do not. Each
/// serialized atom token then gets real `Atom.evaluate_conditionals`
/// (`evaluate_atom_conditionals`, resolving 2-/4-style USE-deps against
/// the installed package's own `USE`) and real `strip_slots`
/// (`strip_slot_operator_one`); real `strip_libc_deps` is applied
/// *top-level only* (real `strip_libc_deps` iterates just the outer
/// per-key list, never recursing into `||`/nested groups) and by
/// `(category, package)` identity (`libc_provider_cps`).
///
/// A vdb-side dependency string that fails to parse -- including an
/// individual atom token real `use_reduce(token_class=Atom)` would
/// reject -- counts as "changed" unconditionally, matching real portage's
/// own `except InvalidDependString: changed = True`; a repo-side one that
/// fails to parse instead reports "unchanged" (`false`), the same
/// tolerant "can't tell, don't crash" fallback `enqueue_dependencies`
/// already uses, since real portage has no equivalent fallback to mirror
/// there (the repo side is assumed always well-formed).
/// One `*DEPEND` string, real `use_reduce(token_class=Atom)` ->
/// `strip_slots` -> `strip_libc_deps`, reduced to a canonical token
/// stream (`(`/`)` around every group, `||` markers kept). `None` if the
/// string -- or any atom in it -- doesn't parse (real
/// `except InvalidDependString`). Shared by `deps_changed` (installed
/// side) and `binary_deps_changed` (a binhost binary's baked deps).
pub(crate) fn canonical_dep_key(
    depstr: &str,
    use_flags: &HashSet<String>,
    libc_cps: &HashSet<(String, String)>,
) -> Option<Vec<String>> {
    let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
    let reduced = portage_use_reduce::use_reduce_structured(
        &tokens,
        use_flags,
        portage_use_reduce::MatchMode::Normal,
    )
    .ok()?;
    let mut out: Vec<String> = Vec::with_capacity(reduced.len());
    let mut depth: usize = 0;
    for tok in reduced {
        match tok.as_str() {
            "(" => {
                depth += 1;
                out.push(tok);
            }
            ")" => {
                depth = depth.saturating_sub(1);
                out.push(tok);
            }
            "||" => out.push(tok),
            _ => {
                let evaluated = portage_dep::evaluate_atom_conditionals(&tok, use_flags)?;
                let stripped = strip_slot_operator_one(&evaluated);
                if depth == 0
                    && !libc_cps.is_empty()
                    && let Some(atom) = portage_dep::parse_atom(&stripped)
                    && libc_cps.contains(&(atom.category, atom.package))
                {
                    continue;
                }
                out.push(stripped);
            }
        }
    }
    Some(out)
}

/// `--binpkg-changed-deps` (auto-enabled whenever `--usepkgonly` is not
/// given, `create_depgraph_params.py:196-203`): real
/// `_wrapped_select_pkg_highest_available_imp` (`depgraph.py:8288-8300`)
/// rejects a *binary* candidate whose recorded `*DEPEND` differs from the
/// current ebuild's (`self._changed_deps(binpkg)` -> `ignored_binaries
/// [pkg]["changed_deps"] = True; continue`), so a binhost package built
/// against a since-changed ebuild is not merged -- the ebuild is rebuilt
/// instead. Same canonical `use_reduce(token_class=Atom)` / `strip_slots`
/// / `strip_libc_deps` comparison as `deps_changed`, but the built side
/// is the binary's own `Packages` metadata, use-reduced with the flags it
/// was *actually built with* (`Candidate::binary_use`). A binary whose
/// metadata can't be read, or whose deps parse but the ebuild's don't,
/// stays "unchanged" (`false`); an unparsable binary dep string is
/// "changed" (`true`), matching real's `except InvalidDependString`.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn binary_deps_changed(
    root: &Path,
    repos: &[RepoConfig],
    candidate: &Candidate,
    category: &str,
    package: &str,
    with_bdeps: bool,
) -> bool {
    let dep_keys: &[&str] = if with_bdeps {
        &["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"]
    } else {
        &["RDEPEND", "PDEPEND", "IDEPEND"]
    };
    let built_use = candidate.binary_use.clone().unwrap_or_default();
    let binary_metadata = &candidate.binary_deps;
    let Ok(repo_candidates) = list_candidates(repos, category, package) else {
        return false;
    };
    let Some(resolved) = repo_candidates
        .iter()
        .filter(|c| c.version == candidate.version)
        .max_by_key(|c| c.repo_priority)
    else {
        return false;
    };
    let pf = format!("{package}-{}", candidate.version);
    let Ok(ebuild_metadata) = repo_aux_metadata(&resolved.repo_location, category, &pf) else {
        return false;
    };
    let libc_cps = libc_provider_cps(root);

    let mut built_by_key: Vec<Vec<String>> = Vec::with_capacity(dep_keys.len());
    for key in dep_keys {
        match canonical_dep_key(
            binary_metadata
                .get(*key)
                .map(String::as_str)
                .unwrap_or_default(),
            &built_use,
            &libc_cps,
        ) {
            Some(canon) => built_by_key.push(canon),
            None => return true,
        }
    }
    let mut ebuild_by_key: Vec<Vec<String>> = Vec::with_capacity(dep_keys.len());
    for key in dep_keys {
        match canonical_dep_key(
            ebuild_metadata
                .get(*key)
                .map(String::as_str)
                .unwrap_or_default(),
            &built_use,
            &libc_cps,
        ) {
            Some(canon) => ebuild_by_key.push(canon),
            None => return false,
        }
    }
    built_by_key != ebuild_by_key
}

#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn deps_changed(
    root: &Path,
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    version: &str,
    with_bdeps: bool,
) -> bool {
    let dep_keys: &[&str] = if with_bdeps {
        &["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"]
    } else {
        &["RDEPEND", "PDEPEND", "IDEPEND"]
    };

    let installed_use = read_vdb_flag_set(root, category, package, version, "USE");

    let Ok(repo_candidates) = list_candidates(repos, category, package) else {
        return false;
    };
    let Some(resolved) = repo_candidates
        .iter()
        .filter(|c| c.version == version)
        .max_by_key(|c| c.repo_priority)
    else {
        return false;
    };
    let pf = format!("{package}-{version}");
    let Ok(metadata) = repo_aux_metadata(&resolved.repo_location, category, &pf) else {
        return false;
    };

    let libc_cps = libc_provider_cps(root);
    let canonical_key = |depstr: &str| canonical_dep_key(depstr, &installed_use, &libc_cps);

    // vdb side first (real `_changed_deps`'s own `built_deps` loop, whose
    // `except InvalidDependString: changed = True` makes an unparsable
    // vdb dependency string an unconditional "changed").
    let mut vdb_by_key: Vec<Vec<String>> = Vec::with_capacity(dep_keys.len());
    for key in dep_keys {
        match canonical_key(&read_vdb_string(root, category, package, version, key)) {
            Some(canon) => vdb_by_key.push(canon),
            None => return true,
        }
    }

    // repo side (real `unbuilt_deps` loop) -- the repo's own current
    // ebuild metadata is assumed well-formed, so an unparsable one here
    // stays the tolerant "can't tell, don't crash" `false`.
    let mut repo_by_key: Vec<Vec<String>> = Vec::with_capacity(dep_keys.len());
    for key in dep_keys {
        match canonical_key(metadata.get(*key).map(String::as_str).unwrap_or_default()) {
            Some(canon) => repo_by_key.push(canon),
            None => return false,
        }
    }

    // Per-key comparison (real `_changed_deps` compares `built_deps` to
    // `unbuilt_deps` element-wise, each a per-key struct).
    vdb_by_key != repo_by_key
}

/// Splits a raw `SLOT` string into `(slot, sub_slot)` -- real portage:
/// `SLOT="main/sub"`, `sub_slot` defaulting to the slot itself when no
/// `/` is present (real `portage.versions._pkg_str`'s own slot-parsing
/// branch). An empty string (missing `SLOT` file/key) defaults to
/// `("0", "0")`, matching the same `"0"` fallback `list_candidates`/
/// `installed_candidates` already use for a missing `SLOT`.
pub(crate) fn split_slot(raw: &str) -> (String, String) {
    if raw.is_empty() {
        return ("0".to_string(), "0".to_string());
    }
    match raw.split_once('/') {
        Some((slot, sub_slot)) => (slot.to_string(), sub_slot.to_string()),
        None => (raw.to_string(), raw.to_string()),
    }
}

/// Reads `<root>/var/db/pkg/<category>/<package>-<version>/SLOT` and
/// splits it via `split_slot` -- real `vardbapi`'s own `SLOT` file is
/// written verbatim from the same `SLOT` variable a repo's own ebuild
/// declares, so this is the identical format `list_candidates`'s own
/// (main-slot-only) parsing already reads from the repo side, just with
/// the sub-slot component kept too instead of discarded.
pub(crate) fn read_vdb_slot(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> (String, String) {
    split_slot(&read_vdb_string(root, category, package, version, "SLOT"))
}

/// `--changed-slot`: whether `version`'s own vdb-recorded `SLOT`
/// (main+sub) differs from the repo's own *current* ebuild for that
/// exact version. Real `depgraph.py`'s own `_changed_slot`: `ebuild =
/// self._equiv_ebuild(pkg); return ebuild is not None and (ebuild.slot,
/// ebuild.sub_slot) != (pkg.slot, pkg.sub_slot)`.
///
/// KNOWN, DOCUMENTED SCOPE CUT: real portage's own consumers of
/// `_changed_slot` live deep inside binary-package/slot-operator-rebuild
/// scheduling portuale has none of
/// (`_slot_operator_replace_installed`, `built`/`useoldpkg` branches in
/// `_privileged_size_fetch`/candidate selection) -- rejecting a matched
/// installed candidate and, depending on context, either aborting the
/// search or continuing to look for a binary package with the right
/// `SLOT`. Ported here as simply another independent
/// `PretendOutcome::Reinstall` trigger instead, the same "report a
/// reinstall" simplification `--changed-deps` already established --
/// captures the dominant real-world effect (a package whose `SLOT`
/// metadata changed upstream, e.g. an ABI-bump `SLOT="0"` ->
/// `SLOT="0/2"`, gets flagged for reinstall) without replicating real
/// portage's own considerably messier, binpkg-entangled control flow.
/// Deliberately does *not* reuse `Candidate::slot` (which
/// `list_candidates` already truncates to the main component only, see
/// its own doc comment) -- re-reads the repo's own raw `SLOT` value
/// directly instead, the same "re-read metadata portuale's general
/// `Candidate` model doesn't carry" approach `deps_changed` already uses
/// for `DEPEND`/`RDEPEND`. A repo-side lookup that fails (version no
/// longer in the tree, unreadable metadata) reports "unchanged" (`false`),
/// the same tolerant fallback `deps_changed` already uses, matching real
/// `_equiv_ebuild(pkg) is None` -> `False` exactly.
pub(crate) fn slot_changed(
    root: &Path,
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    version: &str,
) -> bool {
    let vdb_slot = read_vdb_slot(root, category, package, version);

    let Ok(repo_candidates) = list_candidates(repos, category, package) else {
        return false;
    };
    let Some(resolved) = repo_candidates
        .iter()
        .filter(|c| c.version == version)
        .max_by_key(|c| c.repo_priority)
    else {
        return false;
    };
    let pf = format!("{package}-{version}");
    let Ok(metadata) = repo_aux_metadata(&resolved.repo_location, category, &pf) else {
        return false;
    };
    let repo_slot = split_slot(
        metadata
            .get("SLOT")
            .map(String::as_str)
            .unwrap_or("")
            .trim(),
    );

    vdb_slot != repo_slot
}

/// `--newrepo`: whether `version`'s own vdb-recorded `repository` file
/// differs from `current_repo_name` (the repo the caller has already
/// established currently provides this exact version -- the resolved
/// `Candidate::repo_name` at each of this function's own two call
/// sites, not re-derived here the way `slot_changed`'s own re-lookup
/// works, since the caller already has it in hand). Real `depgraph.py`:
/// `"--newrepo" in myopts and myeb.repo != pkg.repo` /
/// `pkg.repo != inst_pkg.repo` -- a straight repo-name comparison, no
/// md5-cache re-read needed at all, unlike `slot_changed`. A vdb entry
/// with no `repository` file at all (real portage predates this
/// tracking, or a hand-installed/synthetic entry) is treated as real
/// `portage.versions._unknown_repo` (`"__unknown__"`) exactly -- not
/// "unchanged" the way `slot_changed`/`deps_changed`'s own missing-data
/// fallbacks work, since real portage's own comparison has no such
/// tolerant fallback at all: an unrecorded repo is a real, distinct
/// value (`"__unknown__"`), and it either equals `current_repo_name` or
/// it doesn't, the same as any other string.
pub(crate) fn new_repo_changed(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
    current_repo_name: &str,
) -> bool {
    let vdb_repo = read_vdb_string(root, category, package, version, "repository");
    let vdb_repo = if vdb_repo.is_empty() {
        "__unknown__"
    } else {
        &vdb_repo
    };
    vdb_repo != current_repo_name
}

/// `--rebuilt-binaries`: real `depgraph.py`'s own reinstall trigger
/// (lines ~8394-8429, confirmed by reading it) comparing a binary
/// candidate's own `BUILD_TIME` against the already-installed package's
/// own recorded `BUILD_TIME` -- "replace installed packages with binary
/// packages that have been rebuilt" (real `main.py`'s own help text),
/// the common real-world case being a same-version binary rebuilt
/// against updated dependencies (a toolchain/ABI bump), not a version
/// change at all. Real code's own "skip the check if a newer *source*
/// (unbuilt) candidate exists" branch has no equivalent here: this
/// function is only ever called once the caller has already established
/// `version` is both the best *visible* candidate and what's already
/// installed, so nothing newer (built or unbuilt) can exist by
/// construction -- see this function's own two call sites in
/// `resolve_pretend`. `rebuilt_binaries_timestamp` mirrors real
/// `--rebuilt-binaries-timestamp`: when given, only a *newer*
/// (`built_timestamp > installed_timestamp`) binary at or above that
/// cutoff triggers a reinstall ("use `--rebuilt-binaries-timestamp 0` if
/// you want only newer binaries pulled in", real code comment); when
/// absent, any *different* `BUILD_TIME` triggers one either direction
/// ("don't care ... this is for closely tracking a binhost", same
/// comment) -- real portage's own asymmetry, not a simplification here.
/// A missing/unparseable `BUILD_TIME` on either side (binary index entry
/// or vdb) never triggers a reinstall, matching real code's own `if
/// built_timestamp and ...` guard (an empty string is falsy in Python
/// too, real code's own comment on `bug #306659` cites exactly this: a
/// missing local/remote `BUILD_TIME` must never cause a spurious
/// reinstall). Both call sites gate this on `usepkg || usepkgonly`, not
/// `usepkg` alone -- real `built_pkg` only ever exists in
/// `matched_packages` when the binary db is part of the candidate pool
/// at all, and (see `resolve_pretend`'s own doc comment on `--usepkg`/
/// `--usepkgonly`) *either* flag makes that true, matching real
/// `depgraph.py`'s own `dbs` construction exactly.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn rebuilt_binary_changed(
    root: &Path,
    // The `BUILD_TIME` of the binary candidate that would be selected for
    // this exact version -- local `$PKGDIR` *or* a `--getbinpkg` remote
    // binhost (real `matched_packages`' `built_pkg` is whichever the pool
    // actually offers). `None` when no binary candidate exists, or its
    // `Packages` entry carried no `BUILD_TIME` (real code's own falsy
    // guard, `bug #306659`).
    binary_build_time: Option<i64>,
    category: &str,
    package: &str,
    version: &str,
    rebuilt_binaries_timestamp: Option<u64>,
) -> bool {
    let Some(built_timestamp) = binary_build_time else {
        return false;
    };
    let Some(installed_timestamp) = read_vdb_string(root, category, package, version, "BUILD_TIME")
        .parse::<i64>()
        .ok()
    else {
        return false;
    };
    match rebuilt_binaries_timestamp {
        Some(minimal) => built_timestamp > installed_timestamp && built_timestamp >= minimal as i64,
        None => built_timestamp != installed_timestamp,
    }
}

/// The highest `BUILD_TIME` among binary candidates (local `$PKGDIR` or a
/// `--getbinpkg` remote binhost) at exactly `version` -- what real
/// `matched_packages`' `built_pkg.build_time` would be. `None` if there's
/// no binary candidate for that version.
pub(crate) fn best_binary_build_time(candidates: &[Candidate], version: &str) -> Option<i64> {
    candidates
        .iter()
        .filter(|c| c.source == CandidateSource::Binary && c.version == version)
        .filter_map(|c| c.build_time)
        .max()
}

/// Real `_equiv_ebuild_visible(pkg)` (`depgraph.py:7399`) at a bare
/// version: whether a *visible* ebuild still exists in the tree at
/// `version` (no slot comparison -- real queries `=cat/pkg-ver`). Queried
/// from the tree via `list_candidates` (memoized), independent of the
/// resolver's own candidate pool, so it stays meaningful under
/// `--usepkgonly` (where the pool excludes ebuilds but real still asks
/// the tree whether the installed built instance's ebuild remains).
/// Also read at render time by portuale's slot-collision
/// `need_rebuild` check (real `slot_collision.py`'s
/// `_equiv_ebuild_visible(ppkg)` gate).
pub fn ebuild_visible_at(
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    version: &str,
    config: &portage_profile::Config,
) -> bool {
    list_candidates(repos, category, package)
        .map(|tree| {
            tree.iter()
                .any(|c| c.version == version && is_visible(c, category, package, config))
        })
        .unwrap_or(false)
}

/// Whether a binary at the installed `version` warrants a reinstall, real
/// `depgraph.py`'s two independent `rebuilt_binary` triggers folded into
/// one:
///   - `--rebuilt-binaries`: any `BUILD_TIME` difference (or a strictly-
///     newer one past `--rebuilt-binaries-timestamp`) -- the "closely
///     tracking a binhost" escalation, ebuild-visibility notwithstanding.
///   - without it: the `identical_binary` + `_equiv_ebuild_visible`
///     rejection of an *installed* built instance (`depgraph.py:7999-8030`)
///     -- a binary at the installed version whose `BUILD_TIME` differs
///     *and* whose ebuild is no longer visible is merged over the
///     installed package. Real only rejects the installed instance this
///     way when `not avoid_update` (`avoid_update = "--update" not in
///     myopts`, `depgraph.py:7826`); without `--update` real's own
///     `_select_pkg_highest_available` final `if avoid_update:` step
///     keeps the installed package, so `update` gates this branch. (With
///     an identical `BUILD_TIME` -- `identical_binary` -- or a
///     still-visible ebuild, the installed instance is left alone.) The
///     `update`-less "bare top-level always replaces" case is a separate
///     mechanism (`is_top_level && !selective`), not this one.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn binary_reinstall_warranted(
    root: &Path,
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    category: &str,
    package: &str,
    version: &str,
    binary_build_time: Option<i64>,
    rebuilt_binaries: bool,
    rebuilt_binaries_timestamp: Option<u64>,
    update: bool,
) -> bool {
    if rebuilt_binaries {
        return rebuilt_binary_changed(
            root,
            binary_build_time,
            category,
            package,
            version,
            rebuilt_binaries_timestamp,
        );
    }
    update
        && !ebuild_visible_at(repos, category, package, version, config)
        && rebuilt_binary_changed(root, binary_build_time, category, package, version, None)
}

/// A candidate's own `is_valid_flag` domain: its declared `IUSE`
/// (`declared`) unioned with the profile's real EAPI 5+ `IUSE_EFFECTIVE`
/// (`config.iuse_effective` -- `USE_EXPAND_IMPLICIT`-derived `elibc_*`/
/// `kernel_*`/... and `IUSE_IMPLICIT` flags). Matches real
/// `pkg.iuse.is_valid_flag` for an EAPI 5+ package, so a USE-dep like
/// `foo[elibc_glibc]` matches a `foo` that never lists `elibc_glibc` in
/// its own `IUSE`. Used only for a USE-dep's own `.required`/`(+)`/`(-)`
/// check (`use_deps_satisfied`, portage-dep) -- deliberately NOT for
/// `--newuse`'s own IUSE-*presence* diff, which must stay strictly
/// declared-IUSE (an implicit flag would otherwise read as "newly added
/// to IUSE" and spuriously trigger a reinstall). Cheap no-op clone when
/// `iuse_effective` is empty (every fixture/profile without an explicit
/// `USE_EXPAND_IMPLICIT`).
pub(crate) fn valid_iuse(
    declared: &HashSet<String>,
    config: &portage_profile::Config,
) -> HashSet<String> {
    if config.iuse_effective.is_empty() {
        return declared.clone();
    }
    declared.union(&config.iuse_effective).cloned().collect()
}

/// `candidate`'s own current IUSE (read fresh from its own md5-cache
/// entry -- the current tree's metadata, not the vdb) and its own
/// effective (computed) USE set, via `effective_use_flags`. Shared by
/// `reinstall_flags_for_use_change`'s own "cur_iuse"/"cur_use" (the
/// current-tree side of a `--newuse`/`--changed-use` comparison) and
/// `resolve_pretend`'s own USE-dep filtering (`use_deps_satisfied`,
/// portage-dep) -- both need exactly this same pair for a candidate
/// that's about to be installed or is already installed, computed the
/// same way regardless of which. Returns `None` if this candidate's own
/// metadata can't be read at all (e.g. `IUSE` missing).
pub(crate) fn candidate_iuse_and_use(
    candidate: &Candidate,
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> Option<(HashSet<String>, Rc<HashSet<String>>)> {
    // A binary candidate has no `repo_location` md5-cache entry to read,
    // and -- unlike an ebuild -- its USE isn't recomputed from the
    // profile: it carries the flags it was *actually built with*
    // (`Packages` `USE:` field, `binary_use`) over its own recorded
    // `IUSE`. Real `_iter_match_pkgs` checks an atom's `[use]` deps
    // against `pkg.use.enabled` (the baked set) for a built package.
    if candidate.source == CandidateSource::Binary {
        let iuse: HashSet<String> = candidate
            .iuse
            .split_whitespace()
            .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
            .collect();
        // Owned inside a fresh `Rc` (no shared source exists for the
        // baked set): one small allocation, no content clone saved or
        // added versus before #103.
        let use_flags = Rc::new(candidate.binary_use.clone().unwrap_or_default());
        return Some((iuse, use_flags));
    }
    let pf = format!("{package}-{}", candidate.version);
    let metadata = repo_aux_metadata(&candidate.repo_location, category, &pf).ok()?;
    // A missing IUSE key is a real, valid "declares no USE flags at all"
    // state (same "absence is real, not an error" precedent
    // read_vdb_flag_set already sets for a missing vdb IUSE/USE file),
    // not a reason to treat this whole candidate as unreadable -- unlike
    // a missing md5-cache entry entirely (the `?` just above), which
    // really does mean "can't tell anything about this candidate".
    let iuse: HashSet<String> = metadata
        .get("IUSE")
        .map(|s| s.as_str())
        .unwrap_or_default()
        .split_whitespace()
        .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
        .collect();
    let candidate_str = format!(
        "{category}/{package}-{}:{}/{}::{}",
        candidate.version, candidate.slot, candidate.sub_slot, candidate.repo_name
    );
    let use_flags = effective_use_flags(
        config,
        metadata.get("IUSE").map(String::as_str).unwrap_or_default(),
        &candidate.keywords,
        &candidate_str,
        category,
        package,
    );
    Some((iuse, use_flags))
}

/// `--newuse`/`--changed-use`: ports both the `newuse` and `elif
/// changed_use` branches of real `depgraph.py`'s `_reinstall_for_flags`
/// -- whether `candidate` (a version already installed) needs
/// reinstalling because its currently-effective USE differs from what
/// the vdb recorded at merge time. Returns the sorted list of flags
/// that triggered it, or `None` if nothing did. Only ever called when
/// at least one of `newuse`/`changed_use` is set; if both are (real
/// emerge accepts giving both at once), `newuse` wins -- matching real
/// portage's own `if newuse or (...): ... elif changed_use or (...):
/// ...`, which checks `newuse` first.
///
/// Both branches share one term: `(orig_iuse∩orig_use) ^
/// (cur_iuse∩cur_use)` (which flags were enabled, among those actually
/// declared on each side) -- this alone is the *entire* `--changed-use`
/// formula, deliberately narrower than `--newuse`: it only ever reacts
/// to an *enablement* change of a flag that exists in IUSE on both
/// sides, never to IUSE gaining or losing a flag entirely. `--newuse`
/// adds a second term on top, `(orig_iuse ^ cur_iuse) - forced_flags`
/// (`config.use_force ∪ config.use_mask`, subtracted here and only
/// here -- real portage's own `flags -= forced_flags` line sits between
/// the `^=` and the final `|=`, so a flag forced on/off by the profile
/// never *by itself* triggers a `--newuse` reinstall, but can still
/// contribute via the shared enablement term above, exactly like real
/// portage): whether a flag exists in IUSE changed at all, regardless of
/// whether it's even enabled.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn reinstall_flags_for_use_change(
    root: &Path,
    category: &str,
    package: &str,
    candidate: &Candidate,
    config: &portage_profile::Config,
    newuse: bool,
) -> Option<Vec<String>> {
    let version = &candidate.version;
    let orig_use = read_vdb_flag_set(root, category, package, version, "USE");
    let orig_iuse = read_vdb_flag_set(root, category, package, version, "IUSE");

    let (cur_iuse, cur_use) = candidate_iuse_and_use(candidate, category, package, config)?;

    let orig_enabled: HashSet<String> = orig_iuse.intersection(&orig_use).cloned().collect();
    let cur_enabled: HashSet<String> = cur_iuse.intersection(&cur_use).cloned().collect();
    let mut flags: HashSet<String> = orig_enabled
        .symmetric_difference(&cur_enabled)
        .cloned()
        .collect();

    if newuse {
        let mut presence_diff: HashSet<String> =
            orig_iuse.symmetric_difference(&cur_iuse).cloned().collect();
        for forced in config.use_force.union(&config.use_mask) {
            presence_diff.remove(forced);
        }
        flags.extend(presence_diff);
    }

    if flags.is_empty() {
        return None;
    }
    let mut sorted: Vec<String> = flags.into_iter().collect();
    sorted.sort();
    Some(sorted)
}
