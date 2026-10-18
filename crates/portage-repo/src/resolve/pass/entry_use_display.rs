//! Record the entry's USE display: changed flags, IUSE and USE_EXPAND groups.

use crate::resolve::pass::*;
use portage_profile::Config;
use std::sync::Arc;

/// The values [`record_entry_use_display`] reads from its caller.
pub(crate) struct RecordEntryUseDisplayArgs<'a> {
    pub(crate) already_installed_outcome: bool,
    pub(crate) metadata: &'a Arc<HashMap<String, String>>,
    pub(crate) config: &'a Config,
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) key: &'a (String, String),
    pub(crate) version: &'a str,
    pub(crate) resolved: &'a Candidate,
    pub(crate) owner: &'a Option<(String, String)>,
    pub(crate) state: &'a mut PassState,
    pub(crate) unevaluated_atom: &'a Option<String>,
    pub(crate) current_atom: &'a str,
    pub(crate) entry_idx: usize,
    pub(crate) keywords: &'a [String],
    pub(crate) candidate_str: &'a str,
}

/// Record the entry's USE display: changed flags, IUSE and USE_EXPAND groups.
pub(crate) fn record_entry_use_display(
    args: RecordEntryUseDisplayArgs<'_>,
) -> Result<ControlFlow<()>, Error> {
    let RecordEntryUseDisplayArgs {
        already_installed_outcome,
        metadata,
        config,
        use_flags,
        ctx,
        key,
        version,
        resolved,
        owner,
        state,
        unevaluated_atom,
        current_atom,
        entry_idx,
        keywords,
        candidate_str,
    } = args;
    // Display needs only the bare flag name paired with the resolved set;
    // the default markers were already consumed where `use_flags` came from.
    // REQUIRED_USE (PMS 7.3.4/8.2) is a post-selection check, never part of
    // matching/visibility. Violations collect and the walk keeps going; only
    // an invalid expression fails immediately.
    // Portage: depgraph.py, _add_pkg; check_required_use raises on invalid input
    if !already_installed_outcome
        && let Some(required_use) = metadata.get("REQUIRED_USE")
        && !required_use.trim().is_empty()
    {
        // Validate against the implicit IUSE too (arch.list, use.mask/force,
        // `build`/`bootstrap`), not just literal IUSE, or flags valid by
        // config spuriously fail. Verified live against the installed system.
        // Portage: config.py, _get_implicit_iuse
        let iuse_set = implicit_iuse_set(
            metadata.get("IUSE").map(String::as_str).unwrap_or_default(),
            config,
        );
        match portage_required_use::check_required_use(required_use, use_flags, &iuse_set) {
            Ok(true) => {}
            Ok(false) => {
                let normalized = required_use
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                // The failing atom, the candidate and its USE display, the minimal
                // unsatisfied sub-expression plus the full one, then the dep chain.
                // Portage: _show_unsatisfied_dep (REQUIRED_USE block)
                let reduced = match portage_required_use::unsatisfied_reduced(
                    required_use,
                    use_flags,
                    &iuse_set,
                ) {
                    Ok(Some(r)) => portage_required_use::human_readable(&r),
                    _ => portage_required_use::human_readable(&normalized),
                };
                let full = portage_required_use::human_readable(&normalized);
                let use_display = pkg_use_display_for(&ctx.repos, config, &key.0, &key.1, version);
                let cpv_repo = match &resolved.repo_name {
                    repo if !repo.is_empty() => {
                        format!("{}/{}-{version}::{repo}", key.0, key.1)
                    }
                    _ => format!("{}/{}-{version}", key.0, key.1),
                };
                let dep_chain = required_use_dep_chain(owner, &ctx.top_level, &state.entries);
                state
                    .required_use_violations
                    .push(render_required_use_block(RenderRequiredUseBlockArgs {
                        xinfo: unevaluated_atom.as_deref().unwrap_or(current_atom),
                        cpv_repo: &cpv_repo,
                        use_display: &use_display,
                        reduced_human: &reduced,
                        full_human: &full,
                        dep_chain: &dep_chain,
                    }));
                return Ok(ControlFlow::Break(()));
            }
            Err(e) => {
                return Err(Error::InvalidRequiredUse {
                    package: format!("{}/{}-{version}", key.0, key.1),
                    source: e,
                });
            }
        }
    }

    if let Some(iuse) = metadata.get("IUSE") {
        // Dedup double-named flags (eclass + ebuild); Portage renders a sorted set.
        let mut iuse_seen: HashSet<String> = HashSet::new();
        let mut display: Vec<(String, bool)> = iuse
            .split_whitespace()
            .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
            .filter(|flag| iuse_seen.insert(flag.clone()))
            .map(|flag| {
                let enabled = use_flags.contains(&flag);
                (flag, enabled)
            })
            .collect();
        // Alnum sort, like Portage's `_create_use_string`.
        display.sort_by_key(|p| alnum_sort_key(&p.0));
        // The installed version's recorded USE/IUSE, for the `*`/`%` diff
        // markers. Only replacers (Upgrade/Downgrade/Reinstall) have one.
        // Portage: _display_use (previous_pkg)
        let installed_version = match &state.entries[entry_idx].outcome {
            PretendOutcome::Upgrade { from, .. } | PretendOutcome::Downgrade { from, .. } => {
                Some(from.clone())
            }
            PretendOutcome::Reinstall { version, .. } => Some(version.to_string()),
            _ => None,
        };
        let installed = installed_version.map(|v| {
            let old_iuse = read_vdb_flag_set(ctx.root, &key.0, &key.1, &v, "IUSE");
            let mut old_use = read_vdb_flag_set(ctx.root, &key.0, &key.1, &v, "USE");
            old_use.retain(|f| old_iuse.contains(f));
            InstalledUseState { old_use, old_iuse }
        });
        // The `pkg.use.force | pkg.use.mask` flags, wrapped in `( … )` by `-pv`.
        // Portage: _display_use (forced_flags)
        let forced = forced_or_masked_flags(ForcedOrMaskedFlagsArgs {
            iuse,
            keywords,
            candidate_str,
            build_id: resolved.build_id.as_deref(),
            category: &key.0,
            package: &key.1,
            config,
        });
        // The `_reinstall_for_flags` trigger set, force-shown even at plain
        // `-p`. Only a USE-triggered `Reinstall` carries one.
        // Portage: _display_use (reinst_flags_map)
        let reinst_flags: HashSet<String> = match &state.entries[entry_idx].outcome {
            PretendOutcome::Reinstall { changed_flags, .. } => {
                changed_flags.iter().cloned().collect()
            }
            _ => HashSet::new(),
        };
        // Two renderings of the same flag set: `-pv` shows every flag,
        // plain `-p` only the changed ones (plus `reinst_flags`); `New`
        // renders everything regardless. `pretend.rs` picks by verbosity.
        state.entries[entry_idx].use_expand_display =
            build_use_expand_display(BuildUseExpandDisplayArgs {
                use_flags_display: &display,
                config,
                installed: installed.as_ref(),
                forced: &forced,
                all_flags: true,
                reinst_flags: &reinst_flags,
            });
        state.entries[entry_idx].use_expand_display_p =
            build_use_expand_display(BuildUseExpandDisplayArgs {
                use_flags_display: &display,
                config,
                installed: installed.as_ref(),
                forced: &forced,
                all_flags: false,
                reinst_flags: &reinst_flags,
            });
        state.entries[entry_idx].use_flags_display = display;
    }
    Ok(ControlFlow::Continue(()))
}
