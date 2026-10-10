//! `package.env` matching and its incremental variables.

use crate::ebuild_phases::*;

/// Real `const.INCREMENTALS` (`3rdparty/portage/lib/portage/const.py:125`):
/// a key in this list is appended to the container's current value by
/// `_grab_pkg_env`, then folded onto the lower layers by `regenerate()`
/// (`config.py:2778-2825`: `-*` clears the set, `-tok` removes, sorted
/// union). Kept complete even though the acceptance gate below drops most
/// of them, so a future gate change cannot silently lose the semantics.
pub(crate) const PACKAGE_ENV_INCREMENTALS: [&str; 12] = [
    "ACCEPT_KEYWORDS",
    "CONFIG_PROTECT",
    "CONFIG_PROTECT_MASK",
    "ENV_UNSET",
    "FEATURES",
    "IUSE_IMPLICIT",
    "PROFILE_ONLY_VARIABLES",
    "USE",
    "USE_EXPAND",
    "USE_EXPAND_HIDDEN",
    "USE_EXPAND_IMPLICIT",
    "USE_EXPAND_UNPREFIXED",
];

pub(crate) fn is_package_env_incremental(key: &str) -> bool {
    PACKAGE_ENV_INCREMENTALS.contains(&key)
}

/// Real `_grab_pkg_env`'s per-key acceptance gate
/// (`config.py:2269-2300`), minus the keys portuale's own phase pipeline
/// owns:
///
/// * `USE` is real's special case (popped before the call, re-appended
///   after `package.use`); portuale folds it via
///   `Config::package_env_use`, so it must not also appear here.
/// * `ENV_BLACKLIST` is real's `env_blacklist` (it already covers
///   `PKGUSE`, the only `protected_pkg_keys` member).
/// * `ENVIRON_FILTER` keys real accepts into the config but never exports
///   to a phase (`config.environ()`, `config.py:3263-3350`); portuale's
///   `package_env_vars` layer only exists in the phase env, so they are
///   dropped.
/// * `PORTUALE_COMPUTED` keys portuale's own phase runner computes and
///   must always win (`FEATURES` and `PORTAGE_TMPDIR` are filed residues
///   #98/#99; `PATH`, `DISTDIR`, `D`, … are owned by design).
/// * `profile_only_variables` is the profile's dynamic
///   `PROFILE_ONLY_VARIABLES` (`config.py:724-733`; `ARCH`, `ELIBC`,
///   `IUSE_IMPLICIT`, `USE_EXPAND*`, …).
///
/// Everything else is accepted, including empty values (real blanks a
/// variable with `VAR=""`) and `__`-prefixed names (real does not strip
/// them here).
pub(crate) fn package_env_key_allowed(key: &str, profile_only_variables: &[String]) -> bool {
    // Real `_grab_pkg_env` (backlog #95) rejects `protected_keys` ∪
    // `non_user_variables` (PROFILE_ONLY ∪ env_blacklist ∪
    // CONFIG_PROTECT); portuale additionally rejects its
    // `ENVIRON_FILTER` export-filter list here -- a deliberate
    // conservative narrowing, except for the compression family below.
    // Real honors a per-package `BINPKG_COMPRESS` (and its FLAGS
    // sisters: `doebuild.py:697-750` reads all three from the
    // package's own `mysettings`, and none is in real's
    // `non_user_variables` -- S0 B-xpak proves gzip magic), so
    // backlog #147 S2 exempts exactly that family; every other
    // `ENVIRON_FILTER` key stays rejected. `PORTAGE_BZIP2_COMMAND` is
    // listed for symmetry only: it is not on real's `environ_filter`,
    // so accepting it here exempts nothing.
    fn is_compression_key(key: &str) -> bool {
        key == "BINPKG_COMPRESS"
            || key == "BINPKG_COMPRESS_FLAGS"
            || key == "PORTAGE_BZIP2_COMMAND"
            || key
                .strip_prefix("BINPKG_COMPRESS_FLAGS_")
                .is_some_and(|rest| !rest.is_empty())
    }
    !key.is_empty()
        && key != "USE"
        && !portage_profile::ENV_BLACKLIST.contains(&key)
        && !(portage_profile::ENVIRON_FILTER.contains(&key) && !is_compression_key(key))
        && !portage_profile::PORTUALE_COMPUTED.contains(&key)
        && !profile_only_variables.iter().any(|k| k == key)
}

