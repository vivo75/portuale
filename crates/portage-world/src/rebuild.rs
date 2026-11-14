//! Installed packages a plan keeps but must build again.

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use portage_dep::SlotOperator;
use portage_profile::Config;
use portage_repo::EffectiveUseFlagsArgs;
use portage_use_reduce::{MatchMode, UseReduceFlatDisjunctiveArgs, use_reduce_flat_disjunctive};
use portage_vdb::EntryKey;

use crate::{DEP_KEYS, Memos, World};

impl World {
    /// One installed package the plan does not replace: its `cat/pkg:slot`
    /// node at the main slot, with the installed record. The set `plan_json`
    /// prints as `kept`, so the two cannot drift.
    pub fn kept_installed(
        &self,
        root: &std::path::Path,
    ) -> Vec<(String, portage_repo::InstalledPackage)> {
        // `World::change`, not the slot's own: a kept package the plan builds
        // again is replaced too, even though its version does not move.
        let changed: HashSet<String> = self
            .slots
            .iter()
            .filter(|s| self.change(s).is_some())
            .map(|s| s.node())
            .collect();
        portage_repo::all_installed_packages(root)
            .iter()
            .filter(|p| {
                let slot = p.slot.split('/').next().unwrap_or_default();
                !changed.contains(&format!("{}/{}:{slot}", p.category, p.package))
            })
            .map(|p| {
                let slot = p.slot.split('/').next().unwrap_or_default();
                (format!("{}/{}:{slot}", p.category, p.package), p.clone())
            })
            .collect()
    }
}

/// A stale slot-operator binding the plan does not repair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusedBinding {
    /// `cat/pkg` of the kept consumer.
    pub consumer: String,
    /// The recorded atom that forbids the repair, or the stale `:=` atom
    /// itself when the consumer has no visible candidate.
    pub atom: String,
    /// `cat/pkg-ver` of the provider version the plan leaves behind, `None`
    /// when the consumer has no visible tree candidate.
    pub provider: Option<String>,
}

/// What [`slot_operator`] reports: the kept consumers a rebuild repairs,
/// and the stale bindings it does not.
pub(crate) struct StaleBindings {
    pub(crate) rebuild: BTreeSet<String>,
    pub(crate) refused: Vec<RefusedBinding>,
}

/// The installed sub-slot and `cat/pkg-ver` of the provider version the plan
/// leaves behind in `(category, package, slot)`: the chosen version when the
/// plan changes the slot, else the installed VDB version. `None` when neither
/// exists.
fn final_provider(
    world: &World,
    db: &Arc<dyn portage_vdb::InstalledDb>,
    root: &std::path::Path,
    provider: (&str, &str, &str),
) -> Option<(String, String)> {
    let (category, package, slot) = provider;
    if let Some(s) = world
        .slots
        .iter()
        .find(|s| s.cp == format!("{category}/{package}") && s.slot == slot)
        && s.change().is_some()
    {
        let chosen = s.chosen()?;
        return Some((
            chosen.sub_slot.clone(),
            format!("{category}/{package}-{}", chosen.version),
        ));
    }
    let version = portage_repo::all_installed_packages(root)
        .iter()
        .find(|p| {
            p.category == category
                && p.package == package
                && p.slot.split('/').next().unwrap_or_default() == slot
        })
        .map(|p| p.version.clone())?;
    let raw = db
        .aux_get(
            &EntryKey::new(category, format!("{package}-{version}")),
            "SLOT",
        )
        .ok()
        .flatten()
        .unwrap_or_default();
    let sub = raw
        .split_once('/')
        .map_or_else(|| raw.clone(), |(_, sub)| sub.to_string());
    // PMS: a `SLOT` without a sub-slot part carries an implicit sub-slot
    // equal to the slot; an empty read means the entry is gone.
    if sub.is_empty() {
        return None;
    }
    Some((sub, format!("{category}/{package}-{version}")))
}

/// The recorded `USE` of the installed `key`, `+`/`-` markers stripped.
fn recorded_use(db: &Arc<dyn portage_vdb::InstalledDb>, key: &EntryKey) -> HashSet<String> {
    db.aux_get(key, "USE")
        .ok()
        .flatten()
        .unwrap_or_default()
        .split_whitespace()
        .map(|f| f.trim_start_matches(['+', '-']).to_string())
        .collect()
}

