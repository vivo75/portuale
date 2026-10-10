//! USE displays shared by slot-conflict and skipped-update reports.

use crate::*;

/// Real `pkg_use_display(pkg, opts, modified_use=...)` for a
/// scheduled-for-merge package (`_emerge/UseFlagDisplay.py:55`) -- the
/// `USE="…"` (+ one `VAR="…"` per non-hidden `USE_EXPAND` group) pairs
/// on a slot-conflict instance's / parent's line (and the REQUIRED_USE
/// violation block, which renders the same helper). Backlog #264: this
/// is the full display [`skipped_update_use_display_for`] ships for the
/// skipped-update block -- the effective USE masked to the package's
/// valid-IUSE domain, so implicit-expand flags (e.g. `elibc_glibc`
/// under `USE_EXPAND_IMPLICIT="ELIBC"`) render in their own group
/// instead of vanishing. Empty when the version is gone from every
/// repo (rendered as a bare `USE=""`).
pub(crate) fn pkg_use_display_for(
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    category: &str,
    package: &str,
    version: &str,
) -> Vec<(String, String)> {
    skipped_update_use_display_for(repos, config, category, package, version)
}

/// Backlog #230: real `pkg_use_display(pkg, opts, modified_use=...)` for a
/// scheduled-for-merge package (`_emerge/UseFlagDisplay.py:55`), built for
/// the skipped-update block -- the FULL per-package display. Backlog #264
/// unified the slot-collision notice onto this same implementation
/// ([`pkg_use_display_for`] delegates here): real renders both blocks
/// through the one helper, so portuale does too.
///
/// Two real behaviors the approximation misses, both bed-grounded (bed
/// `l0-fx-20260927T125711Z`, `--backtrack=0 dev-libs/blk0b dev-libs/blk0c
/// dev-libs/blk0a`: real renders `USE="(test-rust)" ABI_X86="(64)"
/// LLVM_TARGETS="(X86)"` on the missed line and `USE="(globalforceflag)"`
/// on the parents where portuale printed bare `USE=""`):
///
/// 1. `modified_use` is the package's whole effective USE
///    (`PORTAGE_USE`), so profile-global flags and forced flags render
///    even when no ebuild declares them in `IUSE`.
/// 2. `PORTAGE_USE` itself is the effective USE *masked* to the
///    package's valid-IUSE domain (`config.py` setcpv, `:2166-2220`): a
///    flag survives iff it is in the declared `IUSE` or matches the
///    implicit domain -- EAPI 5+ `IUSE_EFFECTIVE`
///    (`_calc_iuse_effective`: `IUSE_IMPLICIT` + unprefixed +
///    expand-implicit values), pre-EAPI-5 `_get_implicit_iuse` (`ARCH` +
///    `arch.list` + `USE_EXPAND_HIDDEN`-derived `xxx_.*` + the
///    per-package use.mask/use.force + `build`/`bootstrap`).
///
/// Bed-grounded against the real 3.0.82.2 container probe in
/// `docs/history/` (see `skipped_update_use_display_for`'s callers for
/// the pointer): `emerge --info dev-libs/pkginfopkg` shows
/// `USE="alpha -beta" ELIBC="glibc"` -- the `foo`/`bar`/forced globals
/// masked out, the implicit `elibc_glibc` kept -- and a flagless blk0
/// package keeps exactly `amd64`→(discarded as `ARCH`), the hidden-expand
/// `cpu_flags_x86_sse2`, and the forced `globalforceflag`.
///
/// Deliberate narrowings: no `--alphabetical` interleave (no contract or
/// bed cell passes it with this block) and no ANSI colour (a no-op under
/// `emerge -p` without `--color=y`, matching `render_pkg_use_display`'s
/// own cut). The installed-consumer twin is
/// [`skipped_update_installed_use_display_for`].
///
/// NOTE (post-#220): the blk0 ebuilds are EAPI 8 now, so the
/// `eapi_has_iuse_effective` gate selects the `IUSE_EFFECTIVE` domain
/// and the profile globals mask out -- a flagless blk0 package renders
/// `USE="" ELIBC="glibc"`. The `pre-EAPI-5 ... forced flags survive`
/// pin in pmtest's `test_upstream_blocker_pg0_all_orders_pin_x1_and_uninstall_y1`
/// predates that merge and is stale (see the fix-round-1 report).
pub fn skipped_update_use_display_for(
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    category: &str,
    package: &str,
    version: &str,
) -> Vec<(String, String)> {
    let candidates = match list_candidates(repos, category, package) {
        Ok(cs) => cs,
        Err(_) => return Vec::new(),
    };
    let Some(cand) = candidates
        .iter()
        .filter(|c| c.version == version)
        .max_by_key(|c| c.repo_priority)
        .cloned()
    else {
        return Vec::new();
    };
    let pf = format!("{package}-{version}");
    let metadata = match repo_aux_metadata(&cand.repo_location, category, &pf) {
        Ok(md) => md,
        Err(_) => return Vec::new(),
    };
    // Same "absence is real" as `pkg_use_display_for`: a missing IUSE key
    // declares no flags; a missing md5-cache entry entirely (the `?`
    // above... here the Err arm) yields no display.
    let use_flags = match candidate_iuse_and_use(&cand, category, package, config) {
        Some((_, uf)) => uf,
        None => return Vec::new(),
    };
    assemble_tree_pkg_use_display(AssembleTreePkgUseDisplayArgs {
        cand: &cand,
        metadata: &metadata,
        enabled: &use_flags,
        config,
        category,
        package,
        version,
    })
}

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