/// Real `regenerate()`'s incremental fold (`config.py:2735`, `:2778-2825`)
/// for one key, in real's layer order `[base-lower, pkg, calling-env]`:
/// the already-folded lower layers' tokens, then the package.env value's
/// tokens in order, then the calling environment's tokens in order
/// (`-*` clears, `-tok` removes), sorted and space-joined. The calling
/// env folds **last**, so a `-tok` there prunes a token a package.env
/// file added (backlog #101, S0 cell B), while a package.env `-tok`
/// cannot prune a calling-env token. Re-applying the calling-env tokens
/// after a base that already folded them in is idempotent (set
/// add/remove), so `base_lower` may be the caller's own already-folded
/// value for the key (`""` when the run-wide env doesn't carry it).
pub(crate) fn fold_package_env_incremental(
    base_lower: &str,
    pkg: &str,
    calling_env: &str,
) -> String {
    let mut set: std::collections::BTreeSet<String> =
        base_lower.split_whitespace().map(String::from).collect();
    for tok in pkg.split_whitespace().chain(calling_env.split_whitespace()) {
        if tok == "-*" {
            set.clear();
        } else if let Some(rest) = tok.strip_prefix('-') {
            set.remove(rest);
        } else {
            set.insert(tok.to_string());
        }
    }
    set.into_iter().collect::<Vec<_>>().join(" ")
}

/// Atom-match `package_env_vars` (real `_grab_pkg_env` folding a
/// matching `/etc/portage/package.env` entry's env file into
/// `configdict["pkg"]`) against one `cat/pkg-ver:slot/sub` string.
///
/// The container follows real exactly: files of one entry in order,
/// matching entries in list order (portuale keeps parse order; real
/// applies `ordered_by_atom_specificity`, a pre-existing narrowing), an
/// incremental appends, a scalar replaces, an empty value blanks.
/// Incrementals are then folded in real's layer order
/// `[base-lower, pkg, calling-env]` (`regenerate()`), and a package.env
/// **scalar is dropped when the calling environment carries the same
/// key**: real's `USE_ORDER` puts the `env` layer above `pkg`
/// (`config.py:1031-1035`), so the process value wins (backlog #101, S0
/// cell A). `calling_env` is the same source `phase_environ`'s step 2
/// uses (`portage_profile::config_env_all()`), compared by key presence
/// — *not* `base_env` as a whole, which also contains config scalars a
/// package.env value legitimately outranks.
///
/// Shared by the merge path (`emerge_build::entry_package_env_vars`,
/// matching a resolved graph entry and passing `options.build_env`) and
/// the standalone path below (matching the ebuild's own md5-cache
/// identity and passing the narrow base -- no resolved graph needed).
/// Both paths thread the real calling environment, so the precedence
/// rule holds on both: the S0 capture shows the standalone path is
/// inverted today too, the same bug in this shared function (S2's base
/// replacement is a separate change and is untouched here).
pub(crate) fn match_package_env_vars(
    package_env_vars: &[(String, Vec<(String, String)>)],
    cpv_slot: &str,
    profile_only_variables: &[String],
    base_env: &[(String, String)],
    calling_env: &[(String, String)],
) -> Vec<(String, String)> {
    let mut container: Vec<(String, String)> = Vec::new();
    for (atom, vars) in package_env_vars {
        if !portage_dep::match_from_list(atom, &[cpv_slot]).is_some_and(|m| !m.is_empty()) {
            continue;
        }
        for (k, v) in vars {
            if !package_env_key_allowed(k, profile_only_variables) {
                continue;
            }
            if is_package_env_incremental(k) {
                match container.iter_mut().rev().find(|(ck, _)| ck == k) {
                    Some((_, current)) => {
                        if !current.is_empty() && !v.is_empty() {
                            current.push(' ');
                        }
                        current.push_str(v);
                    }
                    None => container.push((k.clone(), v.clone())),
                }
            } else {
                container.retain(|(ck, _)| ck != k);
                container.push((k.clone(), v.clone()));
            }
        }
    }
    container
        .into_iter()
        .filter_map(|(k, v)| {
            if is_package_env_incremental(&k) {
                let base = base_env
                    .iter()
                    .rev()
                    .find(|(bk, _)| *bk == k)
                    .map(|(_, bv)| bv.as_str())
                    .unwrap_or("");
                let calling = calling_env
                    .iter()
                    .rev()
                    .find(|(ck, _)| *ck == k)
                    .map(|(_, cv)| cv.as_str())
                    .unwrap_or("");
                Some((k, fold_package_env_incremental(base, &v, calling)))
            } else if calling_env.iter().any(|(ck, _)| *ck == k) {
                // Real's `env` layer outranks `pkg` for a scalar: the
                // process value (already in the caller's base env) wins,
                // so the package.env value is dropped, not layered.
                None
            } else {
                Some((k, v))
            }
        })
        .collect()
}

