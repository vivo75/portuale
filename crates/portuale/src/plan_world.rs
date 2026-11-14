//! `mrg --plan-world[=update|emptytree] [--sort alpha|depend|resolver]`: what
//! `emerge -uDNp @world` (or, with `emptytree`, `emerge -pe @world`) would
//! install, worked out by `portage-world` instead of the resolver. One
//! row per slot whose best visible version is not the installed one, and an `R`
//! row for a kept package whose recorded `:slot/sub-slot=` dependency the plan
//! breaks or that is already stale (with `plan-world-newuse`, also one whose
//! USE changed). A stale binding the plan cannot repair is named on stderr
//! instead of gaining a row. It sees no blockers
//! and no `REQUIRED_USE`, so it can list less or more than the resolver; `--sort resolver` shows the
//! resolver's own order for the same rows. `--sort depend` puts a package
//! after what it needs before it is merged or usable: a runtime edge out of
//! a package the plan builds with still orders (PMS 8.1, backlog #746), and
//! a cycle with no cuttable edge is reported, never papered over
//! (charter 2a, backlog #749) — except the charter 2a bootstrap, which
//! serializes the one shape a single USE-flag reduction breaks (backlog
//! #753): the boot row (reduced USE, temporary, marked) plus the definitive
//! rebuild with the declared USE. A `PDEPEND` orders nothing (PMS). One row
//! per slot, always, plus the boot row when a cycle bootstraps.

use crate::pretend::json::json_string as q;
use clap::ArgMatches;
use portage_use_reduce::BootstrapUse;
use portage_world::{Change, Order, SlotSet, World};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::process::{Command, ExitCode};

/// Prints the rows and returns 0, or 2 when the tree cannot be read or the
/// resolver order cannot be had.
pub(crate) fn run(matches: &ArgMatches) -> ExitCode {
    let (mut opts, future) = match crate::check_world::options(matches, "plan-world") {
        Ok(setup) => setup,
        Err(code) => return code,
    };
    let mode = match Mode::from_arg(matches) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("plan-world: {message}");
            return ExitCode::from(2);
        }
    };
    opts.rebuilds = true;
    opts.newuse = future.on("plan-world-newuse");
    opts.emptytree = mode == Mode::Emptytree;
    opts.with_bdeps |= opts.emptytree;
    let sort = matches
        .get_one::<String>("sort")
        .map_or("alpha", String::as_str);
    let (root, config_root) = (opts.root.clone(), opts.config_root.clone());
    let world = match portage_world::analyse(&opts) {
        Ok(w) => w,
        Err(message) => {
            eprintln!("plan-world: {message}");
            return ExitCode::from(2);
        }
    };
    let ranks;
    let order = match sort {
        "depend" => Order::Depend,
        "resolver" => match resolver_ranks(&world, &root, &config_root) {
            Ok(r) => {
                ranks = r;
                Order::Rank(&ranks)
            }
            Err(message) => {
                eprintln!("plan-world: {message}");
                return ExitCode::from(2);
            }
        },
        _ => Order::Alpha,
    };
    let rows = world.ordered(&order);
    // Charter 2a bootstrap (backlog #753): when a single USE-flag
    // reduction breaks a reported build-time cycle, the plan carries the
    // boot row (reduced USE, temporary) plus the definitive rebuild with
    // the declared USE. The report line below then names the reduction;
    // the checker accepts only the complete pair. Anything the reduction
    // does not break keeps today's shape: single rows plus the bare
    // report line.
    let cycles = portage_world::build_time_cycles(&world.edges, &world.edge_kinds);
    let bootstrap = select_bootstrap(&world, &cycles);
    for cycle in &cycles {
        let mut chain = cycle.to_vec();
        if let Some(first) = cycle.first() {
            chain.push(first.clone());
        }
        let mut line = format!(
            "plan-world: build cycle with no serial order: {} (breakable only by a bootstrap variant with reduced USE; see backlog #753",
            chain.join(" -> ")
        );
        if let Some((boot, flag)) = bootstrap.as_ref()
            && cycle.iter().any(|m| {
                m == boot
                    && world.use_guards.iter().any(|((from, to), flags)| {
                        from == boot && cycle.contains(to) && flags.contains(flag)
                    })
            })
        {
            line.push_str(&format!(
                "; bootstrap with USE=\"-{flag}\" {boot} temporary — will be rebuilt with the declared USE"
            ));
        }
        line.push(')');
        eprintln!("{line}");
    }
    let final_rows = apply_bootstrap(&world, &rows, bootstrap.as_ref());
    if matches.get_flag("json") {
        println!("{}", plan_json(&world, &final_rows, &root));
    } else {
        for row in &final_rows {
            println!("{}", text_row(&world, row));
        }
    }
    let mut slots: BTreeSet<String> = BTreeSet::new();
    for row in &final_rows {
        slots.insert(row.slot.node());
    }
    let boot_rows = final_rows.iter().filter(|r| r.bootstrap.is_some()).count();
    let unread = match world.unreadable.len() {
        0 => String::new(),
        n => format!(", {n} not read"),
    };
    eprintln!(
        "plan-world: {} to install ({} builds including {boot_rows} bootstrap), order {sort}{unread}",
        slots.len(),
        final_rows.len()
    );
    eprintln!("{}", not_checked(mode, opts.newuse));
    if !world.refused.is_empty() {
        eprintln!("{}", refused_line(&world));
    }
    ExitCode::SUCCESS
}

/// One row of the final plan: the slot plus, on the boot row only, the
/// reduction override. The boot row renders the same slot with the reduced
/// flag forced off; the definitive row carries the declared USE and no
/// marker.
struct PlanRow<'a> {
    slot: &'a SlotSet,
    bootstrap: Option<BootstrapUse>,
}

/// The charter 2a bootstrap over this world's build-time cycles, if one
/// single-flag reduction applies: `(boot slot, reduced flag)`. The caller
/// keeps today's single-rows-plus-report shape on `None` (STOP rules 1–3:
/// no single-flag reduction, a boot that still needs a member, more than
/// 3 affected rows, or no recorded USE to reduce from).
fn select_bootstrap(world: &World, cycles: &[Vec<String>]) -> Option<(String, String)> {
    if cycles.is_empty() {
        return None;
    }
    let (boot, flag) = portage_use_reduce::bootstrap_reduction(
        cycles,
        &world.edges,
        &world.edge_kinds,
        &world.use_guards,
    )?;
    let members: BTreeSet<String> = cycles.iter().flatten().cloned().collect();
    if members.len() + 1 > 3
        || portage_use_reduce::boot_closure_reaches_member(
            &world.edges,
            &world.edge_kinds,
            &world.use_guards,
            &boot,
            &flag,
            &members,
        )
        || !world.planned_use.contains_key(&boot)
    {
        return None;
    }
    Some((boot, flag))
}

