//! Recognition tables for Portage's full `emerge` CLI option surface
//! (`lib/_emerge/main.py`: the `options` list, the `shortmapping` and
//! `argument_options` dicts, and the `actions` frozenset).
//!
//! These tables exist purely for recognition, not behavior: using a
//! Portage flag portuale doesn't implement yet reports "recognized, but
//! not implemented", distinct from a genuinely unknown flag. Every flag
//! portuale implements is excluded here and handled directly by the caller
//! (the implemented set is what `emerge --help` documents, in
//! `pretend/help.txt`); the contract suite pins the text each table
//! produces. Per-flag grounding for each exclusion lives in the notes.
//!
//! Two parsing facts the caller relies on: short boolean flags bundle
//! (`-pv` is argparse-native bundling for plain boolean shorts, not
//! `insert_optional_args`, which separately lets `-v` take an optional
//! value); and `Category::Value` marks which options take an argument,
//! but lookups report-and-exit immediately, so the argument is never
//! parsed here.

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

/// Portage's boolean (no-argument) options: the `options` list plus the two
/// `longopt_aliases` entries. Implemented flags (handled directly by the
/// caller, listed in the module docs) are excluded.
pub const BOOLEAN_OPTIONS: &[(&str, Option<&str>)] = &[
    ("--alphabetical", None),
    ("--buildpkgonly", Some("-B")),
    ("--columns", None),
    ("--debug", Some("-d")),
    ("--digest", None),
    ("--emptytree", Some("-e")),
    ("--verbose-conflicts", None),
    ("--fetchonly", Some("-f")),
    ("--fetch-all-uri", Some("-F")),
    ("--noconfmem", None),
    ("--newrepo", None),
    ("--nobindeps", None),
    ("--quiet-repo-display", None),
    ("--quiet-unmerge-warn", None),
    ("--resume", Some("-r")),
    ("--searchdesc", Some("-S")),
    ("--skipfirst", None),
    ("--tree", Some("-t")),
    ("--unordered-display", None),
    ("--update-if-installed", None),
    ("--cols", None),
    ("--skip-first", None),
];

/// Portage's value-taking options: the `argument_options` dict (each key,
/// with its `shortopt` if any). Same exclusion rule as above.
pub const VALUE_OPTIONS: &[(&str, Option<&str>)] = &[
    ("--alert", Some("-A")),
    // Implemented: the interactive prompt (see pretend.rs).
    ("--autounmask", None),
    // Recognized; inert under --pretend except one warning (see pretend.rs).
    // Implemented: resolve, show problems, exit 0 (see pretend.rs).
    ("--autounmask-license", None),
    ("--autounmask-unrestricted-atoms", None),
    ("--autounmask-use", None),
    ("--autounmask-keep-keywords", None),
    ("--autounmask-keep-masks", None),
    ("--autounmask-write", None),
    ("--accept-properties", None),
    ("--accept-restrict", None),
    ("--backtrack", None),
    ("--buildpkg", Some("-b")),
    ("--buildpkg-exclude", None),
    ("--config-root", None),
    ("--color", None),
    // Implemented: forced deep walk (see pretend.rs and portage-repo).
    ("--depclean-lib-check", None),
    // Implemented: on by default; `=n` walks the vdb dep snapshot.
    ("--fail-clean", None),
    // Implemented: fuzzy matching + regex-auto detection (see pretend.rs).
    ("--ignore-built-slot-operator-deps", None),
    ("--ignore-soname-deps", None),
    // Implemented: drops `@world` from the complete-graph seeds (see pretend.rs).
    ("--jobs", Some("-j")),
    ("--jobs-tmpdir-require-free-gb", None),
    ("--keep-going", None),
    ("--load-average", Some("-l")),
    // Implemented: close-name suggestions for unknown `cat/pkg` (see pretend.rs).
    ("--reinstall", None),
    // Implemented: force a matching installed package to re-merge (see pretend.rs).
    ("--binpkg-respect-use", None),
    ("--getbinpkg", Some("-g")),
    ("--getbinpkgonly", Some("-G")),
    ("--getbinpkg-exclude", None),
    ("--getbinpkg-include", None),
    ("--usepkg-exclude", None),
    ("--usepkg-include", None),
    // Implemented: blank an `--onlydeps` root's runtime deps on `=n` (see pretend.rs).
    // Implemented: filters for the `--rebuild-if-*` scan (see pretend.rs).
    // Implemented: `=n` disables `profiles/updates/` move application.
    ("--prefix", None),
    ("--pkg-format", None),
    // Implemented: source root's installed packages join the binary pool (see portage-repo).
    // Implemented: verbosity control, parsed by the caller (see pretend.rs).
    ("--quiet-fail", None),
    // Implemented: offer `eselect news read` with `--ask` (see pretend.rs).
    // Implemented: rebuild an installed package whose build-time dep is merged this run.
    ("--rebuilt-binaries", None),
    ("--rebuilt-binaries-timestamp", None),
    // Implemented, see the `--fuzzy-search` note above.
    ("--root", None),
    ("--root-deps", None),
    ("--search-index", None),
    ("--select", Some("-w")),
    ("--sync-submodule", None),
    ("--sysroot", None),
    // Implemented: prefer an existing binary over a newer unbuilt ebuild (see portage-repo).
    ("--usepkg", Some("-k")),
    ("--usepkgonly", Some("-K")),
    ("--usepkg-exclude-live", None),
    ("--verbose-missing-ebuilds", None),
    ("--verbose-slot-rebuilds", None),
];

