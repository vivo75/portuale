//! Co-upgrade: an installed holder of a pin that an upgrade breaks may follow
//! the upgrade instead of withholding it.
//!
//! The decision uses repo facts only (no solver state), so every solver can
//! share it. Charter property 1 (every kept package has its dependencies)
//! holds either way; withholding is only needed when a holder cannot follow.

use crate::resolve::cycle_report::DepTree;
use crate::resolve::feedback::*;
use std::collections::HashSet;

/// Arguments of [`holder_has_route`].
pub(crate) struct HolderRouteArgs<'a> {
    pub(crate) gate: CategoryGate<'a>,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) root: &'a Path,
    pub(crate) holder: &'a (String, String, String),
    pub(crate) provider: &'a (String, String),
    pub(crate) new_provider: &'a [String],
    pub(crate) with_bdeps: bool,
}

/// A holder that follows an upgrade: `(holder, provider)` as `(category, package)`.
pub(crate) type Followers = ((String, String), (String, String));

/// The version the installed holder `(category, package, version)` should move to so it can follow an
/// upgrade of `provider` to the versions rendered in `new_provider`
/// (`cat/pkg-ver:slot/sub::repo`): some visible candidate of the holder (a masked
/// version is not a route the user can install), in its installed slot and not older than it, whose `*DEPEND` names `provider`
/// only in ways the new versions satisfy. A rebuild of the same version
/// counts (its tree dependencies, not the vdb ones, are what binds).
///
/// A versioned atom on `provider` is a menu of acceptable versions (the
/// `virtual/perl-*` shape): one of them must match. Unversioned atoms
/// (`cat/pkg:=`, `cat/pkg:0`) must all match.
pub(crate) fn holder_has_route(args: HolderRouteArgs<'_>) -> Option<String> {
    let HolderRouteArgs {
        gate,
        config,
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
                || !crate::visibility::is_visible(c, cat, pkg, config)
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
            let atoms = tree_route_atoms(&texts, &use_flags, provider);
            new_provider
                .iter()
                .all(|new| provider_atoms_accept(&atoms, new))
        })
        .map(|c| c.version.clone())
}

/// Arguments of [`holder_routes`].
pub(crate) struct WithheldProvider<'a> {
    pub(crate) gate: CategoryGate<'a>,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) root: &'a Path,
    pub(crate) with_bdeps: bool,
    pub(crate) provider: &'a (String, String),
    /// The version the upgrade moves to, `cat/pkg-ver:slot/sub`.
    pub(crate) new: &'a str,
    pub(crate) slot: &'a str,
    pub(crate) installed_version: &'a str,
    pub(crate) skipped_version: &'a str,
    /// The installed holders, each with the recorded atom that pins the old version.
    pub(crate) holders: &'a [((String, String, String), String)],
}

/// The route of each holder of one withheld provider, and the note that names
/// the holders without one (`None` when all can follow). Shared by every solver.
pub(crate) fn holder_routes(
    args: WithheldProvider<'_>,
) -> (Vec<Option<String>>, Option<HeldBackNoRoute>) {
    let routes: Vec<Option<String>> = args
        .holders
        .iter()
        .map(|(holder, _)| {
            holder_has_route(HolderRouteArgs {
                gate: args.gate,
                config: args.config,
                root: args.root,
                holder,
                provider: args.provider,
                new_provider: &[args.new.to_string()],
                with_bdeps: args.with_bdeps,
            })
        })
        .collect();
    let mut stuck: Vec<(String, String, String)> = args
        .holders
        .iter()
        .zip(&routes)
        .filter(|(_, route)| route.is_none())
        .map(|(((c, p, v), atom), _)| (format!("{c}/{p}"), v.clone(), atom.clone()))
        .collect();
    stuck.sort();
    let note = (!stuck.is_empty()).then(|| HeldBackNoRoute {
        category: args.provider.0.clone(),
        package: args.provider.1.clone(),
        slot: args.slot.to_string(),
        installed_version: args.installed_version.to_string(),
        skipped_version: args.skipped_version.to_string(),
        holders: stuck,
    });
    (routes, note)
}