/// The final rows: the ordered rows with, when bootstrapping, the boot
/// row (same slot, reduced USE) taking the earliest of the boot slot's own
/// position and its transitive dependents, and the definitive entry moving
/// to immediately after the last entry that transitively build-depends on
/// the boot slot (reduced build graph).
fn apply_bootstrap<'a>(
    world: &World,
    rows: &[&'a SlotSet],
    bootstrap: Option<&(String, String)>,
) -> Vec<PlanRow<'a>> {
    let Some((boot_node, flag)) = bootstrap else {
        return rows
            .iter()
            .map(|slot| PlanRow {
                slot,
                bootstrap: None,
            })
            .collect();
    };
    let Some(boot_pos) = rows.iter().position(|s| s.node() == *boot_node) else {
        return rows
            .iter()
            .map(|slot| PlanRow {
                slot,
                bootstrap: None,
            })
            .collect();
    };
    // The reduced graph: the boot slot's flag-guarded outgoing edges are gone.
    let reduced = |from: &str, to: &str| {
        world
            .use_guards
            .get(&(from.to_string(), to.to_string()))
            .is_none_or(|flags| !flags.contains(flag))
    };
    // Every row that transitively build-depends on the boot slot.
    let reaches_boot = |start: &str| {
        let mut seen: BTreeSet<String> = BTreeSet::from([start.to_string()]);
        let mut stack = vec![start.to_string()];
        while let Some(node) = stack.pop() {
            if node == *boot_node {
                return true;
            }
            if let Some(to) = world.edges.get(&node) {
                for next in to {
                    if reduced(&node, next) && seen.insert(next.clone()) {
                        stack.push(next.clone());
                    }
                }
            }
        }
        false
    };
    let mut dependents: BTreeSet<usize> = BTreeSet::new();
    for (i, slot) in rows.iter().enumerate() {
        if i != boot_pos && reaches_boot(&slot.node()) {
            dependents.insert(i);
        }
    }
    let shift = |i: usize| if i > boot_pos { i - 1 } else { i };
    let dep_pos: BTreeSet<usize> = dependents.iter().map(|&i| shift(i)).collect();
    let declared: Vec<String> = world
        .planned_use
        .get(boot_node)
        .map(|use_set| {
            let mut declared: Vec<String> = use_set.iter().cloned().collect();
            declared.sort();
            declared
        })
        .unwrap_or_default();
    let boot_use = BootstrapUse {
        reduced_flag: flag.clone(),
        declared_use: declared,
    };
    let first = dep_pos.iter().copied().min().unwrap_or(shift(boot_pos));
    let boot_at = first.min(shift(boot_pos));
    let mut out: Vec<PlanRow<'a>> = Vec::with_capacity(rows.len() + 1);
    for (i, slot) in rows.iter().enumerate() {
        if i != boot_pos {
            out.push(PlanRow {
                slot,
                bootstrap: None,
            });
        }
    }
    out.insert(
        boot_at,
        PlanRow {
            slot: rows[boot_pos],
            bootstrap: Some(boot_use),
        },
    );
    let last_dep = dep_pos
        .iter()
        .copied()
        .max()
        .map(|p| if p >= boot_at { p + 1 } else { p });
    let definitive_at = last_dep.map_or(out.len(), |p| p + 1).min(out.len());
    out.insert(
        definitive_at,
        PlanRow {
            slot: rows[boot_pos],
            bootstrap: None,
        },
    );
    out
}

/// Which plan `--plan-world` works out.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// What `emerge -uDNp @world` would change.
    Update,
    /// Every slot `@world` reaches, as `emerge -pe @world` lists them.
    Emptytree,
}

impl Mode {
    /// The mode named by `--plan-world[=mode]`.
    fn from_arg(matches: &ArgMatches) -> Result<Mode, String> {
        match matches.get_one::<String>("plan_world").map(String::as_str) {
            Some("update") => Ok(Mode::Update),
            Some("emptytree") => Ok(Mode::Emptytree),
            other => Err(format!(
                "--plan-world={}: the modes are `update` and `emptytree`",
                other.unwrap_or_default()
            )),
        }
    }

    /// The `mrg` command that gives the full plan for this mode.
    fn full_plan(self) -> &'static str {
        match self {
            Mode::Update => "mrg -puDN @world",
            Mode::Emptytree => "mrg -pe @world",
        }
    }
}

/// The second summary line: what the plan did not look at, and the command
/// that does. A changed USE only needs naming for `update`: `emptytree`
/// builds everything anyway.
fn not_checked(mode: Mode, newuse: bool) -> String {
    let use_rebuilds = if mode == Mode::Update && !newuse {
        ", USE rebuilds"
    } else {
        ""
    };
    format!(
        "not checked: blockers, REQUIRED_USE, slot conflicts{use_rebuilds}; run '{}' for the full plan",
        mode.full_plan()
    )
}