/// The non-blocker atoms of one recorded `*DEPEND` text, with `||` groups
/// and `flag? ( )` conditionals reduced against the installed `USE` the way
/// the planner reduces tree metadata. `None` when the text does not reduce:
/// the caller refuses the consumer rather than mis-judging it.
fn reduced_recorded_atoms(
    text: &str,
    use_flags: &HashSet<String>,
    memos: &Memos,
    config: &Config,
) -> Option<Vec<String>> {
    let tokens: Vec<String> = text.split_whitespace().map(String::from).collect();
    let mut first = |_: &[Vec<String>]| 0usize;
    let flat = use_reduce_flat_disjunctive(
        UseReduceFlatDisjunctiveArgs {
            tokens: &tokens,
            uselist: use_flags,
            mode: MatchMode::Normal,
            empty_groups_always_true: true,
            tie_break: &mut first,
        },
        &mut |branch| memos.branch_preference(branch, config),
    )
    .ok()?;
    Some(
        flat.into_iter()
            .map(|tok| portage_dep::evaluate_atom_conditionals(&tok, use_flags).unwrap_or(tok))
            .collect(),
    )
}

/// `cat/pkg-ver` of the provider version the final state binds `atom_str`
/// against: the `cat/pkg-ver` [`final_provider`] leaves behind in the slot
/// the atom names. `None` when the atom names no slot or no version is left.
fn provider_cpv(
    world: &World,
    db: &Arc<dyn portage_vdb::InstalledDb>,
    root: &std::path::Path,
    atom_str: &str,
) -> Option<String> {
    let a = portage_repo::parse_dep_atom(atom_str, "rebuild_provider")?;
    let slot = a.slot.as_deref()?;
    final_provider(world, db, root, (&a.category, &a.package, slot)).map(|(_, cpv)| cpv)
}

/// Whether the chosen ebuild's tree metadata still names any of the stale
/// providers, evaluated against its effective USE: a rebuild re-records the
/// live dependencies, so a tree that dropped them has nothing stale left to
/// repair. Unreadable metadata counts as naming (the pin stands).
fn tree_names_provider(args: TreeNamesProviderArgs<'_>) -> bool {
    let TreeNamesProviderArgs {
        chosen,
        category,
        package,
        providers,
        memos,
        config,
    } = args;
    let pf = format!("{package}-{}", chosen.version);
    let Ok(meta) = portage_repo::repo_aux_metadata(&chosen.repo_location, category, &pf) else {
        return true;
    };
    let use_flags: HashSet<String> = portage_repo::effective_use_flags(EffectiveUseFlagsArgs {
        config,
        iuse: &chosen.iuse,
        keywords: &chosen.keywords,
        candidate_str: &chosen.match_str(category, package),
        build_id: None,
        category,
        package,
    })
    .iter()
    .cloned()
    .collect();
    DEP_KEYS.iter().any(|dep| {
        let text = meta.get(*dep).map(String::as_str).unwrap_or_default();
        reduced_recorded_atoms(text, &use_flags, memos, config)
            .unwrap_or_default()
            .iter()
            .filter_map(|tok| portage_repo::parse_dep_atom(tok, "rebuild_naming"))
            .any(|a| {
                a.blocker == portage_dep::Blocker::None
                    && providers.contains(&(a.category, a.package))
            })
    })
}

/// Arguments of [`tree_names_provider`].
struct TreeNamesProviderArgs<'a> {
    chosen: &'a portage_repo::Candidate,
    category: &'a str,
    package: &'a str,
    providers: &'a [(String, String)],
    memos: &'a Memos<'a>,
    config: &'a Config,
}

