//! The `<pkgdir>/Packages` index header portuale writes after building or
//! backing up a binpkg: Portage's full settings-derived key set, not just
//! `TIMESTAMP`/`VERSION`.
//!
//! Two snapshots, because Portage writes the header from two different
//! processes. A `--buildpkg`/`package` inject runs in the parent (`bintree.inject`
//! reads `self.settings`), while an `unmerge-backup` `quickpkg` runs in a
//! child that inherits the parent's resolved settings as its environment and
//! stacks them over the files again (`vartree._quickpkg_dblink`). The child
//! re-fold resurrects env-negated tokens (`sandbox` is back in its `FEATURES`)
//! and duplicates pruned ones (`ACCEPT_LICENSE: @FREE @FREE`).
//!
//! Scope cuts: `USE`/`ACCEPT_KEYWORDS`/`IUSE_IMPLICIT`/`USE_EXPAND*` reuse the
//! parent fold for the child (they differ only when the calling env negates
//! one of their tokens, which the per-layer sources below do not track);
//! `REPO_REVISIONS` (needs sync history) is never set; entry sorting stays
//! append order (the bed sorts stanzas before comparing).
// Portage: dbapi/bintree.py, _update_pkgindex_header; getbinpkg.py,
//   PackageIndex.write; package/ebuild/config.py, regenerate;
//   package/ebuild/_config/helper.py, prune_incremental

use crate::*;

/// The header key set for one writer, as `(key, value)` with `None`
/// meaning "ensure absent" (Portage pops falsy settings values on every
/// inject). Sorted by key, like Portage's `keys.sort()` write path.
pub type HeaderSnapshot = Vec<(String, Option<String>)>;

/// The two header snapshots: `parent` for the in-process `--buildpkg`
/// inject, `child` for the `quickpkg`-as-a-subprocess re-fold.
#[derive(Clone, Debug, Default)]
pub struct PkgIndexHeader {
    pub parent: HeaderSnapshot,
    pub child: HeaderSnapshot,
}

fn sorted_join(set: &HashSet<String>) -> Option<String> {
    (!set.is_empty()).then(|| {
        let mut tokens: Vec<&str> = set.iter().map(String::as_str).collect();
        tokens.sort_unstable();
        tokens.join(" ")
    })
}

fn scalar_var(config: &Config, key: &str) -> Option<String> {
    config
        .other_vars
        .get(key)
        .filter(|v| !v.is_empty())
        .cloned()
}

fn incremental_var(config: &Config, key: &str) -> Option<String> {
    config
        .resolved_incremental(key)
        .map(|tokens| tokens.join(" "))
        .filter(|v| !v.is_empty())
}

/// The stacked file-layer tokens for one `ACCEPT_*` key, in Portage's
/// stacking order (`globals`, profile `defaults`, `make.conf`), each layer
/// last-wins, with the `package.*` `*/*` tokens joined onto the `conf`
/// layer like Portage's `extract_global_changes` / `conf[...] += ...`.
fn accept_file_tokens(config: &Config, key: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for layer in [
        AcceptRawLayer::Globals,
        AcceptRawLayer::Defaults,
        AcceptRawLayer::Conf,
    ] {
        if let Some((_, _, tokens)) = config
            .accept_raw_layers
            .iter()
            .rev()
            .find(|(l, k, _)| *l == layer && k == key)
        {
            out.extend(tokens.iter().cloned());
        }
        if layer == AcceptRawLayer::Conf {
            let wildcards = match key {
                "ACCEPT_LICENSE" => &config.package_license,
                "ACCEPT_PROPERTIES" => &config.package_properties,
                "ACCEPT_RESTRICT" => &config.package_accept_restrict,
                _ => continue,
            };
            if let Some((_, tokens)) = wildcards.iter().find(|(atom, _)| atom == "*/*") {
                out.extend(tokens.iter().cloned());
            }
        }
    }
    out
}

fn env_tokens(config: &Config, key: &str) -> Vec<String> {
    config
        .pkgindex_env_raw
        .get(key)
        .map(|v| v.split_whitespace().map(String::from).collect())
        .unwrap_or_default()
}

/// Portage's stacked `ACCEPT_*` value: the file layers plus the calling-env
/// layer, order-preserving with duplicates kept.
fn accept_parent_value(config: &Config, key: &str) -> Option<String> {
    let mut stacked = accept_file_tokens(config, key);
    stacked.extend(env_tokens(config, key));
    let pruned = prune_token_layers(&stacked);
    (!pruned.is_empty()).then(|| pruned.join(" "))
}

