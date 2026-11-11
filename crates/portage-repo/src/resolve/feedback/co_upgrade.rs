//! Co-upgrade: an installed holder of a pin that an upgrade breaks may follow
//! the upgrade instead of withholding it (backlog #698).
//!
//! The decision uses repo facts only (no solver state), so every solver can
//! share it. Charter property 1 (every kept package has its dependencies)
//! holds either way; withholding is only needed when a holder cannot follow.

use crate::resolve::feedback::*;
use std::collections::HashSet;

/// Arguments of [`holder_has_route`].
pub(crate) struct HolderRouteArgs<'a> {
    pub(crate) gate: CategoryGate<'a>,
    pub(crate) root: &'a Path,
    pub(crate) holder: &'a (String, String, String),
    pub(crate) provider: &'a (String, String),
    pub(crate) new_provider: &'a [String],
    pub(crate) with_bdeps: bool,
}

/// The version the installed holder `(category, package, version)` should move to so it can follow an
/// upgrade of `provider` to the versions rendered in `new_provider`
/// (`cat/pkg-ver:slot/sub::repo`): some visible candidate of the holder, in
/// its installed slot and not older than it, whose `*DEPEND` names `provider`
/// only in ways the new versions satisfy. A rebuild of the same version
/// counts (its tree dependencies, not the vdb ones, are what binds).
///
/// A versioned atom on `provider` is a menu of acceptable versions (the
/// `virtual/perl-*` shape): one of them must match. Unversioned atoms
/// (`cat/pkg:=`, `cat/pkg:0`) must all match.
pub(crate) fn holder_has_route(args: HolderRouteArgs<'_>) -> Option<String> {
    let HolderRouteArgs {
        gate,
        root,
        holder,
        provider,
        new_provider,
        with_bdeps,
    } = args;
    let (cat, pkg, ver) = holder;
    let installed_slot = crate::reinstall::read_vdb_slot(root, cat, pkg, ver).0;
    let use_flags: HashSet<String> = read_vdb_flag_set(root, cat, pkg, ver, "USE");
    let candidates = gate.repo_candidates(cat, pkg).ok()?;
    // Newest acceptable version first: the system stays as up to date as it can.
    let mut ordered: Vec<&Candidate> = candidates.iter().collect();
    ordered.sort_by(|a, b| {
        vercmp(&b.version, &a.version).map_or(std::cmp::Ordering::Equal, |o| o.cmp(&0))
    });
    ordered
        .into_iter()
        .find(|c| {
            if (!installed_slot.is_empty() && installed_slot != c.slot)
                || vercmp(&c.version, ver).is_none_or(|o| o < 0)
            {
                return false;
            }
            let Ok(meta) =
                repo_aux_metadata(&c.repo_location, cat, &format!("{pkg}-{}", c.version))
            else {
                return false;
            };
            let keys: &[&str] = if with_bdeps {
                &["RDEPEND", "DEPEND", "PDEPEND", "BDEPEND", "IDEPEND"]
            } else {
                &["RDEPEND", "PDEPEND", "IDEPEND"]
            };
            let texts: Vec<&str> = keys
                .iter()
                .filter_map(|k| meta.get(*k).map(String::as_str))
                .collect();
            let atoms = provider_atoms(&texts, &use_flags, provider);
            new_provider
                .iter()
                .all(|new| provider_atoms_accept(&atoms, new))
        })
        .map(|c| c.version.clone())
}