/// Shared assembly tail of [`skipped_update_use_display_for`] and the
/// #244-replay conflict displays below: real `pkg_use_display`'s own
/// grouping (`_emerge/UseFlagDisplay.py:55-122`) over an explicit
/// enabled set -- the masked `PORTAGE_USE`-equivalent, whose source
/// differs per caller (the walk-time effective USE for the skipped
/// block, the flip-applied entry USE for a replayed conflict).
/// `explicit` is the declared `IUSE` names (the disabled side),
/// `force_mask` the unfiltered per-package force ∪ mask stack for the
/// `( )` wraps.
pub(crate) fn assemble_tree_pkg_use_display(
    args: AssembleTreePkgUseDisplayArgs<'_>,
) -> Vec<(String, String)> {
    let AssembleTreePkgUseDisplayArgs {
        cand,
        metadata,
        enabled,
        config,
        category,
        package,
        version,
    } = args;
    let iuse_str = metadata.get("IUSE").map(String::as_str).unwrap_or_default();
    let explicit: HashSet<String> = iuse_str
        .split_whitespace()
        .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
        .collect();
    let eapi = metadata.get("EAPI").map(String::as_str).unwrap_or("");
    let candidate_str = format!(
        "{category}/{package}-{version}:{}/{}::{}",
        cand.slot, cand.sub_slot, cand.repo_name
    );
    // Real `pkg.use.force ∪ pkg.use.mask` (`pkgsettings.useforce` /
    // `usemask` after `setcpv`): the unfiltered per-package stack, since
    // the `( )` wraps apply to implicit-domain flags too (an implicit
    // flag is displayed, so a forced implicit flag must wrap).
    let force_mask = forced_or_masked_flags_unfiltered(
        &cand.keywords,
        &candidate_str,
        category,
        package,
        config,
    );
    let kept = mask_use_to_valid_domain(
        &explicit,
        enabled,
        md5_dict::eapi_has_iuse_effective(eapi),
        config,
        &force_mask,
    );
    assemble_pkg_use_display(&kept, &explicit, &force_mask, config)
}

/// Backlog #230: the installed-consumer twin of
/// [`skipped_update_use_display_for`] -- real `pkg_use_display` for an
/// installed parent (`depgraph.py:1696-1700` calls it with the vdb
/// package's own `use.enabled`). The vdb `USE` is ground truth (real
/// validates it against the implicit domain at `Package` construction
/// but never re-masks it), so no masking runs here: enabled is the
/// recorded `USE`, disabled the recorded `IUSE` minus that. Narrowing,
/// documented: no profile-force/mask `( )` wraps (those need the tree
/// candidate's keywords via `forced_or_masked_flags_unfiltered`, and an
/// installed consumer's version may be gone from every repo -- the same
/// narrowing `installed_use_display_for` already documents). The exact
/// trigger that would expose the gap is an installed skipped-block
/// consumer carrying a flag from the profile/repo force/mask stack:
/// real wraps it (`forced_flags = chain(pkg.use.force, pkg.use.mask)`,
/// `UseFlagDisplay.py:60`, applied to installed parents too via
/// `depgraph.py:1696-1700`) -- e.g. a globally `use.force`d flag
/// recorded enabled in the vdb `USE` renders `(flag)`, a `use.mask`ed
/// flag still present in the recorded `IUSE` renders `(-flag)` --
/// while portuale renders either bare. No grounded case exercises it
/// (every bed blk0 parent is a merge node), so the wrap gap is pinned
/// nowhere.
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

/// Backlog #230: one consumer's full `pkg_use_display` for a
/// `SkippedUpdate` row, shared by the three producers. `parent_cpv` is
/// the `cat/pkg-ver:slot/sub::repo` form `slot_conflict_puller_cpv`
/// builds (empty for a top-level `(Argument)` parent, whose row the
/// renderer prints bare -- the display value is unused there).
/// Installed consumers read the vdb twin, merge consumers the tree twin.
pub(crate) fn skipped_consumer_use_display(
    repos: &[RepoConfig],
    root: &Path,
    config: &portage_profile::Config,
    parent_cpv: &str,
    installed: bool,
) -> Vec<(String, String)> {
    let cpv = parent_cpv.split(':').next().unwrap_or("");
    let Some((cc, cp2, cv)) = split_cpv(cpv) else {
        return Vec::new();
    };
    if installed {
        skipped_update_installed_use_display_for(root, config, &cc, &cp2, &cv)
    } else {
        skipped_update_use_display_for(repos, config, &cc, &cp2, &cv)
    }
}

