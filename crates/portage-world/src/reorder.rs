//! Second pass: per package slot, put the versions that satisfy every
//! atom aimed at that slot first.

use std::collections::{BTreeMap, BTreeSet};

use portage_profile::Config;
use portage_repo::{Candidate, RepoConfig};

/// One package slot with its versions in final order.
pub struct SlotSet {
    pub cp: String,
    pub slot: String,
    /// `(candidate, visible, satisfies every requester)`, final order.
    versions: Vec<(Candidate, bool, bool)>,
    /// Each atom aimed at this slot and who asked for it.
    pub atoms: Vec<(String, Vec<String>)>,
    /// How many visible versions satisfy every atom.
    satisfying: usize,
    /// No visible version satisfies the requesters, here or in another slot.
    pub invalid: bool,
    /// Nothing satisfies them here, but another slot of the package does.
    pub unneeded: bool,
    /// The version installed in this slot, if any.
    pub installed: Option<String>,
}

/// What a plan would do to a slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    New,
    Upgrade,
    Downgrade,
}

impl SlotSet {
    /// `cat/pkg:slot`, the name the dependency edges use.
    #[must_use]
    pub fn node(&self) -> String {
        format!("{}:{}", self.cp, self.slot)
    }

    /// The version a plan would install: the first visible one that satisfies
    /// every requester.
    #[must_use]
    pub fn chosen(&self) -> Option<&Candidate> {
        self.versions
            .iter()
            .find(|(_, visible, ok)| *visible && *ok)
            .map(|(c, _, _)| c)
    }

    /// What installing [`chosen`](Self::chosen) would change, `None` when the
    /// slot already has that version or has no choice.
    #[must_use]
    pub fn change(&self) -> Option<Change> {
        let chosen = self.chosen()?;
        match &self.installed {
            None => Some(Change::New),
            Some(have) => match crate::version_cmp(&chosen.version, have) {
                std::cmp::Ordering::Greater => Some(Change::Upgrade),
                std::cmp::Ordering::Less => Some(Change::Downgrade),
                std::cmp::Ordering::Equal => None,
            },
        }
    }
}

impl SlotSet {
    pub fn print(&self, only_best: bool) {
        let state = match (self.invalid, self.unneeded) {
            (true, _) => "INVALID",
            (_, true) => "unneeded (another slot serves its requesters)",
            _ => "ok",
        };
        println!(
            "{}:{}  {state} ({} satisfy)",
            self.cp, self.slot, self.satisfying
        );
        if self.invalid || self.unneeded {
            for (atom, by) in &self.atoms {
                println!("    wants {atom}  <- {}", by.join(", "));
            }
        }
        let n = if only_best { 1 } else { usize::MAX };
        for (c, visible, ok) in self.versions.iter().take(n) {
            let vis = if *visible { ' ' } else { 'M' };
            let sat = if *ok { '*' } else { ' ' };
            println!("  {sat}{vis} {}::{}", c.version, c.repo_name);
        }
    }
}

/// What every package shares: who asked for each atom, and the tree.
pub struct Ctx<'a> {
    pub requesters: &'a BTreeMap<String, BTreeSet<String>>,
    pub repos: &'a [RepoConfig],
    pub config: &'a Config,
    pub memo: &'a crate::Memos<'a>,
    /// More than one satisfying version also counts as invalid.
    pub unique: bool,
}

/// Reorders every slot of `category/package` against the `atoms` aimed at
/// the package. An atom is aimed at a slot when it matches at least one
/// version of it; a version satisfies the slot when it matches all of them.
/// A slot nothing satisfies is `invalid`, unless a visible version of
/// another slot satisfies all the same atoms (then it is `unneeded`).
pub fn reorder(category: &str, package: &str, atoms: &[&String], ctx: &Ctx<'_>) -> Vec<SlotSet> {
    let Ok(all) = portage_repo::list_candidates(ctx.repos, category, package) else {
        return Vec::new();
    };
    let matches = |atom: &str, c: &Candidate| {
        ctx.memo
            .atom_matches(atom, c, (category, package), ctx.config)
    };
    let visible = |c: &Candidate| ctx.memo.visible(c, category, package, ctx.config);
    let mut slots: BTreeMap<&str, Vec<&Candidate>> = BTreeMap::new();
    for c in all.iter() {
        slots.entry(c.slot.as_str()).or_default().push(c);
    }
    let mut out: Vec<(SlotSet, Vec<&String>)> = slots
        .iter()
        .map(|(slot, cands)| {
            let aimed: Vec<&String> = atoms
                .iter()
                .copied()
                .filter(|a| cands.iter().any(|c| matches(a, c)))
                .collect();
            let mut versions: Vec<(Candidate, bool, bool)> = cands
                .iter()
                .map(|c| {
                    let ok = aimed.iter().all(|a| matches(a, c));
                    ((*c).clone(), visible(c), ok)
                })
                .collect();
            versions.sort_by(|(a, av, aok), (b, bv, bok)| {
                bok.cmp(aok).then_with(|| crate::rank(b, *bv, a, *av))
            });
            let satisfying = versions.iter().filter(|(_, v, ok)| *v && *ok).count();
            let set = SlotSet {
                cp: format!("{category}/{package}"),
                slot: (*slot).to_string(),
                versions,
                atoms: aimed
                    .iter()
                    .map(|a| {
                        let by = ctx.requesters.get(*a).map(|r| r.iter().cloned().collect());
                        ((*a).clone(), by.unwrap_or_default())
                    })
                    .collect(),
                satisfying,
                invalid: satisfying == 0 || (ctx.unique && satisfying > 1),
                unneeded: false,
                installed: ctx
                    .memo
                    .installed_versions
                    .get(&(format!("{category}/{package}"), (*slot).to_string()))
                    .cloned(),
            };
            (set, aimed)
        })
        .collect();
    // A slot nothing satisfies is not a problem when another slot serves
    // all the atoms aimed at it.
    let served_elsewhere: Vec<bool> = out
        .iter()
        .enumerate()
        .map(|(i, (set, aimed))| {
            set.satisfying == 0
                && out.iter().enumerate().any(|(j, (other, _))| {
                    j != i
                        && other
                            .versions
                            .iter()
                            .any(|(c, v, _)| *v && aimed.iter().all(|a| matches(a, c)))
                })
        })
        .collect();
    for ((set, _), elsewhere) in out.iter_mut().zip(served_elsewhere) {
        if elsewhere {
            set.invalid = false;
            set.unneeded = true;
        }
    }
    out.into_iter().map(|(set, _)| set).collect()
}
