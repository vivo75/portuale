//! Configuration for standalone `ebuild` phases and unmerge.

use crate::ebuild_phases::*;

/// Real `doebuild()`'s own `SRC_URI`-vs-`DISTDIR` fetch check (see
/// `crate::fetch`'s own module doc comment for the real mechanics),
/// run once per `run_commands_async` call rather than per-phase.
/// `SRC_URI` itself is read from the ebuild's own repo's real
/// `metadata/md5-cache` entry (the same source `ebuild_package.rs`
/// already trusts for `Packages`-index metadata) -- absent entirely
/// (no fetch attempted, `A`/`AA` both empty) for a standalone ebuild
/// file outside any repo checkout, the same tolerance `repo_root_for`
/// already established. Returns `(A, AA)`: `A` is the real, actually-
/// fetched filename list (portuale's own always-empty USE set, see
/// `crate::fetch::fetch_src_uri`'s own doc comment); `AA` is every
/// filename `SRC_URI` could ever reference regardless of USE (real
/// PMS's own definition), computed but never itself fetched.
/// Real `RESTRICT=mirror` (real `PORTAGE_RESTRICT`, and `fetch.py:880` --
/// the deprecated negative `nomirror` counts too). The md5-cache
/// `RESTRICT` field is the raw ebuild value, so it's USE-conditional-
/// evaluated first (real `_PackageMetadataWrapper`'s own `use_reduce`
/// pass, same as `PROPERTIES`/`LICENSE`) against portuale's own
/// always-empty fetch-side USE set (see `fetch::fetch_src_uri`'s own doc
/// comment) -- so every `foo? ( … )` group drops and only an
/// unconditional `mirror`/`nomirror` counts. An unparsable value yields
/// `false` (the "can't tell, so don't claim it" precedent).
pub(crate) fn restrict_mirror_from_restrict(restrict: &str) -> bool {
    flat_field_has_token(restrict, &["mirror", "nomirror"])
}

/// Real `RESTRICT=fetch` (`fetch.py:1061`, `restrict_fetch = "fetch" in
/// restrict`), USE-conditional-evaluated the same way
/// `restrict_mirror_from_restrict` does. Gates the plain-`SRC_URI`-URI
/// and public-`GENTOO_MIRRORS` candidates in
/// `crate::fetch::fetch_src_uri` (see `FetchOptions::restrict_fetch`).
pub(crate) fn restrict_fetch_from_restrict(restrict: &str) -> bool {
    flat_field_has_token(restrict, &["fetch"])
}

/// Real `RESTRICT=primaryuri` (real `fetch.py:1187`, `"primaryuri" in
/// restrict`), USE-conditional-evaluated the same way
/// `restrict_mirror_from_restrict` does. Moves the file's own literal
/// `SRC_URI` URIs to the front of its candidate list (see
/// `FetchOptions::restrict_primaryuri`).
pub(crate) fn restrict_primaryuri_from_restrict(restrict: &str) -> bool {
    flat_field_has_token(restrict, &["primaryuri"])
}

/// USE-conditional-evaluates a raw md5-cache `RESTRICT`/`PROPERTIES`
/// value (real `_PackageMetadataWrapper`'s own `use_reduce` pass)
/// against portuale's own always-empty phase-side USE set (see
/// `restrict_and_properties`'s own doc comment for why), then checks
/// whether any of `wanted` survived. Shared by every `RESTRICT`/
/// `PROPERTIES` single-token check in this module -- the field doesn't
/// matter to the tokenizing/reducing/matching logic itself, only to the
/// caller's own choice of `wanted`.
pub(crate) fn flat_field_has_token(raw: &str, wanted: &[&str]) -> bool {
    if raw.trim().is_empty() {
        return false;
    }
    let tokens: Vec<String> = raw.split_whitespace().map(String::from).collect();
    portage_use_reduce::use_reduce_flat(
        &tokens,
        &std::collections::HashSet::new(),
        portage_use_reduce::MatchMode::Normal,
    )
    .map(|flat| flat.iter().any(|t| wanted.contains(&t.as_str())))
    .unwrap_or(false)
}