/// Whether the request wants `provider` updated: it names the package itself
/// (a command-line atom or a set member), or it is an `--update` that walks
/// dependencies (`--deep`). Otherwise a withheld update stays withheld, the
/// smallest plan. Portage: depgraph.py, `_want_update_pkg`
/// (`arg_atoms or update`, within `--deep`).
pub(crate) fn request_updates(
    atoms: &[String],
    update: bool,
    deep: &Deep,
    provider: &(String, String),
) -> bool {
    (update && *deep != Deep::NotRequested)
        || atoms.iter().any(|a| {
            parse_dep_atom(a, "co_upgrade")
                .is_some_and(|p| (&p.category, &p.package) == (&provider.0, &provider.1))
        })
}

/// The request that lets every installed holder of a withheld provider follow
/// its upgrade, when each of them has a route: the walk is run again with the
/// holders as extra roots (`=cat/pkg-ver` for those that must move to a newer
/// version, a forced reinstall for those a rebuild mends). Providers with a
/// holder that cannot follow (or whose pin is a USE dependency) stay withheld.
/// `None` when nothing is withheld or nothing can follow. Next to the request
/// comes each `(holder, provider)` `(category, package)` pair, so the result of
/// the second walk can name why a holder is in the plan. The notes name
/// the holders that cannot follow, whether or not a second walk runs.
pub(crate) fn co_upgrade_request(
    ctx: &ResolveCtx<'_>,
    req: &ResolveRequest,
    first: &GraphResult,
) -> (
    Option<(ResolveRequest, Vec<Followers>)>,
    Vec<HeldBackNoRoute>,
) {
    // Without a world (no complete-graph seeds) nobody is protected and nobody
    // follows, as in the bridge.
    if ctx.config.complete_seed_atoms.is_empty() && ctx.config.blocker_retry_seed_atoms.is_empty() {
        return (None, Vec::new());
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
    let mut followers: Vec<Followers> = Vec::new();
    let mut notes: Vec<HeldBackNoRoute> = Vec::new();
    for cp in providers {
        if !request_updates(&req.atoms, req.update, &req.deep, &cp) {
            continue;
        }
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
        let mut holders: Vec<((String, String, String), String)> = Vec::new();
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
                    holders.push((holder, row.atom.clone()));
                }
                _ => usable = false,
            }
        }
        holders.sort();
        holders.dedup_by(|b, a| a.0 == b.0);
        if !usable || holders.is_empty() {
            continue;
        }
        let (routes, note) = holder_routes(WithheldProvider {
            gate: ctx.gate(),
            config: ctx.config,
            root: ctx.root,
            with_bdeps: ctx.with_bdeps,
            provider: &cp,
            new: &new,
            slot: &rows[0].slot,
            installed_version: installed
                .iter()
                .find(|i| (&i.category, &i.package, &i.slot) == (&cp.0, &cp.1, &rows[0].slot))
                .map_or("", |i| i.version.as_str()),
            skipped_version: &rows[0].skipped_version,
            holders: &holders,
        });
        if let Some(note) = note {
            notes.push(note);
            continue;
        }
        for (((cat, pkg, ver), _), route) in holders.iter().zip(routes) {
            let target = route.expect("every holder has a route");
            if target == *ver {
                extra.atoms.push(format!("{cat}/{pkg}"));
                extra.reinstall_atoms.push(format!("{cat}/{pkg}"));
            } else {
                extra.atoms.push(format!("={cat}/{pkg}-{target}"));
            }
            followers.push(((cat.clone(), pkg.clone()), cp.clone()));
        }
    }
    // `--backtrack=0` asks for no second walk.
    let request = (ctx.backtrack_max != 0 && !followers.is_empty()).then_some((extra, followers));
    (request, notes)
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

