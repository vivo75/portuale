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
    // IUSE's own "+flag"/"-flag" default markers only matter for
    // resolving a flag's default when nothing else decides it --
    // already handled upstream, wherever `use_flags` itself came
    // from -- so display only needs the bare flag name, paired with
    // whatever `use_flags` (the real resolved set) says. Computed
    // (and shown by `--pretend -v`) regardless of `nodeps` below --
    // real portage's own USE display is about the package's own
    // metadata, unrelated to whether its dependencies get walked.
    // REQUIRED_USE (PMS 7.3.4/8.2): checked once, here, right after a
    // candidate is newly resolved -- real depgraph.py's own "NOTE:
    // REQUIRED_USE checks are delayed until after package selection"
    // (it's a genuine *post*-selection check, no part of matching/
    // visibility at all, unlike package.use/package.mask). A
    // violation eventually fails the whole run regardless of whether
    // this candidate was reached as a top-level atom or a dependency
    // deep in the graph -- but NOT immediately: real depgraph.py's
    // own `_add_pkg` (~line 3600) sets
    // `_dynamic_config._required_use_unsatisfied = True` and returns
    // 0 on a violation, which does NOT stop the rest of the graph
    // walk (unlike a top-level atom's own `NoVisibleCandidate`,
    // which genuinely does abort immediately). Every violation
    // anywhere in the walk is collected into
    // `required_use_violations` and the BFS keeps going -- see that
    // variable's own doc comment, near the top of this function, for
    // the full grounding and where the collected violations actually
    // get turned into this call's own `Err`. An genuinely *invalid*
    // REQUIRED_USE (the `Err(e)` branch below, e.g. referencing a
    // flag that isn't even valid IUSE) is different: real
    // `check_required_use` itself raises for that case, outside the
    // explicit `if not required_use_is_sat:` branch that the delayed
    // collection above lives in -- so portuale keeps that one
    // immediately fatal, same as before.
    // Real's own gate is `not pkg.built` (`depgraph.py:3629-3632`):
    // an installed (`built`) package's REQUIRED_USE was already
    // satisfied at build time and is never re-validated against a
    // later profile. #74 S1: once the `||` selection legitimately
    // picks an installed instance whose current-profile USE no longer
    // enables a REQUIRED_USE flag (`dev-lang/rust-1.94.1`'s
    // `llvm_slot_21`, built with it, `LLVM_SLOT=22` now), the
    // re-validation turned the whole run into a bogus
    // "ebuild selected ... has unmet requirements" failure.
    if !already_installed_outcome
        && let Some(required_use) = metadata.get("REQUIRED_USE")
        && !required_use.trim().is_empty()
    {
        // Real `check_required_use` validates a referenced flag
        // against `pkg.iuse.is_valid_flag`, not a package's own
        // literal IUSE alone -- real config.py's own
        // `_get_implicit_iuse()` folds `PORTAGE_ARCHLIST`
        // (profiles/arch.list), `use.mask ∪ use.force`, and
        // literal `build`/`bootstrap` into every package's
        // effective IUSE regardless of what that package's own
        // IUSE declares. Without this, a REQUIRED_USE referencing
        // an implicit flag never mentioned in a package's own
        // IUSE (e.g. real media-libs/mesa's own REQUIRED_USE
        // referencing "x86", a valid arch.list entry that isn't
        // the profile's own active arch) spuriously fails with
        // "USE flag ... is not in IUSE" -- confirmed live against
        // the real, installed system. See `portage_profile::
        // Config::archlist`'s own doc comment for the full
        // grounding and the deliberate USE_EXPAND_HIDDEN
        // (elibc_*/kernel_*/userland_*) simplification.
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
                // Real `_show_unsatisfied_dep`'s REQUIRED_USE block:
                // the failing atom as `xinfo` (real's own
                // `atom.unevaluated_atom`), the candidate + its
                // `pkg_use_display`, the minimal unsatisfied
                // sub-expression (real `check_required_use(...).
                // tounicode()`), the complete expression when it
                // differs, then the `(dependency required by …)`
                // chain.
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
                    .push(render_required_use_block(
                        unevaluated_atom.as_deref().unwrap_or(current_atom),
                        &cpv_repo,
                        &use_display,
                        &reduced,
                        &full,
                        &dep_chain,
                    ));
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
        // Dedup a flag `IUSE` names twice (eclass + ebuild, e.g.
        // gnome-extra/gnome-color-manager's `IUSE="test test"`) --
        // real renders `sorted(pkg.iuse.all)`, a `frozenset`.
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
        // Real `_create_use_string`'s `any_iuse.sort(key=_alnum_sort_key)`.
        display.sort_by_key(|p| alnum_sort_key(&p.0));
        // Real `_display_use`'s `previous_pkg`: the installed
        // version's own recorded USE/IUSE, for the `*`/`%` diff
        // markers -- only for an entry that actually replaces an
        // installed one (`Upgrade`/`Downgrade` from `from`,
        // `Reinstall` at the same version). `New`/`AlreadyInstalled`/
        // `NoVisibleCandidate` have no installed side to diff.
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
        // Real `_display_use`'s `self.forced_flags` (`pkg.use.force |
        // pkg.use.mask`): the flags `-pv` wraps in `( … )`.
        let forced = forced_or_masked_flags(iuse, keywords, candidate_str, &key.0, &key.1, config);
        // Real `reinst_flags_map`: the `_reinstall_for_flags` trigger
        // set (`Reinstall::changed_flags`), force-shown in the USE
        // line even at plain `-p`. Only a USE-triggered `Reinstall`
        // carries one; `Upgrade`/`Downgrade`/`New` don't (their
        // still-present changed flags already render via the
        // `*`/`%` markers).
        let reinst_flags: HashSet<String> = match &state.entries[entry_idx].outcome {
            PretendOutcome::Reinstall { changed_flags, .. } => {
                changed_flags.iter().cloned().collect()
            }
            _ => HashSet::new(),
        };
        // Two renderings of the same flag set: `use_expand_display`
        // is the `-pv` (`all_flags = verbosity == 3`) form -- every
        // flag shown -- and `use_expand_display_p` is the plain `-p`
        // (`all_flags` off) form -- only the *changed* flags for a
        // Reinstall/Upgrade/Downgrade (plus its `reinst_flags`), the
        // full list for a `New` (`is_new` renders everything
        // regardless). `pretend.rs`'s `use_suffix` picks by
        // verbosity. Both are cheap; for a `New` (no `installed`
        // diff) they're identical.
        state.entries[entry_idx].use_expand_display = build_use_expand_display(
            &display,
            config,
            installed.as_ref(),
            &forced,
            true,
            &reinst_flags,
        );
        state.entries[entry_idx].use_expand_display_p = build_use_expand_display(
            &display,
            config,
            installed.as_ref(),
            &forced,
            false,
            &reinst_flags,
        );
        state.entries[entry_idx].use_flags_display = display;
    }
    Ok(ControlFlow::Continue(()))
}