/// USE-conditional-evaluates a raw `RESTRICT`/`PROPERTIES` value the
/// same way `flat_field_has_token` does, but returns the whole flattened
/// token list joined back into a plain-text string -- real portage's
/// own `PORTAGE_RESTRICT`/`PORTAGE_PROPERTIES` env vars carry exactly
/// this shape (`doebuild_environment()`'s own `str(self._pkg.restrict)`/
/// `str(self._pkg.properties)`, both already USE-reduced `_pkg`
/// accessors -- real bash's own `contains_word … "${PORTAGE_RESTRICT}"`
/// checks, e.g. `phase-functions.sh:549`'s `RESTRICT=test` skip and
/// `:777`'s `RESTRICT=nostrip`/`RESTRICT=strip`, consume the reduced
/// form, not the raw ebuild one). An unparsable value degrades to `""`,
/// the same "can't tell, so don't claim it" precedent
/// `flat_field_has_token` already uses. `use_set` is the reduction
/// input: empty for every phase but `depend` (see
/// `restrict_and_properties`), the config `USE` set for `depend` (see
/// `depend_use_set`).
///
/// Deduped and sorted like real `config.py::_flatten` (`:1681-1688`):
/// `" ".join(sorted(set(use_reduce(..., flat=True))))`. This is
/// load-bearing, not cosmetic: ncurses' `RESTRICT="!test? ( test ) test"`
/// use-reduces (with `test` off) to two `test` tokens, and `bin/ebuild.sh:
/// 721-724` overwrites the phase's `RESTRICT` with this value -- without
/// the set, the saved env (and the vdb `environment.bz2`) carries
/// `RESTRICT="test test"` where real has `"test"` (#45 P2a finding).
pub(crate) fn flat_field_on(raw: &str, use_set: &std::collections::HashSet<String>) -> String {
    if raw.trim().is_empty() {
        return String::new();
    }
    let tokens: Vec<String> = raw.split_whitespace().map(String::from).collect();
    portage_use_reduce::use_reduce_flat(&tokens, use_set, portage_use_reduce::MatchMode::Normal)
        .map(|flat| {
            let unique: std::collections::BTreeSet<String> = flat.into_iter().collect();
            unique.into_iter().collect::<Vec<_>>().join(" ")
        })
        .unwrap_or_default()
}

/// Real `config.environ()` for a standalone `ebuild <file> <phase>`:
/// the package's effective `USE` plus the whole resolved config env --
/// `phase_environ(config, None)` (make.conf + profile + env layer, with
/// the same `PORTUALE_COMPUTED` exclusions the merge path's run-wide env
/// already carries) -- exported as base vars into every phase. Merge builds pass their fully-resolved flags via `extra_env`
/// (appended after these base vars downstream, so it keeps overriding
/// them), which means this computation only ever surfaces for standalone
/// `ebuild <file> <phase>` runs -- resolved the same way `ebuild_merge::
/// blocked_installed_packages`' own standalone resolution does
/// (`find_repos` + `resolve_config` + md5-cache `IUSE` +
/// `effective_use_flags`, via `candidate_use_flags_display` for USE and
/// `phase_environ` for the flags), and empty on any failure (missing
/// `repos.conf`, unreadable cache, an ebuild path outside any real
/// repo).
///
/// Two gates, both load-bearing. When `extra_env` already carries `USE`
/// the whole computation is skipped (zero cost and zero behavior change
/// for merge builds -- their `extra_env` carries the complete flag set
/// from the same config, so nothing is lost). And the `depend` phase
/// keeps the old empty base exactly: metadata extraction must stay on
/// the empty set -- config-derived values there would rewrite `--regen`
/// cache bytes for USE-conditional metadata and reload the whole profile
/// once per ebuild (against the F.10/F.11 perf work); every pinned
/// golden assumes it.
///
/// Standalone `package.env`: both halves flow here, not just on merge
/// builds. The `USE=` half rides `candidate_use_flags_display` above
/// (`effective_use_flags` atom-matches `package_env_use` itself); the
/// build-vars half is atom-matched below against the ebuild's own
/// md5-cache identity (`match_package_env_vars`).
///
/// `(USE display pairs, build flag pairs)` from one standalone config
/// load -- the alias keeps the loader closure below under clippy's
/// `type_complexity` lint.
pub(crate) type StandaloneBaseEnv = (Vec<(String, bool)>, Vec<(String, String)>);

