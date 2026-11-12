//! `mrg --plan-world [--sort alpha|depend|resolver]`: what `emerge -uDNp @world`
//! would install, worked out by `portage-world` instead of the resolver. One
//! row per slot whose best visible version is not the installed one. It sees
//! no blockers, no `REQUIRED_USE`, no USE change that rebuilds a package, so
//! it can list less or more than the resolver; `--sort resolver` shows the
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
    let (opts, _) = match crate::check_world::options(matches, "plan-world") {
        Ok(setup) => setup,
        Err(code) => return code,
    };
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
            println!("{}", row(slot));
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
    ExitCode::SUCCESS
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
    let (outcome, from) = match (slot.change()?, slot.installed.as_deref()) {
        (Change::New, _) => ("new", None),
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
fn row(slot: &SlotSet) -> String {
    let Some(chosen) = slot.chosen() else {
        return String::new();
    };
    let (mark, old) = match (slot.change(), slot.installed.as_deref()) {
        (Some(Change::New), _) => ("  N     ", String::new()),
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
    use crate::check_world::tests::mrg_on_fixtures;

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
