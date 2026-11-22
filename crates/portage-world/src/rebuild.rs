//! Installed packages a plan keeps but must build again.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use portage_dep::SlotOperator;
use portage_profile::Config;
use portage_repo::EffectiveUseFlagsArgs;
use portage_use_reduce::{MatchMode, UseReduceFlatDisjunctiveArgs, use_reduce_flat_disjunctive};
use portage_vdb::EntryKey;

use crate::{DEP_KEYS, Memos, World};

/// The `*DEPEND` variables whose `:=` binding breaks an installed package
/// the walk from the world did not reach.
const OUTSIDE_WALK_KEYS: [&str; 3] = ["RDEPEND", "PDEPEND", "IDEPEND"];

impl World {
    /// One installed package the plan does not replace: its `cat/pkg:slot`
    /// node at the main slot, with the installed record. The set `plan_json`
    /// prints as `kept`, so the two cannot drift.
    pub fn kept_installed(
        &self,
        root: &std::path::Path,
    ) -> Vec<(String, portage_repo::InstalledPackage)> {
        self.kept_of(&portage_repo::all_installed_packages(root))
    }

    /// [`World::kept_installed`] over an already-read installed list.
    fn kept_of(
        &self,
        installed: &[portage_repo::InstalledPackage],
    ) -> Vec<(String, portage_repo::InstalledPackage)> {
        // `World::change`, not the slot's own: a kept package the plan builds
        // again is replaced too, even though its version does not move.
        let changed: HashSet<String> = self
            .slots
            .iter()
            .filter(|s| self.change(s).is_some())
            .map(|s| s.node())
            .collect();
        installed
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
    /// itself when the consumer has no visible ebuild in its installed slot.
    pub atom: String,
    /// `cat/pkg-ver` of the provider version the plan leaves behind; `None`
    /// when the consumer has no visible ebuild in its installed slot.
    pub provider: Option<String>,
}

/// What [`slot_operator`] reports: the kept consumers a rebuild repairs,
/// and the stale bindings it does not.
pub(crate) struct StaleBindings {
    pub(crate) rebuild: BTreeSet<String>,
    pub(crate) refused: Vec<RefusedBinding>,
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
/// the caller falls back to the tree ebuild.
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
        keys,
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
    keys.iter().any(|dep| {
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
    keys: &'a [&'a str],
    memos: &'a Memos<'a>,
    config: &'a Config,
}

/// One stale-binding scan: the plan, the installed records read once, and
/// what the reduction of recorded text needs.
struct Scan<'a> {
    world: &'a World,
    db: Arc<dyn portage_vdb::InstalledDb>,
    /// `(category, package, slot)` -> the installed version, first found.
    installed: HashMap<(String, String, String), String>,
    memos: &'a Memos<'a>,
    config: &'a Config,
}

/// The refusal of `atom` for the kept `installed` consumer.
fn refuse(
    installed: &portage_repo::InstalledPackage,
    atom: &str,
    provider: Option<String>,
) -> RefusedBinding {
    RefusedBinding {
        consumer: format!("{}/{}", installed.category, installed.package),
        atom: atom.to_string(),
        provider,
    }
}

impl Scan<'_> {
    /// The sub-slot and `cat/pkg-ver` of the provider version the plan leaves
    /// behind in `(category, package, slot)`: the chosen version when the
    /// plan changes the slot, else the installed one. `None` when neither
    /// exists.
    fn final_provider(
        &self,
        (category, package, slot): (&str, &str, &str),
    ) -> Option<(String, String)> {
        let cp = format!("{category}/{package}");
        if let Some(s) = self
            .world
            .slots
            .iter()
            .find(|s| s.cp == cp && s.slot == slot)
            && s.change().is_some()
        {
            let chosen = s.chosen()?;
            return Some((chosen.sub_slot.clone(), format!("{cp}-{}", chosen.version)));
        }
        let key = (category.to_string(), package.to_string(), slot.to_string());
        let version = self.installed.get(&key)?;
        let raw = self
            .db
            .aux_get(
                &EntryKey::new(category, format!("{package}-{version}")),
                "SLOT",
            )
            .ok()
            .flatten()
            .unwrap_or_default();
        // PMS: a `SLOT` without a sub-slot part carries an implicit sub-slot
        // equal to the slot; an empty read means the entry is gone.
        let sub = raw.split_once('/').map_or(raw.as_str(), |(_, sub)| sub);
        (!sub.is_empty()).then(|| (sub.to_string(), format!("{cp}-{version}")))
    }