/// The resolved config for the standalone `ebuild <file>` paths: real
/// `portage` reads `repos.conf` and the profile chain once per
/// `doebuild` call, so every standalone helper below loads it the same
/// way (shared with `depend_use_set`/`resolved_fetch_config`'s own
/// sibling loaders, kept separate since this needs the whole `Config`).
/// `None` outside any real repo checkout (same tolerance as
/// `restrict_and_properties`): no md5-cache to read IUSE from, no
/// `package.env` to match.
pub(crate) fn resolve_standalone_config(
    pkg_dir: &Path,
    config_root: &Path,
    eroot: &Path,
) -> Option<portage_profile::Config> {
    let _ = repo_root_for(pkg_dir)?;
    let repos = portage_repo::find_repos(config_root).ok()?;
    let main_repo = repos.iter().find(|r| r.is_main)?;
    let overlay_repos: Vec<(String, PathBuf)> = repos
        .iter()
        .filter(|r| !r.is_main)
        .map(|r| (r.name.clone(), r.location.clone()))
        .collect();
    let repo_aliases: Vec<(String, PathBuf)> = repos
        .iter()
        .flat_map(|r| r.aliases.iter().map(|a| (a.clone(), r.location.clone())))
        .collect();
    let repo_masters: std::collections::HashMap<String, Vec<PathBuf>> = repos
        .iter()
        .map(|r| (r.name.clone(), r.masters.clone()))
        .collect();
    portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root,
        main_repo_location: &main_repo.location,
        overlay_repos: &overlay_repos,
        repo_aliases: &repo_aliases,
        main_repo_name: &main_repo.name,
        repo_masters: &repo_masters,
        eroot,
    })
    .ok()
}

/// The `cat/pkg-ver:slot/sub` identity real `_grab_pkg_env` matches
/// against, for an ebuild that has no resolved graph entry: the md5
/// cache's own `SLOT` (`slot/sub_slot` when a sub-slot is declared, bare
/// slot otherwise, `"0"` when the key is missing — the same fallback
/// shape the merge path's `entry_package_env_vars` uses).
pub(crate) fn standalone_cpv_slot(pkg_dir: &Path, category: &str, pf: &str) -> Option<String> {
    let repo_root = repo_root_for(pkg_dir)?;
    let slot_raw = portage_repo::repo_aux_metadata(&repo_root, category, pf)
        .ok()
        .and_then(|m| m.get("SLOT").cloned())
        .unwrap_or_default();
    let (slot, sub_slot) = match slot_raw.split_once('/') {
        Some((s, ss)) => (s.to_string(), ss.to_string()),
        None if slot_raw.is_empty() => ("0".to_string(), "0".to_string()),
        None => (slot_raw.clone(), slot_raw),
    };
    // `pf` already carries `name-version`; the category + slot complete
    // the match string.
    Some(format!("{category}/{pf}:{slot}/{sub_slot}"))
}

pub(crate) fn phase_standalone_base_env(
    env: &Environment,
    config_root: &Path,
    eroot: &Path,
    ebuild_phase_value: &str,
    extra_env: &[(String, String)],
) -> (String, Vec<(String, String)>) {
    if ebuild_phase_value == "depend" || extra_env.iter().any(|(k, _)| k == "USE") {
        return (String::new(), Vec::new());
    }
    let Some((display, flags)) = (|| -> Option<StandaloneBaseEnv> {
        let config = resolve_standalone_config(&env.pkg_dir, config_root, eroot)?;
        let repos = portage_repo::find_repos(config_root).ok()?;
        let display = portage_repo::candidate_use_flags_display(
            &repos,
            &config,
            &env.category,
            &env.split.pn,
            &env.split.pvr,
        );
        let flags = portage_profile::phase_environ(&config, None);
        // Per-package `package.env` build vars, atom-matched against
        // the ebuild's own md5-cache identity (real `_grab_pkg_env`
        // folding a matching entry into `configdict["pkg"]`): the
        // standalone equivalent of the merge path's
        // `entry_package_env_vars`, which needed a resolved graph
        // entry only to name the same `cat/pkg-ver:slot/sub` string
        // rebuilt here. Layered after the base flags so they win,
        // like the merge path's own `entry_build_env` order. The
        // `USE=` half needs no separate step: `candidate_use_flags_display`
        // above already folds `package_env_use` in via
        // `effective_use_flags`' own atom matching.
        let cpv_slot = standalone_cpv_slot(&env.pkg_dir, &env.category, &env.split.pf)?;
        let mut flags = flags;
        // Real's layer stacking: the run-wide base is the full resolved
        // config env here (`phase_environ`), an incremental `package.env`
        // value folds onto it in `[base, pkg, calling-env]` order, and a
        // scalar loses to the calling environment when it carries the
        // same key (#101).
        let profile_only_variables = config
            .resolved_incremental("PROFILE_ONLY_VARIABLES")
            .unwrap_or_default();
        let base = flags.clone();
        flags.extend(match_package_env_vars(
            &config.package_env_vars,
            &cpv_slot,
            &profile_only_variables,
            &base,
            &portage_profile::config_env_all(),
        ));
        // Per-package `FEATURES` (#98): an incremental, so a flat pair
        // would *replace* the run-wide folded list. Fold the matched
        // raw value onto the resolved run-wide list in `[run-wide, pkg,
        // calling-env]` order instead, and export both computed keys —
        // the same treatment the merge path's `entry_build_env` gives.
        // No append when nothing matched, so fixtures without a
        // `package.env` `FEATURES` entry keep byte-identical output.
        let pkg_features =
            match_package_env_incremental_raw(&config.package_env_vars, &cpv_slot, "FEATURES");
        if !pkg_features.is_empty() {
            let run_wide = config
                .resolved_incremental("FEATURES")
                .or_else(|| {
                    config
                        .other_vars
                        .get("FEATURES")
                        .map(|f| f.split_whitespace().map(String::from).collect())
                })
                .unwrap_or_default()
                .join(" ");
            let folded = fold_package_env_incremental(
                &run_wide,
                &pkg_features,
                &std::env::var("FEATURES").unwrap_or_default(),
            );
            flags.push(("FEATURES".to_string(), folded.clone()));
            flags.push(("PORTAGE_FEATURES".to_string(), folded));
        }
        Some((display, flags))
    })() else {
        return (String::new(), Vec::new());
    };
    let enabled: Vec<&str> = display
        .iter()
        .filter(|(_, on)| *on)
        .map(|(f, _)| f.as_str())
        .collect();
    (enabled.join(" "), flags)
}

