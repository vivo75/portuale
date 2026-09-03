//! USE flag display columns.

use crate::*;

/// The installed version's recorded `USE` / `IUSE` (both bare flag
/// names, `USE` already intersected with `IUSE`), for the `*`/`%`
/// diff markers. `None` for a `New` entry -> no markers.
pub(crate) struct InstalledUseState {
    pub(crate) old_use: HashSet<String>,
    pub(crate) old_iuse: HashSet<String>,
}

/// Natural sort key: split on runs of digits and compare those as
/// numbers, so `python3_9` sorts before `python3_12`.
/// Portage: output_helpers.py, _alnum_sort_key
///
/// A digit run longer than `u128` holds is kept as `Str`, still
/// deterministic.
pub fn alnum_sort_key(s: &str) -> Vec<AlnumKey> {
    let mut out = Vec::new();
    let mut rest = s;
    loop {
        match rest.find(|c: char| c.is_ascii_digit()) {
            None => {
                out.push(AlnumKey::Str(rest.to_string()));
                break;
            }
            Some(start) => {
                out.push(AlnumKey::Str(rest[..start].to_string()));
                let digits_end = rest[start..]
                    .find(|c: char| !c.is_ascii_digit())
                    .map(|i| start + i)
                    .unwrap_or(rest.len());
                let digits = &rest[start..digits_end];
                out.push(match digits.parse::<u128>() {
                    Ok(n) => AlnumKey::Num(n),
                    Err(_) => AlnumKey::Str(digits.to_string()),
                });
                rest = &rest[digits_end..];
            }
        }
    }
    out
}

/// One segment of an `alnum_sort_key`: a non-digit run (`Str`) or a
/// digit run as a number (`Num`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlnumKey {
    Str(String),
    Num(u128),
}

/// Re-render the USE-display fields of every merge entry for `cp`
/// (used to reflect an accumulated `package.use` change the loop did
/// not re-walk); a no-op for entries with no matching candidate.
pub(crate) fn refresh_entry_use_display(
    entries: &mut [GraphEntry],
    repos: &[RepoConfig],
    root: &Path,
    cp: &(String, String),
    config: &portage_profile::Config,
) {
    let (cat, pkg) = (cp.0.as_str(), cp.1.as_str());
    let Ok(cands) = list_candidates(repos, cat, pkg) else {
        return;
    };
    for e in entries.iter_mut() {
        if e.category != cat || e.package != pkg {
            continue;
        }
        let Some(cand) = cands.iter().find(|c| match &e.outcome {
            PretendOutcome::New { version } => *c.version == *version,
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                *c.version == *to
            }
            PretendOutcome::Reinstall { version, .. } => *c.version == *version,
            _ => false,
        }) else {
            continue;
        };
        refresh_one_entry_display(e, cand, root, config);
    }
}

/// Repaint every running-rooted merge entry's USE displays from the
/// running root's own profile (cross-root rows use their own root).
/// A no-op for entries the candidate lookup misses.
/// Portage: output.py, _display_use (per-root config)
pub fn repaint_running_root_entry_displays(
    entries: &mut [GraphEntry],
    repos: &[RepoConfig],
    running_root: &Path,
    running_config: &portage_profile::Config,
) {
    let mut cps: Vec<(String, String)> = entries
        .iter()
        .filter(|e| e.targets_running_root)
        .map(|e| (e.category.clone(), e.package.clone()))
        .collect();
    cps.sort();
    cps.dedup();
    for (cat, pkg) in &cps {
        let Ok(cands) = list_candidates(repos, cat, pkg) else {
            continue;
        };
        for e in entries.iter_mut() {
            if !e.targets_running_root || e.category != *cat || e.package != *pkg {
                continue;
            }
            let outcome_version = match &e.outcome {
                PretendOutcome::New { version } => Some(version.as_str()),
                PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                    Some(to.as_str())
                }
                PretendOutcome::Reinstall { version, .. } => Some(version.as_str()),
                _ => None,
            };
            let Some(version) = outcome_version else {
                continue;
            };
            let Some(cand) = cands.iter().find(|c| c.version == version) else {
                continue;
            };
            refresh_one_entry_display(e, cand, running_root, running_config);
        }
    }
}

