//! `--json` output.

use crate::pretend::*;
use portage_repo::DepInput;
use std::collections::BTreeSet;

/// Escapes `s` for a JSON string literal (quote, backslash, control
/// characters). Hand-rolled: `--json` is a small flat shape with no other
/// JSON need anywhere in the workspace.
pub(crate) fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

pub(crate) fn json_string(s: &str) -> String {
    format!("\"{}\"", json_escape(s))
}

/// One JSON object per `GraphEntry`: a structured mirror of the plain-text
/// merge-list lines, plus `requested`/`required_by` (which no text line
/// carries) and the unconditional `--json`-only detail fields below.
/// Deliberately not affected by `--onlydeps` suppression: a consumer can
/// filter on `requested` itself. With `verbose`, merge-bound entries also
/// carry `sub_slot`, `dependencies`, `required_use`, `iuse` and `use` (see
/// [`dep_input_fields`]).
pub(crate) fn entry_to_json(
    entry: &GraphEntry,
    merge_order: usize,
    top_level_pkgs: &HashSet<(String, String)>,
    verbose: bool,
    running_root: Option<&Path>,
) -> String {
    let requested = top_level_pkgs.contains(&(entry.category.clone(), entry.package.clone()));
    let mut fields: Vec<String> = vec![
        format!("\"category\":{}", json_string(&entry.category)),
        format!("\"package\":{}", json_string(&entry.package)),
        // 0-based position in the dependency-first merge order; the array is
        // already in this order, so a consumer that re-sorts keeps the schedule.
        format!("\"merge_order\":{merge_order}"),
    ];
    let outcome_tag = match &entry.outcome {
        PretendOutcome::New { .. } => "new",
        PretendOutcome::Upgrade { .. } => "upgrade",
        PretendOutcome::Downgrade { .. } => "downgrade",
        PretendOutcome::Reinstall { .. } => "reinstall",
        PretendOutcome::AlreadyInstalled { .. } => "already_installed",
        PretendOutcome::NoVisibleCandidate => "no_visible_candidate",
        // `Uninstall` entries are filtered out of `--json` (see `print_json`).
        PretendOutcome::Uninstall { .. } => "uninstall",
    };
    fields.push(format!("\"outcome\":{}", json_string(outcome_tag)));
    match &entry.outcome {
        PretendOutcome::New { version }
        | PretendOutcome::AlreadyInstalled { version }
        | PretendOutcome::Uninstall { version } => {
            fields.push(format!("\"version\":{}", json_string(version)));
        }
        PretendOutcome::Upgrade { from, to } | PretendOutcome::Downgrade { from, to } => {
            fields.push(format!("\"version\":{}", json_string(to)));
            fields.push(format!("\"from_version\":{}", json_string(from)));
        }
        PretendOutcome::Reinstall {
            version,
            changed_flags,
            deps_changed,
            slot_changed,
            rebuilt_binary,
            new_repo,
            slot_operator_rebuild,
        } => {
            fields.push(format!("\"version\":{}", json_string(version)));
            let changed_use: Vec<String> = changed_flags.iter().map(|f| json_string(f)).collect();
            fields.push(format!("\"changed_use\":[{}]", changed_use.join(",")));
            fields.push(format!("\"changed_deps\":{deps_changed}"));
            fields.push(format!("\"changed_slot\":{slot_changed}"));
            fields.push(format!("\"rebuilt_binary\":{rebuilt_binary}"));
            fields.push(format!("\"new_repo\":{new_repo}"));
            fields.push(format!("\"slot_operator_rebuild\":{slot_operator_rebuild}"));
        }
        PretendOutcome::NoVisibleCandidate => {}
    }
    // The `S` bracket column (`GraphEntry::new_slot`): a `New` into a slot the
    // package is not installed in while another slot is.
    // Portage: output.py
    if let PretendOutcome::New { .. } = &entry.outcome {
        fields.push(format!("\"new_slot\":{}", entry.new_slot));
    }
    // The `I` bracket column (`GraphEntry::interactive`) and the `f`/`F`
    // fetch-restrict column, for merge-bound entries. Portage: output.py
    if matches!(
        entry.outcome,
        PretendOutcome::New { .. }
            | PretendOutcome::Upgrade { .. }
            | PretendOutcome::Downgrade { .. }
            | PretendOutcome::Reinstall { .. }
    ) {
        fields.push(format!("\"interactive\":{}", entry.interactive));
        fields.push(format!("\"fetch_restrict\":{}", entry.fetch_restrict));
        fields.push(format!(
            "\"fetch_restrict_satisfied\":{}",
            entry.fetch_restrict_satisfied
        ));
    }
    fields.push(format!(
        "\"slot\":{}",
        entry
            .slot
            .as_deref()
            .map(json_string)
            .unwrap_or_else(|| "null".to_string())
    ));
    if !matches!(entry.outcome, PretendOutcome::NoVisibleCandidate) {
        let source_tag = match entry.source {
            portage_repo::CandidateSource::Binary => "binary",
            portage_repo::CandidateSource::Ebuild => "ebuild",
        };
        fields.push(format!("\"source\":{}", json_string(source_tag)));
        if let Some(bid) = &entry.build_id {
            fields.push(format!("\"build_id\":{}", json_string(bid)));
        }
        let opt_str = |v: &Option<String>| {
            v.as_deref()
                .map(json_string)
                .unwrap_or_else(|| "null".to_string())
        };
        fields.push(format!(
            "\"provenance\":{{\"mask_entry\":{},\"unmask_entry\":{},\"keyword_entry\":{}}}",
            opt_str(&entry.provenance.mask_entry),
            opt_str(&entry.provenance.unmask_entry),
            opt_str(&entry.provenance.keyword_entry),
        ));
    } else {
        fields.push(format!(
            "\"keyword_suggestion\":{}",
            entry
                .keyword_suggestion
                .as_ref()
                .map(|(version, keyword)| format!(
                    "{{\"version\":{},\"keyword\":{}}}",
                    json_string(version),
                    json_string(keyword)
                ))
                .unwrap_or_else(|| "null".to_string())
        ));
        fields.push(format!(
            "\"use_suggestion\":{}",
            entry
                .use_suggestion
                .as_ref()
                .map(|(version, flip)| {
                    let flags: Vec<String> = flip
                        .iter()
                        .map(|(flag, enabled)| {
                            format!("{{\"flag\":{},\"enabled\":{enabled}}}", json_string(flag))
                        })
                        .collect();
                    format!(
                        "{{\"version\":{},\"flags\":[{}]}}",
                        json_string(version),
                        flags.join(",")
                    )
                })
                .unwrap_or_else(|| "null".to_string())
        ));
        fields.push(format!(
            "\"parent_use_suggestion\":{}",
            entry
                .parent_use_suggestion
                .as_ref()
                .map(|(parent_category, parent_package, parent_version, flip)| {
                    let flags: Vec<String> = flip
                        .iter()
                        .map(|(flag, enabled)| {
                            format!("{{\"flag\":{},\"enabled\":{enabled}}}", json_string(flag))
                        })
                        .collect();
                    format!(
                        "{{\"category\":{},\"package\":{},\"version\":{},\"flags\":[{}]}}",
                        json_string(parent_category),
                        json_string(parent_package),
                        json_string(parent_version),
                        flags.join(",")
                    )
                })
                .unwrap_or_else(|| "null".to_string())
        ));
    }
    fields.push(format!("\"requested\":{requested}"));
    let required_by: Vec<String> = entry
        .required_by
        .iter()
        .map(|(category, package)| {
            format!(
                "{{\"category\":{},\"package\":{}}}",
                json_string(category),
                json_string(package)
            )
        })
        .collect();
    fields.push(format!("\"required_by\":[{}]", required_by.join(",")));
    // The `to <root>` distinction the plain-text output carries, as an explicit
    // field: the running-root path for `--root-deps` build entries, else `null`.
    fields.push(format!(
        "\"builds_against_running_root\":{}",
        if entry.targets_running_root {
            running_root
                .map(|r| json_string(&r.display().to_string()))
                .unwrap_or_else(|| "null".to_string())
        } else {
            "null".to_string()
        }
    ));
    if verbose && !entry.use_flags_display.is_empty() {
        let use_flags: Vec<String> = entry
            .use_flags_display
            .iter()
            .map(|(flag, enabled)| format!("{}:{enabled}", json_string(flag)))
            .collect();
        fields.push(format!("\"use_flags\":{{{}}}", use_flags.join(",")));
    }
    if verbose
        && is_merge_bound(&entry.outcome)
        && let Some(input) = &entry.dep_input
    {
        let sub_slot = entry
            .sub_slot
            .as_deref()
            .map_or("null".to_string(), json_string);
        fields.push(format!("\"sub_slot\":{sub_slot}"));
        fields.extend(dep_input_fields(input));
    }
    let blockers: Vec<String> = entry
        .blockers
        .iter()
        .map(|b| {
            format!(
                "{{\"atom\":{},\"strong\":{},\"matched_category\":{},\"matched_package\":{},\"matched_version\":{},\"unsolvable\":{}}}",
                json_string(&b.atom_str),
                b.strong,
                json_string(&b.matched_category),
                json_string(&b.matched_package),
                json_string(&b.matched_version),
                b.unsolvable
            )
        })
        .collect();
    fields.push(format!("\"blockers\":[{}]", blockers.join(",")));
    format!("{{{}}}", fields.join(","))
}

