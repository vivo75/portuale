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
mod tests {
    use std::path::Path;
    use std::process::Command;

    /// A copy of the fixture tree whose world file gets `extra` appended,
    /// and `mrg --check-world` run on it: (exit code, stdout).
    fn check(extra: &[&str]) -> (i32, String) {
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
        let out = Command::new(crate::self_exe::built_portuale_bin())
            .args(["mrg", "--check-world", "--root"])
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
}
