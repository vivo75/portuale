//! USE flag display columns.

use crate::*;

/// Real `output.py::_display_use` + `map_to_use_expand` +
/// The installed version's own recorded `USE` / `IUSE` (both bare flag
/// names, `USE` already intersected with `IUSE`), for
/// `build_use_expand_display`'s own `*`/`%` diff markers -- real
/// `_display_use`'s `old_use` / `old_iuse`. `None` at the call site for a
/// `New` entry (no installed side) -> no markers, every flag shown plain.
pub(crate) struct InstalledUseState {
    pub(crate) old_use: HashSet<String>,
    pub(crate) old_iuse: HashSet<String>,
}

/// `output_helpers.py::_create_use_string`, for `emerge --pretend -v`'s
/// own `USE="…" VAR="…"` line.
///
/// Splits `use_flags_display` (already IUSE-declared, enabled-resolved,
/// and `alnum_sort_key`-ordered -- real `_create_use_string`'s own
/// `any_iuse.sort(key=_alnum_sort_key)`, a *natural* sort so `python3_9`
/// precedes `python3_12`) into the plain `USE` group plus one group per
/// `config.use_expand` variable whose `lowercase(name)_` prefixes the
/// flag -- the prefix is stripped from the grouped flag (real
/// `map_to_use_expand`: `val[len(exp) + 1:]`). `config.use_expand_hidden`
/// groups are dropped (real `remove_hidden`). Within each group the
/// enabled flags render first, then the disabled ones, each keeping that
/// natural order -- real `_create_use_string`'s own
/// `" ".join(enabled + disabled)`. `emerge --alphabetical` collapses
/// the two back into one interleaved list (still `alnum_sort_key`); that
/// is applied at render time (`pretend.rs::use_suffix`), not here, since
/// it needs no resolver state.
///
/// When `installed` is `Some` (an `Upgrade`/`Downgrade`/`Reinstall` entry
/// -- i.e. real `pkg_info.previous_pkg is not None`, `is_new` false),
/// each flag is diffed against the installed version's own recorded
/// `USE`/`IUSE`. Real `_DisplayConfig` sets `verbosity = 3` whenever
/// `--verbose` is given (`output_helpers.py:180-187`), so `all_flags =
/// (verbosity == 3)` is **always true** for `emerge -pv` -- meaning the
/// diff shows *every* flag, not just the changed ones: an enabled flag
/// not in old IUSE renders `flag%*`; enabled + in old IUSE + not in old
/// USE renders `flag*`; enabled + unchanged renders `flag` (plain); a
/// disabled flag not in old IUSE renders `-flag%`; disabled + was in old
/// USE renders `-flag*`; disabled + unchanged renders `-flag` (plain);
/// and a flag that was in old IUSE but is gone from the current ebuild's
/// IUSE renders `(-flag%)`, or `(-flag%*)` if it was enabled -- real
/// `_create_use_string`'s own `removed` list. `reinst_flags` (the extra
/// per-flag "this flag triggered the reinstall" force) is still not
/// modelled -- it only widens what `all_flags` already shows here.
///
/// `forced` (full flag names -- real `_display_use`'s `self.forced_flags`
/// = `pkg.use.force | pkg.use.mask`, see `forced_or_masked_flags`) is any
/// flag the user can't actually control: real `_create_use_string` wraps
/// its rendered token in `( … )`, and skips the trailing `%` on a
/// `-flag%` (a masked flag brand-new to IUSE renders `(-flag)`, not
/// `(-flag%)`).
///
/// Deliberately NOT ported (separate cut): real ANSI colorization.
///
/// Returns `[(VAR_NAME, "rendered")]` with `USE` first, then the
/// `USE_EXPAND` vars in sorted order (real `sorted(self.use_expand)` with
/// `"USE"` inserted at position 0). An empty group produces no entry at
/// all (real `_create_use_string`'s own `if ret:` guard), so an
/// `Upgrade` whose USE didn't actually change shows *no* `USE=` line, and
/// a package with no displayable flags returns `[]`.
/// Real `output_helpers.py::_alnum_sort_key`: split a string on runs of
/// digits (`re.split(r"(\d+)", x)` -- so the result alternates
/// non-digit / digit / non-digit / …), and compare the digit runs as
/// numbers rather than lexically. `python3_9` then sorts before
/// `python3_12` (`9 < 12`), not after (`"9" > "12"`). Used for the
/// `emerge -pv` `USE="…"` flag list -- real `_create_use_string`'s own
/// `any_iuse.sort(key=_alnum_sort_key)` (and the `--alphabetical` combined
/// list, and the removed-from-IUSE list).
///
/// A digit run longer than `u128` can hold (~39 digits) is kept as a
/// `Str` part instead of overflowing -- no real USE flag has one, and
/// `Str` vs `Str` still orders it deterministically.
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

