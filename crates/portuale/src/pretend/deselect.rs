//! `--deselect`.

use crate::pretend::*;

/// `emerge --deselect <atom-or-bare-name> [...]`: real `action_deselect`
/// (`lib/_emerge/actions.py`). Under `--pretend` this previews (`Would
/// remove ...`); **without** it, after the same preview (`Removing ...`),
/// `run_deselect` rewrites both `var/lib/portage/world` and
/// `var/lib/portage/world_sets` with the discarded entries dropped, each
/// file sorted + one entry per line + comments dropped -- real
/// `world_set.replace(remaining)` -> `WorldSelectedSet.write`. Needs no
/// repo/config resolution at all -- only the world files and the vdb --
/// so this doesn't call `find_repos`/`resolve_config` either, unlike
/// every other real feature in portuale.
///
/// For each target in `targets`: a bare package name (no `/` at all) is
/// expanded via real portage's own "null category" mechanism -- scan the
/// world file's own atoms for one sharing that package name, and
/// substitute in its category (real `Atom(..., category="null")`
/// handling; portuale's own atom parser has no equivalent, so this is
/// a dedicated lookup instead), added to the candidate set directly,
/// **unconditionally, no installed check at all**: confirmed by reading
/// `action_deselect`'s own null-category-substitution loop, which adds
/// the substituted atom to `expanded_atoms` before ever touching the
/// vardb. Real `action_deselect` *does* separately call
/// `vardb.match(atom)` for this same original (still-null-category)
/// atom, but that call can never match a real vardb entry -- no package
/// is ever catalogued under category "null" -- so it's dead code for
/// this branch specifically, and correctly contributes nothing here.
///
/// An *explicit*-category target (already has a `/`) is likewise added
/// to the candidate set directly, with **no installed check at all** --
/// confirmed by reading real portage's own call chain feeding
/// `action_deselect`'s own `atoms` parameter: `action_uninstall`'s own
/// `dep_expand(x, mydb=vardb, ...)` (`lib/portage/dbapi/dep_expand.py`)
/// returns an explicit-category atom completely unchanged, `if
/// mydep.category != "virtual": return mydep`, *before* it ever reaches
/// `cpv_expand` (the vardb-dependent part, only reached for a bare
/// name); `action_deselect` itself then seeds `expanded_atoms =
/// set(atoms)` with that same atom, unconditionally. So `--deselect
/// cat/pkg` (or a bare `pkg` resolvable via the world file) genuinely
/// discards a matching world entry even if never installed -- this
/// portuale's own earlier doc comment (and test) claimed installation was
/// always required, an incorrect generalization: real portage's own
/// vardb-derived narrowing (`vardb.match`) is a *separate, additional*
/// contribution on top of the unconditional substitution/literal-target
/// candidate, for BOTH the bare-name and explicit-category cases -- not
/// a gate on it. For an explicit-category target specifically, that
/// separate vardb contribution (`portage_repo::installed_candidates`,
/// portuale's own vdb scan, via `match_from_list`) still runs and adds
/// a further bare `category/package:slot` candidate (real
/// `Atom(f"{pkg.cp}:{pkg.slot}")`, no version/operator at all) for
/// whatever version(s) are actually installed; for a bare name it's
/// correctly omitted, per the dead-code reasoning above.
///
/// Every candidate atom collected this way -- installed-derived (bare
/// `category/package:slot`) or the literal target/substituted atom
/// (version/operator intact, if given) -- is compared against every
/// world-file entry via real `Atom.intersects()` (`portage_dep::
/// atom_intersects`, see its own doc comment) plus real
/// `action_deselect`'s own separate repo check (`not (arg_atom.repo and
/// not atom.repo)`), replacing portuale's own previous narrower
/// category/package(+slot)-only equality check.
///
/// `--deselect @some-set`: real `action_deselect`'s own combined
/// `world_set` (`WorldSelectedSet`) iterates BOTH `world`'s own plain
/// atoms AND `world_sets`'s own literal `@name` reference *strings* --
/// confirmed by reading `WorldSelectedSet.load`'s own `self._setAtoms(
/// chain(self._pkgset, self._setset))`: a `@name` string fails real
/// `Atom(...)` parsing and lands in `_nonatoms`, so it's carried through
/// *unexpanded*, never resolved into its own member atoms at all.
/// `action_deselect`'s own matching loop confirms this: a `@`-prefixed
/// CLI target can only ever discard a `@`-prefixed `world_set` entry via
/// *exact string equality* (`arg_atom == atom`) -- there is no
/// installed-candidate matching, no member-atom expansion, for either
/// side. So despite `resolve_custom_set`'s own real, working nested-set
/// expansion (built for -- and still only used by -- `@world`'s own
/// dependency-resolution walk, a genuinely different real mechanism,
/// `SetConfig.getSetAtoms`), it has no role here at all: portuale's
/// own equivalent is a plain membership check against `read_world_sets`
/// (`@name` stripped of its own leading `@` for the comparison), nothing
/// more. Each discarded entry is reported against its own real source
/// file (`"world"` for a plain atom, `"world_sets"` for a `@name`
/// reference), matching real `action_deselect`'s own `filename =
/// "world_sets" if str(atom).startswith(SETPREFIX) else "world"` --
/// sorted together into one combined list, not two separate blocks,
/// mirroring real `sorted(discard_atoms, key=str)` exactly.
///
/// Real `action_deselect` always returns `os.EX_OK` on every reachable
/// path here (found matches, no matches, even no targets at all) --
/// ported the same way, unconditionally `ExitCode::SUCCESS`.
pub(crate) fn run_deselect(
    targets: &[&str],
    root: &Path,
    pretend: bool,
    ask: bool,
    color: &Colorizer,
) -> ExitCode {
    let world_atoms = match read_world_atoms(root) {
        Ok(atoms) => atoms,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };
    let world_sets = match read_world_sets(root) {
        Ok(sets) => sets,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };

    let mut expanded: Vec<Atom> = Vec::new();
    // A `@name` target only ever matches a `world_sets` entry by exact
    // name -- see this function's own doc comment -- so it's collected
    // separately, never fed through the atom-expansion/vardb-matching
    // path below at all.
    let mut set_targets: HashSet<&str> = HashSet::new();
    for target in targets {
        if let Some(name) = target.strip_prefix('@') {
            set_targets.insert(name);
            continue;
        }
        if let Some(atom) = parse_atom(target).filter(|_| target.contains('/')) {
            // Explicit-category target: real `expanded_atoms =
            // set(atoms)` seeds with the literal target atom itself,
            // version/operator intact, no installed check at all -- see
            // this function's own doc comment. `vardb.match(atom)`
            // (real `installed_candidates` + `match_from_list` here)
            // separately contributes a bare `category/package:slot`
            // candidate for whatever version(s) are *actually*
            // installed, real `Atom(f"{pkg.cp}:{pkg.slot}")` -- no
            // version/operator at all, regardless of the real installed
            // one.
            expanded.push(atom.clone());
            for (version, slot, _sub_slot) in
                portage_repo::installed_candidates(root, &atom.category, &atom.package)
            {
                let candidate_str = format!("{}/{}-{version}:{slot}", atom.category, atom.package);
                if match_from_list(target, &[candidate_str.as_str()]).is_some_and(|m| !m.is_empty())
                    && let Some(vardb_atom) =
                        parse_atom(&format!("{}/{}:{slot}", atom.category, atom.package))
                {
                    expanded.push(vardb_atom);
                }
            }
        } else if !target.contains('/') {
            // Bare name: null-category substitution, unconditional, no
            // installed check -- see this function's own doc comment.
            for w in world_atoms.iter().filter_map(|w| parse_atom(w)) {
                if w.package != *target {
                    continue;
                }
                if let Some(substituted) = parse_atom(&format!("{}/{}", w.category, target)) {
                    expanded.push(substituted);
                }
            }
        } else {
            eprintln!("emerge: invalid atom {target:?}");
            return ExitCode::from(1);
        }
    }

    // (entry text, source file) pairs -- combined and sorted together as
    // one list, matching real `sorted(discard_atoms, key=str)` (not two
    // separate "world" then "world_sets" blocks).
    let mut discard: Vec<(String, &'static str)> = world_atoms
        .iter()
        .filter(|world_atom_str| {
            let Some(w) = parse_atom(world_atom_str) else {
                return false;
            };
            expanded.iter().any(|arg_atom| {
                portage_dep::atom_intersects(arg_atom, &w)
                    && !(arg_atom.repo.is_some() && w.repo.is_none())
            })
        })
        .map(|s| (s.clone(), "world"))
        .collect();
    discard.extend(
        world_sets
            .iter()
            .filter(|name| set_targets.contains(name.as_str()))
            .map(|name| (format!("@{name}"), "world_sets")),
    );

    if discard.is_empty() {
        println!(">>> No matching atoms found in \"world\" favorites file...");
        return ExitCode::SUCCESS;
    }
    discard.sort_by(|a, b| a.0.cmp(&b.0));
    // Real `action_deselect`: `Would remove` under `--pretend`, `Removing`
    // when it actually writes.
    let verb = if pretend { "Would remove" } else { "Removing" };
    for (entry, filename) in &discard {
        println!(">>> {verb} {entry} from \"{filename}\" favorites file...");
    }

    // Real `action_deselect`: `--ask` prompts once, after the
    // `>>> Removing ...` lines, before either file is rewritten. `No`
    // aborts with `128 + SIGINT`. `--pretend` never rewrites, so the
    // prompt only matters for a real run (real `main.py` drops `--ask`
    // under `-p` entirely).
    if ask
        && !pretend
        && !ask_confirm(
            color,
            "Would you like to remove these packages from your world favorites?",
        )
    {
        return ExitCode::from(130);
    }

    if !pretend {
        // Real `world_set.replace(remaining)` -> `WorldSelectedSet.write`:
        // both files are rewritten, atoms/`@name`s each sorted, one per
        // line, comment lines dropped (exactly `WorldSelectedPackagesSet.
        // write` / `WorldSelectedSetsSet.write`).
        let drop_world: HashSet<&str> = discard
            .iter()
            .filter(|(_, f)| *f == "world")
            .map(|(e, _)| e.as_str())
            .collect();
        let drop_sets: HashSet<&str> = discard
            .iter()
            .filter(|(_, f)| *f == "world_sets")
            .map(|(e, _)| e.as_str())
            .collect();

        let mut remaining_world: Vec<String> = world_atoms
            .into_iter()
            .filter(|a| !drop_world.contains(a.as_str()))
            .collect();
        remaining_world.sort();
        remaining_world.dedup();

        let mut remaining_sets: Vec<String> = world_sets
            .into_iter()
            .map(|n| format!("@{n}"))
            .filter(|s| !drop_sets.contains(s.as_str()))
            .collect();
        remaining_sets.sort();
        remaining_sets.dedup();

        // One transaction: `var/lib/portage` is created once, then
        // `world`, then `world_sets` are written (S1.6).
        let db = portage_vdb::for_root(root);
        let rewritten = db.begin_write().and_then(|mut txn| {
            txn.set_world(&portage_vdb::World {
                atoms: remaining_world,
            })?;
            txn.set_world_sets(&portage_vdb::WorldSets {
                sets: remaining_sets
                    .into_iter()
                    .map(|s| s.strip_prefix('@').unwrap_or(&s).to_string())
                    .collect(),
            })?;
            txn.commit()
        });
        if let Err(e) = rewritten {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    }
    ExitCode::SUCCESS
}
