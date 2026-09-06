// `mrg`: a new applet with deliberately relaxed requirements. The whole
// point is that `emerge`'s tight constraints do NOT apply to it: `mrg`
// is a from-scratch, plain-spoken re-take on the package-manager front
// end, allowed to lean on major mainstream crates (`clap` here, `serde`
// and friends later) instead of portuale's hand-rolled, minimal-
// dependency machinery. It does not have to live on a minimal static
// musl box, match a Python reference byte-for-byte, or stay near-zero-
// dependency. `mrg` is a **portuale-only applet -- there will never be a
// portage counterpart or Python reference implementation**; only this
// Rust code and its tests define it. What it DOES share is real
// `emerge`'s actual option surface:
// every short option and every long option keeps real emerge's own
// spelling exactly (the authoritative source is `lib/_emerge/main.py`'s
// `options` list, `shortmapping` dict, `longopt_aliases` dict,
// `argument_options` dict, and `actions` frozenset), so muscle memory
// carries straight over. What may differ is only the *semantics* beyond
// the names (and even those mostly line up, where clap lets them
// cheaply).
//
// This first slice is deliberately just the parser: `mrg` accepts the
// real emerge option surface via `clap`, and reports exactly what it
// parsed (options, values, and the target package atoms) -- a
// transparent, deterministic echo that the clap parser is the thing
// being exercised. No resolution, no merge, no world-file writes. That
// is the "simple command-line option parser" step the applet was asked
// to start with.
//
// FULL option-surface fidelity notes (grounded in main.py, read before
// deviating):
//
//   - Every plain boolean from `options` + `actions` becomes a clap
//     `ArgAction::SetTrue` flag with its real short alias (from
//     `shortmapping`) attached. clap's native short-flag bundling
//     (`-pv`, `-pv1`, ...) therefore behaves like real `argparse`.
//     `--cols`/`--skip-first` are real `longopt_aliases` secondary
//     names for `--columns`/`--skipfirst`, modelled as clap `alias`es.
//   - `-h`/`--help` is clap's own (real emerge declares `help` in its
//     `actions` set, and `-h` in `shortmapping`; clap's built-in help
//     keeps that exact spelling).
//   - Deliberate cuts, all within the "full equality is NOT required,
//     keep short and long options as-is" licence:
//       * The y/n/`True` *optional-value* options (main.py's own
//         `insert_optional_args`/`default_arg_opts`: `--ask`, `--verbose`,
//         `--quiet`, `--autounmask`, `--buildpkg`, `--usepkg`, ...) are
//         modelled as plain `SetTrue` flags. That keeps the
//         overwhelmingly common bare forms (`-a`, `-v`, `-k`,
//         `--autounmask`) AND the `-av <pkg>` / `-pv <pkg>`
//         atom-after-flag spellings working exactly like real emerge,
//         instead of clap greedily swallowing the following atom as an
//         (invalid) value -- which is the one real failure mode a
//         `num_args(0..=1)` + choices clap arg would introduce. The
//         explicit `=y`/`=n` forms are not parsed yet.
//       * The three real `argument_options` with an *optional* numeric
//         value (`--deep`/`-D`, `--jobs`/`-j`, `--load-average`/`-l`)
//         mirror real emerge's own `insert_optional_args` semantics
//         exactly: a following token is consumed as the value only when
//         real emerge's own validator would accept it (`int(s) >= 0`
//         for `--deep`/`--jobs`, which also accept `y`/`n`, and
//         `float(s) >= 0` for `--load-average`); otherwise the option is
//         bare and gets real emerge's own literal inserted default,
//         `"True"`. This is achieved by marking the clap arg
//         `require_equals(true)` (so it can never swallow a following
//         atom) plus a small pre-pass (`join_optional_values`) that
//         joins the space-separated valid-value forms (e.g. `-j 4`) into
//         the explicit `--jobs=4` spelling before clap sees them.
//       * `--with-bdeps`, `--color`, `--reinstall changed-use`, the
//         `--autounmask-*` y/n pair, and the other real *required*-value
//         choice options keep their required value (with the real
//         possible values). A bare `--color` is as much an error here as
//         in real argparse.
//       * `action: "append"` options (`--exclude`/`-X`,
//         `--buildpkg-exclude`, the `--*binpkg-exclude` /
//         `--*binpkg-include` family, `--reinstall-atoms`,
//         `--rebuild-exclude`/`-ignore`, `--useoldpkg-atoms`,
//         `--sync-submodule`) are clap `Append` args, repeatable per the
//         real `"append"` action.
//
// Exit code conventions match real emerge/argparse: 0 for success and
// help, 2 for a usage error (unknown option, missing/extra value), all
// via clap's own rendering.

use clap::builder::PossibleValuesParser;
use clap::{Arg, ArgAction, ArgMatches, Command};
use std::fmt::Write as _;
use std::process::ExitCode;