/// The atoms on `provider` that a candidate's tree dependencies *require*: every
/// atom outside a `||` group, and for a `||` group the atoms of its arms only
/// when every arm names `provider`. A group with an arm on another package can
/// be met without the provider, so it constrains nothing.
pub(crate) fn tree_route_atoms(
    texts: &[&str],
    use_flags: &HashSet<String>,
    provider: &(String, String),
) -> Vec<String> {
    fn walk(nodes: &[DepTree], provider: &(String, String), out: &mut Vec<String>) -> bool {
        // Returns whether the nodes name the provider at all.
        let mut named = false;
        for node in nodes {
            match node {
                DepTree::Atom(text) => {
                    let hit = parse_dep_atom(text, "co_upgrade").is_some_and(|a| {
                        a.blocker == portage_dep::Blocker::None
                            && a.category == provider.0
                            && a.package == provider.1
                    });
                    if hit {
                        out.push(text.clone());
                        named = true;
                    }
                }
                DepTree::AllOf(children) => named |= walk(children, provider, out),
                DepTree::AnyOf(arms) => {
                    let mut group = Vec::new();
                    let all_name = arms
                        .iter()
                        .all(|arm| walk(std::slice::from_ref(arm), provider, &mut group));
                    if all_name {
                        out.extend(group);
                        named = true;
                    }
                }
            }
        }
        named
    }
    let mut out = Vec::new();
    for text in texts {
        let tokens: Vec<String> = text.split_whitespace().map(String::from).collect();
        // A string that does not reduce keeps the flat reading (the conservative direction).
        match crate::resolve::cycle_report::reduced_dep_tree(&tokens, use_flags) {
            Some(tree) => {
                walk(&tree, provider, &mut out);
            }
            None => out.extend(provider_atoms(&[text], use_flags, provider)),
        }
    }
    out.sort();
    out.dedup();
    out
}

/// What the consumer's tree needs for the provider to be `new`
/// (`cat/pkg-ver:slot/sub::repo`): `None` when an atom outside any `||` group, or
/// a group whose arms all name the provider, rejects it; otherwise the atoms to
/// ask for so that each group is met without a rejecting arm (the first arm
/// that accepts the provider, else the first arm on other packages).
pub(crate) fn tree_follow_atoms(
    texts: &[&str],
    use_flags: &HashSet<String>,
    provider: &(String, String),
    new: &str,
) -> Option<Vec<String>> {
    let on_provider = |text: &str| {
        parse_dep_atom(text, "co_upgrade").is_some_and(|a| {
            a.blocker == portage_dep::Blocker::None
                && a.category == provider.0
                && a.package == provider.1
        })
    };
    // `Some((names_provider, atoms off the provider))` when the nodes accept `new`.
    fn walk(
        nodes: &[DepTree],
        on_provider: &dyn Fn(&str) -> bool,
        new: &str,
        in_arm: bool,
    ) -> Option<(bool, Vec<String>)> {
        let (mut named, mut asks) = (false, Vec::new());
        for node in nodes {
            match node {
                DepTree::Atom(text) if on_provider(text) => {
                    let ok = portage_dep::match_from_list(portage_dep::without_use(text), &[new])
                        .is_some_and(|m| !m.is_empty());
                    if !ok {
                        return None;
                    }
                    named = true;
                }
                DepTree::Atom(text) => {
                    if in_arm {
                        asks.push(text.clone());
                    }
                }
                DepTree::AllOf(children) => {
                    let (n, a) = walk(children, on_provider, new, in_arm)?;
                    named |= n;
                    asks.extend(a);
                }
                DepTree::AnyOf(arms) => {
                    let ok: Vec<(bool, Vec<String>)> = arms
                        .iter()
                        .filter_map(|arm| walk(std::slice::from_ref(arm), on_provider, new, true))
                        .collect();
                    let pick = ok
                        .iter()
                        .find(|(n, _)| *n)
                        .or_else(|| ok.iter().find(|(_, a)| !a.is_empty()))?;
                    named |= pick.0;
                    // Only a group with a rejecting arm needs asking; a menu
                    // that already accepts the provider is met as it stands.
                    if !pick.0 {
                        asks.extend(pick.1.iter().cloned());
                    }
                }
            }
        }
        Some((named, asks))
    }
    let mut out = Vec::new();
    for text in texts {
        let tokens: Vec<String> = text.split_whitespace().map(String::from).collect();
        let tree = crate::resolve::cycle_report::reduced_dep_tree(&tokens, use_flags)?;
        let (_, asks) = walk(&tree, &on_provider, new, false)?;
        out.extend(asks);
    }
    out.sort();
    out.dedup();
    Some(out)
}

