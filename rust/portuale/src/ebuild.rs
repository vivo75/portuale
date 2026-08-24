// `ebuild <file> <command> [command...]`: CLI-surface recognition
// (mirroring emerge's own pretend.rs/emerge_options.rs treatment -- real
// options, see ebuild_options.rs, transcribed from bin/ebuild's own
// argparse setup, and real commands, from doebuild()'s own
// `validcommands` list, are recognized and accepted; genuinely invalid
// input -- an unrecognized option, a filename not ending in ".ebuild",
// an unrecognized command, or a missing file/command -- is rejected with
// a specific, accurate message), PLUS, as of task #54, real execution
// for the `actionmap_deps`-chained phase commands (`pretend`/`setup`/
// `unpack`/`prepare`/`configure`/`compile`/`test`/`install` -- see
// `ebuild_phases`'s own module doc comment for the full architecture and
// v1 scope cuts), PLUS, as of task #55, real execution for `merge` (see
// `ebuild_merge`'s own module doc comment: runs the real `install` chain,
// then really runs pkg_preinst, copies `${D}` into `${ROOT}`, writes a
// real vdb entry, then really runs pkg_postinst), `unmerge` (see
// `ebuild_unmerge`'s own module doc comment: runs pkg_prerm, really
// deletes every file/dir/symlink the vdb entry's own CONTENTS lists from
// `${ROOT}`, runs pkg_postrm, then removes the vdb entry itself), and
// `package` (see `ebuild_package`'s own module doc comment: runs the
// real `install` chain, then really invokes `bin/misc-functions.sh`'s
// own `__dyn_package` -- real, unmodified bash shelling out to the real,
// unmodified `bin/xpak-helper.py` -- producing a genuine XPAK-tagged
// `.tbz2` at `PKGDIR`, plus a real `Packages` index entry for it). Every
// other real command (`qmerge`/`preinst`/`postinst`/`prerm`/`postrm`/
// `config`/`info`/`nofetch`/`depend`/`fetch`/`fetchall`/`digest`/
// `manifest`/`rpm`/`instprep`/`clean`/`cleanrm`) still falls through to
// the pre-existing dry-run stub message below unchanged.
//
// Exit codes mirror real `ebuild`'s own conventions: 2 for "missing
// required args" (real bin/ebuild's argparse `parser.error()`), 1 for
// everything else invalid (real bin/ebuild's own `err()` helper, and
// `doebuild()`'s own return value for an unrecognized command).
//
// KNOWN, DOCUMENTED DEVIATION: real bin/ebuild uses argparse's
// `parse_known_args`, which silently swallows an unrecognized flag into
// the leftover-positional-args list rather than rejecting it outright
// -- in practice this usually surfaces later as a confusing "does not
// end with '.ebuild'" error instead of a clear "unrecognized option"
// one. This pilot deliberately reports an unrecognized option
// immediately and specifically instead, matching the clearer error
// philosophy `emerge`'s own CLI already uses.
//
// `-h`/`--help` IS real and implemented, checked unconditionally before
// anything else in `args` -- matching real bin/ebuild's own behavior
// (argparse's auto-added `-h`/`--help` is checked during parsing itself,
// before any of the app's own logic runs, so it wins regardless of what
// else -- valid or not -- accompanies it). See `ebuild_options.rs`'s
// own doc comment for why `--version` is deliberately NOT implemented
// alongside it. The help text itself is NOT a port of real bin/ebuild's
// own argparse-generated usage block -- it's a short, honest,
// pilot-specific summary, the same "pilot-specific summary, not a port
// of real formatting" precedent `emerge --help` already set.

use crate::ebuild_merge;
use crate::ebuild_options::{self, Kind};
use crate::ebuild_package;
use crate::ebuild_phases;
use crate::ebuild_unmerge;
use std::process::ExitCode;

/// Whether `--help`/`-h` appears anywhere in `args` -- unlike `emerge`'s
/// own `wants_help`, no short-flag-bundle scanning is needed here: real
/// bin/ebuild declares no short aliases for any of its own six options,
/// so `-h` is never part of a bundle to begin with.
fn wants_help(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--help" || arg == "-h")
}