fn is_merge_bound(outcome: &PretendOutcome) -> bool {
    matches!(
        outcome,
        PretendOutcome::New { .. }
            | PretendOutcome::Upgrade { .. }
            | PretendOutcome::Downgrade { .. }
            | PretendOutcome::Reinstall { .. }
    )
}

fn json_string_array<'a>(items: impl IntoIterator<Item = &'a str>) -> String {
    let items: Vec<String> = items.into_iter().map(json_string).collect();
    format!("[{}]", items.join(","))
}

/// What a plan checker needs to evaluate one merge-bound entry on its own: the
/// raw dependency strings by class, `REQUIRED_USE`, the `IUSE` names, and the
/// enabled flags among those the strings can test (`IUSE`, every flag a
/// `flag?` conditional names and every `REQUIRED_USE` flag, which covers
/// implicit ones such as `elibc_glibc` or `kernel_linux`).
pub(crate) fn dep_input_fields(input: &DepInput) -> Vec<String> {
    let meta = |key: &str| input.metadata.get(key).map_or("", |v| v.trim());
    let dependencies: Vec<String> = ["DEPEND", "BDEPEND", "RDEPEND", "PDEPEND", "IDEPEND"]
        .into_iter()
        .filter(|key| !meta(key).is_empty())
        .map(|key| format!("{}:{}", json_string(key), json_string(meta(key))))
        .collect();
    let iuse: BTreeSet<&str> = meta("IUSE")
        .split_whitespace()
        .map(|flag| flag.trim_start_matches(['+', '-']))
        .collect();
    let conditionals = ["DEPEND", "BDEPEND", "RDEPEND", "PDEPEND", "IDEPEND"]
        .into_iter()
        .flat_map(|key| meta(key).split_whitespace())
        .filter_map(|token| token.strip_suffix('?'));
    // Every `REQUIRED_USE` token but the group operators names a flag.
    let required = meta("REQUIRED_USE")
        .split_whitespace()
        .filter(|token| !matches!(*token, "||" | "^^" | "??" | "(" | ")"))
        .map(|token| token.trim_end_matches('?'));
    let named = conditionals
        .chain(required)
        .map(|flag| flag.trim_start_matches('!'));
    let testable: BTreeSet<&str> = iuse.iter().copied().chain(named).collect();
    let enabled = testable
        .into_iter()
        .filter(|flag| input.use_flags.contains(*flag));
    vec![
        format!("\"dependencies\":{{{}}}", dependencies.join(",")),
        format!("\"required_use\":{}", json_string(meta("REQUIRED_USE"))),
        format!("\"iuse\":{}", json_string_array(iuse)),
        format!("\"use\":{}", json_string_array(enabled)),
    ]
}