/// Real `PORTAGE_RESTRICT`/`PORTAGE_PROPERTIES` (`doebuild_environment()`
/// sets both, unconditionally, for every phase): the ebuild's own
/// `RESTRICT`/`PROPERTIES` metadata, USE-reduced. Read from the same
/// repo's own `metadata/md5-cache` entry `fetch_sources`'s own `RESTRICT`
/// read already trusts. `use_set` is the reduction input -- empty for
/// every phase (no resolved graph reaches a standalone `ebuild <file>
/// <phase>`, and `entry_build_env`'s own resolved USE (an `emerge -b`
/// build) doesn't reach this deep yet either -- see this module's own
/// "KNOWN, DOCUMENTED GAPS"), except `depend` (see `depend_use_set`),
/// whose config-`USE` reduction real `doebuild(mydo="depend")` runs
/// with. `("", "")` outside any repo checkout, matching
/// `repo_root_for`'s own established tolerance.
pub(crate) fn restrict_and_properties(
    env: &Environment,
    use_set: &std::collections::HashSet<String>,
) -> (String, String) {
    let Some(repo_root) = repo_root_for(&env.pkg_dir) else {
        return (String::new(), String::new());
    };
    let metadata = portage_repo::repo_aux_metadata(&repo_root, &env.category, &env.split.pf).ok();
    let get = |key: &str| {
        metadata
            .as_ref()
            .and_then(|m| m.get(key))
            .map(String::as_str)
            .unwrap_or("")
    };
    (
        flat_field_on(get("RESTRICT"), use_set),
        flat_field_on(get("PROPERTIES"), use_set),
    )
}

/// Real per-package `PORTAGE_TMPDIR` (backlog #99): `_grab_pkg_env`
/// accepts the key into `configdict["pkg"]` (it is not in
/// `env_blacklist`, `environ_filter`, `global_only_vars` or the
/// profile-only set), and `doebuild_environment` re-derives
/// `PORTAGE_TMPDIR`/`BUILD_PREFIX`/`PORTAGE_BUILDDIR` from the resolved
/// value (`doebuild.py:435-458`, `:504-506`).
///
/// Atom-match just this key against `package_env_vars` (last matching
/// file wins — scalar semantics; an empty value blanks to absent) and,
/// when the calling environment does NOT carry `PORTAGE_TMPDIR` itself
/// (real's `env` layer outranks `pkg`, #101), re-derive the tmpdir from
/// the matched value. `run_tmpdir` is the caller's otherwise-resolved
/// tmpdir and stands whenever nothing matched. `process_tmpdir` is
/// `Some` iff the process environment carries the key — a CLI-boundary
/// read threaded in as a parameter (never an ambient read here), so the
/// precedence is unit-testable without touching process-global state.
/// A matched directory that does not exist is real's `_check_temp_dir`
/// failure (`doebuild.py:1682`), byte for byte; the caller maps the
/// `Err` to exit 1.
pub(crate) fn resolve_entry_portage_tmpdir(
    package_env_vars: &[(String, Vec<(String, String)>)],
    cpv_slot: &str,
    run_tmpdir: &Path,
    process_tmpdir: Option<&Path>,
) -> Result<PathBuf, String> {
    if let Some(process) = process_tmpdir {
        return Ok(process.to_path_buf());
    }
    let matched = match_package_env_scalar_raw(package_env_vars, cpv_slot, "PORTAGE_TMPDIR");
    let Some(dir) = matched.as_deref().filter(|v| !v.is_empty()) else {
        return Ok(run_tmpdir.to_path_buf());
    };
    if !Path::new(dir).is_dir() {
        return Err(format!(
            "The directory specified in your PORTAGE_TMPDIR variable, '{dir}',\n\
             does not exist.  Please create this directory or correct your PORTAGE_TMPDIR setting."
        ));
    }
    Ok(PathBuf::from(dir))
}