    /// The recorded `:=` atoms in `texts` naming a sub-slot the final
    /// provider version no longer has.
    fn stale_atoms(&self, texts: &[String]) -> Vec<String> {
        let mut raw: Vec<String> = Vec::new();
        for tok in texts.iter().flat_map(|t| t.split_whitespace()) {
            let Some(a) = portage_repo::parse_dep_atom(tok, "rebuild_stale") else {
                continue;
            };
            let (Some(slot), Some(recorded)) = (&a.slot, &a.sub_slot) else {
                continue;
            };
            if a.blocker != portage_dep::Blocker::None
                || a.slot_operator != Some(SlotOperator::Equals)
                || recorded == "*"
            {
                continue;
            }
            let Some((sub, _)) = self.final_provider((&a.category, &a.package, slot)) else {
                continue;
            };
            if sub != *recorded && !raw.iter().any(|t| t == tok) {
                raw.push(tok.to_string());
            }
        }
        raw
    }

    /// The first recorded non-`:=` atom of `reduced` that the version the
    /// plan chooses in a slot it changes no longer matches: a rebuild would
    /// hold that package back.
    fn held_back(
        &self,
        installed: &portage_repo::InstalledPackage,
        reduced: &[String],
    ) -> Option<RefusedBinding> {
        for tok in reduced {
            let Some(a) = portage_repo::parse_dep_atom(tok, "rebuild_holdback") else {
                continue;
            };
            if a.blocker != portage_dep::Blocker::None
                || a.slot_operator == Some(SlotOperator::Equals)
            {
                continue;
            }
            let cp = format!("{}/{}", a.category, a.package);
            for s in self
                .world
                .slots
                .iter()
                .filter(|s| s.cp == cp && s.change().is_some())
            {
                if a.slot.as_ref().is_some_and(|want| *want != s.slot) {
                    continue;
                }
                let Some(next) = s.chosen() else {
                    continue;
                };
                if !self
                    .memos
                    .atom_matches(tok, next, (&a.category, &a.package), self.config)
                {
                    let provider = self
                        .final_provider((&a.category, &a.package, &s.slot))
                        .map(|(_, cpv)| cpv);
                    return Some(refuse(installed, tok, provider));
                }
            }
        }
        None
    }

