//! The per-entry build environment.

use crate::emerge_build::*;

/// Real per-package `PORTAGE_TMPDIR` (backlog #99) for one scheduler
/// entry: re-derive the tmpdir this entry's build directories live under
/// from its matched `package.env` value
/// (`ebuild_phases::resolve_entry_portage_tmpdir`), so the pre-clean,
/// the phase chain, the build log and the post-merge clean all share
/// the same per-entry root. A missing matched directory is real's
/// `_check_temp_dir` failure and aborts the entry before anything runs.
pub(crate) fn entry_portage_tmpdir(
    options: &ebuild_merge::MergeOptions,
    entry: &GraphEntry,
    version: &str,
    portage_tmpdir: &Path,
) -> Result<PathBuf, String> {
    let cpv_slot = entry_cpv_slot(entry, version);
    crate::ebuild_phases::resolve_entry_portage_tmpdir(
        &options.package_env_vars,
        &cpv_slot,
        portage_tmpdir,
        options.process_tmpdir.as_deref(),
    )
}

/// Real per-package `FEATURES` (backlog #98) for one `cat/pkg-ver:slot/sub`
/// identity, given the `package.env` table and the run-wide folded
/// `FEATURES` list to fold onto: `FEATURES` is an incremental, so the
/// matched env-file value folds in `[run-wide, pkg, calling-env]` order
/// (`regenerate()`, `config.py:2735`, `:2778-2825`) instead of replacing
/// it. `None` when nothing matched — the caller then keeps the run-wide
/// value untouched, so entries without a `package.env` `FEATURES` entry
/// keep byte-identical output. Raw pieces, not `&MergeOptions`, so
/// `run_buildpkgonly` (backlog #129, no `MergeOptions` in scope) shares
/// this with the merge scheduler's [`entry_resolved_features`].
pub(crate) fn resolved_features_for(
    package_env_vars: &[(String, Vec<(String, String)>)],
    cpv_slot: &str,
    run_wide_features: &str,
    calling_features: &str,
) -> Option<String> {
    let pkg_raw = crate::ebuild_phases::match_package_env_incremental_raw(
        package_env_vars,
        cpv_slot,
        "FEATURES",
    );
    if pkg_raw.is_empty() {
        return None;
    }
    Some(crate::ebuild_phases::fold_package_env_incremental(
        run_wide_features,
        &pkg_raw,
        calling_features,
    ))
}

/// [`resolved_features_for`] for one scheduler entry: `calling_features`
/// is the raw calling-environment value (a parameter, not an ambient
/// read, so the fold stays unit-testable); the run-wide base is
/// `options.features` (the resolved list on `emerge` paths, the raw
/// process fallback elsewhere). `None` when the entry is not buildable
/// either.
pub(crate) fn entry_resolved_features(
    options: &ebuild_merge::MergeOptions,
    entry: &GraphEntry,
    calling_features: &str,
) -> Option<String> {
    let version = entry_version(&entry.outcome)?;
    let cpv_slot = entry_cpv_slot(entry, version);
    resolved_features_for(
        &options.package_env_vars,
        &cpv_slot,
        &options.features,
        calling_features,
    )
}

/// `[("USE", "<space-joined enabled IUSE flags>")]` for `entry` -- the
/// build-phase env every `emerge <atom>` source build/merge passes so
/// `bin/ebuild.sh`'s `use()` sees the resolved flags. Empty vec (no
/// `USE` entry) when the entry declares no enabled flags, so
/// `phase_env_vars`' own `USE=""` stands. `GraphEntry::use_flags_display`
/// is already the package's IUSE, enabled-resolved and bare-name-sorted.
pub(crate) fn build_use_env(entry: &GraphEntry) -> Vec<(String, String)> {
    let enabled: Vec<&str> = entry
        .use_flags_display
        .iter()
        .filter(|(_, on)| *on)
        .map(|(f, _)| f.as_str())
        .collect();
    if enabled.is_empty() {
        Vec::new()
    } else {
        vec![("USE".to_string(), enabled.join(" "))]
    }
}

