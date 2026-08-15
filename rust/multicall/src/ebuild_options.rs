// Enumerates the real `ebuild` CLI's option and command surface (see
// bin/ebuild's own `argparse` setup for the six options, and
// `lib/portage/package/ebuild/doebuild.py`'s `validcommands` list --
// what real `doebuild()` actually accepts as a phase/command argument,
// which is a superset of `lib/portage/const.py`'s `EBUILD_PHASES`: it
// also includes non-EAPI-phase actions like `clean`, `digest`,
// `manifest`, `merge`, `qmerge`, `rpm`, `unmerge`, `depend`, `fetch`,
// `fetchall`, `cleanrm`, and `help`). Nothing here is behaviorally
// implemented -- real phase execution is explicitly deferred (see
// PROMPT.md's "Deferred: ebuild phase execution") -- but recognizing
// real syntax lets `ebuild.rs` tell "a real ebuild option/command this
// pilot doesn't implement yet" apart from "not valid ebuild syntax at
// all", the same distinction `emerge_options.rs` draws for `emerge`.
// Mirrored exactly in
// PORTING/python/emerge_pretend_reference.py's sibling `ebuild`
// reference script (see that file), so both sides report identical
// text for identical input.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Boolean,
    Value,
}

/// Real `ebuild` CLI options, from `bin/ebuild`'s own `argparse` setup.
/// None have short aliases.
pub const OPTIONS: &[(&str, Kind)] = &[
    ("--force", Kind::Boolean),
    ("--color", Kind::Value),
    ("--debug", Kind::Boolean),
    ("--version", Kind::Boolean),
    ("--ignore-default-opts", Kind::Boolean),
    ("--skip-manifest", Kind::Boolean),
];

/// Real ebuild commands, from `doebuild()`'s own `validcommands` list --
/// the authoritative set (not just EAPI phase names): includes
/// non-phase doebuild actions like `clean`/`digest`/`manifest`/`merge`/
/// `qmerge`/`rpm`/`unmerge`/`depend`/`fetch`/`fetchall`/`cleanrm`/`help`
/// alongside the real EAPI phases.
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
/// `OPTIONS`. Returns the option's `Kind` if it's a real one; `None` if
/// it isn't real `ebuild` syntax at all.
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

/// Whether `token` is one of the real `doebuild()` `validcommands`.
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
