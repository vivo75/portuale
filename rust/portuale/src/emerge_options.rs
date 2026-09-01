// Enumerates the real `emerge` CLI's full option surface (see
// lib/_emerge/main.py: the `options` list, `shortmapping` dict,
// `argument_options` dict, and `actions` frozenset), so that using any
// real emerge flag this pilot doesn't implement yet produces a clear
// "recognized, but not implemented" message -- distinct from a
// genuinely unknown/misspelled flag. Only `--pretend`/`-p`,
// `--verbose`/`-v`, `--help`/`-h`, `--newuse`/`-N`, `--changed-use`/`-U`,
// `--nodeps`/`-O`, `--onlydeps`/`-o`, and `--update`/`-u` are actually
// implemented (see pretend.rs); every table here exists purely for
// recognition, not
// behavior. Mirrored exactly in
// python/emerge_pretend_reference.py's own copy of these same
// three tables, so both sides report identical text for identical input
// (verified by the shared contract suite). `--deep`/`-D`,
// `--exclude`/`-X`, `--deselect`/`-W`, `--with-bdeps`,
// `--with-bdeps-auto`, `--changed-deps`, `--changed-slot`, and
// `--with-test-deps` are ALSO implemented now (see below) -- all
// excluded from `VALUE_OPTIONS` too, not just `BOOLEAN_OPTIONS`.
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
//   - `--newuse`/`-N` and `--changed-use`/`-U` (a real, narrower
//     alternative) ARE both implemented now too, deliberately excluded
//     from `BOOLEAN_OPTIONS` for the same reason -- see pretend.rs for
//     the flag parsing and portage-repo's
//     `reinstall_flags_for_use_change` for the reinstall-detection
//     logic (and how the two flags combine when both are given).
//   - `--nodeps`/`-O` IS implemented now too, deliberately excluded from
//     `BOOLEAN_OPTIONS` for the same reason -- see
//     `resolve_pretend_graph`'s own doc comment (portage-repo) for how it
//     disables the dependency walk entirely.
//   - `--onlydeps`/`-o`, `--nodeps`'s real complement (man/emerge.1:
//     "Only merge (or pretend to merge) the dependencies of the packages
//     specified, not the packages themselves"), IS implemented now too,
//     deliberately excluded from `BOOLEAN_OPTIONS` for the same reason --
//     see pretend.rs's print loop, which is the only thing it changes:
//     `resolve_pretend_graph` itself needs no changes at all, since
//     dependency recursion already happens identically regardless of
//     which top-level entries end up printed.
//   - `--update`/`-u` IS implemented now too, deliberately excluded from
//     `BOOLEAN_OPTIONS` for the same reason -- see `resolve_pretend`'s
//     own doc comment (portage-repo) for the real `avoid_update`/
//     `dont_miss_updates` behavior it ports: without it, an
//     already-installed version that still satisfies the requested atom
//     is kept as-is, never upgraded to a newer visible version just
//     because one exists.
//   - `--deep`/`-D` IS implemented now too, deliberately excluded from
//     `VALUE_OPTIONS` for the same reason -- it's real `argument_options`
//     with an *optional* value (`--deep` alone means unlimited depth;
//     `--deep=N`/`--deep N` bounds it), not a plain boolean, so
//     pretend.rs's own parsing follows the same `insert_optional_args`
//     pattern already established there for `--verbose`/`-v`. See
//     `resolve_pretend_graph`'s own doc comment (portage-repo) for
//     `Deep`, the real depth-cutoff semantics it ports.
//   - `--exclude`/`-X` IS implemented now too, deliberately excluded from
//     `VALUE_OPTIONS` for the same reason -- unlike every other value
//     option here, its own value is *required*, not optional, and real
//     `main.py` declares it `"action": "append"` (repeatable, each
//     occurrence's own value itself a space-separated atom list -- see
//     pretend.rs's own parsing). Deliberately NOT bundle-compatible (a
//     bundled `-X` gets its own specific "requires an argument, can't be
//     bundled" message instead), since there's no sensible default value
//     the way a bundled `-v`/`-D` has. See `resolve_pretend`'s own doc
//     comment (portage-repo) for the real `excluded_pkgs`/
//     `WildcardPackageSet` behavior it ports, and its own documented
//     scope cut relative to real depgraph.py's ~18 call sites.
//   - `--deselect`/`-W` IS implemented now too, deliberately excluded
//     from `VALUE_OPTIONS` for the same reason -- like `--verbose`/`-v`,
//     it's real `argument_options` with a `y_or_n`-choices *optional*
//     value, ported with the exact same "peek the next token, consume
//     only if it's exactly y/n" pattern. Unlike every other implemented
//     flag here, though, a bare `--deselect`/`-W` turns the whole
//     invocation into a different, standalone action (real `main.py`'s
//     own "if myaction is None and myoptions.deselect is True: myaction
//     = 'deselect'") rather than modifying the ordinary `--pretend`
//     resolution -- see `pretend.rs`'s own `run_deselect` for the real
//     `action_deselect` behavior it ports, and its own documented
//     `Atom.intersects()` scope cut.
//   - `--with-bdeps` IS implemented now too, deliberately excluded from
//     `VALUE_OPTIONS` for the same reason -- real `argument_options` with
//     `"choices": ("y", "n")`, a REQUIRED closed-choice value (unlike
//     `--exclude`'s arbitrary text, or `--deep`/`--verbose`'s optional
//     peek): a missing value, or one that's neither `y` nor `n`, is a
//     real, immediate usage error either way (the latter is real
//     `argparse`'s own choices validation), so it has no short alias and
//     is deliberately NOT bundle-compatible either -- real `main.py`
//     declares no `shortopt` for it at all, so there's no bundling
//     concept to begin with, unlike `--exclude`'s own deliberate bundling
//     cut. See `resolve_pretend_graph`'s own doc comment (portage-repo)
//     for the real `depgraph.py`/`create_depgraph_params.py` `bdeps`
//     semantics it ports (`n` skips DEPEND/BDEPEND for an
//     already-installed package's own dependency walk under `--deep`;
//     `y`/the real default `auto` both keep them, collapsed into one
//     bool since `depgraph.py` itself never distinguishes the two).
//     `--with-bdeps-auto` (the only other real lever on this same
//     `bdeps` value) IS implemented too now, deliberately excluded from
//     `VALUE_OPTIONS` for the same reason -- the identical REQUIRED
//     closed-choice shape `--with-bdeps` itself has. See `pretend.rs`'s
//     own CLI parsing for the precedence: `--with-bdeps-auto=n` only
//     changes the *default* `with_bdeps` value (from real portage's own
//     "auto" to "n") -- an explicit `--with-bdeps` always wins
//     regardless, matching real `create_depgraph_params.py`'s own `bdeps
//     = myopts.get("--with-bdeps"); if bdeps is not None: ... elif
//     ... myopts.get("--with-bdeps-auto") != "n" ...: myparams["bdeps"] =
//     "auto"` -- the `--usepkg`-gated half of that same real condition
//     is always true here, since this pilot's CLI has no `--usepkg` at
//     all.
//   - `--changed-deps` IS implemented now too, deliberately excluded from
//     `VALUE_OPTIONS` for the same reason -- real `default_arg_opts` with
//     a `y_or_n` *optional* value, the identical shape `--verbose`/`-v`
//     and `--deselect`/`-W` already have (no short alias for this one,
//     though -- real `main.py` declares none). See `PretendOutcome::
//     Reinstall`'s own doc comment (portage-repo) and `deps_changed`'s
//     own doc comment for the real `depgraph.py::_changed_deps` behavior
//     it ports (reinstalls an already-installed package whose own
//     vdb-recorded dependency strings differ from the repo's current
//     ebuild) and its own documented flat-comparison scope cut (this
//     pilot has no structured, non-flat `use_reduce` anywhere, so a
//     dependency moved between two dep-string keys with the same net
//     atom set, or a pure `||`-restructuring, isn't detected as
//     "changed" here the way real portage's own structured comparison
//     would). `--changed-deps-report` IS implemented now too,
//     deliberately excluded from `VALUE_OPTIONS` for the same reason --
//     the identical `y_or_n` optional-value shape `--changed-deps`
//     itself already has (no short alias for this one either; real
//     `main.py` declares none). See `resolve_pretend_graph`'s own doc
//     comment (portage-repo) for the real `depgraph.py::
//     _changed_deps_report` behavior this ports: a report-only WARN,
//     never a resolution change, reusing `deps_changed` unmodified for
//     the comparison itself. Real portage: "This is completely silent...
//     if --changed-deps or --dynamic-deps is enabled" -- ported as
//     simply never bothering to compute anything at all when
//     `changed_deps` is true, since real portage's own
//     `_changed_deps_pkgs` collection is discarded unread in that case
//     anyway (a documented, behavior-preserving simplification, not a
//     guess). `--dynamic-deps` itself stays unimplemented/unrecognized
//     in this pilot (real portage's own now-defunct alternate resolver
//     strategy), so only the `--changed-deps` half of that real
//     silencing condition is reachable here at all.
//   - `--changed-slot` IS implemented now too, deliberately excluded
//     from `VALUE_OPTIONS` for the same reason -- real `default_arg_opts`
//     with a `y_or_n` *optional* value, the identical shape
//     `--changed-deps` already has (no short alias for this one either).
//     See `PretendOutcome::Reinstall`'s own doc comment (portage-repo)
//     and `slot_changed`'s own doc comment for the real
//     `depgraph.py::_changed_slot` behavior it ports (reinstalls an
//     already-installed package whose own vdb-recorded `SLOT` differs
//     from the repo's current ebuild) and its own documented scope cut:
//     real portage's own consumers of `_changed_slot` live deep inside
//     binary-package/slot-operator-rebuild scheduling this pilot has
//     none of, so this is ported as simply another independent
//     `Reinstall` trigger instead of replicating that considerably
//     messier real control flow.
//   - `--with-test-deps` IS implemented now too, deliberately excluded
//     from `VALUE_OPTIONS` for the same reason -- the identical
//     `y_or_n` optional-value shape `--changed-slot` already has (no
//     short alias for this one either). See `resolve_pretend_graph`'s
//     own doc comment (portage-repo) for the real `depgraph.py::
//     _add_pkg` gating it ports (top-level atoms only, "test" a valid,
//     not-already-enabled, not-use-masked IUSE flag) and
//     `use_reduce_flat_subset`'s own doc comment (portage-use-reduce)
//     for the real `use_reduce(..., subset={"test"})` extraction it's
//     built on.
//   - `--noreplace`/`-n` and `--selective` ARE implemented now too,
//     found and grounded by comparing this pilot's own output against
//     the real, installed system `emerge` on a real package
//     (`sys-apps/portage`) and tracing real portage's own decision
//     live: a bare `emerge <atom>` with no other flags does NOT keep an
//     already-installed, already-satisfying package as-is in real
//     portage -- it's reinstalled anyway, unless real `myparams[
//     "selective"]` is set, which `--update`/`--newuse`/`--changed-use`/
//     `--changed-deps`/`--changed-slot`/`--noreplace`/`--selective`/
//     `--newrepo` all independently do (`create_depgraph_params.py`).
//     `--noreplace` is a plain boolean, hence bundle-compatible
//     (`-pn`, alongside the other bundle-compatible booleans);
//     `--selective` has the identical *meaning* but a real `y_or_n`
//     *optional* value instead (the same shape `--changed-deps` already
//     has, deliberately excluded from `VALUE_OPTIONS` for the same
//     reason, no short alias of its own -- "n" explicitly cancels
//     `selective` even if another flag already set it, real
//     `create_depgraph_params.py`'s own unconditional `if myopts.get(
//     "--selective") == "n": pop`). See `resolve_pretend`'s own doc
//     comment (portage-repo) for the full grounding, including why this
//     pilot's own `selective` computation needs no separate
//     `--reinstall` flag (`--changed-use` already covers its whole real
//     contribution) and the documented, narrower scope cut around real
//     `--newrepo` (needs a vdb `REPOSITORY` reader this pilot doesn't
//     have).

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
/// `--pretend`/`-p`, `--verbose`/`-v`, `--newuse`/`-N`,
/// `--changed-use`/`-U`, `--nodeps`/`-O`, `--onlydeps`/`-o`,
/// `--oneshot`/`-1`, and
/// `--update`/`-u` are all deliberately excluded, since they're
/// implemented and handled directly by the caller, not through this "not
/// implemented" table.
pub const BOOLEAN_OPTIONS: &[(&str, Option<&str>)] = &[
    ("--alphabetical", None),
    ("--ask-enter-invalid", None),
    ("--buildpkgonly", Some("-B")),
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
    ("--nobindeps", None),
    ("--nospinner", None),
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
    ("--config-root", None),
    ("--color", None),
    ("--complete-graph", None),
    ("--complete-graph-if-new-use", None),
    ("--complete-graph-if-new-ver", None),
    ("--depclean-lib-check", None),
    ("--dynamic-deps", None),
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
    // `--quiet`/`-q` (real `true_y_or_n`, verbosity level 1) IS
    // implemented now -- deliberately excluded here for the same reason
    // `--verbose`/`-v` is: the caller parses it directly. See pretend.rs.
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
    ("--sync-submodule", None),
    ("--sysroot", None),
    ("--use-ebuild-visibility", None),
    ("--useoldpkg-atoms", None),
    ("--usepkg", Some("-k")),
    ("--usepkgonly", Some("-K")),
    ("--usepkg-exclude-live", None),
    ("--verbose-missing-ebuilds", None),
    ("--verbose-slot-rebuilds", None),
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
        // --pretend/-p is handled directly by the caller (it's
        // implemented), not through this "not implemented" table.
        assert!(lookup("--pretend").is_none());
        assert!(lookup("-p").is_none());
    }

    #[test]
    fn does_not_recognize_newuse_itself() {
        // --newuse/-N is handled directly by the caller (it's
        // implemented), not through this "not implemented" table.
        assert!(lookup("--newuse").is_none());
        assert!(lookup("-N").is_none());
    }

    #[test]
    fn does_not_recognize_changed_use_itself() {
        // --changed-use/-U, --newuse's real, narrower alternative, is
        // now implemented and handled directly by the caller too, not
        // through this "not implemented" table.
        assert!(lookup("--changed-use").is_none());
        assert!(lookup("-U").is_none());
    }

    #[test]
    fn does_not_recognize_nodeps_itself() {
        // --nodeps/-O is handled directly by the caller (it's
        // implemented), not through this "not implemented" table.
        assert!(lookup("--nodeps").is_none());
        assert!(lookup("-O").is_none());
    }

    #[test]
    fn does_not_recognize_onlydeps_itself() {
        // --onlydeps/-o, --nodeps's real complement, is now implemented
        // and handled directly by the caller too, not through this "not
        // implemented" table.
        assert!(lookup("--onlydeps").is_none());
        assert!(lookup("-o").is_none());
    }

    #[test]
    fn does_not_recognize_oneshot_itself() {
        // --oneshot/-1 is implemented now (suppresses the world-file
        // write on a real merge, and the world colour at --pretend) --
        // handled directly by the caller, not this "not implemented" table.
        assert!(lookup("--oneshot").is_none());
        assert!(lookup("-1").is_none());
    }

    #[test]
    fn does_not_recognize_deep_itself() {
        // --deep/-D is handled directly by the caller (it's
        // implemented), not through this "not implemented" table.
        assert!(lookup("--deep").is_none());
        assert!(lookup("-D").is_none());
    }

    #[test]
    fn does_not_recognize_exclude_itself() {
        // --exclude/-X is handled directly by the caller (it's
        // implemented), not through this "not implemented" table.
        assert!(lookup("--exclude").is_none());
        assert!(lookup("-X").is_none());
    }

    #[test]
    fn does_not_recognize_deselect_itself() {
        // --deselect/-W is handled directly by the caller (it's
        // implemented), not through this "not implemented" table.
        assert!(lookup("--deselect").is_none());
        assert!(lookup("-W").is_none());
    }

    #[test]
    fn does_not_recognize_with_bdeps_or_with_bdeps_auto_either() {
        // Both are handled directly by the caller now (both
        // implemented), not through this "not implemented" table.
        assert!(lookup("--with-bdeps").is_none());
        assert!(lookup("--with-bdeps-auto").is_none());
    }

    #[test]
    fn does_not_recognize_changed_deps_or_changed_deps_report_either() {
        // Both are handled directly by the caller now (both
        // implemented), not through this "not implemented" table.
        assert!(lookup("--changed-deps").is_none());
        assert!(lookup("--changed-deps-report").is_none());
    }

    #[test]
    fn does_not_recognize_changed_slot_itself() {
        // --changed-slot is handled directly by the caller (it's
        // implemented), not through this "not implemented" table.
        assert!(lookup("--changed-slot").is_none());
    }

    #[test]
    fn does_not_recognize_with_test_deps_itself() {
        // --with-test-deps is handled directly by the caller (it's
        // implemented), not through this "not implemented" table.
        assert!(lookup("--with-test-deps").is_none());
    }

    #[test]
    fn does_not_recognize_noreplace_or_selective_either() {
        // Both are handled directly by the caller now (both
        // implemented), not through this "not implemented" table.
        assert!(lookup("--noreplace").is_none());
        assert!(lookup("--selective").is_none());
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