/// The plan as the one-line document `emerge -pv --json` prints and the
/// health checker reads: the rows as `entries` in order, and every
/// installed package they do not replace as `kept` (the checker reads their
/// dependencies from the VDB).
fn plan_json(world: &World, rows: &[PlanRow<'_>], root: &Path) -> String {
    let entries: Vec<String> = rows
        .iter()
        .enumerate()
        .filter_map(|(order, row)| entry_json(world, row.slot, order, row.bootstrap.as_ref()))
        .collect();
    let kept: Vec<String> = world
        .kept_installed(root)
        .iter()
        .map(|(_, p)| {
            format!(
                "{{\"category\":{},\"package\":{},\"version\":{},\"slot\":{}}}",
                q(&p.category),
                q(&p.package),
                q(&p.version),
                q(&p.slot)
            )
        })
        .collect();
    let stale: Vec<String> = world
        .refused
        .iter()
        .map(|r| {
            let provider = r.provider.as_deref().map_or("null".to_string(), q);
            format!(
                "{{\"consumer\":{},\"atom\":{},\"provider\":{},\"reason\":{}}}",
                q(&r.consumer),
                q(&r.atom),
                provider,
                q(&refusal_reason(r))
            )
        })
        .collect();
    // The checker reads `entries` and `kept`; `stale-bindings` rides along
    // only when a binding is actually unrepaired, so output without one is
    // byte-identical to a plan that never knew the field.
    let stale = if stale.is_empty() {
        String::new()
    } else {
        format!("\"stale-bindings\":[{}],", stale.join(","))
    };
    format!(
        "{{\"entries\":[{}],\"kept\":[{}],{}\"request\":[\"@world\"],\"aborted\":null}}",
        entries.join(","),
        kept.join(","),
        stale
    )
}

/// One `entries` object of [`plan_json`]; `None` for a slot with no chosen
/// version (it has no row either). One entry per row, in row order; a
/// rebuilt slot is one row like any other. The boot row carries the
/// `bootstrap` marker and the reduced USE; every other row is unchanged.
fn entry_json(
    world: &World,
    slot: &SlotSet,
    order: usize,
    bootstrap: Option<&BootstrapUse>,
) -> Option<String> {
    let chosen = slot.chosen()?;
    let (category, package) = slot.cp.split_once('/')?;
    let (outcome, from) = match (world.change(slot)?, slot.installed.as_deref()) {
        (Change::New, _) => ("new", None),
        (Change::Rebuild, _) => ("reinstall", None),
        (Change::Upgrade, have) => ("upgrade", have),
        (Change::Downgrade, have) => ("downgrade", have),
    };
    let mut fields = vec![
        format!("\"category\":{}", q(category)),
        format!("\"package\":{}", q(package)),
        format!("\"version\":{}", q(&chosen.version)),
        format!("\"slot\":{}", q(&slot.slot)),
        format!("\"sub_slot\":{}", q(&chosen.sub_slot)),
        format!("\"outcome\":{}", q(outcome)),
        format!("\"merge_order\":{order}"),
        "\"source\":\"ebuild\"".to_string(),
    ];
    if let Some(boot) = bootstrap {
        fields.push("\"bootstrap\":true".to_string());
        fields.push(format!(
            "\"reduced_use\":[{}]",
            q(&format!("-{}", boot.reduced_flag))
        ));
    }
    if let Some(from) = from {
        fields.push(format!("\"from_version\":{}", q(from)));
    }
    let pf = format!("{package}-{}", chosen.version);
    if let Ok(metadata) = portage_repo::repo_aux_metadata(&chosen.repo_location, category, &pf) {
        let mut use_flags: BTreeSet<String> = world
            .planned_use
            .get(&slot.node())
            .cloned()
            .unwrap_or_default();
        if let Some(boot) = bootstrap {
            use_flags.remove(&boot.reduced_flag);
        }
        let input = portage_repo::DepInput {
            metadata,
            use_flags: use_flags.into_iter().collect(),
        };
        fields.extend(crate::pretend::json::dep_input_fields(&input));
    }
    Some(format!("{{{}}}", fields.join(",")))
}

/// Why one stale slot-operator binding stays unrepaired: the recorded atom
/// names a provider version the plan leaves behind, or the consumer has no
/// visible tree candidate.
fn refusal_reason(refusal: &portage_world::RefusedBinding) -> String {
    match &refusal.provider {
        Some(provider) => format!("not satisfied by {provider}"),
        None => "no visible ebuild".to_string(),
    }
}

/// The property-5 line for the stale bindings the plan does not repair, each
/// named once with its atom and reason. Empty means byte-identical output.
fn refused_line(world: &World) -> String {
    let parts: Vec<String> = world
        .refused
        .iter()
        .map(|r| match &r.provider {
            Some(provider) => format!("{} ({} not satisfied by {provider})", r.consumer, r.atom),
            None => format!("{} ({}, no visible ebuild)", r.consumer, r.atom),
        })
        .collect();
    format!(
        "plan-world: {} stale slot-operator binding(s) not rebuilt: {}",
        parts.len(),
        parts.join(", ")
    )
}

/// `[ebuild  N     ] cat/pkg-ver:slot`, with the installed version of an
/// upgrade or downgrade after it. The boot row carries the
/// `(bootstrap: … temporary …)` suffix (property 5: the user is told);
/// every other row is unchanged.
fn text_row(world: &World, row: &PlanRow<'_>) -> String {
    let slot = row.slot;
    let Some(chosen) = slot.chosen() else {
        return String::new();
    };
    let (mark, old) = match (world.change(slot), slot.installed.as_deref()) {
        (Some(Change::New), _) => ("  N     ", String::new()),
        (Some(Change::Rebuild), _) => ("   R    ", String::new()),
        (Some(Change::Upgrade), Some(have)) => ("     U  ", format!(" [{have}]")),
        (_, Some(have)) => ("      D ", format!(" [{have}]")),
        _ => ("        ", String::new()),
    };
    let suffix = match &row.bootstrap {
        Some(boot) => format!(
            " (bootstrap: USE=\"-{}\" temporary — will be rebuilt with the declared USE)",
            boot.reduced_flag
        ),
        None => String::new(),
    };
    format!(
        // match-str-ok: a display row (cat/pkg-ver:slot), not a match string
        "[ebuild{mark}] {}-{}:{}{old}{suffix}",
        slot.cp, chosen.version, slot.slot
    )
}

/// The position of each slot of `world` in the resolver's merge list for
/// `-puDN @world`, by running the resolver as a child process: it is slow
/// (seconds to a minute) and this program must not link its state.
fn resolver_ranks(
    world: &World,
    root: &std::path::Path,
    config_root: &std::path::Path,
) -> Result<HashMap<String, usize>, String> {
    let out = Command::new(crate::self_exe::self_exe())
        .args([
            "emerge",
            "--pretend",
            "--update",
            "--deep",
            "--newuse",
            "--getbinpkg=n",
            "--color",
            "n",
            "@world",
        ])
        .env("ROOT", root)
        .env("PORTAGE_CONFIGROOT", config_root)
        .output()
        .map_err(|e| format!("cannot run the resolver: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let by_row = parse_plan_rows(&text);
    if by_row.is_empty() && !out.status.success() {
        return Err(format!(
            "the resolver failed (exit {}):\n{}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let mut ranks = HashMap::new();
    for slot in &world.slots {
        let rank = by_row.get(&slot.node()).or_else(|| by_row.get(&slot.cp));
        if let Some(rank) = rank {
            ranks.insert(slot.node(), *rank);
        }
    }
    Ok(ranks)
}

/// `cat/pkg:slot` (or just `cat/pkg` when the row names no slot) -> its
/// position among the `[ebuild`/`[binary` rows of a plan.
fn parse_plan_rows(text: &str) -> HashMap<String, usize> {
    let mut rows = HashMap::new();
    for line in text.lines() {
        let Some(rest) = line
            .strip_prefix("[ebuild")
            .or_else(|| line.strip_prefix("[binary"))
        else {
            continue;
        };
        let Some((_, rest)) = rest.split_once("] ") else {
            continue;
        };
        let token = rest.split_whitespace().next().unwrap_or_default();
        let token = token.split("::").next().unwrap_or(token);
        let (cpv, slot) = match token.split_once(':') {
            Some((cpv, slot)) => (cpv, slot.split('/').next()),
            None => (token, None),
        };
        let Some((category, pf)) = cpv.split_once('/') else {
            continue;
        };
        let Some((package, _version)) = portage_repo::split_pf(pf) else {
            continue;
        };
        let key = match slot {
            Some(slot) => format!("{category}/{package}:{slot}"),
            None => format!("{category}/{package}"),
        };
        let next = rows.len();
        rows.entry(key).or_insert(next);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::parse_plan_rows;
    use crate::check_world::tests::{
        add_ebuild, add_installed, mrg_on_fixtures, run_on_copy, run_on_copy_keeping_tree,
        run_on_copy_with_stderr,
    };
    use std::path::Path;

    /// The package of every row `--plan-world --sort <sort>` prints on the fixtures.
    fn rows(sort: &str) -> Vec<String> {
        let (rc, out) = mrg_on_fixtures(&["--plan-world", "--sort", sort], &[]);
        assert_eq!(rc, 0, "{out}");
        out.lines()
            .map(|l| {
                l.split("] ")
                    .nth(1)
                    .unwrap()
                    .split([' ', ':'])
                    .next()
                    .unwrap()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn alpha_lists_what_would_change_by_name() {
        assert_eq!(
            rows("alpha"),
            [
                "dev-libs/innernestedsetpkg-1.0",
                "dev-libs/newpkg-1.0",
                "dev-libs/upgradepkg-2.0",
                "dev-libs/withdeps-1.0",
            ]
        );
    }

    #[test]
    fn depend_puts_a_package_after_what_it_needs() {
        // withdeps needs newpkg and upgradepkg.
        let r = rows("depend");
        let at = |p: &str| r.iter().position(|x| x == p).unwrap();
        assert!(
            at("dev-libs/newpkg-1.0") < at("dev-libs/withdeps-1.0"),
            "{r:?}"
        );
        assert!(
            at("dev-libs/upgradepkg-2.0") < at("dev-libs/withdeps-1.0"),
            "{r:?}"
        );
        assert_eq!(r.len(), 4);
    }

    /// The value of the first `"key":"..."` in `text` (no escapes in fixtures).
    fn field(text: &str, key: &str) -> String {
        let marker = format!("\"{key}\":\"");
        let start = text
            .find(&marker)
            .unwrap_or_else(|| panic!("no {key} in {text}"))
            + marker.len();
        text[start..].split('"').next().unwrap().to_string()
    }

    /// Charter property 1: `--json` is the same plan as the text, row for
    /// row, in the `emerge --json` shape the health checker reads. An
    /// acyclic plan builds every slot once: no second `built_twice` entry
    /// and no repeated `merge_order` (charter 2a never fires here, so the
    /// document is byte-identical with or without the 2a rule).
    #[test]
    fn json_rows_equal_text_rows_in_order() {
        let (rc, out) = mrg_on_fixtures(&["--plan-world", "--json", "--sort", "depend"], &[]);
        assert_eq!(rc, 0, "{out}");
        let line = out
            .lines()
            .find(|l| l.starts_with("{\"entries\":"))
            .expect(&out);
        let entries = line.split("],\"kept\":").next().unwrap();
        assert!(
            !entries.contains("\"built_twice\""),
            "no second build on the acyclic fixtures:\n{out}"
        );
        let json: Vec<String> = entries
            .split("{\"category\":")
            .skip(1)
            .enumerate()
            .map(|(i, e)| {
                let e = format!("{{\"category\":{e}");
                assert!(e.contains(&format!("\"merge_order\":{i}")), "{e}");
                format!(
                    "{}/{}-{}",
                    field(&e, "category"),
                    field(&e, "package"),
                    field(&e, "version")
                )
            })
            .collect();
        assert_eq!(json, rows("depend"));
    }

    #[test]
    fn resolver_order_is_the_resolvers_own() {
        assert_eq!(
            rows("resolver"),
            [
                "dev-libs/newpkg-1.0",
                "dev-libs/upgradepkg-2.0",
                "dev-libs/innernestedsetpkg-1.0",
                "dev-libs/withdeps-1.0",
            ]
        );
    }

    /// A provider upgraded across a sub-slot (0/1 to 0/2), a consumer that
    /// recorded `:0/1=` and builds with a tool, one that recorded `:*`.
    fn sub_slot_tree(root: &Path) {
        add_ebuild(root, "dev-libs/w693prov-2", "0/2", &[]);
        add_installed(root, "dev-libs/w693prov-1", "0/1", &[]);
        add_ebuild(root, "dev-libs/w693tool-1", "0", &[]);
        let cons = [
            ("RDEPEND", "dev-libs/w693prov:="),
            ("DEPEND", "dev-libs/w693tool"),
        ];
        add_ebuild(root, "dev-libs/w693cons-1", "0", &cons);
        add_installed(
            root,
            "dev-libs/w693cons-1",
            "0",
            &[("RDEPEND", "dev-libs/w693prov:0/1=")],
        );
        add_ebuild(
            root,
            "dev-libs/w693star-1",
            "0",
            &[("RDEPEND", "dev-libs/w693prov:*")],
        );
        add_installed(
            root,
            "dev-libs/w693star-1",
            "0",
            &[("RDEPEND", "dev-libs/w693prov:*")],
        );
    }

    /// PMS `dependencies.tex`, `sec:slot-dep`: `:=` breaks when the
    /// sub-slot of the provider changes, `:*` does not. The rebuilt consumer
    /// is built, so its `DEPEND` is followed and ordered too.
    #[test]
    fn a_slot_operator_consumer_is_rebuilt_after_its_provider() {
        let world = ["dev-libs/w693cons", "dev-libs/w693star"];
        let (rc, out) = run_on_copy(
            &["--plan-world", "--sort", "depend"],
            &world,
            &[],
            "",
            sub_slot_tree,
        );
        assert_eq!(rc, 0, "{out}");
        let rows: Vec<&str> = out.lines().filter(|l| l.contains("w693")).collect();
        let at = |what: &str| rows.iter().position(|r| r.ends_with(what)).expect(&out);
        assert_eq!(rows.len(), 3, "{out}");
        assert!(
            rows.contains(&"[ebuild   R    ] dev-libs/w693cons-1:0"),
            "{out}"
        );
        assert!(
            rows.contains(&"[ebuild  N     ] dev-libs/w693tool-1:0"),
            "{out}"
        );
        assert!(at("w693prov-2:0 [1]") < at("w693cons-1:0"), "{out}");
        assert!(at("w693tool-1:0") < at("w693cons-1:0"), "{out}");
    }

    /// The `health_checker.py` of the sibling pmtest checkout, through the
    /// `fixtures` symlink: the plan `--json` below is judged with the copy's
    /// own VDB, the way the L0 bed judges the host plan.
    fn health_checker() -> std::path::PathBuf {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        fixtures
            .canonicalize()
            .expect("the fixtures symlink resolves")
            .join("../pytests-contract-suite/health_checker.py")
    }

    /// No `build-order` failure in the checker's verdict on the `--json` plan
    /// `out`, read with the VDB at `vdb`: no finding the plan introduces
    /// (a finding already true of the installed tree is `pre-existing` and
    /// not the plan's fault, as in pmtest `test_plan_world.py`). Returns the
    /// verdict for tests that also count its notes.
    fn assert_no_build_order(checker_out: &str, vdb: &Path, plan: &Path) -> String {
        let result = std::process::Command::new("python3")
            .arg(checker_out)
            .arg("--vdb")
            .arg(vdb)
            .arg(plan)
            .output()
            .expect("python3 runs the checker");
        let report = String::from_utf8_lossy(&result.stdout);
        let check = std::process::Command::new("python3")
            .arg("-c")
            .arg(
                "import json,sys\n\
                 record=json.loads(sys.argv[1])\n\
                 bad=[f for f in record['findings'] if f['failure'] and not f['pre_existing']]\n\
                 order=[f for f in bad if f['property']=='build-order']\n\
                 print(f\"{len(bad)} introduced failures, {len(order)} build-order\")\n\
                 [print(f['property'], f['package'], f['reason'][:200]) for f in bad]\n\
                 sys.exit(1 if bad else 0)\n",
            )
            .arg(report.as_ref())
            .output()
            .expect("python3 filters the verdict");
        assert!(
            check.status.success(),
            "checker failures for {}:\n{}",
            plan.display(),
            String::from_utf8_lossy(&check.stdout)
        );
        report.into_owned()
    }

    /// The #746 shape: `w746auto` builds with the kept `w746help`, whose
    /// `RDEPEND` needs the rebuilt `w746loc`; `w746get` and the new `w746icu`
    /// close the cycle back to `w746auto`. The installed `w746loc` records a
    /// stale `:=` on `w746prov` (installed `0/2`, recorded `0/1=`), so the
    /// plan rebuilds it without moving the provider (backlog #741).
    fn kept_middle_tree(root: &Path) {
        add_ebuild(
            root,
            "dev-libs/w746auto-1",
            "0",
            &[("BDEPEND", "dev-libs/w746help")],
        );
        add_ebuild(
            root,
            "dev-libs/w746help-1",
            "0",
            &[("RDEPEND", "dev-libs/w746loc")],
        );
        add_ebuild(
            root,
            "dev-libs/w746loc-1",
            "0",
            &[("RDEPEND", "dev-libs/w746get dev-libs/w746prov")],
        );
        add_ebuild(
            root,
            "dev-libs/w746get-1",
            "0",
            &[("RDEPEND", "dev-libs/w746icu")],
        );
        add_ebuild(
            root,
            "dev-libs/w746icu-1",
            "0",
            &[("BDEPEND", "dev-libs/w746auto")],
        );
        add_ebuild(root, "dev-libs/w746prov-2", "0/2", &[]);
        add_installed(
            root,
            "dev-libs/w746help-1",
            "0",
            &[("RDEPEND", "dev-libs/w746loc")],
        );
        add_installed(
            root,
            "dev-libs/w746loc-1",
            "0",
            &[("RDEPEND", "dev-libs/w746get dev-libs/w746prov:0/1=")],
        );
        add_installed(
            root,
            "dev-libs/w746get-1",
            "0",
            &[("RDEPEND", "dev-libs/w746icu")],
        );
        add_installed(root, "dev-libs/w746prov-2", "0/2", &[]);
    }

    /// Charter property 2 (PMS 8.1: a build tool must be usable, backlog
    /// #746): the repaired provider is ordered before the package that builds
    /// with the tool needing it, and the checker reports no `build-order`.
    #[test]
    fn a_cycle_through_a_kept_package_orders_the_repaired_provider_first() {
        let world = ["dev-libs/w746auto"];
        let (rc, out) = run_on_copy(
            &["--plan-world", "--sort", "depend"],
            &world,
            &[],
            "",
            kept_middle_tree,
        );
        assert_eq!(rc, 0, "{out}");
        let rows: Vec<&str> = out.lines().filter(|l| l.contains("w746")).collect();
        let at = |what: &str| rows.iter().position(|r| r.ends_with(what)).expect(&out);
        assert_eq!(rows.len(), 3, "{out}");
        assert!(at("w746loc-1:0") < at("w746auto-1:0"), "{out}");
        let (tmp, rc, out, _) = run_on_copy_keeping_tree(
            &["--plan-world", "--json", "--sort", "depend"],
            &world,
            &[],
            "",
            kept_middle_tree,
        );
        assert_eq!(rc, 0, "{out}");
        let plan = tmp.path().join("plan.json");
        std::fs::write(&plan, out.trim()).expect("plan file");
        let checker = health_checker();
        assert_no_build_order(
            checker.to_str().expect("checker path"),
            &tmp.path().join("root/var/db/pkg"),
            &plan,
        );
    }

    /// Charter property 2a (backlog #753): the owner's USE-conditional pair
    /// bootstraps to three rows — the reduced-USE boot build, the middle,
    /// the definitive rebuild with the declared USE — with the `bootstrap`
    /// marker and suffix on the boot row only, rc 0, and the report line
    /// naming the reduction. The failure the checker reports on the same
    /// plan without the bootstrap is still pinned in pmtest
    /// (`test_use_conditional_build_cycle_fails`).
    #[test]
    fn a_genuine_build_cycle_bootstraps_with_a_marked_boot_row() {
        fn genuine_cycle_tree(root: &Path) {
            add_ebuild(
                root,
                "dev-libs/w749pkgnew-1",
                "0",
                &[
                    ("IUSE", "+usefoo +use3"),
                    ("BDEPEND", "usefoo? ( dev-libs/w749otherpkg[useblah] )"),
                ],
            );
            add_ebuild(
                root,
                "dev-libs/w749otherpkg-1",
                "0",
                &[
                    ("IUSE", "+useboh +useblah"),
                    ("BDEPEND", "useboh? ( dev-libs/w749pkgnew[use3] )"),
                ],
            );
        }
        let world = ["dev-libs/w749pkgnew", "dev-libs/w749otherpkg"];
        let (rc, out, err) = run_on_copy_with_stderr(
            &["--plan-world", "--sort", "depend"],
            &world,
            &[],
            "",
            genuine_cycle_tree,
        );
        assert_eq!(rc, 0, "{out}\n{err}");
        let rows: Vec<&str> = out.lines().filter(|l| l.contains("w749")).collect();
        assert_eq!(
            rows,
            [
                "[ebuild  N     ] dev-libs/w749otherpkg-1:0 (bootstrap: USE=\"-useboh\" temporary — will be rebuilt with the declared USE)",
                "[ebuild  N     ] dev-libs/w749pkgnew-1:0",
                "[ebuild  N     ] dev-libs/w749otherpkg-1:0",
            ],
            "{out}"
        );
        assert!(
            err.contains("plan-world: build cycle with no serial order: dev-libs/w749otherpkg:0 -> dev-libs/w749pkgnew:0 -> dev-libs/w749otherpkg:0 (breakable only by a bootstrap variant with reduced USE; see backlog #753; bootstrap with USE=\"-useboh\" dev-libs/w749otherpkg:0 temporary — will be rebuilt with the declared USE)"),
            "{err}"
        );
        assert!(
            err.contains("builds including 1 bootstrap), order depend"),
            "{err}"
        );
        let (tmp, rc, out, _) = run_on_copy_keeping_tree(
            &["--plan-world", "--json", "--sort", "depend"],
            &world,
            &[],
            "",
            genuine_cycle_tree,
        );
        assert_eq!(rc, 0, "{out}");
        let plan = tmp.path().join("plan.json");
        std::fs::write(&plan, out.trim()).expect("plan file");
        let text = std::fs::read_to_string(&plan).expect("plan file");
        assert_eq!(
            text.match_indices("\"package\":\"w749pkgnew\"").count(),
            1,
            "{text}"
        );
        assert_eq!(
            text.match_indices("\"package\":\"w749otherpkg\"").count(),
            2,
            "{text}"
        );
        assert_eq!(
            text.match_indices("\"bootstrap\":true").count(),
            1,
            "{text}"
        );
        assert!(text.contains("\"reduced_use\":[\"-useboh\"]"), "{text}");
        // merge_order stays array-positional across the inserted boot row.
        assert!(text.contains("\"merge_order\":0"), "{text}");
        assert!(text.contains("\"merge_order\":1"), "{text}");
        assert!(text.contains("\"merge_order\":2"), "{text}");
        // The boot row's USE is the declared set minus the reduced flag;
        // the definitive row carries the declared USE and no marker.
        let mut boot_uses = Vec::new();
        let mut definitive_uses = Vec::new();
        for entry in text.split("{\"category\":").skip(1) {
            if !entry.contains("\"package\":\"w749otherpkg\"") {
                continue;
            }
            let start = entry.find("\"use\":[").expect("a use list") + "\"use\":[".len();
            let end = entry[start..].find(']').expect("its end") + start;
            let uses = &entry[start..end];
            if entry.contains("\"bootstrap\":true") {
                boot_uses.push(uses.to_string());
            } else {
                definitive_uses.push(uses.to_string());
            }
        }
        assert_eq!(boot_uses.len(), 1, "{text}");
        assert_eq!(definitive_uses.len(), 1, "{text}");
        assert!(
            boot_uses[0].contains("\"useblah\"") && !boot_uses[0].contains("useboh"),
            "{text}"
        );
        assert!(
            definitive_uses[0].contains("\"useblah\"") && definitive_uses[0].contains("\"useboh\""),
            "{text}"
        );
    }

    /// The P-proof for the 2a marker: an acyclic plan carries no `bootstrap`
    /// key, no `reduced_use` key and no boot suffix anywhere — the marker
    /// is byte-identical-by-construction, confirmed again by the S3 suite run.
    #[test]
    fn an_acyclic_plan_carries_no_bootstrap_key() {
        let (rc, out) = mrg_on_fixtures(&["--plan-world", "--json", "--sort", "depend"], &[]);
        assert_eq!(rc, 0, "{out}");
        assert!(!out.contains("bootstrap"), "{out}");
        assert!(!out.contains("reduced_use"), "{out}");
        let (rc, out, err) =
            run_on_copy_with_stderr(&["--plan-world", "--sort", "depend"], &[], &[], "", |_| {});
        assert_eq!(rc, 0, "{out}\n{err}");
        assert!(!out.contains("(bootstrap:"), "{out}");
        assert!(
            err.contains("plan-world: 4 to install (4 builds including 0 bootstrap), order depend"),
            "{err}"
        );
    }

    /// Resolver/fast-plan agreement (backlog #753, S1.4): both emitters call
    /// the shared leaf with the same cycles + guard map, so the same tree
    /// yields the same affected-slot rows — same order, same USE sets, same
    /// markers. No new sort, no new tie-break anywhere.
    #[test]
    fn resolver_and_fast_plan_agree_on_the_bootstrap_rows() {
        use super::{apply_bootstrap, select_bootstrap};
        use std::collections::{BTreeSet, HashMap};
        fn tree(root: &Path) {
            add_ebuild(
                root,
                "dev-libs/w749pkgnew-1",
                "0",
                &[
                    ("IUSE", "+usefoo +use3"),
                    ("BDEPEND", "usefoo? ( dev-libs/w749otherpkg[useblah] )"),
                ],
            );
            add_ebuild(
                root,
                "dev-libs/w749otherpkg-1",
                "0",
                &[
                    ("IUSE", "+useboh +useblah"),
                    ("BDEPEND", "useboh? ( dev-libs/w749pkgnew[use3] )"),
                ],
            );
        }
        let atoms = ["dev-libs/w749pkgnew", "dev-libs/w749otherpkg"];
        let (tmp, ..) =
            run_on_copy_keeping_tree(&["--plan-world", "--sort", "depend"], &atoms, &[], "", tree);
        let root = tmp.path().join("root");
        // Fast plan, through the same `select_bootstrap`/`apply_bootstrap`
        // `run()` uses.
        let mut opts = portage_world::Options::new(root.clone(), root.clone());
        opts.threads = 2;
        opts.rebuilds = true;
        opts.targets = atoms.iter().map(|s| s.to_string()).collect();
        let world = portage_world::analyse(&opts).expect("the fixture tree analyses");
        let cycles = portage_world::build_time_cycles(&world.edges, &world.edge_kinds);
        let bootstrap = select_bootstrap(&world, &cycles).expect("the pair bootstraps");
        assert_eq!(
            bootstrap,
            ("dev-libs/w749otherpkg:0".to_string(), "useboh".to_string())
        );
        let fast = apply_bootstrap(
            &world,
            &world.ordered(&portage_world::Order::Depend),
            Some(&bootstrap),
        );
        // Resolver rows over the same tree.
        let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
            config_root: &root,
            main_repo_location: &root.join("repo"),
            overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
            repo_aliases: &[],
            main_repo_name: "testrepo",
            repo_masters: &HashMap::new(),
            eroot: &root,
        })
        .expect("the copied fixture config resolves");
        let owned: Vec<String> = atoms.iter().map(|s| s.to_string()).collect();
        let resolved = portage_repo::ResolveRequest::new(
            &root,
            &root,
            &owned,
            &config,
            &root.join("distfiles"),
        )
        .resolve()
        .expect("the pair resolves");
        assert_eq!(resolved.entries.len(), fast.len());
        for (entry, row) in resolved.entries.iter().zip(fast.iter()) {
            let cp = format!("{}/{}", entry.category, entry.package);
            assert_eq!(cp, row.slot.cp);
            let fast_boot = row.bootstrap.as_ref().map(|b| b.reduced_flag.clone());
            let resolved_boot = entry.bootstrap.as_ref().map(|b| b.reduced_flag.clone());
            assert_eq!(fast_boot, resolved_boot, "{cp}");
            let fast_use: BTreeSet<String> = {
                let mut use_set: BTreeSet<String> = world
                    .planned_use
                    .get(&row.slot.node())
                    .cloned()
                    .unwrap_or_default();
                if fast_boot.is_some() {
                    use_set.remove("useboh");
                }
                use_set
            };
            let resolved_use: BTreeSet<String> = entry
                .use_flags_display
                .iter()
                .filter(|(_, on)| *on)
                .map(|(f, _)| f.clone())
                .collect();
            // The fast plan's global USE set covers the resolver row's own
            // IUSE display (the resolver shows per-package flags only).
            for flag in resolved_use {
                assert!(fast_use.contains(&flag), "{cp}: {flag}");
            }
        }
    }

    /// Charter property 2 (backlog #749): the mixed `R/B/R` #745 shape is a
    /// runtime-closed cycle, not a genuine build-time one — every build
    /// atom is satisfied at start, so one row per slot and no finding at
    /// all. No report line either: the `prov -> base` half is `RDEPEND`,
    /// so the report reads nothing here.
    #[test]
    fn a_runtime_cycle_needs_no_second_build() {
        fn runtime_cycle_tree(root: &Path) {
            add_ebuild(
                root,
                "dev-libs/w745prov-1",
                "0",
                &[("RDEPEND", "dev-libs/w745user")],
            );
            add_ebuild(
                root,
                "dev-libs/w745user-1",
                "0",
                &[("BDEPEND", "dev-libs/w745virt")],
            );
            add_ebuild(
                root,
                "dev-libs/w745virt-1",
                "0",
                &[("RDEPEND", "dev-libs/w745prov")],
            );
        }
        let world = [
            "dev-libs/w745prov",
            "dev-libs/w745virt",
            "dev-libs/w745user",
        ];
        let (rc, out, err) = run_on_copy_with_stderr(
            &["--plan-world", "--sort", "depend"],
            &world,
            &[],
            "",
            runtime_cycle_tree,
        );
        assert_eq!(rc, 0, "{out}\n{err}");
        let rows: Vec<&str> = out.lines().filter(|l| l.contains("w745")).collect();
        assert_eq!(
            rows,
            [
                "[ebuild  N     ] dev-libs/w745prov-1:0",
                "[ebuild  N     ] dev-libs/w745virt-1:0",
                "[ebuild  N     ] dev-libs/w745user-1:0",
            ],
            "{out}"
        );
        assert!(!err.contains("build cycle"), "{err}");
        let (tmp, rc, out, _) = run_on_copy_keeping_tree(
            &["--plan-world", "--json", "--sort", "depend"],
            &world,
            &[],
            "",
            runtime_cycle_tree,
        );
        assert_eq!(rc, 0, "{out}");
        let plan = tmp.path().join("plan.json");
        std::fs::write(&plan, out.trim()).expect("plan file");
        let text = std::fs::read_to_string(&plan).expect("plan file");
        assert_eq!(
            text.match_indices("\"package\":\"w745prov\"").count(),
            1,
            "{text}"
        );
        assert_eq!(
            text.match_indices("\"package\":\"w745virt\"").count(),
            1,
            "{text}"
        );
        assert_eq!(
            text.match_indices("\"package\":\"w745user\"").count(),
            1,
            "{text}"
        );
        assert!(!text.contains("\"built_twice\""), "{text}");
        let checker = health_checker();
        let report = assert_no_build_order(
            checker.to_str().expect("checker path"),
            &tmp.path().join("root/var/db/pkg"),
            &plan,
        );
        // No finding the plan introduces (that is `assert_no_build_order`),
        // and no note either: the `cycle-noted` excuse is gone with the
        // strictness that needed it (the `pre_existing` rows are the
        // fixtures' own broken installed tree, not this plan).
        assert!(!report.contains("cycle-noted"), "{report}");
    }

    /// Charter property 2 (PMS 8.1; backlog #751, plan `02.751` §6-A): the
    /// S0 probe tree — kept `virtual/w751virt`, installed `w751prov-1`
    /// with a stale `w751base:0/1=` binding rebuilt as `w751prov-2`,
    /// `w751base-2` with `BDEPEND w751prov` (so the virtual's provider
    /// edge is the cycle's only cut), consumer `w751user-1` with
    /// `BDEPEND virtual/w751virt`. The rows keep today's cut and order —
    /// one row per slot (backlog #749: the `prov`/`user` duplicates are
    /// gone), the virtual never a merge — and the checker reports no
    /// `build-order`. No report line either: the `base <-> prov` pair is
    /// runtime-closed, not a build-time cycle. Limitation: a kept
    /// virtual's stale provider rebuilds after the consumer that builds
    /// with it, yet nothing exhibits a consumer building against a
    /// broken copy (here the installed copy is still usable at the
    /// consumer's build) — theoretical, pinned, not proven safe.
    #[test]
    fn a_kept_virtual_with_a_stale_provider_keeps_todays_cut_and_order() {
        fn stale_virtual_tree(root: &Path) {
            add_ebuild(
                root,
                "virtual/w751virt-1",
                "0",
                &[("RDEPEND", "dev-libs/w751prov")],
            );
            add_ebuild(
                root,
                "dev-libs/w751prov-2",
                "0/2",
                &[("RDEPEND", "dev-libs/w751base dev-libs/w751user")],
            );
            add_ebuild(
                root,
                "dev-libs/w751base-2",
                "0/2",
                &[("BDEPEND", "dev-libs/w751prov")],
            );
            add_ebuild(
                root,
                "dev-libs/w751user-1",
                "0",
                &[("BDEPEND", "virtual/w751virt")],
            );
            add_installed(
                root,
                "virtual/w751virt-1",
                "0",
                &[("RDEPEND", "dev-libs/w751prov")],
            );
            add_installed(
                root,
                "dev-libs/w751prov-1",
                "0/1",
                &[("RDEPEND", "dev-libs/w751user dev-libs/w751base:0/1=")],
            );
            add_installed(root, "dev-libs/w751base-1", "0/1", &[]);
        }
        let world = ["dev-libs/w751user", "dev-libs/w751prov"];
        let (rc, out) = run_on_copy(
            &["--plan-world", "--sort", "depend"],
            &world,
            &[],
            "",
            stale_virtual_tree,
        );
        assert_eq!(rc, 0, "{out}");
        let rows: Vec<&str> = out.lines().filter(|l| l.contains("w751")).collect();
        assert_eq!(
            rows,
            [
                "[ebuild  N     ] dev-libs/w751user-1:0",
                "[ebuild     U  ] dev-libs/w751prov-2:0 [1]",
                "[ebuild     U  ] dev-libs/w751base-2:0 [1]",
            ],
            "{out}"
        );
        let (tmp, rc, out, _) = run_on_copy_keeping_tree(
            &["--plan-world", "--json", "--sort", "depend"],
            &world,
            &[],
            "",
            stale_virtual_tree,
        );
        assert_eq!(rc, 0, "{out}");
        let plan = tmp.path().join("plan.json");
        std::fs::write(&plan, out.trim()).expect("plan file");
        let text = std::fs::read_to_string(&plan).expect("plan file");
        // One row per slot, locked in place: `prov` and `user` single,
        // the virtual never a merge, and no second-build key anywhere.
        assert_eq!(
            text.match_indices("\"package\":\"w751prov\"").count(),
            1,
            "{text}"
        );
        assert_eq!(
            text.match_indices("\"package\":\"w751user\"").count(),
            1,
            "{text}"
        );
        assert!(!text.contains("\"built_twice\""), "{text}");
        assert_eq!(
            text.match_indices("\"package\":\"w751virt\"").count(),
            1,
            "{text}"
        );
        let checker = health_checker();
        assert_no_build_order(
            checker.to_str().expect("checker path"),
            &tmp.path().join("root/var/db/pkg"),
            &plan,
        );
    }

    /// Charter property 5: a stale binding the plan cannot repair is named
    /// once on stderr, with the atom and the reason; `--json` carries the
    /// same row so the checker sees it.
    #[test]
    fn a_stale_binding_the_plan_cannot_repair_is_named() {
        fn stale_tree(root: &Path) {
            add_ebuild(root, "dev-libs/w741prov-2", "0/2", &[]);
            add_ebuild(
                root,
                "dev-libs/w741user-1",
                "0",
                &[("RDEPEND", "dev-libs/w741prov")],
            );
            add_installed(root, "dev-libs/w741prov-1", "0/1", &[]);
            add_installed(
                root,
                "dev-libs/w741user-1",
                "0",
                &[("RDEPEND", "=dev-libs/w741prov-1* dev-libs/w741prov:0/1=")],
            );
        }
        let world = ["dev-libs/w741user"];
        let (rc, out, err) =
            run_on_copy_with_stderr(&["--plan-world"], &world, &[], "", stale_tree);
        assert_eq!(rc, 0, "{out}\n{err}");
        assert!(!out.contains("w741user-1"), "{out}");
        assert!(
            err.contains("plan-world: 1 stale slot-operator binding(s) not rebuilt: dev-libs/w741user (=dev-libs/w741prov-1* not satisfied by dev-libs/w741prov-2)"),
            "{err}"
        );
        let (rc, out) = run_on_copy(&["--plan-world", "--json"], &world, &[], "", stale_tree);
        assert_eq!(rc, 0, "{out}");
        assert!(
            out.contains("\"stale-bindings\":[{\"consumer\":\"dev-libs/w741user\",\"atom\":\"=dev-libs/w741prov-1*\",\"provider\":\"dev-libs/w741prov-2\""),
            "{out}"
        );
    }

    /// Charter property 3 (plan 02.740): the fast plan takes the `||` arm
    /// whose atoms the best visible candidates match, so the compat package
    /// is new and the provider upgrades instead of staying pinned.
    #[test]
    fn the_fast_plan_takes_the_newest_arm_of_a_disjunctive_dependency() {
        fn newest_arm_tree(root: &Path) {
            add_ebuild(root, "dev-libs/w740prov-3", "0/3", &[]);
            add_ebuild(root, "dev-libs/w740prov-4", "0/4", &[]);
            add_ebuild(root, "dev-libs/w740compat-3", "3", &[]);
            add_ebuild(
                root,
                "dev-libs/w740user-1",
                "0",
                &[(
                    "RDEPEND",
                    "|| ( dev-libs/w740compat:3 dev-libs/w740prov:0/3 )",
                )],
            );
            add_ebuild(
                root,
                "dev-libs/w740sudo-1",
                "0",
                &[("RDEPEND", "dev-libs/w740prov:=")],
            );
            add_installed(root, "dev-libs/w740prov-3", "0/3", &[]);
            add_installed(root, "dev-libs/w740sudo-1", "0", &[]);
        }
        let (rc, out) = run_on_copy(
            &["--plan-world"],
            &["dev-libs/w740user", "dev-libs/w740sudo"],
            &[],
            "",
            newest_arm_tree,
        );
        assert_eq!(rc, 0, "{out}");
        assert!(
            out.contains("[ebuild  N     ] dev-libs/w740compat-3:3"),
            "{out}"
        );
        assert!(
            out.contains("[ebuild     U  ] dev-libs/w740prov-4:0 [3]"),
            "{out}"
        );
    }

    /// Two ebuilds of `w739multi`, slots `1` and `2`, with slot `1` installed.
    fn dual_slot_tree(root: &Path) {
        add_ebuild(root, "dev-libs/w739multi-1", "1", &[]);
        add_ebuild(root, "dev-libs/w739multi-2", "2", &[]);
        add_installed(root, "dev-libs/w739multi-1", "1", &[]);
    }

    /// Charter property 3: a bare world atom names the newest slot even when
    /// an older slot is installed; the installed slot is no row.
    #[test]
    fn a_bare_atom_with_an_installed_older_slot_lists_the_newest_slot_as_new() {
        let (rc, out) = run_on_copy(
            &["--plan-world"],
            &["dev-libs/w739multi"],
            &[],
            "",
            dual_slot_tree,
        );
        assert_eq!(rc, 0, "{out}");
        assert!(
            out.contains("[ebuild  N     ] dev-libs/w739multi-2:2"),
            "{out}"
        );
        assert!(!out.contains("w739multi-1"), "{out}");
    }

    /// Charter property 3: with `emptytree` the same atom lists exactly the
    /// newest slot, not both installed slots.
    #[test]
    fn emptytree_lists_one_slot_of_a_bare_atom() {
        let (rc, out) = run_on_copy(
            &["--plan-world=emptytree"],
            &["dev-libs/w739multi"],
            &[],
            "",
            dual_slot_tree,
        );
        assert_eq!(rc, 0, "{out}");
        let rows: Vec<&str> = out.lines().filter(|l| l.contains("w739multi")).collect();
        assert_eq!(rows, ["[ebuild  N     ] dev-libs/w739multi-2:2"], "{out}");
    }

    /// Two kept packages declaring `IUSE="w693flag"`, neither with the flag
    /// configured: one installed with it on, one with a flag its ebuild no
    /// longer declares.
    fn use_tree(root: &Path) {
        for name in ["w693on", "w693gone"] {
            add_ebuild(
                root,
                &format!("dev-libs/{name}-1"),
                "0",
                &[("IUSE", "w693flag")],
            );
        }
        add_installed(
            root,
            "dev-libs/w693on-1",
            "0",
            &[("USE", "w693flag"), ("IUSE", "w693flag")],
        );
        add_installed(
            root,
            "dev-libs/w693gone-1",
            "0",
            &[("USE", "w693old"), ("IUSE", "w693old")],
        );
    }

    /// PMS `dependencies.tex`: USE is restricted to `IUSE_EFFECTIVE`, so a
    /// flag the kept version does not declare is no difference. Behind
    /// the `plan-world-newuse` future option only.
    #[test]
    fn a_changed_use_rebuilds_only_with_the_future_option() {
        let world = ["dev-libs/w693on", "dev-libs/w693gone"];
        let run = |args: &[&str]| run_on_copy(args, &world, &[], "", use_tree);
        let (rc, out) = run(&["--plan-world", "--future-options=plan-world-newuse"]);
        assert_eq!(rc, 0, "{out}");
        let rows: Vec<&str> = out.lines().filter(|l| l.contains("w693")).collect();
        assert_eq!(rows, ["[ebuild   R    ] dev-libs/w693on-1:0"], "{out}");
        let (rc, out) = run(&["--plan-world"]);
        assert_eq!(rc, 0, "{out}");
        assert!(!out.contains("w693"), "{out}");
    }

    /// The rows `--plan-world=<mode>` prints on the fixtures.
    fn mode_rows(mode: &str, more: &[&str]) -> Vec<String> {
        let mut args = vec![mode];
        args.extend_from_slice(more);
        let (rc, out) = mrg_on_fixtures(&args, &[]);
        assert_eq!(rc, 0, "{out}");
        out.lines().map(String::from).collect()
    }

    /// Owner decision (plan 02.692, Q1): a bare `--plan-world` is `=update`.
    #[test]
    fn a_bare_plan_world_is_the_update_plan() {
        let bare = mode_rows("--plan-world", &["--sort", "depend"]);
        let update = mode_rows("--plan-world=update", &["--sort", "depend"]);
        assert_eq!(bare.len(), 4, "{bare:?}");
        assert_eq!(bare, update);
    }

    /// PMS `dependencies.tex`: everything is built, so a kept version is a
    /// row too (`R`) and its build dependencies are followed.
    #[test]
    fn emptytree_lists_the_kept_versions_as_rebuilds() {
        let rows = mode_rows("--plan-world=emptytree", &["--sort", "alpha"]);
        for want in [
            "[ebuild   R    ] dev-libs/systempkg-1.0:0",
            "[ebuild   R    ] dev-libs/nestedsetpkg-1.0:0",
            "[ebuild     U  ] dev-libs/upgradepkg-2.0:0 [1.0]",
            "[ebuild  N     ] dev-libs/newpkg-1.0:0",
        ] {
            assert!(rows.iter().any(|r| r == want), "{want} not in {rows:?}");
        }
        assert!(
            rows.len() > mode_rows("--plan-world", &[]).len(),
            "{rows:?}"
        );
    }

    /// Charter property 5: a wrong mode is refused and both valid ones named.
    #[test]
    fn an_unknown_plan_mode_is_a_usage_error_naming_the_two() {
        let (rc, out, err) =
            run_on_copy_with_stderr(&["--plan-world=everything"], &[], &[], "", |_| {});
        assert_eq!((rc, out.as_str()), (2, ""), "{err}");
        assert!(err.contains("update") && err.contains("emptytree"), "{err}");
    }

    /// `--json` with emptytree: a kept version is a `reinstall` entry and is
    /// not also in `kept`.
    #[test]
    fn emptytree_json_has_reinstall_entries_and_no_duplicate_in_kept() {
        let (rc, out) = mrg_on_fixtures(&["--plan-world=emptytree", "--json"], &[]);
        assert_eq!(rc, 0, "{out}");
        let (entries, kept) = out.split_once("],\"kept\":").expect(&out);
        assert!(
            entries.contains("\"package\":\"systempkg\"")
                && entries.contains("\"outcome\":\"reinstall\""),
            "{out}"
        );
        assert!(!kept.contains("\"package\":\"systempkg\""), "{out}");
    }

    /// Charter property 5: the summary is two lines on stderr, the second
    /// says what the plan did not look at, and stdout stays rows only.
    #[test]
    fn the_summary_says_what_was_not_checked() {
        let run = |args: &[&str]| run_on_copy_with_stderr(args, &[], &[], "", |_| {});
        let (rc, out, err) = run(&["--plan-world", "--sort", "depend"]);
        assert_eq!(rc, 0, "{err}");
        assert!(out.lines().all(|l| l.starts_with("[ebuild")), "{out}");
        assert_eq!(
            err,
            "plan-world: 4 to install (4 builds including 0 bootstrap), order depend\n\
             not checked: blockers, REQUIRED_USE, slot conflicts, USE rebuilds; \
             run 'mrg -puDN @world' for the full plan\n"
        );
        let (_, _, err) = run(&["--plan-world", "--future-options=plan-world-newuse"]);
        assert!(err.contains("slot conflicts; run 'mrg -puDN"), "{err}");
        let (_, _, err) = run(&["--plan-world=emptytree"]);
        assert!(err.contains("order alpha\nnot checked: blockers"), "{err}");
        assert!(
            err.ends_with("run 'mrg -pe @world' for the full plan\n"),
            "{err}"
        );
        assert!(!err.contains("USE rebuilds"), "{err}");
    }

    #[test]
    fn json_without_a_world_command_is_a_usage_error() {
        let (rc, _) = mrg_on_fixtures(&["--json"], &[]);
        assert_eq!(rc, 2);
    }

    #[test]
    fn sort_without_plan_world_is_a_usage_error() {
        let (rc, _) = mrg_on_fixtures(&["--sort", "alpha"], &[]);
        assert_eq!(rc, 2);
    }

    #[test]
    fn plan_rows_give_the_position_by_package_and_slot() {
        let text = "These are the packages that would be merged, in order:\n\n\
            [ebuild     U  ] sys-apps/hwdata-0.413::gentoo [0.401::gentoo] 2607 KiB\n\
            [ebuild  N     ] app-arch/unrar-7.3.2:0/7::gentoo 0 KiB\n\
            [binary   R    ] dev-libs/foo-1.0-r2:2 \n";
        let rows = parse_plan_rows(text);
        assert_eq!(rows["sys-apps/hwdata"], 0);
        assert_eq!(rows["app-arch/unrar:0"], 1);
        assert_eq!(rows["dev-libs/foo:2"], 2);
        assert_eq!(rows.len(), 3);
    }
}