/// The `cat/pkg-ver:slot/sub` identity real `_grab_pkg_env` matches a
/// build-bound entry against (`entry_package_env_vars` and the
/// per-package `PORTAGE_TMPDIR` resolution below share it). Missing
/// `SLOT` means slot `0`, and a missing sub-slot repeats the slot.
pub(crate) fn entry_cpv_slot(entry: &GraphEntry, version: &str) -> String {
    let slot = entry.slot.as_deref().unwrap_or("0");
    let sub_slot = entry.sub_slot.as_deref().unwrap_or(slot);
    format!(
        "{}/{}-{version}:{slot}/{sub_slot}",
        entry.category, entry.package
    )
}

/// The `package.env` build vars that match one `cat/pkg-ver:slot/sub`
/// identity -- real `_grab_pkg_env` folding a matching
/// `/etc/portage/package.env` entry's env file into `configdict["pkg"]`,
/// with real's acceptance set (`match_package_env_vars`). Incrementals
/// fold onto `base_env` (the run-wide resolved env) instead of replacing
/// it, matching `regenerate()`'s layer stacking. Raw pieces, not
/// `&MergeOptions`, so `run_buildpkgonly` (backlog #129, no
/// `MergeOptions` in scope) shares this with the merge scheduler's
/// [`entry_package_env_vars`].
pub(crate) fn matched_package_env_vars(
    package_env_vars: &[(String, Vec<(String, String)>)],
    cpv_slot: &str,
    profile_only_variables: &[String],
    base_env: &[(String, String)],
) -> Vec<(String, String)> {
    if package_env_vars.is_empty() {
        return Vec::new();
    }
    crate::ebuild_phases::match_package_env_vars(
        package_env_vars,
        cpv_slot,
        profile_only_variables,
        base_env,
        &portage_profile::config_env_all(),
    )
}

/// [`matched_package_env_vars`] for one scheduler entry: `entry`'s cpv
/// against `options.package_env_vars`, layered over `options.build_env`.
pub(crate) fn entry_package_env_vars(
    options: &ebuild_merge::MergeOptions,
    entry: &GraphEntry,
) -> Vec<(String, String)> {
    let Some(version) = entry_version(&entry.outcome) else {
        return Vec::new();
    };
    let cpv_slot = entry_cpv_slot(entry, version);
    let profile_only_variables = options
        .resolved_config
        .as_deref()
        .and_then(|config| config.resolved_incremental("PROFILE_ONLY_VARIABLES"))
        .unwrap_or_default();
    matched_package_env_vars(
        &options.package_env_vars,
        &cpv_slot,
        &profile_only_variables,
        &options.build_env,
    )
}

/// `SLOT`, `PORTAGE_REPO_NAME`, `PORTAGE_REPO_REVISIONS` for `entry` --
/// the per-entry identity real `doebuild_environment()` sets
/// (`doebuild.py:483`) and `EbuildPhase._setup_repo_revisions` builds
/// (`EbuildPhase.py:73-108`). `SLOT` is the ebuild's own `SLOT`
/// (`slot/sub_slot` when a sub-slot is declared, bare slot otherwise,
/// matching the ebuild global real `config.environ()` carries), never
/// empty (`entry.slot` unset -> `"0"`; the `.keep_<cp>-` bug is exactly
/// an empty slot). `PORTAGE_REPO_REVISIONS` is `"{}"` until portuale
/// tracks a repo revision (real `json.dumps({}, sort_keys=True)`).
/// `PORTAGE_REPO_NAME` is omitted only when no repo is known at all.
pub(crate) fn entry_identity_env(
    entry: &GraphEntry,
    candidate: Option<&Candidate>,
) -> Vec<(String, String)> {
    let slot = entry
        .slot
        .as_deref()
        .or_else(|| candidate.map(|c| c.slot.as_str()))
        .unwrap_or("0");
    let sub_slot = entry
        .sub_slot
        .as_deref()
        .or_else(|| candidate.map(|c| c.sub_slot.as_str()))
        .unwrap_or(slot);
    let slot_value = if sub_slot.is_empty() || sub_slot == slot {
        slot.to_string()
    } else {
        format!("{slot}/{sub_slot}")
    };
    let repo_name = entry
        .repo_name
        .as_deref()
        .or_else(|| candidate.map(|c| c.repo_name.as_str()))
        .unwrap_or("");
    let mut env = vec![
        ("SLOT".to_string(), slot_value),
        ("PORTAGE_REPO_REVISIONS".to_string(), "{}".to_string()),
    ];
    if !repo_name.is_empty() {
        env.push(("PORTAGE_REPO_NAME".to_string(), repo_name.to_string()));
    }
    env
}