/// Per-entry core of [`refresh_entry_use_display`]: rebuild one merge
/// entry's `use_flags_display` / `use_expand_display[_p]` from `config`
/// (effective USE, force/mask wraps, expand grouping and the installed
/// old-USE diff all read that config and `root`'s vdb).
pub(crate) fn refresh_one_entry_display(
    e: &mut GraphEntry,
    cand: &Candidate,
    root: &Path,
    config: &portage_profile::Config,
) {
    let (version, installed_version) = match &e.outcome {
        PretendOutcome::New { version } => (version.clone(), None),
        PretendOutcome::Upgrade { from, to } | PretendOutcome::Downgrade { from, to } => {
            (to.clone(), Some(from.clone()))
        }
        PretendOutcome::Reinstall { version, .. } => (version.clone(), Some(version.clone())),
        _ => return,
    };
    let cat = e.category.clone();
    let pkg = e.package.clone();
    let slot = e.slot.clone().unwrap_or_else(|| cand.slot.clone());
    let sub_slot = e.sub_slot.clone().unwrap_or_else(|| cand.sub_slot.clone());
    let repo_name = e
        .repo_name
        .clone()
        .unwrap_or_else(|| cand.repo_name.clone());
    let candidate_str = format!("{cat}/{pkg}-{version}:{slot}/{sub_slot}::{repo_name}");
    let use_flags = effective_use_flags(EffectiveUseFlagsArgs {
        config,
        iuse: &cand.iuse,
        keywords: &cand.keywords,
        candidate_str: &candidate_str,
        category: &cat,
        package: &pkg,
    });
    // IUSE may name a flag more than once (eclass plus ebuild); it
    // still renders exactly once, like Portage's `sorted(iuse.all)`.
    let mut iuse_seen: HashSet<String> = HashSet::new();
    let mut display: Vec<(String, bool)> = cand
        .iuse
        .split_whitespace()
        .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
        .filter(|flag| iuse_seen.insert(flag.clone()))
        .map(|flag| {
            let enabled = use_flags.contains(&flag);
            (flag, enabled)
        })
        .collect();
    display.sort_by_key(|p| alnum_sort_key(&p.0));
    let installed = installed_version.map(|v| {
        let old_iuse = read_vdb_flag_set(root, &cat, &pkg, &v, "IUSE");
        let mut old_use = read_vdb_flag_set(root, &cat, &pkg, &v, "USE");
        old_use.retain(|f| old_iuse.contains(f));
        InstalledUseState { old_use, old_iuse }
    });
    let forced = forced_or_masked_flags(ForcedOrMaskedFlagsArgs {
        iuse: &cand.iuse,
        keywords: &cand.keywords,
        candidate_str: &candidate_str,
        category: &cat,
        package: &pkg,
        config,
    });
    let reinst_flags: HashSet<String> = match &e.outcome {
        PretendOutcome::Reinstall { changed_flags, .. } => changed_flags.iter().cloned().collect(),
        _ => HashSet::new(),
    };
    e.use_expand_display = build_use_expand_display(BuildUseExpandDisplayArgs {
        use_flags_display: &display,
        config,
        installed: installed.as_ref(),
        forced: &forced,
        all_flags: true,
        reinst_flags: &reinst_flags,
    });
    e.use_expand_display_p = build_use_expand_display(BuildUseExpandDisplayArgs {
        use_flags_display: &display,
        config,
        installed: installed.as_ref(),
        forced: &forced,
        all_flags: false,
        reinst_flags: &reinst_flags,
    });
    e.use_flags_display = display;
}
pub(crate) struct BuildUseExpandDisplayArgs<'a> {
    pub(crate) use_flags_display: &'a [(String, bool)],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) installed: Option<&'a InstalledUseState>,
    pub(crate) forced: &'a HashSet<String>,
    pub(crate) all_flags: bool,
    pub(crate) reinst_flags: &'a HashSet<String>,
}

