//! Eliminating unnecessary slot-operator rebuilds.

use crate::*;

/// `<version>` with a trailing `-r<digits>` revision suffix removed.
/// Returns the input unchanged when there is no such suffix.
pub(crate) fn strip_revision(version: &str) -> &str {
    match version.rsplit_once("-r") {
        Some((base, rev)) if !rev.is_empty() && rev.bytes().all(|b| b.is_ascii_digit()) => base,
        _ => version,
    }
}

/// Arguments of [`rebuild_if_entries`].
pub(crate) struct RebuildIfEntriesArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) unbuilt: bool,
    pub(crate) new_rev: bool,
    pub(crate) new_ver: bool,
    pub(crate) rebuild_exclude: &'a [String],
    pub(crate) rebuild_ignore: &'a [String],
}

/// Portage's `_rebuild_config` (`--rebuild-if-unbuilt` / `--rebuild-if-new-rev`
/// / `--rebuild-if-new-ver`): an already-installed package whose own build-time
/// dependency is satisfied by a package this run merges is scheduled for a
/// rebuild. `--rebuild-exclude` matches the parent, `--rebuild-ignore` the dep.
// Portage: _emerge/depgraph.py, _rebuild_config
pub(crate) fn rebuild_if_entries(args: RebuildIfEntriesArgs<'_>) -> Vec<GraphEntry> {
    let RebuildIfEntriesArgs {
        root,
        entries,
        unbuilt,
        new_rev,
        new_ver,
        rebuild_exclude,
        rebuild_ignore,
    } = args;
    if !(unbuilt || new_rev || new_ver) {
        return Vec::new();
    }
    // `cat/pkg` -> merged version, for every entry being installed this run.
    let mut merged: HashMap<(String, String), String> = HashMap::new();
    let mut in_graph: HashSet<(String, String)> = HashSet::new();
    for e in entries {
        let cp = (e.category.clone(), e.package.clone());
        let version = match &e.outcome {
            PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                Some(version)
            }
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => Some(to),
            // A removal merges nothing.
            PretendOutcome::AlreadyInstalled { .. }
            | PretendOutcome::NoVisibleCandidate
            | PretendOutcome::Uninstall { .. } => None,
        };
        if let Some(v) = version {
            in_graph.insert(cp.clone());
            merged.insert(cp, v.clone());
        }
    }
    if merged.is_empty() {
        return Vec::new();
    }

    let mut out: Vec<GraphEntry> = Vec::new();
    for pkg in all_installed_packages(root) {
        let cp = (pkg.category.clone(), pkg.package.clone());
        if in_graph.contains(&cp) {
            continue;
        }
        let parent_cpv = pkg.cpv();
        if rebuild_exclude
            .iter()
            .any(|a| matches_config_entry(a, &parent_cpv, &pkg.category, &pkg.package))
        {
            continue;
        }
        let built_use = read_vdb_flag_set(root, &pkg.category, &pkg.package, &pkg.version, "USE");
        let mut depstr = String::new();
        for key in ["DEPEND", "BDEPEND"] {
            depstr.push_str(&read_vdb_string(
                root,
                &pkg.category,
                &pkg.package,
                &pkg.version,
                key,
            ));
            depstr.push(' ');
        }
        let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
        let Ok(flat) = portage_use_reduce::use_reduce_flat(
            &tokens,
            &built_use,
            portage_use_reduce::MatchMode::Normal,
        ) else {
            continue;
        };

        let triggered = flat
            .iter()
            .filter_map(|t| portage_dep::parse_atom(t))
            .any(|atom| {
                if atom.blocker != portage_dep::Blocker::None {
                    return false;
                }
                let Some(merged_ver) = merged.get(&(atom.category.clone(), atom.package.clone()))
                else {
                    return false;
                };
                let dep_cpv = format!("{}/{}-{merged_ver}", atom.category, atom.package);
                if rebuild_ignore
                    .iter()
                    .any(|a| matches_config_entry(a, &dep_cpv, &atom.category, &atom.package))
                {
                    return false;
                }
                if unbuilt {
                    return true;
                }
                let installed = installed_versions(root, &atom.category, &atom.package);
                if new_rev {
                    return !installed.iter().any(|v| v == merged_ver);
                }
                // new_ver: revision ignored on both sides.
                let want = strip_revision(merged_ver);
                !installed.iter().any(|v| strip_revision(v) == want)
            });
        if !triggered {
            continue;
        }

        let (slot, sub_slot) = read_vdb_slot(root, &pkg.category, &pkg.package, &pkg.version);
        let repo = read_vdb_string(
            root,
            &pkg.category,
            &pkg.package,
            &pkg.version,
            "repository",
        )
        .trim()
        .to_string();
        out.push(GraphEntry {
            discovery: 0,
            category: pkg.category.clone(),
            package: pkg.package.clone(),
            outcome: PretendOutcome::Reinstall {
                version: pkg.version.clone(),
                changed_flags: Vec::new(),
                deps_changed: false,
                slot_changed: false,
                rebuilt_binary: false,
                new_repo: false,
                slot_operator_rebuild: false,
            },
            blockers: Vec::new(),
            slot: Some(slot),
            sub_slot: Some(sub_slot),
            repo_name: (!repo.is_empty()).then_some(repo),
            oldbest: Vec::new(),
            use_flags_display: Vec::new(),
            use_expand_display: Vec::new(),
            use_expand_display_p: Vec::new(),
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: Vec::new(),
            required_by: Vec::new(),
            source: CandidateSource::Ebuild,
            provenance: VisibilityProvenance::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: false,
            build_id: None,
            deps: Vec::new(),
        });
    }
    out.sort_by(|a, b| (a.category.as_str(), a.package.as_str()).cmp(&(&b.category, &b.package)));
    out
}