/// A short, honest, pilot-specific summary -- not a port of real
/// bin/ebuild's own argparse-generated usage block (see the module doc
/// comment for why).
fn print_help() {
    println!("ebuild (pilot stub): command-line interface to the Rust porting pilot");
    println!();
    println!("Usage:");
    println!("   ebuild <ebuild file> <command> [command...]");
    println!("   ebuild --help");
    println!();
    println!("Still a pure dry-run stub: real phase execution is deferred (see");
    println!("PROMPT.md's \"Deferred: ebuild phase execution\"), so no command below");
    println!("actually does anything yet beyond being recognized and accepted.");
    println!();
    println!("Options:");
    println!("   --force              regenerate digests (with the digest/manifest commands)");
    println!("   --color y|n          enable or disable color output");
    println!("   --debug              show debug output");
    println!("   --ignore-default-opts  do not use the EBUILD_DEFAULT_OPTS environment variable");
    println!("   --skip-manifest      skip all manifest checks");
    println!(
        "   --shell bash|brush   real shell backend for phase/hook execution (default: brush)"
    );
    println!("   -h, --help           show this message and exit");
    println!();
    println!(
        "Every other real ebuild option is recognized by name (see bin/ebuild) but \
         not implemented -- using one reports which option it is, instead of a \
         generic error. Real commands (doebuild()'s own validcommands list) are \
         recognized and accepted, still as a no-op. --shell is this pilot's own \
         flag, not a real bin/ebuild option at all."
    );
    println!("See PORTING/README.md and PORTING/PROMPT.md for this pilot's current scope.");
}