/// `cat/pkg:slot` of each kept installed package whose recorded `:slot/sub-slot=`
/// dependency no longer matches the provider version the plan leaves behind:
/// the chosen one when the plan changes the provider's slot, else the
/// installed one (a binding already stale is still repaired). A `:*`
/// dependency never counts, and neither does an atom whose provider version
/// cannot be worked out.
///
/// A stale consumer is rebuilt only when no other recorded atom forbids it:
/// every non-`:=` atom naming a slot the plan changes must still match the
/// chosen version there (a `=`/`~`/`<`/`>=` pin is judged by the matcher, not
/// by a sub-slot compare). A `:=` trigger is judged by the sub-slot compare
/// only: the matcher enforces recorded-sub equality
/// (`portage-dep` `matches_slot`), so it would refuse every stale binding. A
/// consumer with no visible tree candidate, and one whose text does not
/// reduce, is refused rather than mis-judged.
///
/// PMS `dependencies.tex`, `sec:slot-dep`: `:=` binds the consumer to the
/// slot and sub-slot it was built against.
pub(crate) fn slot_operator(
    world: &World,
    root: &std::path::Path,
    memos: &Memos,
    config: &Config,
) -> StaleBindings {
    let db = portage_vdb::for_root(root);
    let mut rebuild = BTreeSet::new();
    let mut refused: Vec<RefusedBinding> = Vec::new();
    for (node, installed) in world.kept_installed(root) {
        let key = EntryKey::new(
            installed.category.clone(),
            format!("{}-{}", installed.package, installed.version),
        );
        let use_flags = recorded_use(&db, &key);
        // Raw stale `:=` candidates: recorded atoms naming a sub-slot the
        // final provider version no longer has.
        let mut raw: Vec<String> = Vec::new();
        for dep in DEP_KEYS {
            let text = db.aux_get(&key, dep).ok().flatten().unwrap_or_default();
            for tok in text.split_whitespace() {
                let Some(a) = portage_repo::parse_dep_atom(tok, "rebuild_stale") else {
                    continue;
                };
                if a.blocker != portage_dep::Blocker::None
                    || a.slot_operator != Some(SlotOperator::Equals)
                {
                    continue;
                }
                let (Some(slot), Some(recorded)) = (&a.slot, &a.sub_slot) else {
                    continue;
                };
                if recorded == "*" {
                    continue;
                }
                let Some((sub, _)) =
                    final_provider(world, &db, root, (&a.category, &a.package, slot))
                else {
                    continue;
                };
                if sub != *recorded && !raw.iter().any(|t| t == tok) {
                    raw.push(tok.to_string());
                }
            }
        }
        if raw.is_empty() {
            continue;
        }
        // Reduced atoms for the confirmation and the hold-back check; a text
        // that does not reduce refuses the consumer rather than mis-judging it.
        let mut reduced: Vec<String> = Vec::new();
        let mut unreduced = false;
        for dep in DEP_KEYS {
            let text = db.aux_get(&key, dep).ok().flatten().unwrap_or_default();
            if text.trim().is_empty() {
                continue;
            }
            match reduced_recorded_atoms(&text, &use_flags, memos, config) {
                Some(atoms) => reduced.extend(atoms),
                None => {
                    unreduced = true;
                    break;
                }
            }
        }
        if unreduced {
            refused.push(RefusedBinding {
                consumer: format!("{}/{}", installed.category, installed.package),
                atom: raw[0].clone(),
                provider: provider_cpv(world, &db, root, &raw[0]),
            });
            continue;
        }
        let active: HashSet<&str> = reduced.iter().map(String::as_str).collect();
        let stale: Vec<&str> = raw
            .iter()
            .map(String::as_str)
            .filter(|tok| {
                active.contains(
                    portage_dep::evaluate_atom_conditionals(tok, &use_flags)
                        .unwrap_or_else(|| (*tok).to_string())
                        .as_str(),
                )
            })
            .collect();
        if stale.is_empty() {
            continue;
        }
        // The final provider version a rebuild would use must exist.
        let Some(chosen) = world
            .slots
            .iter()
            .find(|s| s.node() == node)
            .and_then(|s| s.chosen())
        else {
            refused.push(RefusedBinding {
                consumer: format!("{}/{}", installed.category, installed.package),
                atom: stale[0].to_string(),
                provider: None,
            });
            continue;
        };
        // No rebuild may hold a package back: every recorded non-`:=` atom
        // naming a slot the plan changes must match the chosen version there.
        let mut held_back: Option<RefusedBinding> = None;
        'outer: for tok in &reduced {
            let Some(a) = portage_repo::parse_dep_atom(tok, "rebuild_holdback") else {
                continue;
            };
            if a.blocker != portage_dep::Blocker::None
                || a.slot_operator == Some(SlotOperator::Equals)
            {
                continue;
            }
            for s in world
                .slots
                .iter()
                .filter(|s| s.cp == format!("{}/{}", a.category, a.package) && s.change().is_some())
            {
                if let Some(want) = &a.slot
                    && want != &s.slot
                {
                    continue;
                }
                let Some(next) = s.chosen() else {
                    continue;
                };
                if !memos.atom_matches(tok, next, (&a.category, &a.package), config) {
                    let provider =
                        final_provider(world, &db, root, (&a.category, &a.package, &s.slot))
                            .map(|(_, cpv)| cpv);
                    held_back = Some(RefusedBinding {
                        consumer: format!("{}/{}", installed.category, installed.package),
                        atom: (*tok).to_string(),
                        provider,
                    });
                    break 'outer;
                }
            }
        }
        match held_back {
            Some(refusal) => refused.push(refusal),
            None => {
                // A rebuild re-records the chosen ebuild's live dependencies:
                // when its tree no longer names any stale provider, there is
                // nothing stale left to repair, so the consumer stays out
                // silently (the resolver's third arm applies the same gate).
                let stale_cps: Vec<(String, String)> = stale
                    .iter()
                    .filter_map(|tok| {
                        portage_repo::parse_dep_atom(tok, "rebuild_naming")
                            .map(|a| (a.category, a.package))
                    })
                    .collect();
                if tree_names_provider(TreeNamesProviderArgs {
                    chosen,
                    category: &installed.category,
                    package: &installed.package,
                    providers: &stale_cps,
                    memos,
                    config,
                }) {
                    rebuild.insert(node);
                }
            }
        }
    }
    refused.sort_by(|a, b| (&a.consumer, &a.atom).cmp(&(&b.consumer, &b.atom)));
    refused.dedup_by(|a, b| a.consumer == b.consumer && a.atom == b.atom);
    StaleBindings { rebuild, refused }
}

