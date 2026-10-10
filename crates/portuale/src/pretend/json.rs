//! `--json` output.

use crate::pretend::*;

/// Escapes `s` for embedding in a JSON string literal (quote, backslash,
/// and control characters -- category/package/version/atom text from
/// portuale's own inputs never needs anything fancier). Hand-rolled
/// rather than pulling in a JSON crate: `--json`'s own output is a
/// small, flat shape, and portuale has no other dependency beyond
/// `regex` anywhere in the workspace -- see the module doc comment for
/// why `--json` exists at all (it's NOT a port of any real emerge
/// behavior).
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

/// One JSON object per `GraphEntry` -- a structured mirror of the plain-
/// text `[ebuild ...]`/`[binary ...]`/"already installed"/blocker lines
/// above, plus two fields no plain-text line carries at all: `requested`
/// (was this exact category/package one of `atoms` directly, as opposed
/// to reached only via a dependency string) and `required_by` (which
/// package(s), if any, pulled it in that way -- see
/// `GraphEntry::required_by`'s own doc comment, portage-repo). `source`
/// mirrors `entry.source`/the plain-text loop's own `bracket` variable
/// below (`"binary"`/`"ebuild"`, real `RootConfig.py`'s own
/// `pkg_tree_map`-driven `type_name`) -- until the binary-package slice
/// (`--usepkg`/`--usepkgonly`, `portage-repo`) this was always
/// `"ebuild"` unconditionally; it no longer is.
/// Deliberately NOT affected by `--onlydeps`'s own suppression (a
/// display-only concern for the plain-text loop below): `--json` always
/// dumps the whole resolved graph, letting a consumer filter on
/// `requested` itself if they want the `--onlydeps` view. `provenance`
/// (alongside `source`, so also absent for `NoVisibleCandidate`) mirrors
/// `entry.provenance`/`VisibilityProvenance` (portage-repo) directly --
/// portuale's own state-change trace: which `package.mask`/`.unmask`/
/// `package.accept_keywords` entries, if any, were actually load-bearing
/// for this candidate to be visible at all. Each of its three fields is
/// `null` rather than omitted when not applicable, unlike `use_flags`
/// above -- there's no `verbose` gate here (this is exactly the kind of
/// detail `--json` exists to expose unconditionally, see this module's
/// own doc comment) and a consumer scripting against this output
/// shouldn't have to branch on whether the key is even present.
/// `keyword_suggestion` is `provenance`'s own mirror image -- present
/// (as `{"version", "keyword"}` or `null`) only for `NoVisibleCandidate`
/// entries, since that's the one outcome with nothing visible to trace
/// provenance for and something to suggest instead. Mirrors
/// `entry.keyword_suggestion` (portage-repo) -- see its own doc comment.
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
        // The entry's 0-based position in real portage's dependency-first
        // merge order (`portage_repo::topological_merge_order`). The
        // `entries` array is already emitted in this order -- the field
        // is here so a consumer that re-sorts or filters the array keeps
        // the schedule.
        format!("\"merge_order\":{merge_order}"),
    ];
    let outcome_tag = match &entry.outcome {
        PretendOutcome::New { .. } => "new",
        PretendOutcome::Upgrade { .. } => "upgrade",
        PretendOutcome::Downgrade { .. } => "downgrade",
        PretendOutcome::Reinstall { .. } => "reinstall",
        PretendOutcome::AlreadyInstalled { .. } => "already_installed",
        PretendOutcome::NoVisibleCandidate => "no_visible_candidate",
        // #72 B3: `--json` filters these entries out (see `print_json`);
        // the tag exists for B4's real outcome string.
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
    // Real `output.py`'s own `S` bracket column, exposed unconditionally
    // (like every other `--json` field, see this module's own doc
    // comment): `true` for a `New` into a slot the package isn't
    // installed in while another slot of it is (`GraphEntry::new_slot`).
    if let PretendOutcome::New { .. } = &entry.outcome {
        fields.push(format!("\"new_slot\":{}", entry.new_slot));
    }
    // Real `output.py:833`'s own `I` bracket column, exposed
    // unconditionally: `true` for a merge-bound entry whose evaluated
    // `PROPERTIES` contains `interactive` (`GraphEntry::interactive`).
    if matches!(
        entry.outcome,
        PretendOutcome::New { .. }
            | PretendOutcome::Upgrade { .. }
            | PretendOutcome::Downgrade { .. }
            | PretendOutcome::Reinstall { .. }
    ) {
        fields.push(format!("\"interactive\":{}", entry.interactive));
        // Real `output.py:633`'s own `f`/`F` fetch-restrict column
        // (`GraphEntry::fetch_restrict` / `fetch_restrict_satisfied`).
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
    // `--root-deps`'s own running-root build entries (see `root_suffix`'s
    // own doc comment and `GraphEntry::targets_running_root`): the same
    // `to <root>` distinction the plain-text output carries, as an
    // explicit field -- the running-root path string for such an entry,
    // `null` for every ordinary `ROOT`-targeted one. `null` (rather than
    // absent) universally, same shape as `slot` above.
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

/// The whole `--json` output: `{"entries": [...], "slot_conflicts": [...]}`,
/// one line, no pretty-printing (a portuale-specific convenience format, not
/// a stable schema -- see the module doc comment).
pub(crate) fn autounmask_change_to_json(change: &portage_repo::AutounmaskChange) -> String {
    let chain: Vec<String> = change.dep_chain.iter().map(|l| json_string(l)).collect();
    format!(
        "{{\"atom\":{},\"token\":{},\"dep_chain\":[{}]}}",
        json_string(&change.atom),
        json_string(&change.token),
        chain.join(",")
    )
}

/// Backlog #19 Slice 4: the `--json` `aborted` field for a resolve
/// outcome — `null` on the complete path, `{"reason":…}` on an abort.
/// Reason spellings (`masked-dep`, `unsatisfied-atom`,
/// `unserializable-cycle`) are pinned by the contract suite's abort
/// fixtures.
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
/// Arguments of [`print_json`].
pub(crate) struct PrintJsonArgs<'a> {
    pub(crate) entries: &'a [GraphEntry],
    /// #59 S2 (K1 option (a)): the deterministic `(backtrack: N/M)` count
    /// real's `_show_resolution_report` prints on every run -- `--json`
    /// only, no stdout line (the timing half is a deliberate cut).
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
}

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
    // Backlog #19 Slice 4: the abort outcome as provenance — `null` on
    // the complete path every consumer already parses, `{"reason":…}`
    // on an abort (reason spellings pinned by the abort contract tests).
    // The `entries` array above is already the partial list on
    // an abort (the caller passes the display list), so a `--json`
    // consumer sees the same rows the text list shows.
    let aborted_json = abort_outcome_to_json(outcome);
    println!(
        "{{\"entries\":[{}],\"backtrack\":{{\"restarts\":{},\"max\":{}}},\"slot_conflicts\":[{}],\"changed_deps_report\":[{}],\"autounmask_keyword_changes\":[{}],\"autounmask_use_changes\":[{}],\"autounmask_license_changes\":[{}],\"autounmask_mask_changes\":[{}],\"abi_rebuilds\":[{}],\"aborted\":{}}}",
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
        aborted_json
    );
}