/// The request that lets every installed holder of a withheld provider follow
/// its upgrade, when each of them has a route: the walk is run again with the
/// holders as extra roots (`=cat/pkg-ver` for those that must move to a newer
/// version, a forced reinstall for those a rebuild mends). Providers with a
/// holder that cannot follow (or whose pin is a USE dependency) stay withheld.
/// `None` when nothing is withheld or nothing can follow.
pub(crate) fn co_upgrade_request(
    ctx: &ResolveCtx<'_>,
    req: &ResolveRequest,
    first: &GraphResult,
) -> Option<ResolveRequest> {
    // Without a world (no complete-graph seeds) nobody is protected and nobody
    // follows, as in the bridge.
    // `--backtrack=0` asks for no second walk.
    if ctx.backtrack_max == 0 {
        return None;
    }
    if ctx.config.complete_seed_atoms.is_empty() && ctx.config.blocker_retry_seed_atoms.is_empty() {
        return None;
    }
    let mut providers: Vec<(String, String)> = first
        .skipped_updates
        .iter()
        .map(|row| (row.category.clone(), row.package.clone()))
        .collect();
    providers.sort();
    providers.dedup();
    let installed = all_installed_packages(ctx.root);
    let mut extra = req.clone();
    let mut changed = false;
    for cp in providers {
        let rows: Vec<&SkippedUpdate> = first
            .skipped_updates
            .iter()
            .filter(|r| (&r.category, &r.package) == (&cp.0, &cp.1))
            .collect();
        let Some(new) = rows.first().map(|r| {
            format!(
                // match-str-ok: skipped-update row (separate slot/sub-slot fields), not a Candidate
                "{}/{}-{}:{}/{}",
                r.category, r.package, r.skipped_version, r.slot, r.skipped_sub_slot
            )
        }) else {
            continue;
        };
        let mut holders: Vec<(String, String, String)> = Vec::new();
        let mut usable = true;
        for row in &rows {
            let head = row
                .consumer_cpv
                .split_once(':')
                .map_or(row.consumer_cpv.as_str(), |(h, _)| h);
            // An installed row may carry a trailing build id (`-<n>`) after its
            // version, so match by prefix against the vdb rather than parsing.
            let found = installed.iter().find(|i| {
                let cpv = i.cpv();
                head == cpv
                    || head
                        .strip_prefix(cpv.as_str())
                        .and_then(|rest| rest.strip_prefix('-'))
                        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            });
            let found = found.map(|i| (i.category.clone(), i.package.clone(), i.version.clone()));
            match found {
                Some(holder) if row.consumer_installed && !row.atom.contains('[') => {
                    holders.push(holder);
                }
                _ => usable = false,
            }
        }
        holders.sort();
        holders.dedup();
        if !usable || holders.is_empty() {
            continue;
        }
        let routes: Vec<Option<String>> = holders
            .iter()
            .map(|holder| {
                holder_has_route(HolderRouteArgs {
                    gate: ctx.gate(),
                    root: ctx.root,
                    holder,
                    provider: &cp,
                    new_provider: std::slice::from_ref(&new),
                    with_bdeps: ctx.with_bdeps,
                })
            })
            .collect();
        if routes.iter().any(Option::is_none) {
            continue;
        }
        for ((cat, pkg, ver), route) in holders.iter().zip(routes) {
            let target = route.expect("every holder has a route");
            if target == *ver {
                extra.atoms.push(format!("{cat}/{pkg}"));
                extra.reinstall_atoms.push(format!("{cat}/{pkg}"));
            } else {
                extra.atoms.push(format!("={cat}/{pkg}-{target}"));
            }
            changed = true;
        }
    }
    changed.then_some(extra)
}

/// Every non-blocker atom the dependency strings `texts` (flattened against
/// `use_flags`) name on `provider`.
pub(crate) fn provider_atoms(
    texts: &[&str],
    use_flags: &HashSet<String>,
    provider: &(String, String),
) -> Vec<String> {
    let mut out = Vec::new();
    for text in texts {
        let Some(atoms) = flat_dep_atoms(text, use_flags) else {
            continue;
        };
        for atom_str in atoms {
            let Some(atom) = parse_dep_atom(&atom_str, "co_upgrade") else {
                continue;
            };
            if atom.blocker == portage_dep::Blocker::None
                && atom.category == provider.0
                && atom.package == provider.1
            {
                out.push(atom_str);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Whether `atoms` (as returned by [`provider_atoms`]) accept the provider
/// rendered `new` (`cat/pkg-ver:slot/sub::repo`). Versioned atoms are a menu
/// (one must match); unversioned ones (`:=`, `:0`) must all match.
pub(crate) fn provider_atoms_accept(atoms: &[String], new: &str) -> bool {
    let accepts =
        |atom: &&String| portage_dep::match_from_list(atom, &[new]).is_some_and(|m| !m.is_empty());
    let (versioned, plain): (Vec<&String>, Vec<&String>) = atoms
        .iter()
        .partition(|a| parse_dep_atom(a, "co_upgrade").is_some_and(|p| p.version.is_some()));
    plain.iter().all(accepts) && (versioned.is_empty() || versioned.iter().any(accepts))
}