/// The installed packages the plan leaves alone: not replaced in their slot by
/// a merge-bound entry and not uninstalled. Running-root entries are ignored.
pub(crate) fn kept_packages(
    root: &Path,
    entries: &[GraphEntry],
) -> Vec<portage_repo::InstalledPackage> {
    let mut replaced_slots: HashSet<(&str, &str, &str)> = HashSet::new();
    let mut uninstalled: HashSet<(&str, &str, &str)> = HashSet::new();
    for e in entries.iter().filter(|e| !e.targets_running_root) {
        let cp = (e.category.as_str(), e.package.as_str());
        match &e.outcome {
            PretendOutcome::Uninstall { version } => {
                uninstalled.insert((cp.0, cp.1, version));
            }
            outcome if is_merge_bound(outcome) => {
                if let Some(slot) = &e.slot {
                    replaced_slots.insert((cp.0, cp.1, slot));
                }
            }
            _ => {}
        }
    }
    let mut kept: Vec<portage_repo::InstalledPackage> = portage_repo::all_installed_packages(root)
        .iter()
        .filter(|p| {
            !replaced_slots.contains(&(&p.category, &p.package, &p.slot))
                && !uninstalled.contains(&(&p.category, &p.package, &p.version))
        })
        .cloned()
        .collect();
    // The VDB scan follows directory order; the output must not.
    kept.sort_by_key(portage_repo::InstalledPackage::cpv);
    kept
}

