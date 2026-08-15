// `ebuild <file> <command> [command...]`: still a pure dry-run stub --
// real phase execution is explicitly deferred (see PROMPT.md's
// "Deferred: ebuild phase execution", which requires shelling out to a
// real bash, a deliberate dynamic dependency this pilot isn't taking on
// yet). What this file *does* implement is CLI-surface recognition,
// mirroring emerge's own pretend.rs/emerge_options.rs treatment: real
// options (see ebuild_options.rs, transcribed from bin/ebuild's own
// argparse setup) and real commands (from doebuild()'s own
// `validcommands` list) are recognized and accepted -- still a no-op,
// still exits 0, still prints the "ebuild (pilot stub)" marker -- while
// genuinely invalid input (an unrecognized option, a filename not
// ending in ".ebuild", an unrecognized command, or a missing
// file/command) is now rejected with a specific, accurate message
// instead of silently accepted the way the original bare-bones stub
// accepted literally anything.
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

use crate::ebuild_options::{self, Kind};
use std::process::ExitCode;

pub fn run(args: &[String]) -> ExitCode {
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
}