/// Backlog #230: real `config.py` setcpv's own
/// `PORTAGE_USE` masking (`:2166-2220`) -- keep an effective-USE flag
/// iff it is in the package's declared `IUSE` or matches the implicit
/// domain. `iuse_effective` selects the EAPI 5+ rule
/// (`IUSE_EFFECTIVE`, real `_iuse_effective_match`); otherwise the
/// pre-EAPI-5 `_get_implicit_iuse` rule (`ARCH`, `arch.list`,
/// `USE_EXPAND_HIDDEN`-derived `xxx_*` prefixes, the per-package
/// use.mask/use.force stack, `build`/`bootstrap`).
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

/// Backlog #230: real `pkg_use_display`'s own assembly
/// (`_emerge/UseFlagDisplay.py:55-122`), minus colour and
/// `--alphabetical`. `enabled` is the masked `PORTAGE_USE`-equivalent
/// set, `explicit` the declared `IUSE` names (the disabled side), and
/// `forced` the per-package force ∪ mask stack for the `( )` wraps.
/// `ARCH` is discarded from the `USE` group; `USE_EXPAND_HIDDEN` groups
/// are skipped; groups render `USE` first then byte-sorted var names,
/// each enabled-first then byte-sorted (real's default
/// `sort_separated`, plain string order).
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
    // Real iterates `pkg.iuse.all` for the disabled side; the same
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
    // Real `var_order.insert(0, "USE")`: the USE group always renders
    // first (possibly as the bare `USE=""` real always emits).
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

/// Backlog #230: one `VAR="…"` group of real `pkg_use_display` --
/// enabled flags first, then disabled, each byte-sorted by display name
/// (real's default `sort_separated`); enabled render bare, disabled
/// `-`-prefixed, and force/mask members `( )`-wrapped (real
/// `UseFlagDisplay.__str__`, minus the red/blue that are a no-op without
/// `--color=y`). Entries carry `(full flag name, bare display name,
/// enabled)`; the wrap lookup runs on the full name, the sort on the
/// bare display name exactly like real's own `UseFlagDisplay.name`.
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

/// The same `USE="…"` display pairs as [`pkg_use_display_for`], built from
/// an **installed** package's vdb-recorded `IUSE`/`USE` instead of a tree
/// candidate -- real `pkg_use_display(installed_pkg)` for a nomerge node
/// or an installed parent in the residual installed-instance conflict
/// report (see `build_residual_slot_conflicts`). Backlog #264: this is
/// the vdb twin [`skipped_update_installed_use_display_for`] ships for
/// the skipped-update block, so recorded-USE flags outside the recorded
/// IUSE (e.g. implicit-expand `elibc_glibc`) render in their own group
/// the way real's `use.enabled` side does. An empty `IUSE` with an empty
/// recorded `USE` renders the bare `USE=""` real shows. Narrowing,
/// documented: no profile-force/mask `( )` wraps (those need the tree
/// candidate's keywords, which the vdb does not record); a flagless
/// package -- every residual fixture so far -- renders byte-identical
/// either way.
pub fn installed_use_display_for(
    root: &Path,
    config: &portage_profile::Config,
    category: &str,
    package: &str,
    version: &str,
) -> Vec<(String, String)> {
    skipped_update_installed_use_display_for(root, config, category, package, version)
}

/// Declared-IUSE names and resolved USE for `category/package` at
/// `version` (highest-`repo_priority` candidate, same re-lookup as
/// `slot_conflict_meta`/`pkg_use_display_for`) -- the two sets real
/// `_prepare_conflict_msg_and_check_for_specificity`'s USE branch reads
/// off each conflicting instance (`other_pkg.iuse`,
/// `_pkg_use_enabled(other_pkg)`). Empty sets when the version is gone
/// from every repo or its md5-cache entry is unreadable (same
/// "absence is real" handling as `pkg_use_display_for`: every required
/// flag then counts as missing). Read at render time by portuale's
/// slot-collision `("use", flag)` classification.
pub fn slot_conflict_flag_sets(
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    category: &str,
    package: &str,
    version: &str,
) -> (HashSet<String>, HashSet<String>) {
    let Some(cand) = list_candidates(repos, category, package)
        .ok()
        .and_then(|cs| {
            cs.iter()
                .filter(|c| c.version == version)
                .max_by_key(|c| c.repo_priority)
                .cloned()
        })
    else {
        return (HashSet::new(), HashSet::new());
    };
    match candidate_iuse_and_use(&cand, category, package, config) {
        // Render-time only (slot-collision messages); one clone here.
        Some((iuse, use_flags)) => (iuse, use_flags.as_ref().clone()),
        None => (HashSet::new(), HashSet::new()),
    }
}
