//! `USE="…"` displays shared by slot-conflict and skipped-update reports.

use crate::*;

/// Arguments of [`assemble_tree_pkg_use_display`].
pub(crate) struct AssembleTreePkgUseDisplayArgs<'a> {
    pub(crate) cand: &'a Candidate,
    pub(crate) metadata: &'a std::sync::Arc<HashMap<String, String>>,
    pub(crate) enabled: &'a HashSet<String>,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
}

/// Installed-consumer twin of [`skipped_update_use_display_for`]: reads the
/// vdb-recorded `IUSE`/`USE` with no masking. No force/mask `( )` wraps.
// Portage: depgraph.py, pkg_use_display (installed parent)
pub fn skipped_update_installed_use_display_for(
    root: &Path,
    config: &portage_profile::Config,
    category: &str,
    package: &str,
    version: &str,
) -> Vec<(String, String)> {
    let (iuse, use_flags) = installed_pkg_iuse_and_use(root, category, package, version);
    let enabled: HashSet<String> = use_flags.iter().cloned().collect();
    let explicit: HashSet<String> = iuse.iter().cloned().collect();
    assemble_pkg_use_display(&enabled, &explicit, &HashSet::new(), config)
}

/// Keeps an effective-USE flag iff it is declared in `IUSE` or matches the
/// implicit domain.
// Portage: config.py, setcpv (PORTAGE_USE masking)
pub(crate) fn mask_use_to_valid_domain(
    explicit: &HashSet<String>,
    use_flags: &HashSet<String>,
    iuse_effective: bool,
    config: &portage_profile::Config,
    force_mask: &HashSet<String>,
) -> HashSet<String> {
    if iuse_effective {
        return use_flags
            .iter()
            .filter(|f| explicit.contains(*f) || config.iuse_effective.contains(*f))
            .cloned()
            .collect();
    }
    let hidden_prefixes: Vec<String> = config
        .use_expand_hidden
        .iter()
        .map(|v| format!("{}_", v.to_lowercase()))
        .collect();
    let arch = config.other_vars.get("ARCH").cloned().unwrap_or_default();
    use_flags
        .iter()
        .filter(|f| {
            explicit.contains(*f)
                || **f == arch
                || config.archlist.contains(*f)
                || hidden_prefixes.iter().any(|p| f.starts_with(p))
                || force_mask.contains(*f)
                || **f == "build"
                || **f == "bootstrap"
        })
        .cloned()
        .collect()
}

/// Groups `enabled` by `USE_EXPAND` prefix into `USE` plus one pair per
/// visible group. `ARCH` is dropped; hidden groups are skipped.
// Portage: _emerge/UseFlagDisplay.py, pkg_use_display
pub(crate) fn assemble_pkg_use_display(
    enabled: &HashSet<String>,
    explicit: &HashSet<String>,
    forced: &HashSet<String>,
    config: &portage_profile::Config,
) -> Vec<(String, String)> {
    let arch = config.other_vars.get("ARCH").cloned().unwrap_or_default();
    let hidden: HashSet<String> = config
        .use_expand_hidden
        .iter()
        .map(|s| s.to_uppercase())
        .collect();
    let mut expand_vars: Vec<String> = config.use_expand.iter().cloned().collect();
    expand_vars.sort();
    let prefix_of = |var: &str| format!("{}_", var.to_lowercase());
    // (group, full flag name, bare display name, enabled)
    let mut grouped: HashMap<String, Vec<(String, String, bool)>> = HashMap::new();
    for f in enabled {
        if f == &arch {
            continue;
        }
        let mut placed = false;
        for var in &expand_vars {
            if let Some(bare) = f.strip_prefix(&prefix_of(var)) {
                grouped.entry(var.to_uppercase()).or_default().push((
                    f.clone(),
                    bare.to_string(),
                    true,
                ));
                placed = true;
                break;
            }
        }
        if !placed {
            grouped
                .entry("USE".to_string())
                .or_default()
                .push((f.clone(), f.clone(), true));
        }
    }
    // Portage iterates `pkg.iuse.all` for the disabled side; the same
    // routing by expand prefix applies.
    for f in explicit {
        if enabled.contains(f) {
            continue;
        }
        let mut placed = false;
        for var in &expand_vars {
            if let Some(bare) = f.strip_prefix(&prefix_of(var)) {
                grouped.entry(var.to_uppercase()).or_default().push((
                    f.clone(),
                    bare.to_string(),
                    false,
                ));
                placed = true;
                break;
            }
        }
        if !placed {
            grouped
                .entry("USE".to_string())
                .or_default()
                .push((f.clone(), f.clone(), false));
        }
    }
    let mut names: Vec<String> = grouped.keys().cloned().collect();
    names.sort();
    let mut out = Vec::new();
    // Portage `var_order.insert(0, "USE")`: the USE group always renders first.
    if let Some(flags) = grouped.remove("USE") {
        out.push(("USE".to_string(), render_use_group(flags, forced)));
    } else {
        out.push(("USE".to_string(), String::new()));
    }
    for name in names {
        if name == "USE" || hidden.contains(&name) {
            continue;
        }
        if let Some(flags) = grouped.remove(&name) {
            out.push((name, render_use_group(flags, forced)));
        }
    }
    out
}

/// Renders one `VAR="…"` group: enabled bare then disabled `-`-prefixed,
/// each byte-sorted; force/mask members are `( )`-wrapped.
// Portage: _emerge/UseFlagDisplay.py, UseFlagDisplay.__str__
pub(crate) fn render_use_group(
    flags: Vec<(String, String, bool)>,
    forced: &HashSet<String>,
) -> String {
    let mut enabled: Vec<(String, String)> = Vec::new();
    let mut disabled: Vec<(String, String)> = Vec::new();
    for (full, bare, on) in flags {
        if on {
            enabled.push((full, bare));
        } else {
            disabled.push((full, bare));
        }
    }
    enabled.sort_by(|a, b| a.1.cmp(&b.1));
    disabled.sort_by(|a, b| a.1.cmp(&b.1));
    enabled
        .into_iter()
        .map(|(full, bare)| {
            if forced.contains(&full) {
                format!("({bare})")
            } else {
                bare
            }
        })
        .chain(disabled.into_iter().map(|(full, bare)| {
            let tok = format!("-{bare}");
            if forced.contains(&full) {
                format!("({tok})")
            } else {
                tok
            }
        }))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Installed-package twin of [`pkg_use_display_for`], from vdb-recorded
/// `IUSE`/`USE`. No force/mask wraps.
// Portage: depgraph.py, pkg_use_display (installed node)
pub fn installed_use_display_for(
    root: &Path,
    config: &portage_profile::Config,
    category: &str,
    package: &str,
    version: &str,
) -> Vec<(String, String)> {
    skipped_update_installed_use_display_for(root, config, category, package, version)
}
