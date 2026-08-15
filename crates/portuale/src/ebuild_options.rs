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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_inline_equals_form_of_a_value_option() {
        assert_eq!(lookup_option("--color=y"), Some(Kind::Value));
    }

    #[test]
    fn does_not_recognize_a_fake_option() {
        assert_eq!(lookup_option("--not-a-real-ebuild-option"), None);
    }
}