/// Standalone half of the above, for an `ebuild <file>` run with no
/// resolved graph entry: load the resolved config the same way
/// `phase_standalone_base_env` does, rebuild the ebuild's own
/// `cat/pkg-ver:slot/sub` identity from its path + md5-cache `SLOT`,
/// and resolve. `None` config (outside any repo checkout) or an
/// unparsable path keeps `run_tmpdir` — the later `compute_environment`
/// owns those errors, not this resolver. No process-global reads: the
/// caller (`ebuild.rs`) owns the `PORTAGE_TMPDIR` presence check.
pub(crate) fn resolve_standalone_portage_tmpdir(
    ebuild_path: &Path,
    config_root: &Path,
    eroot: &Path,
    run_tmpdir: &Path,
    process_tmpdir: Option<&Path>,
) -> Result<PathBuf, String> {
    if process_tmpdir.is_some() {
        return Ok(run_tmpdir.to_path_buf());
    }
    let Some((config, cpv_slot)) = standalone_package_env_lookup(ebuild_path, config_root, eroot)
    else {
        return Ok(run_tmpdir.to_path_buf());
    };
    resolve_entry_portage_tmpdir(&config.package_env_vars, &cpv_slot, run_tmpdir, None)
}

/// Real per-package `FEATURES` (backlog #130) for a standalone `ebuild
/// <file> merge`/`qmerge`: fold a matched `package.env` value onto
/// `run_wide_features` in real's `[run-wide, pkg, calling-env]` order
/// (`fold_package_env_incremental`), the same rule `entry_resolved_
/// features`/`resolved_features_for` (`emerge_build.rs`) apply on the
/// resolved-graph paths. `None` when nothing resolves (outside a repo
/// checkout, unparsable path) or nothing matched -- the caller keeps its
/// own already-resolved `FEATURES` value untouched.
pub(crate) fn resolve_standalone_resolved_features(
    ebuild_path: &Path,
    config_root: &Path,
    eroot: &Path,
    run_wide_features: &str,
    calling_features: &str,
) -> Option<String> {
    let (config, cpv_slot) = standalone_package_env_lookup(ebuild_path, config_root, eroot)?;
    let pkg_raw =
        match_package_env_incremental_raw(&config.package_env_vars, &cpv_slot, "FEATURES");
    if pkg_raw.is_empty() {
        return None;
    }
    Some(fold_package_env_incremental(
        run_wide_features,
        &pkg_raw,
        calling_features,
    ))
}

