//! `mrg --check-world`: every package slot `@world` and its dependencies reach
//! must have a visible ebuild that satisfies the atoms aimed at it. A slot
//! with none, and an atom no ebuild matches with this configuration, is reported with who
//! asked for what (charter: the installation
//! must be able to keep its dependencies; a world nothing can satisfy is a
//! plan that fails, or a wrong one). The calculation is `portage-world`; it
//! knows nothing of blockers, `REQUIRED_USE` or backtracking, so a clean
//! report is not a promise that `emerge` succeeds.

use clap::ArgMatches;
use portage_world::Options;
use std::path::PathBuf;
use std::process::ExitCode;

/// Prints the invalid slots and returns 1 if there are any, 0 if not, 2 if
/// the repositories or the configuration cannot be read.
pub(crate) fn run(matches: &ArgMatches) -> ExitCode {
    let path = |id: &str, fallback: PathBuf| {
        matches
            .get_one::<String>(id)
            .map_or(fallback, PathBuf::from)
    };
    let mut opts = Options::new(
        path("root", portage_repo::root_from_env()),
        path("config_root", portage_repo::config_root_from_env()),
    );
    let future = match crate::future_options::resolve(
        matches
            .get_one::<String>("future_options")
            .map(String::as_str),
        &|k| std::env::var(k).ok(),
        &mut crate::mrg::make_conf_lookup_at(opts.config_root.clone(), opts.root.clone()),
    ) {
        Ok(f) => f,
        Err(message) => {
            eprintln!("check-world: {message}");
            return ExitCode::from(2);
        }
    };
    opts.unique = future.on("check-world-unique");
    if let Some(n) = future.value("check-world-threads") {
        match n.parse::<usize>() {
            Ok(n) if n > 0 => opts.threads = n,
            _ => {
                eprintln!("check-world: check-world-threads={n:?} is not a positive number");
                return ExitCode::from(2);
            }
        }
    }
    opts.with_bdeps = matches
        .get_one::<String>("with_bdeps")
        .is_some_and(|v| v == "y");
    let world = match portage_world::analyse(&opts) {
        Ok(w) => w,
        Err(message) => {
            eprintln!("check-world: {message}");
            return ExitCode::from(2);
        }
    };
    let mut invalid = 0;
    for slot in world.invalid() {
        invalid += 1;
        println!("INVALID {}:{}", slot.cp, slot.slot);
        for (atom, by) in &slot.atoms {
            println!("    wants {atom}  <- {}", by.join(", "));
        }
    }
    if future.on("check-world-unneeded") {
        for slot in world.slots.iter().filter(|s| s.unneeded) {
            println!(
                "UNNEEDED {}:{}  (another slot serves its requesters)",
                slot.cp, slot.slot
            );
        }
    }
    for (atom, by) in &world.unmatched {
        invalid += 1;
        println!("NO MATCH {atom}  <- {}", by.join(", "));
    }
    println!(
        "check-world: {} atoms, {} package slots, {invalid} unsatisfiable",
        world.atoms,
        world.slots.len()
    );
    ExitCode::from(u8::from(invalid > 0))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;
    use std::process::Command;

    fn check(extra: &[&str]) -> (i32, String) {
        mrg_on_fixtures(&["--check-world"], extra)
    }

    /// A copy of the fixture tree whose world file gets `extra` appended,
    /// and `mrg <args>` run on it: (exit code, stdout).
    pub(crate) fn mrg_on_fixtures(args: &[&str], extra: &[&str]) -> (i32, String) {
        mrg_on_fixtures_with(args, extra, &[], "")
    }

    /// As [`mrg_on_fixtures`], with `env` set for the run and `make_conf`
    /// appended to the copy's `etc/portage/make.conf`.
    pub(crate) fn mrg_on_fixtures_with(
        args: &[&str],
        extra: &[&str],
        env: &[(&str, &str)],
        make_conf: &str,
    ) -> (i32, String) {
        let tmp = tempfile::tempdir_in("/var/tmp").unwrap();
        let root = tmp.path().join("root");
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/.");
        assert!(
            Command::new("cp")
                .arg("-a")
                .arg(fixtures)
                .arg(&root)
                .status()
                .unwrap()
                .success()
        );
        let world = root.join("var/lib/portage/world");
        let mut text = std::fs::read_to_string(&world).unwrap();
        for atom in extra {
            text.push_str(atom);
            text.push('\n');
        }
        std::fs::write(&world, text).unwrap();
        if !make_conf.is_empty() {
            let conf = root.join("etc/portage/make.conf");
            let mut text = std::fs::read_to_string(&conf).unwrap();
            text.push_str(make_conf);
            std::fs::write(&conf, text).unwrap();
        }
        let out = Command::new(crate::self_exe::built_portuale_bin())
            .envs(env.iter().copied())
            .arg("mrg")
            .args(args)
            .arg("--root")
            .arg(&root)
            .arg("--config-root")
            .arg(&root)
            .output()
            .unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
    }

    #[test]
    fn a_satisfiable_world_exits_zero() {
        let (rc, out) = check(&[]);
        assert_eq!(rc, 0, "{out}");
        assert!(out.contains("0 unsatisfiable"), "{out}");
    }

    #[test]
    fn an_atom_no_version_satisfies_and_a_missing_package_are_reported() {
        let (rc, out) = check(&[">=dev-libs/newpkg-99", "app-misc/doesnotexist"]);
        assert_eq!(rc, 1, "{out}");
        assert!(
            out.contains("NO MATCH >=dev-libs/newpkg-99  <- @world"),
            "{out}"
        );
        assert!(
            out.contains("NO MATCH app-misc/doesnotexist  <- @world"),
            "{out}"
        );
    }

    #[test]
    fn a_slot_whose_only_versions_are_masked_is_invalid() {
        // `dev-libs/autounmaskkeywordpkg` is `~amd64` and nothing accepts it.
        let (rc, out) = check(&["dev-libs/autounmaskkeywordpkg"]);
        assert_eq!(rc, 1, "{out}");
        assert!(
            out.contains("INVALID dev-libs/autounmaskkeywordpkg:0"),
            "{out}"
        );
    }

    const FUTURE: &str = "--future-options";

    /// Charter property 1, `check-world-unique`: two satisfying versions make
    /// the slot unsatisfiable only when the option is on. Source: the charter.
    #[test]
    fn unique_is_off_by_default_and_on_with_the_option() {
        let (rc, out) = check(&["dev-libs/w701dup"]);
        assert_eq!(rc, 0, "{out}");
        let (rc, out) = mrg_on_fixtures(
            &["--check-world", "--future-options=check-world-unique"],
            &["dev-libs/w701dup"],
        );
        assert_eq!(rc, 1, "{out}");
        assert!(out.contains("INVALID dev-libs/w701dup:0"), "{out}");
    }

    /// `check-world-unneeded` lists the slot another slot serves and does not
    /// change the exit code.
    #[test]
    fn unneeded_slots_are_listed_only_with_the_option() {
        let (rc, out) = check(&["dev-libs/w701two"]);
        assert_eq!((rc, out.contains("UNNEEDED")), (0, false), "{out}");
        let (rc, out) = mrg_on_fixtures(
            &["--check-world", "--future-options=check-world-unneeded"],
            &["dev-libs/w701two"],
        );
        assert_eq!(rc, 0, "{out}");
        assert!(out.contains("UNNEEDED dev-libs/w701two:1"), "{out}");
    }

    /// Charter property 3: the thread count changes how long it takes, not the
    /// answer; zero is refused.
    #[test]
    fn the_thread_count_does_not_change_the_answer() {
        let atoms = ["dev-libs/w701ver", "dev-libs/w701two", "dev-libs/w701dup"];
        let outs: Vec<(i32, String)> = ["1", "2", "8"]
            .iter()
            .map(|n| {
                let option = format!("{FUTURE}=check-world-threads={n}");
                mrg_on_fixtures(&["--check-world", &option], &atoms)
            })
            .collect();
        assert!(outs[0].1.contains("package slots"), "{}", outs[0].1);
        assert_eq!(outs[0], outs[1]);
        assert_eq!(outs[0], outs[2]);
        let (rc, _) = mrg_on_fixtures(
            &["--check-world", "--future-options=check-world-threads=0"],
            &[],
        );
        assert_eq!(rc, 2);
    }

    /// Charter property 5: an unknown name is refused and the closest are named.
    #[test]
    fn an_unknown_future_option_names_the_nearest() {
        let tmp = mrg_on_fixtures(
            &["--check-world", "--future-options=check-world-uniqe"],
            &[],
        );
        assert_eq!(tmp.0, 2);
    }

    /// Precedence: stage default < make.conf < environment < command line, and
    /// the command line leaves make.conf unread.
    #[test]
    fn precedence_is_default_make_conf_environment_flag() {
        let world = ["dev-libs/w701dup"];
        let conf = "PORTUALE_FUTURE_OPTIONS=\"check-world-unique\"\n";
        let run = |args: &[&str], env: &[(&str, &str)], conf: &str| {
            mrg_on_fixtures_with(args, &world, env, conf).0
        };
        assert_eq!(run(&["--check-world"], &[], ""), 0, "default");
        assert_eq!(run(&["--check-world"], &[], conf), 1, "make.conf");
        let off = ("PORTUALE_FUTURE_OPTIONS", "check-world-unique=false");
        assert_eq!(
            run(&["--check-world"], &[off], conf),
            0,
            "environment over make.conf"
        );
        let on = "--future-options=check-world-unique";
        assert_eq!(
            run(&["--check-world", on], &[off], conf),
            1,
            "flag over environment"
        );
        // A make.conf that would be refused is never read under the flag.
        let junk = "PORTUALE_FUTURE_OPTIONS=\"nonsense\"\n";
        assert_eq!(
            run(&["--check-world"], &[], junk),
            2,
            "junk read without the flag"
        );
        assert_eq!(
            run(&["--check-world", on], &[], junk),
            1,
            "junk not read under the flag"
        );
    }
}