/// Whether `atoms` (as returned by [`provider_atoms`]) accept the provider
/// rendered `new` (`cat/pkg-ver:slot/sub::repo`). Versioned atoms are a menu
/// (one must match); unversioned ones (`:=`, `:0`) must all match.
pub fn provider_atoms_accept(atoms: &[String], new: &str) -> bool {
    let accepts =
        |atom: &&String| portage_dep::match_from_list(atom, &[new]).is_some_and(|m| !m.is_empty());
    let (versioned, plain): (Vec<&String>, Vec<&String>) = atoms
        .iter()
        .partition(|a| parse_dep_atom(a, "co_upgrade").is_some_and(|p| p.version.is_some()));
    plain.iter().all(accepts) && (versioned.is_empty() || versioned.iter().any(accepts))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atoms(text: &str) -> Vec<String> {
        let prov = ("dev-libs".to_string(), "prov".to_string());
        tree_route_atoms(&[text], &HashSet::new(), &prov)
    }

    #[test]
    fn or_group_with_an_arm_off_the_provider_constrains_nothing() {
        // The `p569virt` shape, a version menu plus another package.
        assert_eq!(
            atoms("|| ( =dev-libs/prov-5.42* dev-libs/core ) dev-libs/prov:="),
            ["dev-libs/prov:="]
        );
    }

    #[test]
    fn or_group_naming_only_the_provider_stays_a_menu() {
        assert_eq!(
            atoms("|| ( =dev-libs/prov-5.42* =dev-libs/prov-5.40* )"),
            ["=dev-libs/prov-5.40*", "=dev-libs/prov-5.42*"]
        );
    }

    fn follow(text: &str) -> Option<Vec<String>> {
        let prov = ("dev-libs".to_string(), "prov".to_string());
        tree_follow_atoms(&[text], &HashSet::new(), &prov, "dev-libs/prov-5.44:0/5.44")
    }

    #[test]
    fn follow_asks_for_the_arm_off_the_provider() {
        // The rejecting arm is skipped, the other package is asked for.
        assert_eq!(
            follow("|| ( =dev-libs/prov-5.42* dev-libs/other ) dev-libs/prov:="),
            Some(vec!["dev-libs/other".to_string()])
        );
    }

    #[test]
    fn follow_asks_nothing_when_a_provider_arm_accepts() {
        assert_eq!(
            follow("|| ( =dev-libs/prov-5.44* dev-libs/other ) dev-libs/prov:="),
            Some(Vec::new())
        );
    }

    #[test]
    fn follow_is_none_for_a_pin_no_arm_escapes() {
        assert_eq!(follow("=dev-libs/prov-5.42* dev-libs/prov:="), None);
        assert_eq!(
            follow("|| ( =dev-libs/prov-5.42* =dev-libs/prov-5.40* )"),
            None
        );
    }

    #[test]
    fn plain_atoms_still_pin() {
        assert_eq!(
            atoms("=dev-libs/prov-5.42* dev-libs/other"),
            ["=dev-libs/prov-5.42*"]
        );
    }
}