/// The run-wide half of real `config.environ()` for a build:
/// `portage_profile::phase_environ(config, None)` plus the dynamic
/// `doebuild_environment()` value that needs portuale's compressor table,
/// `PORTAGE_COMPRESSION_COMMAND` (`doebuild.py:697-750`, set for every
/// build regardless of `BINPKG_FORMAT`). `BINPKG_COMPRESS*` are
/// `environ_filter`ed, so they are read from the calling env / config
/// directly; `MAKEOPTS` from the (already-defaulted) export set.
pub(crate) fn run_wide_phase_env(config: &portage_profile::Config) -> Vec<(String, String)> {
    let mut env = portage_profile::phase_environ(config, None);
    let lookup = |key: &str| {
        env.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .or_else(|| std::env::var(key).ok())
            .or_else(|| config.other_vars.get(key).cloned())
    };
    if let Some(cmd) = ebuild_package::phase_compression_command(lookup) {
        env.push(("PORTAGE_COMPRESSION_COMMAND".to_string(), cmd));
    }
    env
}

/// The package metadata keys real `config.setcpv` loads into
/// `configdict["pkg"]` (`_setcpv_aux_keys`, `config.py:171-189`) that
/// survive `environ_filter` and stay exported in the saved environment:
/// `bin/ebuild.sh` unsets `IUSE`/`REQUIRED_USE`/`PROPERTIES`/`RESTRICT`/
/// `INHERITED`/`EAPI` before sourcing the ebuild (`:634-635`), so only
/// these three keep real's `declare -x`; `SLOT`/`EAPI`/`INHERITED` are
/// computed elsewhere. Values from the md5-cache (real `aux_get`; an
/// omitted key is `""`).
pub(crate) const EXPORTED_PKG_METADATA: [&str; 3] = ["DEFINED_PHASES", "KEYWORDS", "LICENSE"];

pub(crate) fn entry_metadata_env(
    entry: &GraphEntry,
    candidate: &Candidate,
) -> Vec<(String, String)> {
    let pf = format!("{}-{}", entry.package, candidate.version);
    // H4: the repo-metadata decision point (#41's `repo_aux_metadata`:
    // repo md5-cache -> depcachedir -> depend phase) is reached through
    // the `RepoCache` slot on this production path, the same
    // interchangeable-traits seam the scheduler/merge/news/fetch/
    // packages/binpkg slots use. `Md5Cache::metadata` delegates to the
    // identical function the direct call used, so this is
    // behaviour-neutral.
    let cache = mrg_director::Md5Cache::new(&candidate.repo_location, &candidate.repo_name);
    let Ok(metadata) = mrg_director::RepoCache::metadata(&cache, &entry.category, &pf) else {
        return Vec::new();
    };
    EXPORTED_PKG_METADATA
        .iter()
        .map(|k| (k.to_string(), metadata.get(*k).cloned().unwrap_or_default()))
        .collect()
}

