//! Experiment: prints the dependency tree of the given packages or sets as
//! a Graphviz DOT diagram (`... | dot -Tsvg > tree.svg`).
//!
//! Dependents are above their dependencies and every row is one build
//! level: nothing in a row waits for anything else in it. Each dependency
//! kind has its own arrow and each package the look of the most important
//! set it belongs to (`@system`, `@profile`, `@world`, none).
//!
//! `--kinds DEPEND,BDEPEND` (default `all`) draws only those kinds; the rows
//! still come from every dependency, and a package left with no drawn arrow
//! is dropped unless it was asked for.
//!
//! `--reduce` hides an arrow when a longer path between the same two packages
//! already implies it (`a -> b -> c` makes `a -> c` redundant), on the kinds
//! drawn; `--bundle` joins arrows of one kind into the same package.
//!
//! The graph is `portage-world`'s first pass, with its cuts. Further cuts:
//! a set atom claims a package by name and slot only, its version is not
//! compared; no aux-metadata provider is registered, so a repository
//! without `metadata/md5-cache` contributes no dependencies.

mod dot;
mod levels;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use dot::{Node, Set};
use portage_world::{Options, SeedSets, World};

struct Cli {
    opts: Options,
    /// Keep only the packages at most this many dependencies away from a
    /// target (`--depth N`).
    depth: Option<usize>,
    /// Dependency kinds drawn, as bits of `portage_world::DEP_KEYS`
    /// (`--kinds`, default all).
    kinds: u8,
    /// Join the arrows of one kind into the same package (`--bundle`).
    bundle: bool,
    /// Hide arrows a longer path implies (`--reduce`).
    reduce: bool,
}

/// Every bit of `portage_world::DEP_KEYS`.
const ALL_KINDS: u8 = (1 << portage_world::DEP_KEYS.len()) - 1;

/// `all` / `tutte`, or `DEPEND,BDEPEND,...` (any case) as bits of `DEP_KEYS`.
fn parse_kinds(list: &str) -> Result<u8, String> {
    let mut mask = 0;
    for name in list.split(',').map(str::trim) {
        if name.eq_ignore_ascii_case("all") || name.eq_ignore_ascii_case("tutte") {
            return Ok(ALL_KINDS);
        }
        let bit = portage_world::DEP_KEYS
            .iter()
            .position(|k| k.eq_ignore_ascii_case(name))
            .ok_or_else(|| {
                format!(
                    "--kinds: unknown kind {name:?} (all, or any of {})",
                    portage_world::DEP_KEYS.join(",")
                )
            })?;
        mask |= 1 << bit;
    }
    Ok(mask)
}

fn main() {
    let cli = match parse_args() {
        Ok(o) => o,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };
    match run(&cli) {
        Ok(dot) => print!("{dot}"),
        Err(msg) => {
            eprintln!("experiment-tree-diagram: {msg}");
            std::process::exit(1);
        }
    }
}

fn run(cli: &Cli) -> Result<String, String> {
    let opts = &cli.opts;
    let world = portage_world::analyse(opts)?;
    for (atom, _) in &world.unmatched {
        eprintln!("experiment-tree-diagram: no ebuild matches {atom}");
    }
    let sets = portage_world::seed_sets(opts)?;
    let mut nodes = nodes(&world, &sets);
    let mut edges = world.edge_kinds.clone();
    if let Some(depth) = cli.depth {
        let near = within(&targets(&world), &edges, depth);
        nodes.retain(|id, _| near.contains(id));
        edges.retain(|(from, to), _| near.contains(from) && near.contains(to));
    }
    // Levels come from every edge, so a row still means "can be built at the
    // same time" when only some kinds are drawn.
    let levels = levels::levels(&nodes.keys().cloned().collect(), &edges);
    if cli.kinds != ALL_KINDS {
        edges.retain(|_, kinds| *kinds & cli.kinds != 0);
        for kinds in edges.values_mut() {
            *kinds &= cli.kinds;
        }
    }
    // After the kind filter, so no arrow is hidden behind one that is not drawn.
    if cli.reduce {
        levels::reduce(&mut edges, &levels.loose);
    }
    if cli.kinds != ALL_KINDS {
        let ends: BTreeSet<&String> = edges.keys().flat_map(|(a, b)| [a, b]).collect();
        let wanted = targets(&world);
        nodes.retain(|id, _| ends.contains(id) || wanted.contains(id));
    }
    eprintln!(
        "{} packages, {} levels",
        nodes.len(),
        levels.level.values().max().map_or(0, |top| top + 1)
    );
    Ok(dot::render(&nodes, &edges, &levels, cli.bundle))
}

