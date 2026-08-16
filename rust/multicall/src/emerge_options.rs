// Enumerates the real `emerge` CLI's full option surface (see
// lib/_emerge/main.py: the `options` list, `shortmapping` dict,
// `argument_options` dict, and `actions` frozenset), so that using any
// real emerge flag this pilot doesn't implement yet produces a clear
// "recognized, but not implemented" message -- distinct from a
// genuinely unknown/misspelled flag. Only `--pretend`/`-p`,
// `--verbose`/`-v`, and `--help`/`-h` are actually implemented (see
// pretend.rs); every table here exists purely for recognition, not
// behavior. Mirrored exactly in
// PORTING/python/emerge_pretend_reference.py's own copy of these same
// three tables, so both sides report identical text for identical input
// (verified by the shared contract suite).
//
// KNOWN, DOCUMENTED SCOPE CUTS:
//   - Short-flag bundling (`-pv`) IS supported -- see pretend.rs's own
//     doc comment for the algorithm. It's real `argparse`-native
//     behavior for plain boolean short options, not (as an earlier
//     version of this comment claimed) something `insert_optional_args`
//     alone provides; verified empirically against real `argparse`
//     before relying on it. `insert_optional_args` is real and separate:
//     it's what lets a handful of specific short options (`-v` among
//     them) take an *optional* value, which argparse's own bundling
//     can't express on its own -- see pretend.rs for the `-v`-specific
//     port of that piece.
//   - `Category::Value` marks which real options take an argument
//     (real emerge's own `argument_options` dict), for accurate
//     enumeration, but the caller (`pretend.rs`) never needs to parse
//     or skip over that argument: every lookup here reports and exits
//     immediately, so nothing after a recognized-but-unimplemented
//     option is ever looked at again in that invocation.
//   - `--help`/`-h` IS implemented now, deliberately excluded from
//     `ACTIONS` below for the same reason `--pretend`/`-p` and
//     `--verbose`/`-v` are excluded from their own tables -- see
//     pretend.rs for the pilot-specific (not a port of real emerge's own
//     `_emerge/help.py`) help text, and why it's checked before anything
//     else, unconditionally, regardless of position in argv.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Boolean,
    Value,
    Action,
}

#[derive(Debug, Clone, Copy)]
pub struct Lookup {
    pub category: Category,
    pub canonical: &'static str,
}

/// Real boolean (no-argument) options, from `main.py`'s `options` list
/// plus its two `longopt_aliases` entries (`--cols`, `--skip-first`) --
/// `--pretend`/`-p` and `--verbose`/`-v` are both deliberately excluded,
/// since they're implemented and handled directly by the caller, not
/// through this "not implemented" table.
pub const BOOLEAN_OPTIONS: &[(&str, Option<&str>)] = &[
    ("--alphabetical", None),
    ("--ask-enter-invalid", None),
    ("--buildpkgonly", Some("-B")),
    ("--changed-use", Some("-U")),
    ("--columns", None),
    ("--debug", Some("-d")),
    ("--digest", None),
    ("--emptytree", Some("-e")),
    ("--verbose-conflicts", None),
    ("--fetchonly", Some("-f")),
    ("--fetch-all-uri", Some("-F")),
    ("--ignore-default-opts", None),
    ("--noconfmem", None),
    ("--newrepo", None),
    ("--newuse", Some("-N")),
    ("--nobindeps", None),
    ("--nodeps", Some("-O")),
    ("--noreplace", Some("-n")),
    ("--nospinner", None),
    ("--oneshot", Some("-1")),
    ("--onlydeps", Some("-o")),
    ("--quiet-repo-display", None),
    ("--quiet-unmerge-warn", None),
    ("--resume", Some("-r")),
    ("--searchdesc", Some("-S")),
    ("--skipfirst", None),
    ("--tree", Some("-t")),
    ("--unordered-display", None),
    ("--update", Some("-u")),
    ("--update-if-installed", None),
    ("--cols", None),
    ("--skip-first", None),
];