/// The file layers of a tracked incremental for the child's re-fold: every
/// recorded source except the calling-env one (the last entry, pushed by
/// `apply_env_layer`, present exactly when the env set the key).
fn child_file_layers<'a>(config: &'a Config, key: &str) -> Vec<&'a [String]> {
    let Some(sources) = config.incremental_sources.get(key) else {
        return Vec::new();
    };
    let without_env = match config.pkgindex_env_raw.contains_key(key) {
        true => sources.len().saturating_sub(1),
        false => sources.len(),
    };
    sources[..without_env.min(sources.len())]
        .iter()
        .map(Vec::as_slice)
        .collect()
}

/// Build both header snapshots from a resolved config. Values are the
/// resolved settings, the same source the merge already uses; `TIMESTAMP`
/// and `PACKAGES` are set per write by the index writer, not here.
pub fn pkgindex_header(config: &Config) -> PkgIndexHeader {
    // The global USE Portage exposes in settings (and the header): the
    // resolved flags with profile `use.force` folded in and `use.mask`
    // removed. Same fold as `emerge --info`'s display.
    // Portage: config.py, regenerate
    let mut global_use: HashSet<String> = config.use_flags.clone();
    global_use.extend(config.use_force.iter().cloned());
    for masked in &config.use_mask {
        global_use.remove(masked);
    }
    let mut parent: HeaderSnapshot = Vec::new();
    for key in [
        "ARCH",
        "CBUILD",
        "CHOST",
        "ELIBC",
        "GENTOO_MIRRORS",
        "INSTALL_MASK",
        "KERNEL",
    ] {
        parent.push((key.to_string(), scalar_var(config, key)));
    }
    for (key, set) in [
        ("ACCEPT_KEYWORDS", &config.accept_keywords),
        ("IUSE_IMPLICIT", &config.iuse_implicit),
        ("USE", &global_use),
        ("USE_EXPAND", &config.use_expand),
        ("USE_EXPAND_HIDDEN", &config.use_expand_hidden),
        ("USE_EXPAND_IMPLICIT", &config.use_expand_implicit),
        ("USE_EXPAND_UNPREFIXED", &config.use_expand_unprefixed),
    ] {
        parent.push((key.to_string(), sorted_join(set)));
    }
    for key in ["CONFIG_PROTECT", "CONFIG_PROTECT_MASK", "FEATURES"] {
        parent.push((key.to_string(), incremental_var(config, key)));
    }
    for key in ["ACCEPT_LICENSE", "ACCEPT_PROPERTIES", "ACCEPT_RESTRICT"] {
        parent.push((key.to_string(), accept_parent_value(config, key)));
    }
    parent.push((
        "PROFILE".to_string(),
        (!config.pkgindex_profile.is_empty()).then(|| config.pkgindex_profile.clone()),
    ));
    parent.push(("VERSION".to_string(), Some("0".to_string())));
    parent.push((
        "URI".to_string(),
        scalar_var(config, "PORTAGE_BINHOST_HEADER_URI"),
    ));
    parent.push(("TTL".to_string(), scalar_var(config, "PORTAGE_BINHOST_TTL")));
    let mut implicits: Vec<&str> = config
        .use_expand_implicit
        .iter()
        .map(String::as_str)
        .collect();
    implicits.sort_unstable();
    for var in implicits {
        parent.push((
            format!("USE_EXPAND_VALUES_{var}"),
            scalar_var(config, &format!("USE_EXPAND_VALUES_{var}")),
        ));
    }

    let value_of = |snapshot: &[(String, Option<String>)], key: &str| -> Vec<String> {
        snapshot
            .iter()
            .find(|(k, _)| k == key)
            .and_then(|(_, v)| v.as_ref())
            .map(|v| v.split_whitespace().map(String::from).collect())
            .unwrap_or_default()
    };
    let mut child = parent.clone();
    for key in ["FEATURES", "CONFIG_PROTECT", "CONFIG_PROTECT_MASK"] {
        let mut layers = child_file_layers(config, key);
        let parent_tokens = value_of(&parent, key);
        layers.push(parent_tokens.as_slice());
        let folded = fold_token_layers(&layers);
        if let Some(slot) = child.iter_mut().find(|(k, _)| k == key) {
            slot.1 = (!folded.is_empty()).then(|| folded.join(" "));
        }
    }
    for key in ["ACCEPT_LICENSE", "ACCEPT_PROPERTIES", "ACCEPT_RESTRICT"] {
        let mut stacked = accept_file_tokens(config, key);
        stacked.extend(value_of(&parent, key));
        let pruned = prune_token_layers(&stacked);
        if let Some(slot) = child.iter_mut().find(|(k, _)| k == key) {
            slot.1 = (!pruned.is_empty()).then(|| pruned.join(" "));
        }
    }

    fn sort_snapshot(snapshot: &mut HeaderSnapshot) {
        snapshot.sort_by(|a, b| a.0.cmp(&b.0));
    }
    sort_snapshot(&mut parent);
    sort_snapshot(&mut child);
    PkgIndexHeader { parent, child }
}

