//! Portage's `ebuild` CLI option and command surface: the six options from
//! `bin/ebuild`'s `argparse` setup and the command list `doebuild()`
//! accepts. Recognizing (not executing) them lets `ebuild.rs` tell "a
//! Portage option/command portuale doesn't implement yet" apart from
//! invalid syntax, the same distinction `emerge_options.rs` draws.
//!
//! `-h`/`--help` is implemented but excluded from `OPTIONS`, like
//! `emerge_options.rs` excludes its own `--help`; `--version` is
//! deliberately not implemented (see notes).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Boolean,
    Value,
}

/// Portage `ebuild` CLI options, from `bin/ebuild`'s `argparse` setup.
/// None have short aliases.
pub const OPTIONS: &[(&str, Kind)] = &[
    ("--force", Kind::Boolean),
    ("--color", Kind::Value),
    ("--debug", Kind::Boolean),
    ("--version", Kind::Boolean),
    ("--ignore-default-opts", Kind::Boolean),
    ("--skip-manifest", Kind::Boolean),
];

/// Portage ebuild commands, from `doebuild()`'s `validcommands` list: the
/// EAPI phases plus non-phase actions (`clean`/`digest`/`manifest`/`merge`/
/// `qmerge`/`rpm`/`unmerge`/`depend`/`fetch`/`fetchall`/`cleanrm`/`help`).
pub const COMMANDS: &[&str] = &[
    "help",
    "clean",
    "prerm",
    "postrm",
    "cleanrm",
    "preinst",
    "postinst",
    "config",
    "info",
    "setup",
    "depend",
    "pretend",
    "fetch",
    "fetchall",
    "digest",
    "unpack",
    "prepare",
    "configure",
    "compile",
    "test",
    "install",
    "instprep",
    "rpm",
    "qmerge",
    "merge",
    "package",
    "unmerge",
    "manifest",
    "nofetch",
];

/// Looks `token` (e.g. `"--color"` or `"--color=y"`) up against
/// `OPTIONS`. Returns the option's `Kind` if it is Portage `ebuild`
/// syntax; `None` if it isn't.
pub fn lookup_option(token: &str) -> Option<Kind> {
    let name = match token.split_once('=') {
        Some((name, _)) if name.starts_with("--") => name,
        _ => token,
    };
    OPTIONS
        .iter()
        .find(|(long, _)| *long == name)
        .map(|(_, kind)| *kind)
}

/// Whether `token` is one of `doebuild()`'s `validcommands`.
pub fn is_valid_command(token: &str) -> bool {
    COMMANDS.contains(&token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_boolean_option() {
        assert_eq!(lookup_option("--force"), Some(Kind::Boolean));
    }

    #[test]
    fn recognizes_a_value_option() {
        assert_eq!(lookup_option("--color"), Some(Kind::Value));
    }

    #[test]
    fn recognizes_the_inline_equals_form_of_a_value_option() {
        assert_eq!(lookup_option("--color=y"), Some(Kind::Value));
    }

    #[test]
    fn does_not_recognize_a_fake_option() {
        assert_eq!(lookup_option("--not-a-real-ebuild-option"), None);
    }

    #[test]
    fn recognizes_real_commands() {
        assert!(is_valid_command("compile"));
        assert!(is_valid_command("merge"));
        assert!(is_valid_command("qmerge"));
        assert!(is_valid_command("digest"));
    }

    #[test]
    fn does_not_recognize_a_fake_command() {
        assert!(!is_valid_command("not-a-real-phase"));
    }
}