/// `cat/pkg:slot` of every kept installed package that has a version to
/// install: with everything built, each is built again.
pub(crate) fn every_kept(world: &World, root: &std::path::Path) -> BTreeSet<String> {
    world
        .kept_installed(root)
        .into_iter()
        .filter(|(node, _)| {
            world
                .slots
                .iter()
                .any(|s| s.node() == *node && s.chosen().is_some())
        })
        .map(|(node, _)| node)
        .collect()
}

/// `cat/pkg:slot` of each kept installed package whose recorded `USE`, cut to
/// the `IUSE` of the version kept, differs from the configured one.
///
/// PMS `dependencies.tex`: USE is restricted to `IUSE_EFFECTIVE`.
pub(crate) fn changed_use(
    world: &World,
    root: &std::path::Path,
    config: &Config,
) -> BTreeSet<String> {
    let db = portage_vdb::for_root(root);
    let names = |text: &str| -> BTreeSet<String> {
        text.split_whitespace()
            .map(|f| f.trim_start_matches(['+', '-']).to_string())
            .collect()
    };
    world
        .kept_installed(root)
        .into_iter()
        .filter_map(|(node, installed)| {
            let s = world.slots.iter().find(|s| s.node() == node)?;
            let chosen = s.chosen()?;
            let key = EntryKey::new(
                installed.category.clone(),
                format!("{}-{}", installed.package, installed.version),
            );
            let iuse = names(&chosen.iuse);
            let recorded = db.aux_get(&key, "USE").ok().flatten().unwrap_or_default();
            let recorded: BTreeSet<String> =
                names(&recorded).intersection(&iuse).cloned().collect();
            let (category, package) = s.cp.split_once('/').unwrap_or_default();
            let configured = portage_repo::effective_use_flags(EffectiveUseFlagsArgs {
                config,
                iuse: &chosen.iuse,
                keywords: &chosen.keywords,
                candidate_str: &chosen.match_str(category, package),
                build_id: None,
                category,
                package,
            });
            let configured: BTreeSet<String> = configured
                .iter()
                .filter(|f| iuse.contains(*f))
                .cloned()
                .collect();
            (recorded != configured).then(|| node.clone())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{Order, testkit::analyse_fixture_with};

    /// PMS `dependencies.tex`: with everything built, every slot `@world`
    /// reaches is a row, so the rows are the reached slots that have a chosen
    /// version; the installed one that is kept is a `Rebuild`.
    #[test]
    fn emptytree_has_a_row_for_every_reached_slot_with_a_version() {
        let installed = [("dev-libs/newpkg-1.0", "0")];
        let world = analyse_fixture_with(&["dev-libs/withdeps"], &installed, |o| {
            o.rebuilds = true;
            o.emptytree = true;
        });
        let reached = world
            .slots
            .iter()
            .filter(|s| world.reached.contains(&s.node()) && s.chosen().is_some())
            .count();
        let rows = world.ordered(&Order::Alpha);
        assert!(reached > 1);
        assert_eq!(rows.len(), reached);
        let kept = rows
            .iter()
            .find(|r| r.slot.cp == "dev-libs/newpkg")
            .map(|r| &r.slot)
            .unwrap();
        assert_eq!(world.change(kept), Some(crate::Change::Rebuild));
    }
}