/// How one emerge option is modelled. Drives both the clap `Arg` build
/// (in `command()`) and the post-parse report (in `render_report()`), so
/// the two can never drift apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Real plain boolean (`options`/`actions`, plus the y/n optional-
    /// value family modelled as a flag -- see the module-level cuts):
    /// `ArgAction::SetTrue`.
    Flag,
    /// Required value (`argument_options` with `action: "store"`, or a
    /// required `choices` option): `ArgAction::Set`.
    Value,
    /// Real optional numeric value (`--deep`/`--jobs`/`--load-average`):
    /// `ArgAction::Set`, `num_args(0..=1)`, bare -> `missing`.
    OptionalValue,
    /// Repeatable (`argument_options` with `action: "append"`):
    /// `ArgAction::Append`.
    Append,
}

struct Opt {
    /// clap arg id (the dest), also the `render_report` lookup key.
    id: &'static str,
    /// Canonical long option, exactly as real emerge spells it.
    long: &'static str,
    /// Secondary long name for the real `longopt_aliases` pair
    /// (`--cols`/`--skip-first`); `None` for everything else.
    alias: Option<&'static str>,
    /// Real short option, exactly as real emerge's `shortmapping` /
    /// `argument_options` spells it; `None` when real emerge has none.
    short: Option<char>,
    kind: Kind,
    /// Real `choices` for required-value options (empty = accept any
    /// string, like the real `action: "store"`).
    choices: &'static [&'static str],
    /// Bare-flag default for `OptionalValue` options.
    missing: &'static str,
    /// One-line help shown by `mrg --help` (short, honest, portuale's
    /// own wording -- not a transcription of real main.py's help text).
    help: &'static str,
}

/// Splits `OPTIONS` into "Actions" (the real `actions` frozenset) and
/// everything else, for clap's help headings.
const ACTION_IDS: &[&str] = &[
    "clean",
    "check_news",
    "config",
    "depclean",
    "info",
    "list_sets",
    "metadata",
    "moo",
    "prune",
    "rage_clean",
    "regen",
    "search",
    "status",
    "sync",
    "unmerge",
    "version",
];

