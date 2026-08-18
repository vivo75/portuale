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
// v1 scope cuts). Every other real command (`merge`/`qmerge`/`unmerge`/
// `package`/`preinst`/`postinst`/`prerm`/`postrm`/`config`/`info`/
// `nofetch`/`depend`/`fetch`/`fetchall`/`digest`/`manifest`/`rpm`/
// `instprep`/`clean`/`cleanrm`) still falls through to the pre-existing
// dry-run stub message below unchanged -- most notably `merge`, which
// needs the real vdb/CONTENTS merge machinery task #55 explicitly defers
// (`dblink.merge()`/`treewalk()`/`mergeme()` in
// `lib/portage/dbapi/vartree.py`, ~6500 lines).
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

use crate::ebuild_options::{self, Kind};
use crate::ebuild_phases;
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
    println!("   -h, --help           show this message and exit");
    println!();
    println!(
        "Every other real ebuild option is recognized by name (see bin/ebuild) but \
         not implemented -- using one reports which option it is, instead of a \
         generic error. Real commands (doebuild()'s own validcommands list) are \
         recognized and accepted, still as a no-op."
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

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg.starts_with('-') {
            match ebuild_options::lookup_option(arg) {
                Some(Kind::Value) => {
                    // "--opt=value" carries its value inline; "--opt value"
                    // needs the next token skipped so it isn't mistaken for
                    // the ebuild file or a command.
                    if !arg.contains('=') && i + 1 < args.len() {
                        i += 1;
                    }
                }
                Some(Kind::Boolean) => {}
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

    // Real execution (task #54, ebuild_phases's own module doc comment)
    // only when EVERY requested command is one this pilot actually
    // implements for real (the actionmap_deps-chained phase subset) --
    // a deliberate, simple v1 boundary: no partial-real-execution
    // ambiguity when a request mixes a real phase command with one this
    // pilot still only dry-runs (e.g. `ebuild foo.ebuild compile merge`).
    // A purely dry-run request (the common case today, since `merge` is
    // what most real workflows actually want, and that's still task
    // #55's own deferred territory) keeps the exact pre-existing stub
    // message unchanged.
    if commands
        .iter()
        .all(|cmd| ebuild_phases::is_real_phase_command(cmd))
    {
        let root = portage_repo::root_from_env();
        // Real portage's own make.globals default -- see
        // ebuild_phases::run_commands's own doc comment for why this is
        // read here, at the CLI boundary, rather than internally.
        let portage_tmpdir = std::env::var_os("PORTAGE_TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("/var/tmp/portage"));
        return match ebuild_phases::run_commands(
            std::path::Path::new(ebuild_file),
            &commands,
            &root,
            &portage_tmpdir,
        ) {
            Ok(0) => ExitCode::SUCCESS,
            Ok(_) => ExitCode::from(1),
            Err(e) => {
                eprintln!("ebuild: {e}");
                ExitCode::from(1)
            }
        };
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
        let code = run(&args(&["foo-1.0.ebuild", "merge"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn accepts_multiple_real_commands() {
        let code = run(&args(&["foo-1.0.ebuild", "clean", "compile", "install"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn accepts_a_real_boolean_option() {
        let code = run(&args(&["--force", "foo-1.0.ebuild", "merge"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn accepts_a_real_value_option_without_misreading_its_value() {
        let code = run(&args(&["--color", "y", "foo-1.0.ebuild", "merge"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn accepts_the_inline_equals_form_of_a_value_option() {
        let code = run(&args(&["--color=y", "foo-1.0.ebuild", "merge"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn rejects_an_unrecognized_option() {
        let code = run(&args(&["--not-a-real-option", "foo-1.0.ebuild", "merge"]));
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