/// One segment of an `alnum_sort_key` -- a non-digit run (`Str`) or a
/// digit run parsed as a number (`Num`). `Str` is ordered before `Num`
/// so a shorter key sorts before a longer one that extends it with a
/// number (the two variants never meet at the same position between two
/// well-formed keys -- the split always alternates `Str`/`Num`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlnumKey {
    Str(String),
    Num(u128),
}

/// Re-render the USE-display fields of every merge entry for `cp` using
/// `config` -- the `--autounmask-backtrack` off path uses this to reflect
/// an accumulated `package.use` change on a package the loop did not
/// re-walk (real `_pkg_use_enabled` consulting `_needed_use_config_
/// changes` for the display only). Mirrors the main walk's own entry-
/// display block; a no-op for an entry with no on-disk IUSE / no matching
/// candidate / a non-merge outcome.
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

/// Backlog #242 Slice D: repaint every running-rooted merge entry's USE
/// displays from the running root's own profile (`running_config`) — real
/// `_display_use` reads the package's own `root_config` (`output.py:822`),
/// so a cross-root row's `USE=`/`VAR=` groups come from its own root, not
/// the target's. The CLI layer calls this once per resolve when the run
/// is genuinely cross-root (running root differs from target); single-root
/// runs never reach here (no entry is running-rooted there). Candidate
/// lookups reuse the shared `repos` (both roots resolve the same trees —
/// Track X Slice B/C); old-USE diffs read the running root's own vdb,
/// like real's per-root `vardb`. A no-op for entries the candidate lookup
/// misses (same tolerance as [`refresh_entry_use_display`]).
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
    let use_flags = effective_use_flags(
        config,
        &cand.iuse,
        &cand.keywords,
        &candidate_str,
        &cat,
        &pkg,
    );
    // Real builds the USE display from `sorted(pkg.iuse.all)`, where
    // `iuse.all` is a `frozenset` (`Package.py`) -- so a flag that
    // `IUSE` names more than once (an eclass and the ebuild both
    // declaring it, e.g. gnome-extra/gnome-color-manager's
    // `IUSE="test test"`) still renders exactly once. Dedup by bare
    // name, keeping the first spelling.
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
    let forced = forced_or_masked_flags(
        &cand.iuse,
        &cand.keywords,
        &candidate_str,
        &cat,
        &pkg,
        config,
    );
    let reinst_flags: HashSet<String> = match &e.outcome {
        PretendOutcome::Reinstall { changed_flags, .. } => changed_flags.iter().cloned().collect(),
        _ => HashSet::new(),
    };
    e.use_expand_display = build_use_expand_display(
        &display,
        config,
        installed.as_ref(),
        &forced,
        true,
        &reinst_flags,
    );
    e.use_expand_display_p = build_use_expand_display(
        &display,
        config,
        installed.as_ref(),
        &forced,
        false,
        &reinst_flags,
    );
    e.use_flags_display = display;
}

