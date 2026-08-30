//! `--json` output.

use crate::pretend::*;

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
/// filter on `requested` itself.
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