fn kept_to_json(kept: &[portage_repo::InstalledPackage]) -> String {
    let items: Vec<String> = kept
        .iter()
        .map(|p| {
            format!(
                "{{\"category\":{},\"package\":{},\"version\":{},\"slot\":{}}}",
                json_string(&p.category),
                json_string(&p.package),
                json_string(&p.version),
                json_string(&p.slot)
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

pub(crate) fn slot_conflict_to_json(c: &SlotConflict) -> String {
    let instances: Vec<String> = c
        .instances
        .iter()
        .map(|inst| {
            let parents: Vec<String> = inst
                .parents
                .iter()
                .map(|p| {
                    format!(
                        "{{\"parent\":{},\"atom\":{},\"use\":{},\"installed\":{}}}",
                        json_string(&p.parent_cpv),
                        json_string(&p.atom),
                        json_string(&render_pkg_use_display(&p.use_display)),
                        p.installed
                    )
                })
                .collect();
            format!(
                "{{\"version\":{},\"sub_slot\":{},\"repo_name\":{},\"use\":{},\"installed\":{},\"parents\":[{}]}}",
                json_string(&inst.version),
                json_string(&inst.sub_slot),
                json_string(&inst.repo_name),
                json_string(&render_pkg_use_display(&inst.use_display)),
                inst.installed,
                parents.join(",")
            )
        })
        .collect();
    format!(
        "{{\"category\":{},\"package\":{},\"slot\":{},\"resolved_version\":{},\"conflicting_atom\":{},\"instances\":[{}]}}",
        json_string(&c.category),
        json_string(&c.package),
        json_string(&c.slot),
        json_string(&c.resolved_version),
        json_string(&c.conflicting_atom),
        instances.join(",")
    )
}

pub(crate) fn changed_deps_report_entry_to_json(c: &ChangedDepsReportEntry) -> String {
    format!(
        "{{\"category\":{},\"package\":{},\"version\":{},\"repo_name\":{}}}",
        json_string(&c.category),
        json_string(&c.package),
        json_string(&c.version),
        json_string(&c.repo_name)
    )
}

/// One JSON object per autounmask change: atom, token, and dependency chain.
pub(crate) fn autounmask_change_to_json(change: &portage_repo::AutounmaskChange) -> String {
    let chain: Vec<String> = change.dep_chain.iter().map(|l| json_string(l)).collect();
    format!(
        "{{\"atom\":{},\"token\":{},\"dep_chain\":[{}]}}",
        json_string(&change.atom),
        json_string(&change.token),
        chain.join(",")
    )
}

/// The `--json` `aborted` field: `null` on the complete path,
/// `{"reason":…}` on an abort (reason spellings pinned by the abort fixtures).
pub(crate) fn abort_outcome_to_json(outcome: &portage_repo::ResolveOutcome) -> String {
    match outcome {
        portage_repo::ResolveOutcome::Complete => "null".to_string(),
        portage_repo::ResolveOutcome::Aborted { reason, .. } => match reason {
            portage_repo::AbortReason::MaskedDep { atom, parent_cpv } => format!(
                "{{\"reason\":\"masked-dep\",\"atom\":{},\"parent\":{}}}",
                json_string(atom),
                json_string(parent_cpv)
            ),
            portage_repo::AbortReason::UnsatisfiedAtom { atom, parent_cpv } => format!(
                "{{\"reason\":\"unsatisfied-atom\",\"atom\":{},\"parent\":{}}}",
                json_string(atom),
                json_string(parent_cpv)
            ),
            portage_repo::AbortReason::UnserializableCycle { members } => {
                let members: Vec<String> = members.iter().map(|m| json_string(m)).collect();
                format!(
                    "{{\"reason\":\"unserializable-cycle\",\"members\":[{}]}}",
                    members.join(",")
                )
            }
            portage_repo::AbortReason::VirtualCycle { members } => {
                let members: Vec<String> = members.iter().map(|m| json_string(m)).collect();
                format!(
                    "{{\"reason\":\"virtual-cycle\",\"members\":[{}]}}",
                    members.join(",")
                )
            }
        },
    }
}
/// The `--json` document of a pretend removal, one line: each removed package
/// with the libraries it owns that surviving files still link against
/// (`needed_libraries`) and whether its unmerge may preserve them
/// (`preserve_libs`: `FEATURES` and its own `RESTRICT`), then `preserved`,
/// the libraries the removal keeps on disk.
pub(crate) fn print_removal_json(
    root: &Path,
    config: &portage_profile::Config,
    removal: &[(String, String, String)],
) {
    let keys: Vec<(String, String)> = removal
        .iter()
        .map(|(category, package, version)| (category.clone(), format!("{package}-{version}")))
        .collect();
    let feature = config_has_preserve_libs(config);
    let mut preserved: BTreeSet<&str> = BTreeSet::new();
    let libs = crate::ebuild_merge::libs_still_linked(root, &keys);
    let items: Vec<String> = removal
        .iter()
        .zip(&keys)
        .zip(&libs)
        .map(|(((category, package, version), (_, pf)), libs)| {
            let gate =
                feature && !crate::ebuild_merge::installed_restricts_preserve_libs(root, category, pf);
            if gate {
                preserved.extend(libs.iter().map(String::as_str));
            }
            let slot = portage_repo::vdb_entry_slot(root, category, pf);
            format!(
                "{{\"category\":{},\"package\":{},\"version\":{},\"slot\":{},\"needed_libraries\":{},\"preserve_libs\":{gate}}}",
                json_string(category),
                json_string(package),
                json_string(version),
                json_string(slot.split('/').next().unwrap_or_default()),
                json_string_array(libs.iter().map(String::as_str)),
            )
        })
        .collect();
    println!(
        "{{\"removal\":[{}],\"preserved\":{}}}",
        items.join(","),
        json_string_array(preserved)
    );
}

/// The options that decide which candidates a plan could use: a plan checker
/// reads them to know whether a binary package was picked over a newer ebuild
/// on purpose (`--usepkgonly`) or by the version rule (`--usepkg`).
#[derive(Clone, Copy)]
pub(crate) struct CandidateOptions {
    pub(crate) usepkg: bool,
    pub(crate) usepkgonly: bool,
    pub(crate) getbinpkg: bool,
    pub(crate) getbinpkgonly: bool,
}

/// The `options` object of the `-v` `--json` document.
pub(crate) fn candidate_options_to_json(options: CandidateOptions) -> String {
    format!(
        "{{\"usepkg\":{},\"usepkgonly\":{},\"getbinpkg\":{},\"getbinpkgonly\":{}}}",
        options.usepkg, options.usepkgonly, options.getbinpkg, options.getbinpkgonly
    )
}

/// Arguments of [`print_json`].
pub(crate) struct PrintJsonArgs<'a> {
    pub(crate) entries: &'a [GraphEntry],
    /// The deterministic `(backtrack: N/M)` count as data (`--json` only, no
    /// stdout line; the wall-clock half is cut for determinism).
    pub(crate) backtrack_restarts: u64,
    pub(crate) backtrack_max: u32,
    pub(crate) slot_conflicts: &'a [SlotConflict],
    pub(crate) changed_deps_report: &'a [ChangedDepsReportEntry],
    pub(crate) autounmask_keyword_changes: &'a [portage_repo::AutounmaskChange],
    pub(crate) autounmask_use_changes: &'a [portage_repo::AutounmaskChange],
    pub(crate) autounmask_license_changes: &'a [portage_repo::AutounmaskChange],
    pub(crate) autounmask_mask_changes: &'a [portage_repo::AutounmaskChange],
    pub(crate) abi_rebuilds: &'a [(String, String)],
    pub(crate) outcome: &'a portage_repo::ResolveOutcome,
    pub(crate) top_level_pkgs: &'a HashSet<(String, String)>,
    pub(crate) verbose: bool,
    pub(crate) running_root: Option<&'a Path>,
    /// The target root, read for the `-v` `kept` list.
    pub(crate) root: &'a Path,
    /// The command-line atoms and sets, for the `-v` `request` list.
    pub(crate) request: &'a [&'a str],
    /// The binary-package options, for the `-v` `options` object.
    pub(crate) candidate_options: CandidateOptions,
}

/// The whole `--json` output as one line, no pretty-printing (a
/// portuale-specific convenience format, not a stable schema). With
/// `verbose` it ends with `kept` (see [`kept_packages`]), `request` (the
/// command-line atoms) and `options` (see [`CandidateOptions`]), so the
/// document alone describes the final installed set, the user's constraints
/// and the candidates the run could use, for a plan checker.
pub(crate) fn print_json(args: PrintJsonArgs<'_>) {
    let PrintJsonArgs {
        entries,
        backtrack_restarts,
        backtrack_max,
        slot_conflicts,
        changed_deps_report,
        autounmask_keyword_changes,
        autounmask_use_changes,
        autounmask_license_changes,
        autounmask_mask_changes,
        abi_rebuilds,
        outcome,
        top_level_pkgs,
        verbose,
        running_root,
        root,
        request,
        candidate_options,
    } = args;
    let entries_json: Vec<String> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| entry_to_json(e, i, top_level_pkgs, verbose, running_root))
        .collect();
    let conflicts_json: Vec<String> = slot_conflicts.iter().map(slot_conflict_to_json).collect();
    let changed_deps_report_json: Vec<String> = changed_deps_report
        .iter()
        .map(changed_deps_report_entry_to_json)
        .collect();
    let autounmask_kw_json: Vec<String> = autounmask_keyword_changes
        .iter()
        .map(autounmask_change_to_json)
        .collect();
    let autounmask_use_json: Vec<String> = autounmask_use_changes
        .iter()
        .map(autounmask_change_to_json)
        .collect();
    let autounmask_license_json: Vec<String> = autounmask_license_changes
        .iter()
        .map(autounmask_change_to_json)
        .collect();
    let autounmask_mask_json: Vec<String> = autounmask_mask_changes
        .iter()
        .map(autounmask_change_to_json)
        .collect();
    let abi_rebuilds_json: Vec<String> = abi_rebuilds
        .iter()
        .map(|(child, parent)| {
            format!(
                "{{\"provider\":{},\"consumer\":{}}}",
                json_string(child),
                json_string(parent)
            )
        })
        .collect();
    // On an abort the caller passes the partial display list, so a `--json`
    // consumer sees the same rows the text list shows.
    let aborted_json = abort_outcome_to_json(outcome);
    // `-v` adds, for plan checkers, the installed packages the plan leaves
    // alone and the command-line request.
    let kept_json = if verbose {
        format!(
            ",\"kept\":{},\"request\":{},\"options\":{}",
            kept_to_json(&kept_packages(root, entries)),
            json_string_array(request.iter().copied()),
            candidate_options_to_json(candidate_options)
        )
    } else {
        String::new()
    };
    println!(
        "{{\"entries\":[{}],\"backtrack\":{{\"restarts\":{},\"max\":{}}},\"slot_conflicts\":[{}],\"changed_deps_report\":[{}],\"autounmask_keyword_changes\":[{}],\"autounmask_use_changes\":[{}],\"autounmask_license_changes\":[{}],\"autounmask_mask_changes\":[{}],\"abi_rebuilds\":[{}],\"aborted\":{}{}}}",
        entries_json.join(","),
        backtrack_restarts,
        backtrack_max,
        conflicts_json.join(","),
        changed_deps_report_json.join(","),
        autounmask_kw_json.join(","),
        autounmask_use_json.join(","),
        autounmask_license_json.join(","),
        autounmask_mask_json.join(","),
        abi_rebuilds_json.join(","),
        aborted_json,
        kept_json
    );
}