#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn build_use_expand_display(
    use_flags_display: &[(String, bool)],
    config: &portage_profile::Config,
    installed: Option<&InstalledUseState>,
    forced: &HashSet<String>,
    all_flags: bool,
    reinst_flags: &HashSet<String>,
) -> Vec<(String, String)> {
    let mut expand_vars: Vec<String> = config.use_expand.iter().cloned().collect();
    expand_vars.sort();
    let hidden: HashSet<String> = config
        .use_expand_hidden
        .iter()
        .map(|s| s.to_uppercase())
        .collect();

    // One flag's polarity in `_create_use_string`'s own `enabled` /
    // `disabled` / `removed` split (rendered in that order).
    #[derive(Clone, Copy, PartialEq)]
    enum FlagState {
        Enabled,
        Disabled,
        Removed,
    }

    // Real `_create_use_string`'s per-flag marker + `( … )`-wrap logic
    // (see this function's own doc comment). Returns the rendered
    // `flag`/`-flag`/`(-flag%)` token with any `*`/`%` suffix and `( )`
    // wrap, or `None` when this flag isn't shown at this verbosity: real
    // `_create_use_string` leaves `flag_str = None` (so the flag is
    // omitted) for an *unchanged* flag -- and for a removed-from-IUSE
    // flag -- unless `conf.all_flags` (`verbosity == 3`, i.e. `emerge
    // -pv`) OR `reinst_flag` (this flag is in the reinstall's own
    // `_reinstall_for_flags` trigger set -- real `reinst_flags_map`).
    // The one case this actually changes at plain `-p`: a flag the new
    // ebuild dropped from IUSE that nonetheless triggered a
    // `--newuse`/`--changed-use` reinstall now shows in the `(-flag%)`
    // removed list (its enabled-state / IUSE-presence *change* markers
    // already carried the still-present trigger flags).
    let render_flag = |bare: &str, full: &str, state: FlagState| -> Option<String> {
        let is_forced = forced.contains(full);
        let reinst = reinst_flags.contains(full);
        if state == FlagState::Removed {
            if !all_flags && !reinst {
                return None;
            }
            // Real: `yellow("-" + flag) + "%"`, `+ "*"` if it was on,
            // always `"(" + ... + ")"`.
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
                        // An unchanged enabled flag: shown plain only
                        // under `-pv`'s `all_flags` (or if it's a
                        // reinstall trigger); omitted at plain `-p`.
                        bare.to_string()
                    } else {
                        return None;
                    }
                } else if !in_old_iuse {
                    // real: `if flag not in iuse_forced: flag_str += "%"`
                    if is_forced {
                        format!("-{bare}")
                    } else {
                        format!("-{bare}%")
                    }
                } else if in_old_use {
                    format!("-{bare}*")
                } else if all_flags || reinst {
                    // Unchanged disabled flag -- `-pv` only (or a
                    // reinstall trigger).
                    format!("-{bare}")
                } else {
                    return None;
                }
            }
        };
        Some(if is_forced { format!("({core})") } else { core })
    };

    // Group key: empty string == the plain "USE" group. Entries keep the
    // FULL flag name; the prefix is stripped only at render time (markers
    // are computed against the full name).
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
    // Real `removed_iuse = set(old_iuse).difference(cur_iuse)` -- flags
    // the current ebuild dropped from IUSE, shown as `(-flag%)` inside
    // `_create_use_string`'s `if conf.all_flags or reinst_flag` branch.
    // Route them whenever *either* could apply; `render_flag` drops a
    // removed flag that isn't a reinstall trigger when `!all_flags`.
    if let Some(inst) = installed.filter(|_| all_flags || !reinst_flags.is_empty()) {
        let cur: HashSet<&str> = use_flags_display.iter().map(|(f, _)| f.as_str()).collect();
        let mut removed: Vec<&String> = inst
            .old_iuse
            .iter()
            .filter(|f| !cur.contains(f.as_str()))
            .collect();
        // Natural sort, same key as the `display` list above (real
        // `_alnum_sort_key`). Real portage sorts `cur ∪ old` as one list;
        // portuale keeps its own cur-then-removed routing, a
        // pre-existing structural difference -- only the key changes here.
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
        // Real `_create_use_string`: `" ".join(enabled + disabled +
        // removed)` -- in that polarity order, each group already in the
        // incoming bare-name order. `--alphabetical` (which collapses
        // them into one interleaved list) is applied later, at render
        // time in `pretend.rs`, since it needs no resolver state.
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