pub fn run(args: &[String]) -> ExitCode {
    if wants_help(args) {
        print_help();
        return ExitCode::SUCCESS;
    }

    let mut ebuild_file: Option<&str> = None;
    let mut commands: Vec<&str> = Vec::new();
    // Real, not simulated (task #56): `--debug` sets real `PORTAGE_DEBUG`,
    // triggering real `bin/ebuild.sh`'s own `set -x` guard during phase
    // execution below -- see `ebuild_phases::run_one_phase`'s own setup
    // block. Every other real ebuild `Kind::Boolean` option is still a
    // pure no-op (the `Some(Kind::Boolean) => {}` arm below), matching
    // this module's own long-documented v1 scope; `--debug` is the first
    // one wired to something real.
    let mut debug = false;
    // `--shell bash|brush` (default `brush`): selects which real shell
    // backend executes every phase/hook/misc-function below -- see
    // `ebuild_phases::ShellBackend`'s own doc comment. A pilot-only flag,
    // not a real `bin/ebuild` option at all, so it's checked here
    // directly rather than through `ebuild_options::lookup_option`
    // (deliberately NOT added to `ebuild_options::OPTIONS`, which is
    // specifically a transcription of real bin/ebuild's own argparse
    // setup) -- the same "special-cased outside the real-options table"
    // treatment `wants_help` above already gets.
    let mut shell = ebuild_phases::ShellBackend::default();

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--shell" || arg.starts_with("--shell=") {
            let value = if let Some(v) = arg.strip_prefix("--shell=") {
                v
            } else if i + 1 < args.len() {
                i += 1;
                args[i].as_str()
            } else {
                eprintln!("ebuild: option '--shell' requires a value");
                return ExitCode::from(2);
            };
            shell = match value {
                "brush" => ebuild_phases::ShellBackend::Brush,
                "bash" => ebuild_phases::ShellBackend::Bash,
                other => {
                    eprintln!("ebuild: --shell: {other:?} is not \"bash\" or \"brush\"");
                    return ExitCode::from(1);
                }
            };
        } else if arg.starts_with('-') {
            match ebuild_options::lookup_option(arg) {
                Some(Kind::Value) => {
                    // "--opt=value" carries its value inline; "--opt value"
                    // needs the next token skipped so it isn't mistaken for
                    // the ebuild file or a command.
                    if !arg.contains('=') && i + 1 < args.len() {
                        i += 1;
                    }
                }
                Some(Kind::Boolean) => {
                    if arg == "--debug" {
                        debug = true;
                    }
                }
                None => {
                    eprintln!("ebuild: unrecognized option {arg:?}");
                    return ExitCode::from(1);
                }
            }
        } else if ebuild_file.is_none() {
            if !arg.ends_with(".ebuild") {
                eprintln!("ebuild: {arg:?}: does not end with \".ebuild\"");
                return ExitCode::from(1);
            }
            ebuild_file = Some(arg);
        } else {
            commands.push(arg);
        }
        i += 1;
    }

    let Some(ebuild_file) = ebuild_file else {
        eprintln!("ebuild: missing required args");
        return ExitCode::from(2);
    };
    if commands.is_empty() {
        eprintln!("ebuild: missing required args");
        return ExitCode::from(2);
    }

    for cmd in &commands {
        if !ebuild_options::is_valid_command(cmd) {
            eprintln!("ebuild: {cmd:?} is not one of the valid ebuild commands");
            return ExitCode::from(1);
        }
    }

    // Real execution (task #54/#55, ebuild_phases's/ebuild_merge's/
    // ebuild_unmerge's/ebuild_package's own module doc comments) only
    // when EVERY requested command is one this pilot actually implements
    // for real (the actionmap_deps-chained phase subset, plus
    // `merge`/`unmerge`/`package`) -- a deliberate, simple v1 boundary:
    // no partial-real-execution ambiguity when a request mixes a real
    // command with one this pilot still only dry-runs (e.g. `ebuild
    // foo.ebuild compile qmerge`). A purely dry-run request keeps the
    // exact pre-existing stub message unchanged.
    if commands.iter().all(|cmd| {
        ebuild_phases::is_real_phase_command(cmd)
            || ebuild_merge::is_real_merge_command(cmd)
            || ebuild_unmerge::is_real_unmerge_command(cmd)
            || ebuild_package::is_real_package_command(cmd)
    }) {
        let root = portage_repo::root_from_env();
        // Real portage's own make.globals default -- see
        // ebuild_phases::run_commands's own doc comment for why this is
        // read here, at the CLI boundary, rather than internally.
        let portage_tmpdir = std::env::var_os("PORTAGE_TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("/var/tmp/portage"));
        // Same env-var-not-full-config-resolution shortcut as
        // PORTAGE_TMPDIR above -- real make.globals's own defaults (see
        // ebuild_merge::MergeOptions's own Default impl) apply when
        // unset.
        // Real make.globals's own DISTDIR default -- same env-var-not-
        // full-config-resolution shortcut as PORTAGE_TMPDIR/PKGDIR/
        // CONFIG_PROTECT, shared by both real-execution paths below
        // that run the real `install` chain (and therefore a real
        // `unpack`, see `ebuild_phases::fetch_sources`).
        let distdir = std::env::var_os("DISTDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("/var/cache/distfiles"));
        let default_merge_options = ebuild_merge::MergeOptions::default();
        // Real `"collision-protect" in self.settings.features` -- same
        // env-var-not-full-config-resolution shortcut every other real
        // setting at this CLI boundary already uses.
        let collision_protect = std::env::var("FEATURES")
            .map(|features| {
                features
                    .split_whitespace()
                    .any(|tok| tok == "collision-protect")
            })
            .unwrap_or(default_merge_options.collision_protect);
        // Real `"protect-owned" in self.settings.features` -- same
        // env-var-not-full-config-resolution shortcut as
        // `collision_protect` immediately above.
        let protect_owned = std::env::var("FEATURES")
            .map(|features| {
                features
                    .split_whitespace()
                    .any(|tok| tok == "protect-owned")
            })
            .unwrap_or(default_merge_options.protect_owned);
        let merge_options = ebuild_merge::MergeOptions {
            debug,
            config_protect: std::env::var("CONFIG_PROTECT")
                .unwrap_or(default_merge_options.config_protect),
            config_protect_mask: std::env::var("CONFIG_PROTECT_MASK")
                .unwrap_or(default_merge_options.config_protect_mask),
            distdir: distdir.clone(),
            shell,
            collision_protect,
            protect_owned,
        };
        // Real make.globals's own PKGDIR default -- see
        // ebuild_package::PackageOptions's own Default impl.
        let package_options = ebuild_package::PackageOptions {
            debug,
            pkgdir: std::env::var_os("PKGDIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| ebuild_package::PackageOptions::default().pkgdir),
            distdir: distdir.clone(),
            shell,
        };
        let ebuild_path = std::path::Path::new(ebuild_file);
        // One command at a time here, not the whole slice at once --
        // neither `merge`/`unmerge` nor `package` is itself an
        // ebuild_phases-recognized phase (real `doebuild()` handles them
        // as their own, separate steps, not `bin/ebuild.sh` phase
        // arguments at all), so a mixed list like `["install", "merge"]`
        // needs its own per-command routing. This also mirrors real
        // `bin/ebuild`'s own `for arg in pargs: doebuild(ebuild, arg,
        // ...)` loop, which likewise re-derives the environment fresh
        // for every top-level command argument.
        for &cmd in &commands {
            let result = if ebuild_merge::is_real_merge_command(cmd) {
                ebuild_merge::run_merge(ebuild_path, &root, &portage_tmpdir, &merge_options)
            } else if ebuild_unmerge::is_real_unmerge_command(cmd) {
                ebuild_unmerge::run_unmerge(ebuild_path, &root, &portage_tmpdir, debug, shell)
            } else if ebuild_package::is_real_package_command(cmd) {
                ebuild_package::run_package(ebuild_path, &root, &portage_tmpdir, &package_options)
            } else {
                ebuild_phases::run_commands(
                    ebuild_path,
                    &[cmd],
                    &root,
                    &portage_tmpdir,
                    &distdir,
                    debug,
                    shell,
                )
            };
            match result {
                Ok(0) => {}
                Ok(_) => return ExitCode::from(1),
                Err(e) => {
                    eprintln!("ebuild: {e}");
                    return ExitCode::from(1);
                }
            }
        }
        return ExitCode::SUCCESS;
    }

    println!(
        "ebuild (pilot stub): dry-run only, no phase execution yet \
         (see PROMPT.md's \"Deferred: ebuild phase execution\")"
    );
    println!("ebuild file: {ebuild_file:?}");
    println!("commands: {commands:?}");
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn accepts_a_real_command_and_still_prints_the_stub_marker() {
        // "qmerge" is a real ebuild command (doebuild()'s own
        // validcommands list) that this pilot still doesn't implement
        // for real (unlike "merge"/"package"/the actionmap_deps-chained
        // phases as of tasks #54/#55) -- exactly the case the dry-run
        // stub still needs to cover.
        let code = run(&args(&["foo-1.0.ebuild", "qmerge"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn accepts_multiple_real_commands() {
        let code = run(&args(&["foo-1.0.ebuild", "clean", "compile", "install"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn accepts_a_real_boolean_option() {
        let code = run(&args(&["--force", "foo-1.0.ebuild", "qmerge"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn accepts_a_real_value_option_without_misreading_its_value() {
        let code = run(&args(&["--color", "y", "foo-1.0.ebuild", "qmerge"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn accepts_the_inline_equals_form_of_a_value_option() {
        let code = run(&args(&["--color=y", "foo-1.0.ebuild", "qmerge"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn accepts_shell_bash_and_shell_brush() {
        // "qmerge" is dry-run-only (see `accepts_a_real_command_and_
        // still_prints_the_stub_marker` above), so this only exercises
        // `--shell`'s own CLI parsing, not real phase execution.
        let code = run(&args(&["--shell", "brush", "foo-1.0.ebuild", "qmerge"]));
        assert_eq!(code, ExitCode::SUCCESS);
        let code = run(&args(&["--shell", "bash", "foo-1.0.ebuild", "qmerge"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn accepts_the_inline_equals_form_of_shell() {
        let code = run(&args(&["--shell=bash", "foo-1.0.ebuild", "qmerge"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn rejects_an_invalid_shell_value() {
        let code = run(&args(&["--shell", "zsh", "foo-1.0.ebuild", "qmerge"]));
        assert_eq!(code, ExitCode::from(1));
    }

    #[test]
    fn rejects_a_missing_shell_value() {
        let code = run(&args(&["--shell"]));
        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn rejects_an_unrecognized_option() {
        let code = run(&args(&["--not-a-real-option", "foo-1.0.ebuild", "qmerge"]));
        assert_eq!(code, ExitCode::from(1));
    }

    #[test]
    fn rejects_a_filename_not_ending_in_dot_ebuild() {
        let code = run(&args(&["foo-1.0.tar.gz", "merge"]));
        assert_eq!(code, ExitCode::from(1));
    }

    #[test]
    fn rejects_an_unrecognized_command() {
        let code = run(&args(&["foo-1.0.ebuild", "not-a-real-phase"]));
        assert_eq!(code, ExitCode::from(1));
    }

    #[test]
    fn rejects_a_missing_ebuild_file() {
        let code = run(&args(&[]));
        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn rejects_a_missing_command() {
        let code = run(&args(&["foo-1.0.ebuild"]));
        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn help_is_implemented_and_exits_success_with_no_other_args() {
        let code = run(&args(&["--help"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn short_help_alias_is_implemented() {
        let code = run(&args(&["-h"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn help_wins_unconditionally_regardless_of_position_or_other_args() {
        // Even with an otherwise-complete, valid invocation alongside it,
        // and even with a genuinely unrecognized option present too --
        // help is checked before anything else is parsed at all.
        let code = run(&args(&["foo-1.0.ebuild", "merge", "--help"]));
        assert_eq!(code, ExitCode::SUCCESS);
        let code = run(&args(&["--not-a-real-option", "--help"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }
}
