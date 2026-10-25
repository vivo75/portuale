//! The candidates for the resolved version: repo ebuilds, PKGDIR and binhost binaries.

use crate::resolve::pass::*;
use portage_dep::Atom;
use portage_profile::Config;

/// The values [`candidate_pool`] reads from its caller.
pub(crate) struct CandidatePoolArgs<'a> {
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) key: &'a (String, String),
    pub(crate) config: &'a Config,
    pub(crate) current_atom: &'a str,
    pub(crate) atom: &'a Atom,
    pub(crate) bp: &'a BacktrackParams,
}

/// The candidates for the resolved version: repo ebuilds, PKGDIR and binhost binaries.
pub(crate) fn candidate_pool(args: CandidatePoolArgs<'_>) -> ControlFlow<(), Vec<Candidate>> {
    let CandidatePoolArgs {
        ctx,
        key,
        config,
        current_atom,
        atom,
        bp,
    } = args;
    // Re-derive which source the resolved version lives in; must agree
    // with what `resolve_pretend` chose.
    // Portage: depgraph.py, _wrapped_select_pkg_highest_available_imp (ebuild last resort)
    let mut repo_candidates = if ctx.usepkgonly || crate::globals::ebuild_db_dropped() {
        Vec::new()
    } else {
        let Ok(c) = ctx.repo_candidates(&key.0, &key.1) else {
            return ControlFlow::Break(());
        };
        c.to_vec()
    };
    if ctx.usepkg || ctx.usepkgonly {
        let mut binary_candidates = list_binary_candidates(&ctx.local_binpkg, &key.0, &key.1);
        if ctx.getbinpkg {
            binary_candidates.extend(list_remote_binary_candidates(
                &config.binrepos,
                ctx.root,
                &ctx.local_binpkg,
                &key.0,
                &key.1,
            ));
        }
        // A binary that was never eligible at selection time can't be
        // re-derived as the source here either.
        if ctx.binpkg_respect_use
            || ctx.newuse
            || ctx.changed_use
            || !ctx.reinstall_atoms.is_empty()
        {
            let ebuild_at_version = |version: &str| {
                repo_candidates
                    .iter()
                    .find(|e| e.source == CandidateSource::Ebuild && e.version == version)
            };
            binary_candidates.retain(|c| {
                binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
                    candidate: c,
                    ebuild_at_version: ebuild_at_version(&c.version),
                    category: &key.0,
                    package: &key.1,
                    config,
                    newuse: ctx.newuse,
                    changed_use: ctx.changed_use,
                    respect_use: ctx.binpkg_respect_use,
                })
            });
        }
        // Mirror selection's `--binpkg-respect-user-patches` retain: a
        // re-derived binary whose digest disagrees with the local patch
        // set was never eligible either. Portage: depgraph.py
        if ctx.binpkg_respect_user_patches {
            binary_candidates.retain(|c| binary_user_patches_ok(c, &key.0, &key.1, config));
        }
        if binpkg_changed_deps_active(ctx.usepkgonly) {
            binary_candidates.retain(|c| {
                !binary_deps_changed(BinaryDepsChangedArgs {
                    root: ctx.root,
                    candidate: c,
                    category: &key.0,
                    package: &key.1,
                    with_bdeps: ctx.with_bdeps,
                    gate: ctx.gate(),
                })
            });
        }
        // A re-derived binary needs a visible ebuild at its own exact
        // version when any visible ebuild satisfies the atom.
        // Portage: _equiv_ebuild_visible
        let uev = use_ebuild_visibility();
        if (!ctx.usepkgonly || uev) && !binary_candidates.is_empty() {
            let some_ebuild_matches = repo_candidates.iter().any(|c| {
                c.source == CandidateSource::Ebuild && is_visible(c, &key.0, &key.1, config) && {
                    let s = c.match_str(&key.0, &key.1);
                    portage_dep::match_from_list(current_atom, &[s.as_str()])
                        .is_some_and(|r| !r.is_empty())
                }
            });
            if some_ebuild_matches || uev {
                binary_candidates.retain(|c| {
                    (!uev && useoldpkg_atom_matches(&key.0, &key.1, &c.version))
                        || repo_candidates.iter().any(|e| {
                            e.source == CandidateSource::Ebuild
                                && e.version == c.version
                                && is_visible(e, &key.0, &key.1, config)
                        })
                });
            }
        }
        let binary_candidates = dedup_binary_instances(binary_candidates, atom, config);
        // Mirror `resolve_pretend`'s `slot_operator_mask_built` pool filter:
        // a masked binary must not be re-derived here either. Plain
        // (`!=cpv`) negatives read as `None` and are ignored.
        let masked_here = bp.masked_negatives_for(key);
        let binary_candidates: Vec<Candidate> = binary_candidates
            .into_iter()
            .filter(|c| {
                !masked_here.iter().any(|neg| {
                    slot_operator_binary_masked_version(neg, &key.0, &key.1)
                        .is_some_and(|ver| ver == c.version)
                })
            })
            .collect();
        repo_candidates.extend(filter_usepkg_exclude_include(
            binary_candidates,
            &key.0,
            &key.1,
            ctx.usepkg_exclude,
            ctx.usepkg_include,
        ));
    }
    ControlFlow::Continue(repo_candidates)
}