/// Real `actions` frozenset + `options` list + y/n optional-value family
/// (the non-value options). Every entry keeps real emerge's spelling.
const OPTIONS: &[Opt] = &[
    // --- The real `actions` frozenset: a standalone action replaces the
    // ordinary resolve-and-show behavior. (`help` is clap's own.)
    Opt {
        id: "clean",
        long: "--clean",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "remove all objects from the package cache (dangerous)",
    },
    Opt {
        id: "check_news",
        long: "--check-news",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "perform a news check",
    },
    Opt {
        id: "config",
        long: "--config",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "run the pkg_config phase for a package",
    },
    Opt {
        id: "depclean",
        long: "--depclean",
        alias: None,
        short: Some('c'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "remove unused packages from the system",
    },
    Opt {
        id: "info",
        long: "--info",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "show system information for the package manager",
    },
    Opt {
        id: "list_sets",
        long: "--list-sets",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "show all sets",
    },
    Opt {
        id: "metadata",
        long: "--metadata",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "regenerate metadata cache and then exit",
    },
    Opt {
        id: "moo",
        long: "--moo",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "print the Gentoo cow",
    },
    Opt {
        id: "prune",
        long: "--prune",
        alias: None,
        short: Some('P'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "remove older versions of slotted packages",
    },
    Opt {
        id: "rage_clean",
        long: "--rage-clean",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "remove all objects from the package cache, without mercy",
    },
    Opt {
        id: "regen",
        long: "--regen",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "regenerate metadata cache, no questions asked",
    },
    Opt {
        id: "search",
        long: "--search",
        alias: None,
        short: Some('s'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "search for a package by name",
    },
    Opt {
        id: "status",
        long: "--status",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "show portage's view of the world",
    },
    Opt {
        id: "sync",
        long: "--sync",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "sync the repos",
    },
    Opt {
        id: "unmerge",
        long: "--unmerge",
        alias: None,
        short: Some('C'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "remove packages from the system",
    },
    Opt {
        id: "version",
        long: "--version",
        alias: None,
        short: Some('V'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "show the version of the package manager",
    },
    // --- The real `options` list (plain boolean store_true options),
    // with `shortmapping` shorts where real emerge defines one.
    Opt {
        id: "alphabetical",
        long: "--alphabetical",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "list USE flags alphabetically in the verbose output",
    },
    Opt {
        id: "ask_enter_invalid",
        long: "--ask-enter-invalid",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "accept stderr/stdin on --ask, in case it's invalid",
    },
    Opt {
        id: "buildpkgonly",
        long: "--buildpkgonly",
        alias: None,
        short: Some('B'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "build binary packages only (never merge)",
    },
    Opt {
        id: "changed_use",
        long: "--changed-use",
        alias: None,
        short: Some('U'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "reinstall packages whose USE have changed",
    },
    Opt {
        id: "columns",
        long: "--columns",
        alias: Some("cols"),
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "display output in columns",
    },
    Opt {
        id: "debug",
        long: "--debug",
        alias: None,
        short: Some('d'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "enable debug output",
    },
    Opt {
        id: "digest",
        long: "--digest",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "regenerate digests (with the digest/manifest commands)",
    },
    Opt {
        id: "emptytree",
        long: "--emptytree",
        alias: None,
        short: Some('e'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "reinstall every atom in the resolved set, deep",
    },
    Opt {
        id: "verbose_conflicts",
        long: "--verbose-conflicts",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "verbose output about conflicts",
    },
    Opt {
        id: "fetchonly",
        long: "--fetchonly",
        alias: None,
        short: Some('f'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "only fetch the distribution files",
    },
    Opt {
        id: "fetch_all_uri",
        long: "--fetch-all-uri",
        alias: None,
        short: Some('F'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "fetch every URI recorded in SRC_URI",
    },
    Opt {
        id: "ignore_default_opts",
        long: "--ignore-default-opts",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "do not use the EMERGE_DEFAULT_OPTS variable",
    },
    Opt {
        id: "noconfmem",
        long: "--noconfmem",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "no CONFIG_PROTECT support",
    },
    Opt {
        id: "newrepo",
        long: "--newrepo",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "reinstall packages whose repository has changed",
    },
    Opt {
        id: "newuse",
        long: "--newuse",
        alias: None,
        short: Some('N'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "reinstall packages whose USE have changed",
    },
    Opt {
        id: "nobindeps",
        long: "--nobindeps",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "ignore binary package dependencies",
    },
    Opt {
        id: "nodeps",
        long: "--nodeps",
        alias: None,
        short: Some('O'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "skip dependency resolution entirely",
    },
    Opt {
        id: "noreplace",
        long: "--noreplace",
        alias: None,
        short: Some('n'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "do not reinstall already-installed packages",
    },
    Opt {
        id: "nospinner",
        long: "--nospinner",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "suppress the spinner",
    },
    Opt {
        id: "oneshot",
        long: "--oneshot",
        alias: None,
        short: Some('1'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "do not add the packages to the world file",
    },
    Opt {
        id: "onlydeps",
        long: "--onlydeps",
        alias: None,
        short: Some('o'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "only merge the dependencies, not the packages",
    },
    Opt {
        id: "pretend",
        long: "--pretend",
        alias: None,
        short: Some('p'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "only show what would be done (dry run)",
    },
    Opt {
        id: "quiet_repo_display",
        long: "--quiet-repo-display",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "do not display repository name in output",
    },
    Opt {
        id: "quiet_unmerge_warn",
        long: "--quiet-unmerge-warn",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "suppress the unmerge warnings",
    },
    Opt {
        id: "resume",
        long: "--resume",
        alias: None,
        short: Some('r'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "resume the last merge operation",
    },
    Opt {
        id: "searchdesc",
        long: "--searchdesc",
        alias: None,
        short: Some('S'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "search the description too",
    },
    Opt {
        id: "skipfirst",
        long: "--skipfirst",
        alias: Some("skip-first"),
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "skip the first package on a --resume run",
    },
    Opt {
        id: "tree",
        long: "--tree",
        alias: None,
        short: Some('t'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "display the dependency tree",
    },
    Opt {
        id: "unordered_display",
        long: "--unordered-display",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "display the merge list in unsorted order",
    },
    Opt {
        id: "update",
        long: "--update",
        alias: None,
        short: Some('u'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "update packages to the best available version",
    },
    Opt {
        id: "update_if_installed",
        long: "--update-if-installed",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "update packages that are already installed",
    },
    // --- The real y/n/True optional-value family (insert_optional_args'
    // `default_arg_opts`), modelled as plain flags -- see the module
    // doc comment's cut for why. Short aliases are real main.py's own.
    Opt {
        id: "alert",
        long: "--alert",
        alias: None,
        short: Some('A'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "alert (terminal bell) on prompts",
    },
    Opt {
        id: "ask",
        long: "--ask",
        alias: None,
        short: Some('a'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "prompt before performing any actions",
    },
    Opt {
        id: "autounmask",
        long: "--autounmask",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "automatically unmask packages",
    },
    Opt {
        id: "autounmask_continue",
        long: "--autounmask-continue",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "write autounmask changes and continue",
    },
    Opt {
        id: "autounmask_only",
        long: "--autounmask-only",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "only perform --autounmask",
    },
    Opt {
        id: "autounmask_unrestricted_atoms",
        long: "--autounmask-unrestricted-atoms",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "write autounmask changes with >= atoms if possible",
    },
    Opt {
        id: "autounmask_keep_keywords",
        long: "--autounmask-keep-keywords",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "do not add package.accept_keywords entries",
    },
    Opt {
        id: "autounmask_keep_masks",
        long: "--autounmask-keep-masks",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "do not add package.unmask entries",
    },
    Opt {
        id: "autounmask_write",
        long: "--autounmask-write",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "write changes made by --autounmask to disk",
    },
    Opt {
        id: "binpkg_changed_deps",
        long: "--binpkg-changed-deps",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "reject binary packages with outdated dependencies",
    },
    Opt {
        id: "buildpkg",
        long: "--buildpkg",
        alias: None,
        short: Some('b'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "build binary packages",
    },
    Opt {
        id: "changed_deps",
        long: "--changed-deps",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "replace installed packages with outdated dependencies",
    },
    Opt {
        id: "changed_deps_report",
        long: "--changed-deps-report",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "report installed packages with outdated dependencies",
    },
    Opt {
        id: "changed_slot",
        long: "--changed-slot",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "replace installed packages with outdated SLOT metadata",
    },
    Opt {
        id: "complete_graph",
        long: "--complete-graph",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "completely account for all known dependencies",
    },
    Opt {
        id: "depclean_lib_check",
        long: "--depclean-lib-check",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "check for consumers of libraries before removing them",
    },
    Opt {
        id: "deselect",
        long: "--deselect",
        alias: None,
        short: Some('W'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "remove atoms/sets from the world file",
    },
    Opt {
        id: "binpkg_respect_use",
        long: "--binpkg-respect-use",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "discard binary packages whose USE don't match",
    },
    Opt {
        id: "fail_clean",
        long: "--fail-clean",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "clean temp files after build failure",
    },
    Opt {
        id: "fuzzy_search",
        long: "--fuzzy-search",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "enable fuzzy search",
    },
    Opt {
        id: "getbinpkg",
        long: "--getbinpkg",
        alias: None,
        short: Some('g'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "fetch binary packages",
    },
    Opt {
        id: "getbinpkgonly",
        long: "--getbinpkgonly",
        alias: None,
        short: Some('G'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "fetch binary packages only",
    },
    Opt {
        id: "ignore_world",
        long: "--ignore-world",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "ignore the @world set and its dependencies",
    },
    Opt {
        id: "keep_going",
        long: "--keep-going",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "continue as much as possible after an error",
    },
    Opt {
        id: "onlydeps_with_ideps",
        long: "--onlydeps-with-ideps",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "include IDEPEND in --onlydeps",
    },
    Opt {
        id: "onlydeps_with_rdeps",
        long: "--onlydeps-with-rdeps",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "include RDEPEND in --onlydeps",
    },
    Opt {
        id: "package_moves",
        long: "--package-moves",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "perform package moves when necessary",
    },
    Opt {
        id: "quiet",
        long: "--quiet",
        alias: None,
        short: Some('q'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "reduced or condensed output",
    },
    Opt {
        id: "quiet_build",
        long: "--quiet-build",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "redirect build output to logs",
    },
    Opt {
        id: "quiet_fail",
        long: "--quiet-fail",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "suppress display of the build log on stdout",
    },
    Opt {
        id: "read_news",
        long: "--read-news",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "offer to read unread news via eselect",
    },
    Opt {
        id: "rebuild_if_new_slot",
        long: "--rebuild-if-new-slot",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "rebuild when a := dependency moves to a new slot",
    },
    Opt {
        id: "rebuild_if_new_rev",
        long: "--rebuild-if-new-rev",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "rebuild when a build+run dep gets a new revision",
    },
    Opt {
        id: "rebuild_if_new_ver",
        long: "--rebuild-if-new-ver",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "rebuild when a build+run dep gets a new version",
    },
    Opt {
        id: "rebuild_if_unbuilt",
        long: "--rebuild-if-unbuilt",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "rebuild when a build+run dep is built",
    },
    Opt {
        id: "rebuilt_binaries",
        long: "--rebuilt-binaries",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "replace installed packages with rebuilt binaries",
    },
    Opt {
        id: "select",
        long: "--select",
        alias: None,
        short: Some('w'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "add specified packages to the world set",
    },
    Opt {
        id: "selective",
        long: "--selective",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "identical to --noreplace",
    },
    Opt {
        id: "use_ebuild_visibility",
        long: "--use-ebuild-visibility",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "use ebuild metadata for visibility checks on binpkgs",
    },
    Opt {
        id: "usepkg",
        long: "--usepkg",
        alias: None,
        short: Some('k'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "use binary packages",
    },
    Opt {
        id: "usepkgonly",
        long: "--usepkgonly",
        alias: None,
        short: Some('K'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "use only binary packages",
    },
    Opt {
        id: "usepkg_exclude_live",
        long: "--usepkg-exclude-live",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "do not use binary packages for live ebuilds",
    },
    Opt {
        id: "verbose",
        long: "--verbose",
        alias: None,
        short: Some('v'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "verbose output",
    },
    Opt {
        id: "verbose_missing_ebuilds",
        long: "--verbose-missing-ebuilds",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "verbose missing ebuild output",
    },
    Opt {
        id: "verbose_slot_rebuilds",
        long: "--verbose-slot-rebuilds",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "verbose slot rebuild output",
    },
    Opt {
        id: "with_test_deps",
        long: "--with-test-deps",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "pull in test deps for matched packages",
    },
    // --- Required-value options. `choices` non-empty = the real closed
    // choice set; empty = accept any store value like the real action.
    Opt {
        id: "autounmask_backtrack",
        long: "--autounmask-backtrack",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &["y", "n"],
        missing: "",
        help: "continue backtracking on autounmask changes",
    },
    Opt {
        id: "autounmask_license",
        long: "--autounmask-license",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &["y", "n"],
        missing: "",
        help: "allow autounmask to change package.license",
    },
    Opt {
        id: "autounmask_use",
        long: "--autounmask-use",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &["y", "n"],
        missing: "",
        help: "allow autounmask to change package.use",
    },
    Opt {
        id: "color",
        long: "--color",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &["y", "n"],
        missing: "",
        help: "enable or disable color output",
    },
    Opt {
        id: "complete_graph_if_new_use",
        long: "--complete-graph-if-new-use",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &["y", "n"],
        missing: "",
        help: "trigger --complete-graph if USE/IUSE changes",
    },
    Opt {
        id: "complete_graph_if_new_ver",
        long: "--complete-graph-if-new-ver",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &["y", "n"],
        missing: "",
        help: "trigger --complete-graph if a version changes",
    },
    Opt {
        id: "dynamic_deps",
        long: "--dynamic-deps",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &["y", "n"],
        missing: "",
        help: "substitute installed deps with unbuilt ebuild deps",
    },
    Opt {
        id: "ignore_built_slot_operator_deps",
        long: "--ignore-built-slot-operator-deps",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &["y", "n"],
        missing: "",
        help: "ignore the := operator parts of recorded deps",
    },
    Opt {
        id: "ignore_soname_deps",
        long: "--ignore-soname-deps",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &["y", "n"],
        missing: "",
        help: "ignore soname dependencies",
    },
    Opt {
        id: "implicit_system_deps",
        long: "--implicit-system-deps",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &["y", "n"],
        missing: "",
        help: "assume implicit deps on @system packages",
    },
    Opt {
        id: "misspell_suggestions",
        long: "--misspell-suggestions",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &["y", "n"],
        missing: "",
        help: "enable package name misspell suggestions",
    },
    Opt {
        id: "with_bdeps",
        long: "--with-bdeps",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &["y", "n"],
        missing: "",
        help: "include unnecessary build-time dependencies",
    },
    Opt {
        id: "with_bdeps_auto",
        long: "--with-bdeps-auto",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &["y", "n"],
        missing: "",
        help: "automatically enable --with-bdeps (unless --usepkg)",
    },
    Opt {
        id: "regex_search_auto",
        long: "--regex-search-auto",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &["y", "n"],
        missing: "",
        help: "automatic regex detection for search actions",
    },
    Opt {
        id: "search_index",
        long: "--search-index",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &["y", "n"],
        missing: "",
        help: "enable or disable indexed search",
    },
    Opt {
        id: "reinstall",
        long: "--reinstall",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &["changed-use"],
        missing: "",
        help: "specify conditions to trigger reinstallation",
    },
    Opt {
        id: "accept_properties",
        long: "--accept-properties",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "temporarily override ACCEPT_PROPERTIES",
    },
    Opt {
        id: "accept_restrict",
        long: "--accept-restrict",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "temporarily override ACCEPT_RESTRICT",
    },
    Opt {
        id: "backtrack",
        long: "--backtrack",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "how many times to backtrack if dep calculation fails",
    },
    Opt {
        id: "config_root",
        long: "--config-root",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "specify the portage configuration location",
    },
    Opt {
        id: "jobs_tmpdir_require_free_gb",
        long: "--jobs-tmpdir-require-free-gb",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "required free GiB of PORTAGE_TMPDIR before a new job",
    },
    Opt {
        id: "prefix",
        long: "--prefix",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "specify the installation prefix",
    },
    Opt {
        id: "pkg_format",
        long: "--pkg-format",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "format of the resulting binary package",
    },
    Opt {
        id: "quickpkg_direct_root",
        long: "--quickpkg-direct-root",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "root to use as the quickpkg-direct source",
    },
    Opt {
        id: "rebuilt_binaries_timestamp",
        long: "--rebuilt-binaries-timestamp",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "only use binaries newer than this timestamp",
    },
    Opt {
        id: "root",
        long: "--root",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "specify the target root filesystem",
    },
    Opt {
        id: "search_similarity",
        long: "--search-similarity",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "minimum similarity % for fuzzy search",
    },
    Opt {
        id: "sysroot",
        long: "--sysroot",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "location for build deps during cross compilation",
    },
    // --- The real optional-value numeric options. Bare form inserts real
    // emerge's own literal default "True" (main.py's
    // `insert_optional_args`); `join_optional_values` + `require_equals`
    // keep the conditional "consume only valid values" semantics.
    Opt {
        id: "deep",
        long: "--deep",
        alias: None,
        short: Some('D'),
        kind: Kind::OptionalValue,
        choices: &[],
        missing: "True",
        help: "how deep to recurse into dependencies",
    },
    Opt {
        id: "jobs",
        long: "--jobs",
        alias: None,
        short: Some('j'),
        kind: Kind::OptionalValue,
        choices: &[],
        missing: "True",
        help: "number of packages to build simultaneously",
    },
    Opt {
        id: "load_average",
        long: "--load-average",
        alias: None,
        short: Some('l'),
        kind: Kind::OptionalValue,
        choices: &[],
        missing: "True",
        help: "no new builds when the load average is at least this",
    },
    // --- Real `action: "append"` options (repeatable).
    Opt {
        id: "buildpkg_exclude",
        long: "--buildpkg-exclude",
        alias: None,
        short: None,
        kind: Kind::Append,
        choices: &[],
        missing: "",
        help: "space-separated atoms to never build as binpkgs",
    },
    Opt {
        id: "exclude",
        long: "--exclude",
        alias: None,
        short: Some('X'),
        kind: Kind::Append,
        choices: &[],
        missing: "",
        help: "space-separated atoms to exclude entirely",
    },
    Opt {
        id: "getbinpkg_exclude",
        long: "--getbinpkg-exclude",
        alias: None,
        short: None,
        kind: Kind::Append,
        choices: &[],
        missing: "",
        help: "atoms to not fetch from remote binary repos",
    },
    Opt {
        id: "getbinpkg_include",
        long: "--getbinpkg-include",
        alias: None,
        short: None,
        kind: Kind::Append,
        choices: &[],
        missing: "",
        help: "atoms to fetch from remote binary repos",
    },
    Opt {
        id: "reinstall_atoms",
        long: "--reinstall-atoms",
        alias: None,
        short: None,
        kind: Kind::Append,
        choices: &[],
        missing: "",
        help: "atoms to treat as if they were not installed",
    },
    Opt {
        id: "rebuild_exclude",
        long: "--rebuild-exclude",
        alias: None,
        short: None,
        kind: Kind::Append,
        choices: &[],
        missing: "",
        help: "atoms to never rebuild due to the --rebuild flag",
    },
    Opt {
        id: "rebuild_ignore",
        long: "--rebuild-ignore",
        alias: None,
        short: None,
        kind: Kind::Append,
        choices: &[],
        missing: "",
        help: "atoms whose dependents are not rebuilt by --rebuild",
    },
    Opt {
        id: "sync_submodule",
        long: "--sync-submodule",
        alias: None,
        short: None,
        kind: Kind::Append,
        choices: &[],
        missing: "",
        help: "restrict sync to this submodule (--sync only)",
    },
    Opt {
        id: "usepkg_exclude",
        long: "--usepkg-exclude",
        alias: None,
        short: None,
        kind: Kind::Append,
        choices: &[],
        missing: "",
        help: "atoms to ignore in binary packages",
    },
    Opt {
        id: "usepkg_include",
        long: "--usepkg-include",
        alias: None,
        short: None,
        kind: Kind::Append,
        choices: &[],
        missing: "",
        help: "atoms to use from binary packages",
    },
    Opt {
        id: "useoldpkg_atoms",
        long: "--useoldpkg-atoms",
        alias: None,
        short: None,
        kind: Kind::Append,
        choices: &[],
        missing: "",
        help: "atoms to prefer old binpkgs over newer ebuilds",
    },
];

/// Builds the clap `Arg` for one `Opt` entry, keeping real emerge's
/// short/long spellings and the real short alias.
fn build_arg(opt: &Opt) -> Arg {
    let mut arg = Arg::new(opt.id)
        .long(opt.long.trim_start_matches("--"))
        .help(opt.help);
    if let Some(alias) = opt.alias {
        arg = arg.alias(alias.trim_start_matches("--"));
    }
    if let Some(short) = opt.short {
        arg = arg.short(short);
    }
    match opt.kind {
        Kind::Flag => arg.action(ArgAction::SetTrue),
        Kind::Value => {
            if opt.choices.is_empty() {
                arg.action(ArgAction::Set)
            } else {
                arg.action(ArgAction::Set)
                    .value_parser(PossibleValuesParser::new(opt.choices))
            }
        }
        Kind::OptionalValue => arg
            .action(ArgAction::Set)
            .num_args(0..=1)
            .require_equals(true)
            .default_missing_value(opt.missing),
        Kind::Append => arg.action(ArgAction::Append),
    }
}

/// The clap `Command` for the whole `mrg` surface: every real emerge
/// option (grouped into clap's "Actions"/"Options" help sections) plus
/// the target package atoms as positionals.
pub fn command() -> Command {
    let mut cmd = Command::new("mrg")
        .about("a modern command-line front end for the Portuale package manager");
    cmd = cmd.arg(
        Arg::new("package")
            .action(ArgAction::Append)
            .value_name("ATOM"),
    );
    for opt in OPTIONS {
        let heading = if ACTION_IDS.contains(&opt.id) {
            "Actions"
        } else {
            "Options"
        };
        cmd = cmd.arg(build_arg(opt).help_heading(heading));
    }
    cmd
}

/// Renders the parsed result: every option the run actually used (in the
/// stable definition order above, canonical long name, `=value` for the
/// value/append kinds) and the target atoms, each on its own line. The
/// echo makes optional-value defaults (`--deep=unlimited` and friends)
/// explicit rather than silently swallowed.
fn render_report(matches: &ArgMatches) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "mrg: parsed command line:");
    let _ = writeln!(out, "  options:");
    let mut any = false;
    for opt in OPTIONS {
        let name = opt.long;
        match opt.kind {
            Kind::Flag => {
                if matches.get_flag(opt.id) {
                    let _ = writeln!(out, "    {name}");
                    any = true;
                }
            }
            Kind::Value | Kind::OptionalValue => {
                if let Some(v) = matches.get_one::<String>(opt.id) {
                    let _ = writeln!(out, "    {name}={v}");
                    any = true;
                }
            }
            Kind::Append => {
                if let Some(vals) = matches.get_many::<String>(opt.id) {
                    for v in vals {
                        let _ = writeln!(out, "    {name}={v}");
                    }
                    any = true;
                }
            }
        }
    }
    if !any {
        let _ = writeln!(out, "    (none)");
    }
    let _ = writeln!(out, "  packages:");
    let atoms: Vec<&String> = matches
        .get_many::<String>("package")
        .map(|iter| iter.collect())
        .unwrap_or_default();
    if atoms.is_empty() {
        let _ = writeln!(out, "    (none)");
    } else {
        for atom in atoms {
            let _ = writeln!(out, "    {atom}");
        }
    }
    out
}

/// Real emerge's own `valid_integers` (`int(s) >= 0`), matching main.py's
/// `insert_optional_args`.
fn valid_integer(s: &str) -> bool {
    let Ok(v) = s.parse::<i128>() else {
        return false;
    };
    v >= 0
}

/// The `--jobs` variant: real emerge's `valid_integers_or_y_or_n`.
fn valid_integer_or_y_or_n(s: &str) -> bool {
    valid_integer(s) || s == "y" || s == "n"
}

/// Real emerge's own `valid_floats` (`float(s) >= 0`). A non-finite
/// result still compares against 0 like Python's `float(s) >= 0` does.
fn valid_float(s: &str) -> bool {
    let Ok(v) = s.parse::<f64>() else {
        return false;
    };
    v >= 0.0
}

/// Real emerge's `insert_optional_args`, trimmed to just what this parser
/// needs: `--deep`/`-D`, `--jobs`/`-j`, `--load-average`/`-l` consume the
/// *separate* following token as their value only when it passes real
/// emerge's own validator, and never otherwise. Since the clap arg is
/// `require_equals(true)`, that join has to happen here: a bare `-D
/// cat/pkg` must stay `-D` (bare, `"True"`) with `cat/pkg` as an atom,
/// and `-j 4` must become `--jobs=4`. Attached short forms (`-j4`,
/// `-D2`, `-l2.5`) are joined to their explicit long form too, since
/// require_equals also rejects those. Every other token passes through
/// untouched.
fn join_optional_values(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut iter = args.iter();
    'outer: while let Some(arg) = iter.next() {
        // Attached short forms (`-j4`, `-D2`, `-l2.5`): real emerge's
        // insert_optional_args expands these itself, and clap's
        // require_equals rejects the attached spelling, so join them into
        // the explicit long form here when the suffix is a valid value.
        for (short, long, validator) in [
            ("-D", "--deep", valid_integer as fn(&str) -> bool),
            ("-j", "--jobs", valid_integer_or_y_or_n as fn(&str) -> bool),
            ("-l", "--load-average", valid_float as fn(&str) -> bool),
        ] {
            if let Some(suffix) = arg.strip_prefix(short).filter(|s| !s.is_empty()) {
                if validator(suffix) {
                    out.push(format!("{long}={suffix}"));
                } else {
                    out.push(arg.clone());
                }
                continue 'outer;
            }
        }
        let (joinable, validator) = match arg.as_str() {
            "--deep" | "-D" => (Some("--deep"), valid_integer as fn(&str) -> bool),
            "--jobs" | "-j" => (Some("--jobs"), valid_integer_or_y_or_n as fn(&str) -> bool),
            "--load-average" | "-l" => (Some("--load-average"), valid_float as fn(&str) -> bool),
            _ => (None, valid_integer as fn(&str) -> bool),
        };
        if let Some(long) = joinable {
            match iter.clone().next() {
                Some(next) if validator(next) => {
                    let next = next.to_owned();
                    iter.next();
                    out.push(format!("{long}={next}"));
                }
                _ => out.push(arg.clone()),
            }
        } else {
            out.push(arg.clone());
        }
    }
    out
}

/// `mrg`'s entry point, dispatched from main.rs's multicall `run`. Parses
/// `args` (already stripped of the applet name) with the clap command,
/// echoes the result, and maps clap errors to real-emerge-style exit
/// codes (help -> 0, usage error -> 2).
pub fn run(args: &[String]) -> ExitCode {
    let bin = "mrg";
    let joined = join_optional_values(args);
    let argv = std::iter::once(bin).chain(joined.iter().map(String::as_str));
    let run = || command().try_get_matches_from(argv.clone());
    match run() {
        Ok(matches) => {
            print!("{}", render_report(&matches));
            ExitCode::SUCCESS
        }
        Err(err) => {
            let code = err.exit_code() as u8;
            let _ = err.print();
            ExitCode::from(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<ArgMatches, clap::Error> {
        let joined = join_optional_values(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let argv = std::iter::once("mrg").chain(joined.iter().map(String::as_str));
        command().try_get_matches_from(argv)
    }

    /// Every table short option must be unique, so no real emerge short
    /// spelling is ever shadowed by a clap-invented duplicate.
    #[test]
    fn shorts_are_pairwise_distinct() {
        let mut seen = std::collections::HashSet::new();
        for opt in OPTIONS {
            if let Some(short) = opt.short {
                assert!(
                    seen.insert(short),
                    "duplicate short -{short} on {}",
                    opt.long
                );
            }
        }
    }

    /// `-pv1 pkg` bundles a flag, another flag, and its attached value
    /// exactly like real argparse does.
    #[test]
    fn bundled_shorts_parse_independently() {
        let m = parse(&["-pv1", "cat/pkg"]).unwrap();
        assert!(m.get_flag("pretend"));
        assert!(m.get_flag("verbose"));
        assert!(m.get_flag("oneshot"));
        let atoms: Vec<_> = m.get_many::<String>("package").unwrap().collect();
        assert_eq!(atoms, vec!["cat/pkg"]);
        let report = render_report(&m);
        assert!(report.contains("--pretend"));
        assert!(report.contains("--verbose"));
        assert!(report.contains("--oneshot"));
        assert!(report.contains("    cat/pkg"));
    }

    /// `-j4` (attached), `-j 4`/`--jobs=4` (separate) all carry the
    /// value; a bare `-j` falls back to real emerge's inserted "True".
    #[test]
    fn jobs_value_forms_and_bare_default() {
        for args in [
            &["-j4", "-p"][..],
            &["-j", "4", "-p"][..],
            &["--jobs=4", "-p"][..],
        ] {
            let m = parse(args).unwrap();
            assert_eq!(m.get_one::<String>("jobs").map(String::as_str), Some("4"));
            assert!(m.get_flag("pretend"));
        }
        let m = parse(&["-j", "-p"]).unwrap();
        assert_eq!(
            m.get_one::<String>("jobs").map(String::as_str),
            Some("True")
        );
    }

    /// A bare `-D` must matching real emerge: a following flag OR a
    /// following atom is NOT consumed as the (non-valid) value -- the
    /// option stays "True" and the token stays wherever it belongs.
    #[test]
    fn bare_optional_value_does_not_eat_flags_or_atoms() {
        let m = parse(&["-D", "--pretend", "cat/pkg"]).unwrap();
        assert_eq!(
            m.get_one::<String>("deep").map(String::as_str),
            Some("True")
        );
        assert!(m.get_flag("pretend"));
        let atoms: Vec<_> = m.get_many::<String>("package").unwrap().collect();
        assert_eq!(atoms, vec!["cat/pkg"]);

        let m = parse(&["-D", "cat/a"]).unwrap();
        assert_eq!(
            m.get_one::<String>("deep").map(String::as_str),
            Some("True")
        );
        let atoms: Vec<_> = m.get_many::<String>("package").unwrap().collect();
        assert_eq!(atoms, vec!["cat/a"]);
    }

    /// `--jobs y`/`--load-average 2.5` join the separate valid-value
    /// forms like real insert_optional_args does.
    #[test]
    fn optional_value_separate_valid_values_are_consumed() {
        let m = parse(&["-j", "y"]).unwrap();
        assert_eq!(m.get_one::<String>("jobs").map(String::as_str), Some("y"));
        let m = parse(&["-l", "2.5"]).unwrap();
        assert_eq!(
            m.get_one::<String>("load_average").map(String::as_str),
            Some("2.5")
        );
    }

    /// `--columns` and its real `--cols` alias name the same flag.
    #[test]
    fn longopt_alias_columns() {
        for args in [&["--columns"][..], &["--cols"][..]] {
            let m = parse(args).unwrap();
            assert!(m.get_flag("columns"), "{args:?}");
        }
    }

    /// `-1` is the real "add to world" negation (oneshot), not a value.
    #[test]
    fn oneshot_short() {
        assert!(parse(&["-1"]).unwrap().get_flag("oneshot"));
    }

    /// A required value missing (bare `--color`) is a clap usage error,
    /// and so is any short spelling real emerge never defined.
    #[test]
    fn usage_errors_are_errors() {
        for args in [&["--color"][..], &["-I"][..], &["--bogus"][..]] {
            let err = parse(args).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{args:?}");
        }
    }

    /// `--reinstall` keeps its real single "changed-use" choice.
    #[test]
    fn reinstall_choices() {
        assert!(parse(&["--reinstall", "changed-use"]).is_ok());
        assert!(parse(&["--reinstall", "everything"]).is_err());
    }

    /// A repeatable option collects its real `action: "append"` list.
    #[test]
    fn append_collects_repeats() {
        let m = parse(&["-X", "cat/a", "--exclude", "cat/b", "--exclude=cat/c"]).unwrap();
        let vals: Vec<&str> = m
            .get_many::<String>("exclude")
            .unwrap()
            .map(String::as_str)
            .collect();
        assert_eq!(vals, vec!["cat/a", "cat/b", "cat/c"]);
    }

    /// `--help` is clap's own display and exits 0 (via Err kind), never
    /// a parse error.
    #[test]
    fn help_is_ok() {
        let err = parse(&["--help"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
        assert_eq!(err.exit_code(), 0);
    }

    /// Nothing is required: real emerge `--pretend` with no atoms still
    /// runs, and `mrg` with nothing at all prints an empty report.
    #[test]
    fn empty_invocation_reports_none() {
        let m = parse(&[]).unwrap();
        assert!(render_report(&m).contains("(none)"));
    }
}
