//! Add the chosen candidate to the graph, with its autounmask suggestions.

use crate::resolve::pass::*;
use portage_profile::Config;

/// What [`push_graph_entry`] hands back to its caller.
pub(crate) struct PushGraphEntryOut {
    pub(crate) entry_idx: usize,
    pub(crate) candidate_source: CandidateSource,
    pub(crate) candidate_remote: bool,
    pub(crate) candidate_build_id: Option<String>,
    pub(crate) already_installed_outcome: bool,
    pub(crate) candidate_str: String,
}

/// The values [`push_graph_entry`] reads from its caller.
pub(crate) struct PushGraphEntryArgs<'a> {
    pub(crate) state: &'a mut PassState,
    pub(crate) slot_key: &'a (String, String, String, bool),
    pub(crate) key: &'a (String, String),
    pub(crate) slot: &'a str,
    pub(crate) depth: u32,
    pub(crate) resolved: &'a Candidate,
    pub(crate) config: &'a Config,
    pub(crate) version: &'a str,
    pub(crate) sub_slot: &'a str,
    pub(crate) repo_name: &'a str,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) outcome: PretendOutcome,
    pub(crate) item_targets_running_root: bool,
}

/// Add the chosen candidate to the graph, with its autounmask suggestions.
pub(crate) fn push_graph_entry(args: PushGraphEntryArgs<'_>) -> PushGraphEntryOut {
    let PushGraphEntryArgs {
        state,
        slot_key,
        key,
        slot,
        depth,
        resolved,
        config,
        version,
        sub_slot,
        repo_name,
        bp,
        ctx,
        outcome,
        item_targets_running_root,
    } = args;
    let entry_idx = state.entries.len();
    state.resolved_slots.insert(slot_key.clone(), entry_idx);
    // Backlog #218: remember the first-resolving depth for a later
    // in-pass autounmask delta re-expansion (see `resolved_depths`).
    // Still root-blind (one depth per slot): same-cp-both-roots depth
    // arbitration is Slice C's dual-node modeling, not this slice's.
    state
        .resolved_depths
        .insert((key.0.clone(), key.1.clone(), slot.to_string()), depth);
    let candidate_source = resolved.source;
    // Real `output.py:648`: `attr_display.remote_binary = pkg.remote`.
    let candidate_remote = resolved.remote;
    // Real `output.py::_append_build_id`: only a `type_name ==
    // "binary"` package's version gets the `-{build_id}` suffix.
    let candidate_build_id = if candidate_source == CandidateSource::Binary {
        resolved.build_id.clone()
    } else {
        None
    };
    let provenance = visibility_provenance(resolved, &key.0, &key.1, config);
    let keyword_mask =
        keyword_mask_marker(resolved, &key.0, &key.1, config, &provenance.mask_entry);
    let resolved_cpv_str = format!(
        "{}/{}-{version}:{slot}/{sub_slot}::{repo_name}",
        key.0, key.1
    );
    // Real `--autounmask` keyword resolution: `resolve_pretend`
    // accepted this candidate over an unaccepted `KEYWORDS` (real
    // `_pkg_visibility_check` under an `allow_unstable_keywords`
    // level). Record the implicit `=<cpv> <keyword>` change (real
    // `depgraph.py::_display_autounmask`'s `unstable_keyword_msg` +
    // `_get_dep_chain_as_comment`; `autounmask_unrestricted_atoms`
    // defaults to `"n"`, so always the exact-version `=` form). The
    // gate is per-category, not `keyword_masked_only`: a candidate
    // masked by `~arch` *and* `LICENSE` records *both* changes.
    if bp.autounmask_suggest_keywords
        && !keywords_accepted(KeywordsAcceptedArgs {
            keywords: &resolved.keywords,
            candidate_str: &resolved_cpv_str,
            category: &key.0,
            package: &key.1,
            accept_keywords: &config.accept_keywords,
            package_accept_keywords: &config.package_accept_keywords,
        })
        && let Some(kw) = suggested_keyword(resolved)
    {
        state.autounmask_keyword_changes.push(AutounmaskChange {
            atom: format!("={}/{}-{version}", key.0, key.1),
            token: kw.to_string(),
            dep_chain: Vec::new(),
            trigger: None,
        });
    }
    // Real `--autounmask-license`: `resolve_pretend` accepted this
    // candidate over an unaccepted `LICENSE`. Record the missing
    // licenses (real `_display_autounmask`'s `license_msg`;
    // `check_if_latest(pkg)` without `check_visibility` -> the `>=` /
    // `>=…:slot` / `=` form). Per-category gate (the non-empty
    // `missing` list *is* the "license needs a change" signal), so a
    // multi-category mask still records its license part.
    if bp.autounmask_suggest_license {
        let missing = missing_licenses(resolved, &key.0, &key.1, &resolved_cpv_str, config);
        if !missing.is_empty() {
            let all = list_candidates(&ctx.repos, &key.0, &key.1).unwrap_or_default();
            state.autounmask_license_changes.push(AutounmaskChange {
                atom: check_if_latest_atom_form(CheckIfLatestAtomFormArgs {
                    resolved,
                    all_candidates: &all,
                    category: &key.0,
                    package: &key.1,
                    config,
                    check_visibility: false,
                }),
                token: missing.join(" "),
                dep_chain: Vec::new(),
                trigger: None,
            });
        }
    }
    // Real `--autounmask-keep-masks=n`: `resolve_pretend` accepted
    // this candidate over an active `package.mask`. Record `=<cpv>`
    // (real `p_mask_change_msg`; `autounmask_unrestricted_atoms`
    // `"n"` -> always the exact-version form; no token). Per-category
    // gate (`package.mask` matched, nothing unmasked it), so a
    // multi-category mask still records its mask part.
    if bp.autounmask_suggest_masks
        && provenance.mask_entry.is_some()
        && provenance.unmask_entry.is_none()
    {
        state.autounmask_mask_changes.push(AutounmaskChange {
            atom: format!("={}/{}-{version}", key.0, key.1),
            token: String::new(),
            dep_chain: Vec::new(),
            trigger: None,
        });
    }
    // Real `_get_installed_best`'s `new_slot`: a `New` entry whose
    // `category/package` is installed in some *other* slot (the
    // in-slot New/Upgrade decision already happened inside
    // `resolve_pretend`, so `New` here means nothing in *this* slot).
    let new_slot = matches!(outcome, PretendOutcome::New { .. })
        && !installed_candidates(ctx.root, &key.0, &key.1).is_empty();
    // Kept before `outcome` moves into the entry below: real's
    // REQUIRED_USE gate is `not pkg.built` (`depgraph.py:3629-3632`),
    // and an `AlreadyInstalled` outcome is exactly portuale's
    // installed ("built") case.
    let already_installed_outcome = matches!(outcome, PretendOutcome::AlreadyInstalled { .. });
    let candidate_str = format!(
        "{}/{}-{version}:{slot}/{sub_slot}::{repo_name}",
        key.0, key.1
    );
    // Real `output.py:833`: `if "interactive" in pkg.properties and
    // pkg.operation == "merge"`. `pkg.properties` is `PROPERTIES`
    // after real USE-conditional evaluation (`_PackageMetadataWrapper`,
    // gated on `"?" in v`); every graph entry reaching this point is
    // a merge (`New`/`Upgrade`/`Downgrade`/`Reinstall` -- the only
    // outcomes `resolved_slots` ever indexes), so no separate
    // operation check is needed here.
    let interactive = evaluated_metadata_tokens(EvaluatedMetadataTokensArgs {
        value_str: &resolved.properties,
        candidate: resolved,
        category: &key.0,
        package: &key.1,
        candidate_str: &candidate_str,
        config,
    })
    .contains("interactive");
    // Real `_get_installed_best`'s `myoldbest`: an `Upgrade`/
    // `Downgrade` replaces the installed version(s) in *its* slot
    // (`myinslotlist = vardb.match(pkg.slot_atom)`); a new-slot `New`
    // lists every installed version, all slots
    // (`myoldbest = installed_versions`). Version-sorted for a stable
    // display order. A brand-new `New` -> empty.
    //
    // A `Reinstall` is real's `vardb.cpv_exists(pkg.cpv)` arm
    // (`output.py:721-731`): `replace = True`, and `myoldbest` is the
    // installed instance when its `(slot, sub_slot)` differs from the
    // one being merged -- the `[oldver]` bracket a slot/sub-slot move
    // (e.g. a slot-operator cascade rebuild landing at its tree
    // ebuild's new sub-slot) shows -- or when the repo differs and
    // repo display is not quieted (real's third disjunct, `not
    // quiet_repo_display and installed_version.repo != pkg.repo`).
    // Same rule the old post-pass slot-op synthesiser applied to its
    // own entries. The quiet half needs no threading:
    // `--quiet-repo-display` stays an unimplemented option (it exits
    // 2), so every successful run has it off and the disjunct below
    // reduces to the repo comparison (owner B5 reopened this cut,
    // backlog #247).
    let mut oldbest: Vec<InstalledRef> = match &outcome {
        PretendOutcome::Upgrade { .. } | PretendOutcome::Downgrade { .. } => {
            installed_refs(ctx.root, &key.0, &key.1)
                .into_iter()
                .filter(|r| r.slot == slot)
                .collect()
        }
        PretendOutcome::New { .. } if new_slot => installed_refs(ctx.root, &key.0, &key.1),
        PretendOutcome::Reinstall { version, .. } => installed_refs(ctx.root, &key.0, &key.1)
            .into_iter()
            .filter(|r| {
                &r.version == version
                    && (r.slot != slot || r.sub_slot != sub_slot || r.repo != repo_name)
            })
            .collect(),
        _ => Vec::new(),
    };
    oldbest.sort_by(|a, b| vercmp_ordering(&a.version, &b.version));
    state.entries.push(GraphEntry {
        discovery: 0,
        category: key.0.clone(),
        package: key.1.clone(),
        outcome,
        blockers: Vec::new(),
        slot: Some(slot.to_string()),
        sub_slot: Some(sub_slot.to_string()),
        repo_name: Some(repo_name.to_string()),
        oldbest,
        use_flags_display: Vec::new(),
        use_expand_display: Vec::new(),
        use_expand_display_p: Vec::new(),
        keyword_mask,
        new_slot,
        interactive,
        // Real `output.py:633`'s `not pkg.built and … "fetch" in
        // pkg.restrict`. Filled in below, once `metadata`/`use_flags`
        // (for `SRC_URI` flattening) are read -- a binary candidate
        // (`pkg.built`) is never fetch-restricted, so it stays
        // `false` here regardless.
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: Vec::new(),
        required_by: Vec::new(),
        source: candidate_source,
        provenance,
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        // Track X Slice B (#242): the node carries its resolving
        // root -- real `Package.root_config` / `pkg.root`.
        targets_running_root: item_targets_running_root,
        remote_binary: candidate_remote,
        build_id: candidate_build_id.clone(),
        deps: Vec::new(),
    });
    PushGraphEntryOut {
        entry_idx,
        candidate_source,
        candidate_remote,
        candidate_build_id,
        already_installed_outcome,
        candidate_str,
    }
}
