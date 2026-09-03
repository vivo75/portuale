// Enumerates the real `ebuild` CLI's option and command surface (see
// bin/ebuild's own `argparse` setup for the six options, and
// `lib/portage/package/ebuild/doebuild.py`'s `validcommands` list --
// what real `doebuild()` actually accepts as a phase/command argument,
// which is a superset of `lib/portage/const.py`'s `EBUILD_PHASES`: it
// also includes non-EAPI-phase actions like `clean`, `digest`,
// `manifest`, `merge`, `qmerge`, `rpm`, `unmerge`, `depend`, `fetch`,
// `fetchall`, `cleanrm`, and `help`). Nothing here is behaviorally
// implemented -- real phase execution is explicitly deferred (see
// docs/agent-context.md's "Deferred: ebuild phase execution") -- but recognizing
// real syntax lets `ebuild.rs` tell "a real ebuild option/command this
// portuale doesn't implement yet" apart from "not valid ebuild syntax at
// all", the same distinction `emerge_options.rs` draws for `emerge`.
// Unlike `emerge --pretend`, `ebuild` has no Python reference
// implementation at all -- it's Rust-only, tested directly against the
// compiled binary by `tests/test_portuale.py` (see that
// file's own doc comment).
//
// `-h`/`--help` IS implemented now, deliberately excluded from
// `OPTIONS` for the same reason `emerge_options.rs`'s own `--help`/`-h`
// is excluded from its tables -- see `ebuild.rs` for the portuale-specific
// help text. Real bin/ebuild declares no short aliases for any of its
// own six options ("None have short aliases" below still holds), so
// `-h` is purely argparse's own auto-added short form for `--help`; no
// bundling concept exists for `ebuild` at all (unlike `emerge`'s own
// `-pv`-style bundling), so this is a plain whole-token check.
//
// `--version` is deliberately NOT implemented, despite looking like an
// equally simple case at first (and having been scoped that way before
// digging in): real bin/ebuild's own `print("Portage", portage.VERSION)`
// looks static, but `portage.VERSION` (lib/portage/__init__.py) is only
// a fixed, build-substituted `"@VERSION@"` string for an *installed*
// copy -- for a `installation.TYPE == installation.TYPES.SOURCE`
// checkout (exactly what this repo itself is, confirmed by there being
// no `lib/portage/VERSION` file anywhere in it), `VERSION` is instead
// derived live via `git describe --dirty --long --match "portage-*"`
// against the current commit and working-tree state (verified directly:
// `git describe` in this repo returns `portage-3.0.81-272-g1cb1941de`,
// parsed into `3.0.81.dev272+g1cb1941de`) -- host/git-state-dependent,
// non-deterministic output portuale's own pinned-test philosophy
// already rules out elsewhere (the same reasoning that ruled out
// `emerge --version`/`-V`'s own `getportageversion()`, which pulls in
// live python/gcc/libc/uname info -- a different real function, same
// disqualifying trait).

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