/// The per-entry tail of [`entry_build_env`]: resolved `USE` (or the
/// legacy enabled-IUSE-only `USE` when no config is in scope), the
/// `IUSE_EFFECTIVE`/`USE_EXPAND` rows, and the `SLOT`/repo identity.
/// Shared with `run_buildpkgonly`, whose `PackageOptions` carries no
/// `MergeOptions`.
///
/// With `config` set (`emerge <atom>` and friends, #37 S2) the `USE` is
/// real `PORTAGE_USE`: the resolver's full effective set
/// (`candidate_effective_use_flags`, which includes the implicit profile
/// flags no package declares in `IUSE`) narrowed by `portage_use` to
/// `IUSE ∪ IUSE_EFFECTIVE`. Without a config (standalone `ebuild <file>
/// merge`/`qmerge`, tests) the previous enabled-IUSE-only `USE` stands.
pub(crate) fn entry_phase_env_tail(
    config: Option<&portage_profile::Config>,
    repos: &[RepoConfig],
    entry: &GraphEntry,
    candidate: Option<&Candidate>,
) -> Vec<(String, String)> {
    let mut env = Vec::new();
    let Some(config) = config else {
        env.extend(build_use_env(entry));
        return env;
    };
    if let Some(candidate) = candidate {
        env.extend(entry_metadata_env(entry, candidate));
        let enabled = portage_repo::candidate_effective_use_flags(
            repos,
            config,
            &entry.category,
            &entry.package,
            &candidate.version,
        );
        let iuse: Vec<String> = candidate
            .iuse
            .split_whitespace()
            .map(String::from)
            .collect();
        env.extend(portage_profile::phase_environ_pkg(
            config,
            Some(portage_profile::PhaseUse {
                iuse: &iuse,
                enabled: &enabled,
            }),
        ));
    }
    env.extend(entry_identity_env(entry, candidate));
    env
}

/// The full per-entry build-phase env: the run-wide resolved env the
/// caller stashed on `options.build_env` (`portage_profile::
/// phase_environ(config, None)`), then any per-package `package.env`
/// build vars on top of those, then [`entry_phase_env_tail`].
pub(crate) fn entry_build_env(
    options: &ebuild_merge::MergeOptions,
    entry: &GraphEntry,
    repos: &[RepoConfig],
) -> Vec<(String, String)> {
    let candidate = entry_version(&entry.outcome)
        .and_then(|version| locate_candidate(repos, &entry.category, &entry.package, version));
    // Backlog #332: re-read env.d for this package task
    // (`env_d_reloaded`); unchanged `profile.env` keeps the old path.
    let reloaded = options.resolved_config.as_deref().and_then(env_d_reloaded);
    let mut env = match &reloaded {
        Some(r) => run_wide_phase_env(r),
        None => options.build_env.clone(),
    };
    env.extend(entry_package_env_vars(options, entry));
    // Per-package `FEATURES` (#98): the folded per-entry list overrides
    // the run-wide `FEATURES`/`PORTAGE_FEATURES` pair downstream
    // (last-wins), so the phase env and every `extra_env` gate see it.
    // `None` (no match) appends nothing, so entries without a
    // `package.env` `FEATURES` entry keep byte-identical output.
    let calling_features = std::env::var("FEATURES").unwrap_or_default();
    if let Some(features) = entry_resolved_features(options, entry, &calling_features) {
        env.push(("FEATURES".to_string(), features.clone()));
        env.push(("PORTAGE_FEATURES".to_string(), features));
    }
    env.extend(entry_phase_env_tail(
        reloaded.as_ref().or(options.resolved_config.as_deref()),
        repos,
        entry,
        candidate.as_ref(),
    ));
    // Real per-package `BINPKG_COMPRESS` (backlog #147 S2): same
    // re-derivation as `buildpkgonly_entry_build_env` -- this env
    // feeds the merge-scheduler builds whose `package_after_install`
    // side effect (`FEATURES=buildpkg` / `-b`) reads it back through
    // the saved `${T}/environment`. The resolved config may be absent
    // on the config-less paths (`None`: pairs-then-env only).
    crate::ebuild_package::refresh_entry_compression_command(
        &mut env,
        reloaded.as_ref().or(options.resolved_config.as_deref()),
    );
    env
}