/// The package slots the command line asked for.
fn targets(world: &World) -> BTreeSet<String> {
    world
        .slots
        .iter()
        .filter(|s| {
            s.atoms
                .iter()
                .any(|(_, by)| by.iter().any(|b| b == "@world"))
        })
        .map(portage_world::SlotSet::node)
        .filter(|id| world.reached.contains(id))
        .collect()
}

/// `from` and everything at most `depth` dependencies below it.
fn within(from: &BTreeSet<String>, edges: &levels::Edges, depth: usize) -> BTreeSet<String> {
    let mut seen = from.clone();
    let mut frontier = from.clone();
    for _ in 0..depth {
        frontier = edges
            .keys()
            .filter(|(parent, child)| frontier.contains(parent) && !seen.contains(child))
            .map(|(_, child)| child.clone())
            .collect();
        seen.extend(frontier.iter().cloned());
    }
    seen
}

/// Every package slot the walk reached, labelled with the version it
/// would install.
fn nodes(world: &World, sets: &SeedSets) -> BTreeMap<String, Node> {
    let members = [
        (Set::System, claims(&sets.system)),
        (Set::Profile, claims(&sets.profile)),
        (Set::World, claims(&sets.selected)),
    ];
    let mut labels: BTreeMap<String, String> = BTreeMap::new();
    for slot in &world.slots {
        if let Some(chosen) = slot.chosen() {
            let tail = if slot.slot == "0" {
                String::new()
            } else {
                format!("\n:{}", slot.slot)
            };
            labels.insert(slot.node(), format!("{}-{}{tail}", slot.cp, chosen.version));
        }
    }
    world
        .reached
        .iter()
        .map(|id| {
            let (cp, slot) = id.rsplit_once(':').unwrap_or((id, ""));
            let set = members
                .iter()
                .find(|(_, claimed)| {
                    claimed.contains(&(cp.to_string(), None))
                        || claimed.contains(&(cp.to_string(), Some(slot.to_string())))
                })
                .map_or(Set::Other, |(set, _)| *set);
            let label = labels.get(id).unwrap_or(id).clone();
            (id.clone(), Node { label, set })
        })
        .collect()
}

/// `(cat/pkg, slot if the atom names one)` of every atom of a set.
fn claims(atoms: &BTreeSet<String>) -> BTreeSet<(String, Option<String>)> {
    atoms
        .iter()
        .filter_map(|a| portage_dep::parse_atom(a))
        .map(|a| (format!("{}/{}", a.category, a.package), a.slot))
        .collect()
}

fn parse_args() -> Result<Cli, String> {
    const USAGE: &str = "usage: experiment-tree-diagram [--root DIR] [--config-root DIR] \
                         [--threads N] [--without-bdeps] [--depth N] \
                         [--kinds all|KIND[,KIND]...] [--bundle] [--reduce] ATOM|@SET...";
    let mut opts = Options::new(PathBuf::from("/"), PathBuf::from("/"));
    opts.with_bdeps = true;
    let mut depth = None;
    let mut kinds = ALL_KINDS;
    let mut bundle = false;
    let mut reduce = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match a.as_str() {
            "--root" => opts.root = value("--root")?.into(),
            "--config-root" => opts.config_root = value("--config-root")?.into(),
            "--threads" => {
                opts.threads = value("--threads")?
                    .parse::<usize>()
                    .map_err(|e| format!("--threads: {e}"))?
                    .max(1);
            }
            "--without-bdeps" => opts.with_bdeps = false,
            "--bundle" => bundle = true,
            "--reduce" => reduce = true,
            "--kinds" => kinds = parse_kinds(&value("--kinds")?)?,
            "--depth" => {
                depth = Some(
                    value("--depth")?
                        .parse::<usize>()
                        .map_err(|e| format!("--depth: {e}"))?,
                );
            }
            "-h" | "--help" => return Err(USAGE.into()),
            other if other.starts_with('-') => return Err(format!("unknown argument {other:?}")),
            target => opts.targets.push(target.to_string()),
        }
    }
    if opts.targets.is_empty() {
        return Err(USAGE.into());
    }
    Ok(Cli {
        opts,
        depth,
        kinds,
        bundle,
        reduce,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_all_or_a_comma_separated_list_in_any_case() {
        assert_eq!(parse_kinds("all"), Ok(ALL_KINDS));
        assert_eq!(parse_kinds("tutte"), Ok(ALL_KINDS));
        assert_eq!(parse_kinds("depend, BDEPEND"), Ok(0b01001));
        assert_eq!(parse_kinds("IDEPEND"), Ok(0b10000));
        assert!(parse_kinds("DEPEND,nope").is_err());
    }
}