/// Atom-match a single `package.env` key against `cpv_slot` and return
/// the container value with real `_grab_pkg_env`'s append semantics but
/// WITHOUT the acceptance gate and WITHOUT folding: matching entries in
/// list order, files of one entry in order.
///
/// Two flavours, mirroring real's container behaviour exactly: scalars
/// replace (last matching file wins; `None` when nothing matched),
/// incrementals append (`container[k] += " " + v`). The caller owns the
/// key choice and the fold: `match_package_env_vars` (the gated,
/// phase-env export path) keeps its own loop, while per-key recomputes
/// a flat pair would corrupt — `PORTAGE_TMPDIR` (#99) and `FEATURES`
/// (#98), both `PORTUALE_COMPUTED` — match raw and re-derive instead.
pub(crate) fn match_package_env_scalar_raw(
    package_env_vars: &[(String, Vec<(String, String)>)],
    cpv_slot: &str,
    key: &str,
) -> Option<String> {
    let mut matched: Option<&str> = None;
    for (atom, vars) in package_env_vars {
        if !portage_dep::match_from_list(atom, &[cpv_slot]).is_some_and(|m| !m.is_empty()) {
            continue;
        }
        for (k, v) in vars {
            if k == key {
                matched = Some(v.as_str());
            }
        }
    }
    matched.map(String::from)
}

pub(crate) fn match_package_env_incremental_raw(
    package_env_vars: &[(String, Vec<(String, String)>)],
    cpv_slot: &str,
    key: &str,
) -> String {
    let mut container = String::new();
    for (atom, vars) in package_env_vars {
        if !portage_dep::match_from_list(atom, &[cpv_slot]).is_some_and(|m| !m.is_empty()) {
            continue;
        }
        for (k, v) in vars {
            if k == key {
                if !container.is_empty() && !v.is_empty() {
                    container.push(' ');
                }
                container.push_str(v);
            }
        }
    }
    container
}

/// The resolved `Config` and `cat/pkg-ver:slot/sub` identity real
/// `_grab_pkg_env` would match, for an ebuild with no resolved graph
/// entry (standalone `ebuild <file>`/`qmerge`): the ebuild's own path
/// gives the `category`/`pf`, [`resolve_standalone_config`] the
/// `package_env_vars` table, [`standalone_cpv_slot`] the md5-cache
/// `SLOT`. `None` outside a repo checkout or on an unparsable path --
/// shared by [`resolve_standalone_portage_tmpdir`] (#99) and
/// [`resolve_standalone_resolved_features`] (#130) so the path-parsing
/// only happens once.
pub(crate) fn standalone_package_env_lookup(
    ebuild_path: &Path,
    config_root: &Path,
    eroot: &Path,
) -> Option<(portage_profile::Config, String)> {
    let ebuild_abs = ebuild_path.canonicalize().ok()?;
    let pkg_dir = ebuild_abs.parent()?.to_path_buf();
    let file_name = ebuild_abs.file_name()?.to_str()?;
    let pf = file_name.strip_suffix(".ebuild")?;
    let category = pkg_dir.parent()?.file_name()?.to_str()?;
    let config = resolve_standalone_config(&pkg_dir, config_root, eroot)?;
    let cpv_slot = standalone_cpv_slot(&pkg_dir, category, pf)?;
    Some((config, cpv_slot))
}