/// Real value-taking options, from `main.py`'s `argument_options` dict
/// (each key, with its `"shortopt"` if any).
pub const VALUE_OPTIONS: &[(&str, Option<&str>)] = &[
    ("--alert", Some("-A")),
    ("--ask", Some("-a")),
    ("--autounmask", None),
    ("--autounmask-backtrack", None),
    ("--autounmask-continue", None),
    ("--autounmask-only", None),
    ("--autounmask-license", None),
    ("--autounmask-unrestricted-atoms", None),
    ("--autounmask-use", None),
    ("--autounmask-keep-keywords", None),
    ("--autounmask-keep-masks", None),
    ("--autounmask-write", None),
    ("--accept-properties", None),
    ("--accept-restrict", None),
    ("--backtrack", None),
    ("--binpkg-changed-deps", None),
    ("--buildpkg", Some("-b")),
    ("--buildpkg-exclude", None),
    ("--changed-deps", None),
    ("--changed-deps-report", None),
    ("--changed-slot", None),
    ("--config-root", None),
    ("--color", None),
    ("--complete-graph", None),
    ("--complete-graph-if-new-use", None),
    ("--complete-graph-if-new-ver", None),
    ("--deep", Some("-D")),
    ("--depclean-lib-check", None),
    ("--deselect", Some("-W")),
    ("--dynamic-deps", None),
    ("--exclude", Some("-X")),
    ("--fail-clean", None),
    ("--fuzzy-search", None),
    ("--ignore-built-slot-operator-deps", None),
    ("--ignore-soname-deps", None),
    ("--ignore-world", None),
    ("--implicit-system-deps", None),
    ("--jobs", Some("-j")),
    ("--jobs-tmpdir-require-free-gb", None),
    ("--keep-going", None),
    ("--load-average", Some("-l")),
    ("--misspell-suggestions", None),
    ("--with-bdeps", None),
    ("--with-bdeps-auto", None),
    ("--reinstall", None),
    ("--reinstall-atoms", None),
    ("--binpkg-respect-use", None),
    ("--getbinpkg", Some("-g")),
    ("--getbinpkgonly", Some("-G")),
    ("--getbinpkg-exclude", None),
    ("--getbinpkg-include", None),
    ("--usepkg-exclude", None),
    ("--usepkg-include", None),
    ("--onlydeps-with-ideps", None),
    ("--onlydeps-with-rdeps", None),
    ("--rebuild-exclude", None),
    ("--rebuild-ignore", None),
    ("--package-moves", None),
    ("--prefix", None),
    ("--pkg-format", None),
    ("--quickpkg-direct", None),
    ("--quickpkg-direct-root", None),
    ("--quiet", Some("-q")),
    ("--quiet-build", None),
    ("--quiet-fail", None),
    ("--read-news", None),
    ("--rebuild-if-new-slot", None),
    ("--rebuild-if-new-rev", None),
    ("--rebuild-if-new-ver", None),
    ("--rebuild-if-unbuilt", None),
    ("--rebuilt-binaries", None),
    ("--rebuilt-binaries-timestamp", None),
    ("--regex-search-auto", None),
    ("--root", None),
    ("--root-deps", None),
    ("--search-index", None),
    ("--search-similarity", None),
    ("--select", Some("-w")),
    ("--selective", None),
    ("--sync-submodule", None),
    ("--sysroot", None),
    ("--use-ebuild-visibility", None),
    ("--useoldpkg-atoms", None),
    ("--usepkg", Some("-k")),
    ("--usepkgonly", Some("-K")),
    ("--usepkg-exclude-live", None),
    ("--verbose-missing-ebuilds", None),
    ("--verbose-slot-rebuilds", None),
    ("--with-test-deps", None),
];