/// Real per-package `BINPKG_COMPRESS*` (backlog #147 S3) for a
/// standalone `ebuild <file> package`: the matched scalar triple
/// `(compress, flags, bzip2_command)` -- `flags`/`bzip2_command`
/// each `None` when neither the match nor (for flags) its
/// `FLAGS_<NAME>` override names one, in which case the caller
/// keeps its run-wide value. Returns `None` when nothing resolves
/// (outside a repo checkout, unparsable path) or nothing matched.
///
/// Real reads all three from the package's own `mysettings`
/// (`doebuild.py:697-750`), and none is in real's
/// `non_user_variables`, so the match uses the S2-accepted key set
/// (`package_env_key_allowed`); the per-key calling-env-wins
/// scalar precedence (#101) stays the caller's job -- it already
/// resolved the run-wide triple from the calling env, so it masks
/// the match exactly where real would. The caller also keeps the
/// triple run-wide for `gpkg` runs: real's helper reads the global
/// settings (`bin/gpkg-helper.py:49`), never the package's (S0
/// B-gpkg).
pub(crate) fn resolve_standalone_binpkg_compress(
    ebuild_path: &Path,
    config_root: &Path,
    eroot: &Path,
) -> Option<(String, Option<String>, Option<String>)> {
    let (config, cpv_slot) = standalone_package_env_lookup(ebuild_path, config_root, eroot)?;
    let profile_only = config
        .resolved_incremental("PROFILE_ONLY_VARIABLES")
        .unwrap_or_default();
    let matched = |key: &str| {
        if !package_env_key_allowed(key, &profile_only) {
            return None;
        }
        match_package_env_scalar_raw(&config.package_env_vars, &cpv_slot, key)
    };
    let compress = matched("BINPKG_COMPRESS")?;
    let flags = matched(&format!(
        "BINPKG_COMPRESS_FLAGS_{}",
        compress.to_uppercase()
    ))
    .or_else(|| matched("BINPKG_COMPRESS_FLAGS"));
    let bzip2 = matched("PORTAGE_BZIP2_COMMAND");
    Some((compress, flags, bzip2))
}

/// Real global scalar (backlog #180) for a standalone `ebuild <file>
/// package`: the resolved chain value below the calling env
/// (`make.conf`/profile/`make.globals` via `other_vars` -- the same
/// `resolve_config` chain every other standalone helper reads). The
/// calling-env-wins scalar precedence (#101) stays the caller's job --
/// it checks the process env first, exactly as it already does for the
/// `BINPKG_COMPRESS` triple. `None` when nothing resolves
/// (outside a repo checkout, unparsable path) or nothing in the chain
/// names a value, in which case the caller keeps real `make.globals`'s
/// own default. Deliberately not `package.env`-matched: a global
/// scalar, not a per-package one.
pub(crate) fn resolve_standalone_chain_scalar(
    ebuild_path: &Path,
    config_root: &Path,
    eroot: &Path,
    key: &str,
) -> Option<String> {
    let (config, _) = standalone_package_env_lookup(ebuild_path, config_root, eroot)?;
    config.other_vars.get(key).cloned()
}

/// Real `BINPKG_FORMAT` (backlog #173) for a standalone `ebuild <file>
/// package`: `resolve_standalone_chain_scalar` for that key (see its
/// own doc comment for the chain, the caller-kept env precedence, and
/// the `None` contract).
pub(crate) fn resolve_standalone_binpkg_format(
    ebuild_path: &Path,
    config_root: &Path,
    eroot: &Path,
) -> Option<String> {
    resolve_standalone_chain_scalar(ebuild_path, config_root, eroot, "BINPKG_FORMAT")
}

/// The resolved global `Config` for the config-less `emerge -C` path
/// (`execute_unmerge`'s `FEATURES=unmerge-backup` quickpkg, backlog
/// #173): the same `resolve_config` chain (`make.globals` → profile
/// `make.defaults` → `make.conf`) every other path reads, minus any
/// package identity -- `BINPKG_FORMAT` is a global scalar, not a
/// per-package one, so no ebuild path / `package.env` match is needed
/// (unlike the standalone helpers above, which resolve through an
/// ebuild's own path). `None` when no config resolves (missing
/// `repos.conf`, no main repo, `resolve_config` error) -- the caller
/// keeps real `make.globals`'s own default
/// (`PackageOptions::default().binpkg_format`).
pub(crate) fn resolve_unmerge_config(
    config_root: &Path,
    eroot: &Path,
) -> Option<portage_profile::Config> {
    let repos = portage_repo::find_repos(config_root).ok()?;
    let main_repo = repos.iter().find(|r| r.is_main)?;
    let overlay_repos: Vec<(String, PathBuf)> = repos
        .iter()
        .filter(|r| !r.is_main)
        .map(|r| (r.name.clone(), r.location.clone()))
        .collect();
    let repo_aliases: Vec<(String, PathBuf)> = repos
        .iter()
        .flat_map(|r| r.aliases.iter().map(|a| (a.clone(), r.location.clone())))
        .collect();
    let repo_masters: std::collections::HashMap<String, Vec<PathBuf>> = repos
        .iter()
        .map(|r| (r.name.clone(), r.masters.clone()))
        .collect();
    portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root,
        main_repo_location: &main_repo.location,
        overlay_repos: &overlay_repos,
        repo_aliases: &repo_aliases,
        main_repo_name: &main_repo.name,
        repo_masters: &repo_masters,
        eroot,
    })
    .ok()
}
