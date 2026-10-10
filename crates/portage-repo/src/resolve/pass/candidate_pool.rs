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
    // The resolved version may have come from any of `repos`, or
    // from PKGDIR (`--usepkg`/`--usepkgonly`) or a remote binhost
    // (`--getbinpkg`), so re-derive which one it actually lives in
    // -- reusing `list_candidates`/`list_binary_candidates` rather
    // than threading a repo location back out of `PretendOutcome`,
    // since more than one source could carry the identical
    // version. This step MUST agree with what `resolve_pretend`
    // itself chose: real `_wrapped_select_pkg_highest_available_
    // imp` returns `matched_packages[-1]` with `ebuild` built
    // first in `dbs` order -- "ebuild type is the last resort"
    // (`depgraph.py:8492`) -- so at a version tie a
    // `--usepkg`/`--getbinpkg` binary that passed
    // `binpkg-respect-use` beats the ebuild. Mirrors
    // `resolve_pretend`'s own pool construction (`--usepkgonly`
    // excludes ebuilds entirely).
    let mut repo_candidates = if ctx.usepkgonly {
        Vec::new()
    } else {
        let Ok(c) = list_candidates(&ctx.repos, &key.0, &key.1) else {
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
        // Same `--binpkg-respect-use` / `--binpkg-changed-deps`
        // gates `resolve_pretend` applied when it chose this
        // version -- a binary that was never eligible then can't
        // be re-derived as the source here either.
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
        if binpkg_changed_deps_active(ctx.usepkgonly) {
            binary_candidates.retain(|c| {
                !binary_deps_changed(BinaryDepsChangedArgs {
                    root: ctx.root,
                    repos: &ctx.repos,
                    candidate: c,
                    category: &key.0,
                    package: &key.1,
                    with_bdeps: ctx.with_bdeps,
                })
            });
        }
        // `_equiv_ebuild_visible` (see `resolve_pretend`): a
        // binary re-derived here needs a visible ebuild at its
        // own exact version, when any visible ebuild satisfies
        // the atom. `--use-ebuild-visibility` enforces it even
        // under `--usepkgonly` / a `--useoldpkg-atoms` match.
        let uev = use_ebuild_visibility();
        if (!ctx.usepkgonly || uev) && !binary_candidates.is_empty() {
            let some_ebuild_matches = repo_candidates.iter().any(|c| {
                c.source == CandidateSource::Ebuild && is_visible(c, &key.0, &key.1, config) && {
                    let s = format!(
                        "{}/{}-{}:{}/{}::{}",
                        key.0, key.1, c.version, c.slot, c.sub_slot, c.repo_name
                    );
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
        // #212 (v2 `#24c`): mirror of `resolve_pretend`'s own
        // `slot_operator_mask_built` pool filter -- a masked binary
        // must not be re-derived as the entry source here either
        // (this step MUST agree with what `resolve_pretend`
        // chose). Plain (`!=cpv`) negatives read as `None` and are
        // ignored; only the `[binary]`-marked form filters.
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