/// Real actions (things that replace, not augment, "calculate and show a
/// merge list" -- `--depclean`, `--sync`, `--search`, etc.), from
/// `main.py`'s `actions` frozenset, with short aliases from
/// `shortmapping` where real emerge defines one. `--help`/`-h` is
/// deliberately excluded -- see the module doc comment.
pub const ACTIONS: &[(&str, Option<&str>)] = &[
    ("--clean", None),
    ("--check-news", None),
    ("--config", None),
    ("--depclean", Some("-c")),
    ("--info", None),
    ("--list-sets", None),
    ("--metadata", None),
    ("--moo", None),
    ("--prune", Some("-P")),
    ("--rage-clean", None),
    ("--regen", None),
    ("--search", Some("-s")),
    ("--status", None),
    ("--sync", None),
    ("--unmerge", Some("-C")),
    ("--version", Some("-V")),
];

fn find(table: &[(&'static str, Option<&'static str>)], name: &str) -> Option<&'static str> {
    table
        .iter()
        .find(|(long, short)| *long == name || *short == Some(name))
        .map(|(long, _)| *long)
}

/// Looks `token` (a single argv entry, e.g. `"--deep"`, `"-D"`, or
/// `"--deep=1"`) up across all three tables. Returns `None` if it isn't
/// any real emerge option/action this table knows about at all.
pub fn lookup(token: &str) -> Option<Lookup> {
    let name = match token.split_once('=') {
        Some((name, _)) if name.starts_with("--") => name,
        _ => token,
    };
    if let Some(canonical) = find(BOOLEAN_OPTIONS, name) {
        return Some(Lookup {
            category: Category::Boolean,
            canonical,
        });
    }
    if let Some(canonical) = find(VALUE_OPTIONS, name) {
        return Some(Lookup {
            category: Category::Value,
            canonical,
        });
    }
    if let Some(canonical) = find(ACTIONS, name) {
        return Some(Lookup {
            category: Category::Action,
            canonical,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_long_boolean_option() {
        let found = lookup("--debug").unwrap();
        assert_eq!(found.category, Category::Boolean);
        assert_eq!(found.canonical, "--debug");
    }

    #[test]
    fn recognizes_a_short_boolean_option_and_reports_the_canonical_long_name() {
        let found = lookup("-d").unwrap();
        assert_eq!(found.category, Category::Boolean);
        assert_eq!(found.canonical, "--debug");
    }

    #[test]
    fn recognizes_a_long_value_option() {
        let found = lookup("--deep").unwrap();
        assert_eq!(found.category, Category::Value);
        assert_eq!(found.canonical, "--deep");
    }

    #[test]
    fn recognizes_a_short_value_option() {
        let found = lookup("-D").unwrap();
        assert_eq!(found.category, Category::Value);
        assert_eq!(found.canonical, "--deep");
    }

    #[test]
    fn recognizes_the_inline_equals_form_of_a_value_option() {
        let found = lookup("--jobs=4").unwrap();
        assert_eq!(found.category, Category::Value);
        assert_eq!(found.canonical, "--jobs");
    }

    #[test]
    fn recognizes_an_action_and_its_short_alias() {
        let found = lookup("--depclean").unwrap();
        assert_eq!(found.category, Category::Action);
        let found_short = lookup("-c").unwrap();
        assert_eq!(found_short.category, Category::Action);
        assert_eq!(found_short.canonical, "--depclean");
    }

    #[test]
    fn does_not_recognize_pretend_itself() {
        // --pretend/-p is handled directly by the caller (it's
        // implemented), not through this "not implemented" table.
        assert!(lookup("--pretend").is_none());
        assert!(lookup("-p").is_none());
    }

    #[test]
    fn does_not_recognize_a_bundled_short_flag() {
        assert!(lookup("-pv").is_none());
    }

    #[test]
    fn does_not_recognize_a_fake_option() {
        assert!(lookup("--this-is-not-a-real-emerge-option").is_none());
    }
}
