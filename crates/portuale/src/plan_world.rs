//! `mrg --plan-world [--sort alpha|depend|resolver]`: what `emerge -uDNp @world`
//! would install, worked out by `portage-world` instead of the resolver. One
//! row per slot whose best visible version is not the installed one. It sees
//! no blockers, no `REQUIRED_USE`, no USE change that rebuilds a package, so
//! it can list less or more than the resolver; `--sort resolver` shows the
//! resolver's own order for the same rows.

use clap::ArgMatches;
use portage_world::{Change, Options, Order, SlotSet, World};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

/// Prints the rows and returns 0, or 2 when the tree cannot be read or the
/// resolver order cannot be had.
pub(crate) fn run(matches: &ArgMatches) -> ExitCode {
    let path = |id: &str, fallback: PathBuf| {
        matches
            .get_one::<String>(id)
            .map_or(fallback, PathBuf::from)
    };
    let root = path("root", portage_repo::root_from_env());
    let config_root = path("config_root", portage_repo::config_root_from_env());
    let sort = matches
        .get_one::<String>("sort")
        .map_or("alpha", String::as_str);
    let mut opts = Options::new(root.clone(), config_root.clone());
    opts.with_bdeps = matches
        .get_one::<String>("with_bdeps")
        .is_some_and(|v| v == "y");
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
    for slot in &rows {
        println!("{}", row(slot));
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
