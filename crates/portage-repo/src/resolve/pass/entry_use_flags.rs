//! The entry's USE flags: baked for a binary, else computed, plus any autounmask-use flip.

use crate::resolve::pass::*;
use portage_dep::Atom;
use portage_profile::Config;
use std::sync::Arc;

/// The values [`entry_use_flags`] reads from its caller.
pub(crate) struct EntryUseFlagsArgs<'a> {
    pub(crate) candidate_source: CandidateSource,
    pub(crate) resolved: &'a Candidate,
    pub(crate) config: &'a Config,
    pub(crate) metadata: &'a Arc<HashMap<String, String>>,
    pub(crate) keywords: &'a [String],
    pub(crate) candidate_str: &'a str,
    pub(crate) key: &'a (String, String),
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) atom: &'a Atom,
    pub(crate) state: &'a mut PassState,
    pub(crate) repo_candidates: &'a [Candidate],
    pub(crate) owner: &'a Option<(String, String)>,
}

/// The entry's USE flags: baked for a binary, else computed, plus any autounmask-use flip.
pub(crate) fn entry_use_flags(args: EntryUseFlagsArgs<'_>) -> HashSet<String> {
    let EntryUseFlagsArgs {
        candidate_source,
        resolved,
        config,
        metadata,
        keywords,
        candidate_str,
        key,
        bp,
        atom,
        state,
        repo_candidates,
        owner,
    } = args;
    // A built (binary) candidate carries the USE it was built with; never
    // recompute from the profile.
    // Portage: _pkg_use_enabled (returns `pkg.use.enabled` for `pkg.built`)
    let mut use_flags = if candidate_source == CandidateSource::Binary {
        resolved.binary_use.clone().unwrap_or_default()
    } else {
        // Owned: adjusted in place below -- one clone, as before #103.
        effective_use_flags(EffectiveUseFlagsArgs {
            config,
            iuse: metadata.get("IUSE").map(String::as_str).unwrap_or_default(),
            keywords,
            candidate_str,
            category: &key.0,
            package: &key.1,
        })
        .as_ref()
        .clone()
    };

    // Apply the `package.use` flip that kept this candidate, once, at the
    // graph layer, so the USE line, REQUIRED_USE and the dep walk all see it.
    // Portage: _pkg_use_enabled, _display_autounmask
    if bp.autounmask_suggest_use
        && let Some(use_deps) = atom.use_deps.as_deref().filter(|d| !d.is_empty())
    {
        let iuse_declared: HashSet<String> = metadata
            .get("IUSE")
            .map(String::as_str)
            .unwrap_or_default()
            .split_whitespace()
            .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
            .collect();
        let iuse_set = valid_iuse(&iuse_declared, config);
        if !portage_dep::use_deps_satisfied(use_deps, &iuse_set, &use_flags)
            && let Some(flip) = suggested_use_flip(resolved, &key.0, &key.1, use_deps, config)
        {
            for (flag, enabled) in &flip {
                if *enabled {
                    use_flags.insert(flag.clone());
                } else {
                    use_flags.remove(flag);
                }
            }
            let token = flip
                .iter()
                .map(|(f, e)| if *e { f.clone() } else { format!("-{f}") })
                .collect::<Vec<_>>()
                .join(" ");
            state.autounmask_use_changes.push(AutounmaskChange {
                atom: autounmask_use_atom_form(resolved, repo_candidates, &key.0, &key.1, config),
                token,
                dep_chain: Vec::new(),
                // This dep's owner forced the flip.
                trigger: owner.clone(),
            });
        }
    }
    use_flags
}