    /// Whether the kept `installed` consumer at `node` is rebuilt (`Ok(true)`),
    /// left alone (`Ok(false)`), or refused with the reason.
    fn judge(
        &self,
        node: &str,
        installed: &portage_repo::InstalledPackage,
    ) -> Result<bool, RefusedBinding> {
        let key = EntryKey::new(
            installed.category.clone(),
            format!("{}-{}", installed.package, installed.version),
        );
        // Build-time variables bind only a consumer the walk from the world reached.
        let walked = self.world.walked.contains(node);
        let keys: &[&str] = if walked {
            &DEP_KEYS
        } else {
            &OUTSIDE_WALK_KEYS
        };
        let texts: Vec<String> = keys
            .iter()
            .map(|dep| {
                self.db
                    .aux_get(&key, dep)
                    .ok()
                    .flatten()
                    .unwrap_or_default()
            })
            .collect();
        let raw = self.stale_atoms(&texts);
        if raw.is_empty() {
            return Ok(false);
        }
        // A text that does not reduce gives its raw stale atoms and no
        // hold-back atom: the tree ebuild decides.
        let use_flags = recorded_use(&self.db, &key);
        let mut reduced: Vec<String> = Vec::new();
        let mut unreduced: Vec<String> = Vec::new();
        for text in texts.iter().filter(|t| !t.trim().is_empty()) {
            match reduced_recorded_atoms(text, &use_flags, self.memos, self.config) {
                Some(atoms) => reduced.extend(atoms),
                None => unreduced.extend(self.stale_atoms(std::slice::from_ref(text))),
            }
        }
        let active: HashSet<&str> = reduced.iter().map(String::as_str).collect();
        let stale: Vec<&String> = raw
            .iter()
            .filter(|tok| {
                unreduced.contains(*tok)
                    || active.contains(
                        portage_dep::evaluate_atom_conditionals(tok, &use_flags)
                            .unwrap_or_else(|| (*tok).clone())
                            .as_str(),
                    )
            })
            .collect();
        let Some(first) = stale.first() else {
            return Ok(false);
        };
        // The version a rebuild installs: the walk's choice, else the best
        // visible ebuild of the installed slot.
        let slot = installed.slot.split('/').next().unwrap_or_default();
        let walk_choice = self
            .world
            .slots
            .iter()
            .find(|s| s.node() == node)
            .and_then(|s| s.chosen())
            .cloned();
        let Some(chosen) = walk_choice.or_else(|| {
            self.memos.best_visible_in_slot(
                &installed.category,
                &installed.package,
                Some(slot),
                self.config,
            )
        }) else {
            return Err(refuse(installed, first, None));
        };
        if let Some(refusal) = self.held_back(installed, &reduced) {
            return Err(refusal);
        }
        // A rebuild re-records the chosen ebuild's live dependencies: when
        // its tree no longer names any stale provider, nothing stale is left.
        let providers: Vec<(String, String)> = stale
            .iter()
            .filter_map(|tok| {
                portage_repo::parse_dep_atom(tok, "rebuild_naming").map(|a| (a.category, a.package))
            })
            .collect();
        Ok(tree_names_provider(TreeNamesProviderArgs {
            chosen: &chosen,
            category: &installed.category,
            package: &installed.package,
            providers: &providers,
            keys,
            memos: self.memos,
            config: self.config,
        }))
    }
}

/// `cat/pkg:slot` of each kept installed package whose recorded `:slot/sub-slot=`
/// dependency no longer matches the provider version the plan leaves behind (a
/// binding already stale is repaired too). A `:*` dependency never counts.
///
/// A stale consumer is rebuilt only when no other recorded atom forbids it. One
/// the walk does not reach is judged by its best visible ebuild and on its
/// runtime variables only; one with no visible ebuild is refused.
///
/// PMS `dependencies.tex`, `sec:slot-dep`: `:=` binds the consumer to the
/// slot and sub-slot it was built against.
pub(crate) fn slot_operator(
    world: &World,
    root: &std::path::Path,
    memos: &Memos,
    config: &Config,
) -> StaleBindings {
    let all = portage_repo::all_installed_packages(root);
    let mut installed = HashMap::new();
    for p in all.iter() {
        let slot = p.slot.split('/').next().unwrap_or_default().to_string();
        installed
            .entry((p.category.clone(), p.package.clone(), slot))
            .or_insert_with(|| p.version.clone());
    }
    let scan = Scan {
        world,
        db: portage_vdb::for_root(root),
        installed,
        memos,
        config,
    };
    let mut rebuild = BTreeSet::new();
    let mut refused: Vec<RefusedBinding> = Vec::new();
    for (node, installed) in world.kept_of(&all) {
        match scan.judge(&node, &installed) {
            Ok(true) => {
                rebuild.insert(node);
            }
            Ok(false) => {}
            Err(refusal) => refused.push(refusal),
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
            .find(|s| s.cp == "dev-libs/newpkg")
            .copied()
            .unwrap();
        assert_eq!(world.change(kept), Some(crate::Change::Rebuild));
    }
}