/// `emerge --pretend -v`'s `USE="…" VAR="…"` line: split the display
/// flags into `USE` plus one group per USE_EXPAND variable (prefix
/// stripped, hidden groups dropped), diffed against `installed` with
/// `*`/`%` markers when present. `all_flags` shows every flag (`-pv`);
/// otherwise only changed and reinstall-trigger flags show.
/// Portage: output_helpers.py, _create_use_string
pub(crate) fn build_use_expand_display(
    args: BuildUseExpandDisplayArgs<'_>,
) -> Vec<(String, String)> {
    let BuildUseExpandDisplayArgs {
        use_flags_display,
        config,
        installed,
        forced,
        all_flags,
        reinst_flags,
    } = args;
    let mut expand_vars: Vec<String> = config.use_expand.iter().cloned().collect();
    expand_vars.sort();
    let hidden: HashSet<String> = config
        .use_expand_hidden
        .iter()
        .map(|s| s.to_uppercase())
        .collect();

    // One flag's polarity: enabled / disabled / removed, rendered
    // in that order.
    #[derive(Clone, Copy, PartialEq)]
    enum FlagState {
        Enabled,
        Disabled,
        Removed,
    }

    // Render one flag token with its `*`/`%` suffix and `( )` wrap,
    // or `None` when it isn't shown at this verbosity (unchanged
    // flags only show under `all_flags` or as reinstall triggers).
    let render_flag = |bare: &str, full: &str, state: FlagState| -> Option<String> {
        let is_forced = forced.contains(full);
        let reinst = reinst_flags.contains(full);
        if state == FlagState::Removed {
            if !all_flags && !reinst {
                return None;
            }
            // A removed flag always renders `(-flag%[*])`.
            let in_old_use = installed.is_some_and(|inst| inst.old_use.contains(full));
            return Some(format!("(-{bare}%{})", if in_old_use { "*" } else { "" }));
        }
        let enabled = state == FlagState::Enabled;
        let core = match installed {
            None => {
                let sign = if enabled { "" } else { "-" };
                format!("{sign}{bare}")
            }
            Some(inst) => {
                let in_old_iuse = inst.old_iuse.contains(full);
                let in_old_use = inst.old_use.contains(full);
                if enabled {
                    if !in_old_iuse {
                        format!("{bare}%*")
                    } else if !in_old_use {
                        format!("{bare}*")
                    } else if all_flags || reinst {
                        // Unchanged flags only show under `-pv` (or as
                        // reinstall triggers).
                        bare.to_string()
                    } else {
                        return None;
                    }
                } else if !in_old_iuse {
                    // A forced flag skips the trailing `%`.
                    if is_forced {
                        format!("-{bare}")
                    } else {
                        format!("-{bare}%")
                    }
                } else if in_old_use {
                    format!("-{bare}*")
                } else if all_flags || reinst {
                    // Unchanged disabled flag: same `-pv`/trigger gate.
                    format!("-{bare}")
                } else {
                    return None;
                }
            }
        };
        Some(if is_forced { format!("({core})") } else { core })
    };

    // Empty key is the plain "USE" group; entries keep the full flag
    // name until render time.
    let mut groups: Vec<(String, Vec<(String, FlagState)>)> = vec![(String::new(), Vec::new())];
    for var in &expand_vars {
        groups.push((var.to_uppercase(), Vec::new()));
    }
    let route =
        |groups: &mut Vec<(String, Vec<(String, FlagState)>)>, flag: &str, state: FlagState| {
            for var in &expand_vars {
                let prefix = format!("{}_", var.to_lowercase());
                if flag.strip_prefix(&prefix).is_some() {
                    groups
                        .iter_mut()
                        .find(|(n, _)| *n == var.to_uppercase())
                        .unwrap()
                        .1
                        .push((flag.to_string(), state));
                    return;
                }
            }
            groups[0].1.push((flag.to_string(), state));
        };

    for (flag, enabled) in use_flags_display {
        route(
            &mut groups,
            flag,
            if *enabled {
                FlagState::Enabled
            } else {
                FlagState::Disabled
            },
        );
    }
    // Flags the current ebuild dropped from IUSE render `(-flag%)`;
    // routed whenever either display mode could show them.
    if let Some(inst) = installed.filter(|_| all_flags || !reinst_flags.is_empty()) {
        let cur: HashSet<&str> = use_flags_display.iter().map(|(f, _)| f.as_str()).collect();
        let mut removed: Vec<&String> = inst
            .old_iuse
            .iter()
            .filter(|f| !cur.contains(f.as_str()))
            .collect();
        // Natural sort, same key as the display list above.
        removed.sort_by_key(|f| alnum_sort_key(f));
        for flag in removed {
            route(&mut groups, flag, FlagState::Removed);
        }
    }

    let mut out = Vec::new();
    for (name, flags) in groups {
        if !name.is_empty() && hidden.contains(&name) {
            continue;
        }
        let prefix = format!("{}_", name.to_lowercase());
        // Enabled, then disabled, then removed; `--alphabetical` is
        // applied later at render time in `pretend.rs`.
        let mut rendered_pairs: Vec<(u8, String)> = flags
            .iter()
            .filter_map(|(full, state)| {
                let bare = if name.is_empty() {
                    full.as_str()
                } else {
                    full.strip_prefix(&prefix).unwrap_or(full)
                };
                let rank = match state {
                    FlagState::Enabled => 0,
                    FlagState::Disabled => 1,
                    FlagState::Removed => 2,
                };
                render_flag(bare, full, *state).map(|tok| (rank, tok))
            })
            .collect();
        rendered_pairs.sort_by_key(|(rank, _)| *rank);
        let rendered: Vec<String> = rendered_pairs.into_iter().map(|(_, tok)| tok).collect();
        if rendered.is_empty() {
            continue;
        }
        out.push((
            if name.is_empty() {
                "USE".to_string()
            } else {
                name
            },
            rendered.join(" "),
        ));
    }
    out
}
