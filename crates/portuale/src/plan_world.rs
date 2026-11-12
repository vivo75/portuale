//! `mrg --plan-world[=update|emptytree] [--sort alpha|depend|resolver]`: what
//! `emerge -uDNp @world` (or, with `emptytree`, `emerge -pe @world`) would
//! install, worked out by `portage-world` instead of the resolver. One
//! row per slot whose best visible version is not the installed one, and an `R`
//! row for a kept package whose `:slot/sub-slot=` dependency the plan breaks
//! (with `plan-world-newuse`, also one whose USE changed). It sees no blockers
//! and no `REQUIRED_USE`, so it can list less or more than the resolver; `--sort resolver` shows the
//! resolver's own order for the same rows. `--sort depend` puts a package
//! after what it needs before it is merged or usable; a `PDEPEND` orders
//! nothing (PMS).

use crate::pretend::json::json_string as q;
use clap::ArgMatches;
use portage_world::{Change, Order, SlotSet, World};
use std::collections::{HashMap, HashSet};
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
    if matches.get_flag("json") {
        println!("{}", plan_json(&world, &rows, &root));
    } else {
        for slot in &rows {
            println!("{}", row(&world, slot));
        }
    }
    let unread = match world.unreadable.len() {
        0 => String::new(),
        n => format!(", {n} not read"),
    };
    eprintln!(
        "plan-world: {} to install, order {sort}{unread}",
        rows.len()
    );
    eprintln!("{}", not_checked(mode, opts.newuse));
    ExitCode::SUCCESS
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
fn plan_json(world: &World, rows: &[&SlotSet], root: &Path) -> String {
    let entries: Vec<String> = rows
        .iter()
        .enumerate()
        .filter_map(|(order, slot)| entry_json(world, slot, order))
        .collect();
    let replaced: HashSet<(String, String)> = rows
        .iter()
        .map(|s| (s.cp.clone(), s.slot.clone()))
        .collect();
    let kept: Vec<String> = portage_repo::all_installed_packages(root)
        .iter()
        .filter(|p| {
            let slot = p.slot.split('/').next().unwrap_or_default();
            !replaced.contains(&(format!("{}/{}", p.category, p.package), slot.to_string()))
        })
        .map(|p| {
            format!(
                "{{\"category\":{},\"package\":{},\"version\":{},\"slot\":{}}}",
                q(&p.category),
                q(&p.package),
                q(&p.version),
                q(&p.slot)
            )
        })
        .collect();
    format!(
        "{{\"entries\":[{}],\"kept\":[{}],\"request\":[\"@world\"],\"aborted\":null}}",
        entries.join(","),
        kept.join(",")
    )
}

/// One `entries` object of [`plan_json`]; `None` for a slot with no chosen
/// version (it has no row either).
fn entry_json(world: &World, slot: &SlotSet, order: usize) -> Option<String> {
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
    if let Some(from) = from {
        fields.push(format!("\"from_version\":{}", q(from)));
    }
    let pf = format!("{package}-{}", chosen.version);
    if let Ok(metadata) = portage_repo::repo_aux_metadata(&chosen.repo_location, category, &pf) {
        let use_flags = world
            .planned_use
            .get(&slot.node())
            .cloned()
            .unwrap_or_default();
        let input = portage_repo::DepInput {
            metadata,
            use_flags: use_flags.into_iter().collect(),
        };
        fields.extend(crate::pretend::json::dep_input_fields(&input));
    }
    Some(format!("{{{}}}", fields.join(",")))
}

/// `[ebuild  N     ] cat/pkg-ver:slot`, with the installed version of an
/// upgrade or downgrade after it.
fn row(world: &World, slot: &SlotSet) -> String {
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
    format!(
        // match-str-ok: a display row (cat/pkg-ver:slot), not a match string
        "[ebuild{mark}] {}-{}:{}{old}",
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
        add_ebuild, add_installed, mrg_on_fixtures, run_on_copy, run_on_copy_with_stderr,
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
    /// row, in the `emerge --json` shape the health checker reads.
    #[test]
    fn json_rows_equal_text_rows_in_order() {
        let (rc, out) = mrg_on_fixtures(&["--plan-world", "--json", "--sort", "depend"], &[]);
        assert_eq!(rc, 0, "{out}");
        let line = out
            .lines()
            .find(|l| l.starts_with("{\"entries\":"))
            .expect(&out);
        let entries = line.split("],\"kept\":").next().unwrap();
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
            "plan-world: 4 to install, order depend\n\
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