/// Mirror Portage's `--buildpkg`/`--buildpkg=n` post-resolve settings
/// mutation on a header snapshot: the transient CLI flag is folded into
/// `settings.features` for the run, so a `--buildpkg` inject's header
/// carries `buildpkg` (parent snapshot only; `quickpkg` never runs under
/// `--buildpkg`).
// Portage: _emerge/actions.py, action_build
pub fn pkgindex_header_apply_buildpkg_opt(header: &mut PkgIndexHeader, opt: Option<bool>) {
    let Some(slot) = header.parent.iter_mut().find(|(k, _)| k == "FEATURES") else {
        return;
    };
    let mut tokens: Vec<String> = slot
        .1
        .as_ref()
        .map(|v| v.split_whitespace().map(String::from).collect())
        .unwrap_or_default();
    match opt {
        Some(true) if !tokens.iter().any(|t| t == "buildpkg") => {
            tokens.push("buildpkg".to_string());
            tokens.sort();
            slot.1 = Some(tokens.join(" "));
        }
        Some(false) => {
            tokens.retain(|t| t != "buildpkg");
            slot.1 = (!tokens.is_empty()).then(|| tokens.join(" "));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Miniature of the 335 bed container's resolved config: `FEATURES`
    /// negated in the env layer, `ACCEPT_LICENSE` only from the base
    /// profile. Portage citations per assertion are inline.
    fn bed_shaped_config() -> Config {
        let mut config = Config::default();
        for (k, v) in [
            ("ARCH", "amd64"),
            ("CBUILD", "x86_64-pc-linux-gnu"),
            ("CHOST", "x86_64-pc-linux-gnu"),
            ("ELIBC", "glibc"),
            ("KERNEL", "linux"),
            ("GENTOO_MIRRORS", "http://distfiles.gentoo.org"),
            ("USE_EXPAND_VALUES_ARCH", "alpha amd64"),
            ("USE_EXPAND_VALUES_ELIBC", "glibc musl"),
            ("USE_EXPAND_VALUES_KERNEL", "linux"),
        ] {
            config.other_vars.insert(k.to_string(), v.to_string());
        }
        config.accept_keywords = HashSet::from(["amd64".to_string()]);
        config.iuse_implicit = HashSet::from(["prefix".to_string(), "abi_x86_64".to_string()]);
        config.use_flags = HashSet::from(["amd64".to_string(), "acl".to_string(), "X".to_string()]);
        // Profile force/mask fold into the header USE like Portage's
        // settings (and `emerge --info`), not just the raw flags.
        config.use_force = HashSet::from(["forced_flag".to_string()]);
        config.use_mask = HashSet::from(["acl".to_string()]);
        config.use_expand = HashSet::from(["ARCH".to_string()]);
        config.use_expand_implicit = HashSet::from([
            "ARCH".to_string(),
            "ELIBC".to_string(),
            "KERNEL".to_string(),
        ]);
        config.use_expand_unprefixed = HashSet::from(["ARCH".to_string()]);
        config.incremental_sources.insert(
            "FEATURES".to_string(),
            vec![
                vec!["sandbox".to_string(), "buildpkg-live".to_string()],
                vec!["unmerge-backup".to_string(), "-sandbox".to_string()],
            ],
        );
        config.pkgindex_env_raw.insert(
            "FEATURES".to_string(),
            "unmerge-backup -sandbox".to_string(),
        );
        config
            .incremental_sources
            .insert("CONFIG_PROTECT".to_string(), vec![vec!["/etc".to_string()]]);
        config.accept_raw_layers.push((
            AcceptRawLayer::Defaults,
            "ACCEPT_LICENSE".to_string(),
            vec!["-*".to_string(), "@FREE".to_string()],
        ));
        for key in ["ACCEPT_PROPERTIES", "ACCEPT_RESTRICT"] {
            config.accept_raw_layers.push((
                AcceptRawLayer::Globals,
                key.to_string(),
                vec!["*".to_string()],
            ));
        }
        config.pkgindex_profile = "default/linux/amd64/23.0/systemd".to_string();
        config
    }

    fn valued_keys(snapshot: &HeaderSnapshot) -> Vec<&str> {
        snapshot
            .iter()
            .filter(|(_, v)| v.is_some())
            .map(|(k, _)| k.as_str())
            .collect()
    }

    fn snapshot_value<'a>(snapshot: &'a HeaderSnapshot, key: &str) -> &'a str {
        snapshot
            .iter()
            .find(|(k, _)| k == key)
            .and_then(|(_, v)| v.as_ref())
            .map(String::as_str)
            .unwrap_or("<absent>")
    }

    #[test]
    fn pkgindex_header_parent_carries_portages_settings_key_set() {
        // Portage: bintree.py, _update_pkgindex_header (the header-keys
        // loop) + getbinpkg.py, PackageIndex.write (sorted, falsy skipped).
        let header = pkgindex_header(&bed_shaped_config());
        assert_eq!(
            valued_keys(&header.parent),
            vec![
                "ACCEPT_KEYWORDS",
                "ACCEPT_LICENSE",
                "ACCEPT_PROPERTIES",
                "ACCEPT_RESTRICT",
                "ARCH",
                "CBUILD",
                "CHOST",
                "CONFIG_PROTECT",
                "ELIBC",
                "FEATURES",
                "GENTOO_MIRRORS",
                "IUSE_IMPLICIT",
                "KERNEL",
                "PROFILE",
                "USE",
                "USE_EXPAND",
                "USE_EXPAND_IMPLICIT",
                "USE_EXPAND_UNPREFIXED",
                "USE_EXPAND_VALUES_ARCH",
                "USE_EXPAND_VALUES_ELIBC",
                "USE_EXPAND_VALUES_KERNEL",
                "VERSION",
            ]
        );
        // The env negation holds in the parent: Portage stacks the calling
        // env over the files (config.py, regenerate).
        assert_eq!(
            snapshot_value(&header.parent, "FEATURES"),
            "buildpkg-live unmerge-backup"
        );
        // Pruned, not doubled (helper.py, prune_incremental).
        assert_eq!(snapshot_value(&header.parent, "ACCEPT_LICENSE"), "@FREE");
        assert_eq!(snapshot_value(&header.parent, "ACCEPT_PROPERTIES"), "*");
        assert_eq!(snapshot_value(&header.parent, "USE"), "X amd64 forced_flag");
        assert_eq!(
            snapshot_value(&header.parent, "PROFILE"),
            "default/linux/amd64/23.0/systemd"
        );
        // Unset settings values stay managed-but-absent, so a later write
        // removes a stale stored key instead of keeping it.
        assert!(
            header
                .parent
                .iter()
                .any(|(k, v)| k == "INSTALL_MASK" && v.is_none())
        );
    }

    #[test]
    fn pkgindex_header_child_refolds_like_portages_quickpkg_subprocess() {
        // Portage: vartree.py, _quickpkg_dblink (the child inherits the
        // resolved settings as env) + bin/quickpkg (inject from the child).
        let header = pkgindex_header(&bed_shaped_config());
        assert_eq!(valued_keys(&header.child), valued_keys(&header.parent));
        // The env negation is lost in the re-fold: the files' `sandbox`
        // comes back (config.py stacking has no negation left to apply).
        assert_eq!(
            snapshot_value(&header.child, "FEATURES"),
            "buildpkg-live sandbox unmerge-backup"
        );
        // The inherited resolved value stacks after the files again.
        assert_eq!(
            snapshot_value(&header.child, "ACCEPT_LICENSE"),
            "@FREE @FREE"
        );
        assert_eq!(snapshot_value(&header.child, "ACCEPT_PROPERTIES"), "*");
    }

    #[test]
    fn pkgindex_header_buildpkg_opt_adds_or_discards_the_transient_flag() {
        // Portage: _emerge/actions.py, action_build (`--buildpkg` adds,
        // `--buildpkg=n` discards; the flag is not a config variable).
        let mut header = pkgindex_header(&bed_shaped_config());
        pkgindex_header_apply_buildpkg_opt(&mut header, None);
        assert_eq!(
            snapshot_value(&header.parent, "FEATURES"),
            "buildpkg-live unmerge-backup"
        );
        pkgindex_header_apply_buildpkg_opt(&mut header, Some(true));
        assert_eq!(
            snapshot_value(&header.parent, "FEATURES"),
            "buildpkg buildpkg-live unmerge-backup"
        );
        // Adding twice keeps one copy; the child snapshot is untouched.
        pkgindex_header_apply_buildpkg_opt(&mut header, Some(true));
        assert_eq!(
            snapshot_value(&header.parent, "FEATURES"),
            "buildpkg buildpkg-live unmerge-backup"
        );
        assert_eq!(
            snapshot_value(&header.child, "FEATURES"),
            "buildpkg-live sandbox unmerge-backup"
        );
        pkgindex_header_apply_buildpkg_opt(&mut header, Some(false));
        assert_eq!(
            snapshot_value(&header.parent, "FEATURES"),
            "buildpkg-live unmerge-backup"
        );
    }
}