/// Portage's actions (things that replace, not augment, "calculate and show
/// a merge list"): the `actions` frozenset, with `shortmapping` aliases.
/// `--help`/`-h` is excluded (handled by the caller, like `--pretend`).
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
/// `"--deep=1"`) up across all three tables. Returns `None` when it isn't
/// a Portage option/action at all.
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
        let found = lookup("--jobs").unwrap();
        assert_eq!(found.category, Category::Value);
        assert_eq!(found.canonical, "--jobs");
    }

    #[test]
    fn recognizes_a_short_value_option() {
        let found = lookup("-j").unwrap();
        assert_eq!(found.category, Category::Value);
        assert_eq!(found.canonical, "--jobs");
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
        assert!(lookup("--pretend").is_none());
        assert!(lookup("-p").is_none());
    }

    #[test]
    fn does_not_recognize_newuse_itself() {
        assert!(lookup("--newuse").is_none());
        assert!(lookup("-N").is_none());
    }

    #[test]
    fn does_not_recognize_changed_use_itself() {
        assert!(lookup("--changed-use").is_none());
        assert!(lookup("-U").is_none());
    }

    #[test]
    fn does_not_recognize_with_test_deps_itself() {
        assert!(lookup("--with-test-deps").is_none());
    }

    #[test]
    fn does_not_recognize_read_news_itself() {
        assert!(lookup("--read-news").is_none());
    }

    #[test]
    fn does_not_recognize_onlydeps_itself() {
        assert!(lookup("--onlydeps").is_none());
        assert!(lookup("-o").is_none());
    }

    #[test]
    fn does_not_recognize_oneshot_itself() {
        assert!(lookup("--oneshot").is_none());
        assert!(lookup("-1").is_none());
    }

    #[test]
    fn does_not_recognize_deep_itself() {
        assert!(lookup("--deep").is_none());
        assert!(lookup("-D").is_none());
    }

    #[test]
    fn does_not_recognize_exclude_itself() {
        assert!(lookup("--exclude").is_none());
        assert!(lookup("-X").is_none());
    }

    #[test]
    fn does_not_recognize_deselect_itself() {
        assert!(lookup("--deselect").is_none());
        assert!(lookup("-W").is_none());
    }

    #[test]
    fn does_not_recognize_with_bdeps_or_with_bdeps_auto_either() {
        assert!(lookup("--with-bdeps").is_none());
        assert!(lookup("--with-bdeps-auto").is_none());
    }

    #[test]
    fn does_not_recognize_changed_deps_or_changed_deps_report_either() {
        assert!(lookup("--changed-deps").is_none());
        assert!(lookup("--changed-deps-report").is_none());
    }

    #[test]
    fn does_not_recognize_changed_slot_itself() {
        assert!(lookup("--changed-slot").is_none());
    }

    #[test]
    fn does_not_recognize_noreplace_or_selective_either() {
        assert!(lookup("--noreplace").is_none());
        assert!(lookup("--selective").is_none());
    }

    #[test]
    fn does_not_recognize_the_2026_09_02_batch_flags() {
        for flag in [
            "--ask",
            "-a",
            "--autounmask-only",
            "--fuzzy-search",
            "--regex-search-auto",
            "--search-similarity",
        ] {
            assert!(
                lookup(flag).is_none(),
                "{flag} should not be recognized here"
            );
        }
    }

    #[test]
    fn does_not_recognize_ignore_world_itself() {
        // Implemented: handled by the caller. No short alias; `=n` strips before lookup.
        assert!(lookup("--ignore-world").is_none());
        assert!(lookup("--ignore-world=n").is_none());
    }

    #[test]
    fn does_not_recognize_nospinner_itself() {
        // Implemented as a no-op (portuale never animates): handled by the
        // caller, no short alias. Portage: _emerge/main.py
        assert!(lookup("--nospinner").is_none());
    }

    #[test]
    fn does_not_recognize_the_three_3_0_83_options() {
        // Accepted by the caller, so excluded here;
        // `--cycle-break` is not an option in Portage either (a non-goal).
        for flag in [
            "--binpkg-respect-user-patches",
            "--circular-deps-report=json",
            "--merge-wait-scope=none",
            "--cycle-break",
        ] {
            assert!(lookup(flag).is_none(), "{flag}");
        }
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
