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
// `mrg` is a clap front end over portuale's OWN emerge codepath: it
// parses the real emerge option surface, translates the match into a
// canonical long-form argv (options in definition order,
// `--long=value` for value kinds, the target package atoms last -- see
// `to_emerge_argv`), and hands it to `pretend::run`, the exact function
// the `emerge` applet runs. So `portuale mrg --pretend
// app-portage/eix` resolves and prints exactly what
// `portuale emerge --pretend app-portage/eix` does -- the same engine
// behind a clap front end, keeping clap's parser, help, and exit
// behaviour. The rules `to_emerge_argv` folds into that canonical form:
//
//   - Every `Flag` goes BARE (`--pretend`), never `--pretend=y` -- the
//     emerge codepath's peek-based parser treats a plain Boolean flag
//     and a following (unrelated) token correctly on its own.
//   - `Value` and `Append` options the codepath implements go
//     `--long=<value>` (per occurrence for `Append`); ones it does NOT
//     implement yet are forwarded BARE (`--fetchonly`, `--root`, ...)
//     so `pretend::run` names them honestly ("a real emerge option, but
//     is not yet implemented in portuale...") instead of rejecting a
//     `--long=value` spelling it never heard of. `emerge_handles`
//     encodes the implemented surface, keyed to the parse-loop section
//     in `pretend.rs`.
//   - `OptionalValue`s mirror real argparse's inserted literals: the
//     bare forms (`-D`, `-j`, `-l`) carry value `"True"` internally and
//     forward BARE (the codepath's own strict `=` validation would
//     reject `--deep=True`); `--jobs`'s separate-value `y`/`n` unlimited
//     forms also forward BARE (that is their real meaning); any numeric
//     value forwards `--long=N`.
//
// FULL option-surface fidelity notes (grounded in main.py, read before
// deviating):
//
//   - Every plain boolean from `options` + `actions` becomes a clap
//     `ArgAction::SetTrue` flag with its real short alias (from
//     `shortmapping`) attached. clap's native short-flag bundling
//     (`-pv`, `-aN`, ...) therefore behaves like real `argparse`.
//     `--cols`/`--skip-first` are real `longopt_aliases` secondary
//     names for `--columns`/`--skipfirst`, modelled as clap `alias`es.
//   - `-h`/`--help` is clap's own (real emerge declares `help` in its
//     `actions` set, and `-h` in `shortmapping`; clap's built-in help
//     keeps that exact spelling).
//   - Deliberate cuts, all within the "full equality is NOT required,
//     keep short and long options as-is" licence:
//       * The y/n/`True` *optional-value* options (main.py's own
//         `insert_optional_args`/`default_arg_opts`: `--ask`, `--verbose`,
//         `--quiet`, `--buildpkg`, `--usepkg`, ...) are
//         modelled as plain `SetTrue` flags. That keeps the
//         overwhelmingly common bare forms (`-a`, `-v`, `-k`) AND the
//         `-av <pkg>` / `-pv <pkg>`
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
//       * `--with-bdeps`, `--color`, the y/n choice
//         options, and the other real *required*-value
//         choice options keep their required value (with the real
//         possible values). A bare `--color` is as much an error here as
//         in real argparse.
//       * `action: "append"` options (`--exclude`/`-X`,
//         `--buildpkg-exclude`, the `--*binpkg-exclude` /
//         `--*binpkg-include` family, `--reinstall-atoms`,
//         `--useoldpkg-atoms`, `--usepkg-include`) are
//         clap `Append` args, repeatable per the
//         real `"append"` action.
//
// Exit code conventions match real emerge/argparse: 0 for success and
// help, 2 for a usage error (unknown option, missing/extra value), all
// via clap's own rendering.

use clap::builder::PossibleValuesParser;
use clap::{Arg, ArgAction, ArgMatches, Command};
use std::process::ExitCode;

/// How one emerge option is modelled. Drives the clap `Arg` build (in
/// `command()`) and the `to_emerge_argv` forward translation, so the two
/// can never drift apart.
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
    /// clap arg id (the dest), also the `to_emerge_argv` lookup key.
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
    "config",
    "depclean",
    "info",
    "list_sets",
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
        id: "update",
        long: "--update",
        alias: None,
        short: Some('u'),
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "update packages to the best available version",
    },
    // --- The real y/n/True optional-value family (insert_optional_args'
    // `default_arg_opts`), modelled as plain flags -- see the module
    // doc comment's cut for why. Short aliases are real main.py's own.
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
        id: "backtrack",
        long: "--backtrack",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "how many times to backtrack if dep calculation fails",
    },
    // Portuale-only (real emerge has no `--solver`): which dependency
    // solver resolves the graph. Forwarded `--solver=<value>` to the
    // emerge codepath, which parses and validates it.
    Opt {
        id: "solver",
        long: "--solver",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &["portage", "pubgrub", "resolvo"],
        missing: "",
        help: "which dependency solver resolves the graph (default portage)",
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
    // --- Portuale-only remote options (see docs/remote-merge.md). Never
    // forwarded to the emerge codepath: `--remote-hostname` selects the
    // remote executor instead of `to_emerge_argv` + `pretend::run`, and
    // any other `--remote-*` without it is a usage error.
    Opt {
        id: "remote_hostname",
        long: "--remote-hostname",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "remote client hostname for binary-package install (selects remote execution)",
    },
    Opt {
        id: "remote_user",
        long: "--remote-user",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "SSH login user on the client (must own the target ROOT)",
    },
    Opt {
        id: "remote_port",
        long: "--remote-port",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "SSH port on the client (default 22)",
    },
    Opt {
        id: "remote_key_file",
        long: "--remote-key-file",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "private key file for client authentication (ssh-agent recommended)",
    },
    Opt {
        id: "remote_timeout",
        long: "--remote-timeout",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "seconds to wait when establishing the SSH connection (default 10)",
    },
    Opt {
        id: "remote_ssh_args",
        long: "--remote-ssh-args",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "extra arguments passed to every ssh invocation",
    },
    Opt {
        id: "remote_strict_host_key_checking",
        long: "--remote-strict-host-key-checking",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &["accept-new", "yes", "no"],
        missing: "",
        help: "host-key policy: trust-on-first-use (default), strict, or off",
    },
    Opt {
        id: "remote_max_clock_skew",
        long: "--remote-max-clock-skew",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "abort if client/server clocks differ by more seconds (default 900, 0 disables)",
    },
    Opt {
        id: "remote_root",
        long: "--remote-root",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "target ROOT on the client (default /)",
    },
    Opt {
        id: "remote_workdir",
        long: "--remote-workdir",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "per-unit work area on the client (default /var/tmp/portage-remote)",
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

/// Whether the emerge codepath (`pretend::run`, the parse loop in
/// `pretend.rs`) has a branch for `long`. Options it handles are
/// forwarded with their value (`--long=<value>`); the rest go BARE so
/// `pretend::run`'s `report_option` names them by their real spelling
/// ("a real emerge action/option, but is not yet implemented in
/// portuale") rather than rejecting a `--long=value` form it never saw.
/// The list is keyed to the parse-loop sections in `pretend.rs` and the
/// real `lib/_emerge/main.py` option table that `emerge_options.rs` pins.
fn emerge_handles(long: &str) -> bool {
    matches!(
        long,
        // actions -- pretend.rs:7267-7820
        "--clean"
            | "--config"
            | "--depclean"
            | "--info"
            | "--list-sets"
            | "--regen"
            | "--search"
            | "--sync"
            | "--unmerge"
            // flags -- pretend.rs:7240-7920
            | "--buildpkgonly"
            | "--columns"
            | "--debug"
            | "--emptytree"
            | "--newrepo"
            | "--newuse"
            | "--nodeps"
            | "--noreplace"
            | "--onlydeps"
            | "--pretend"
            | "--resume"
            | "--searchdesc"
            | "--skipfirst"
            | "--tree"
            | "--update"
            | "--ask"
            | "--buildpkg"
            | "--deselect"
            | "--fuzzy-search"
            | "--getbinpkg"
            | "--getbinpkgonly"
            | "--keep-going"
            | "--quiet"
            | "--quiet-build"
            | "--use-ebuild-visibility"
            | "--usepkg"
            | "--usepkgonly"
            | "--verbose"
            | "--with-test-deps"
            // value options -- pretend.rs:6835-7888
            | "--color"
            | "--misspell-suggestions"
            | "--with-bdeps"
            | "--with-bdeps-auto"
            | "--backtrack"
            | "--solver"
            | "--quickpkg-direct-root"
            | "--search-similarity"
            // optional-value numerics -- pretend.rs:6886-7875
            | "--deep"
            | "--jobs"
            | "--load-average"
            // append options -- pretend.rs:7170-7218, 7660-7690
            | "--buildpkg-exclude"
            | "--exclude"
            | "--reinstall-atoms"
            | "--usepkg-include"
            | "--useoldpkg-atoms"
    )
}

/// Translates a clap match into the canonical long-form argv the emerge
/// codepath (`pretend::run`) parses. Options are emitted in OPTIONS-table
/// definition order (the codepath is order-independent; a canonical order
/// keeps the result deterministic), then the target atoms last. Every
/// emitted token either starts with `--` or is an atom, so the codepath's
/// `-x y` optional-value peek can never consume an unrelated token.
fn to_emerge_argv(matches: &ArgMatches) -> Vec<String> {
    let mut argv = Vec::new();
    for opt in OPTIONS {
        let long = opt.long;
        match opt.kind {
            Kind::Flag => {
                if matches.get_flag(opt.id) {
                    argv.push(long.to_string());
                }
            }
            Kind::Value => {
                if let Some(v) = matches.get_one::<String>(opt.id) {
                    if emerge_handles(long) {
                        argv.push(format!("{long}={v}"));
                    } else {
                        argv.push(long.to_string());
                    }
                }
            }
            Kind::OptionalValue => {
                if let Some(v) = matches.get_one::<String>(opt.id) {
                    if emerge_handles(long)
                        && (v == "True" || (long == "--jobs" && matches!(v.as_str(), "y" | "n")))
                    {
                        // The bare/inserted-literal and `-j y|n` unlimited
                        // forms forward BARE: the codepath's strict `=`
                        // validation would reject `"--deep=True"` /
                        // `"--jobs=y"`, and bare `--jobs` is precisely
                        // what `-j y` means on the real side.
                        argv.push(long.to_string());
                    } else if emerge_handles(long) {
                        argv.push(format!("{long}={v}"));
                    } else {
                        // Not implemented by the codepath (unreachable for
                        // the three current optionals, but keep the rule
                        // uniform): report the option under its real name.
                        argv.push(long.to_string());
                    }
                }
            }
            Kind::Append => {
                let vals: Vec<&String> = matches
                    .get_many::<String>(opt.id)
                    .map(|iter| iter.collect())
                    .unwrap_or_default();
                if vals.is_empty() {
                    continue;
                }
                if emerge_handles(long) {
                    argv.extend(vals.iter().map(|v| format!("{long}={v}")));
                } else {
                    argv.push(long.to_string());
                }
            }
        }
    }
    if let Some(atoms) = matches.get_many::<String>("package") {
        argv.extend(atoms.map(String::clone));
    }
    argv
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
/// then either runs the remote preflight/executor (`--remote-hostname`
/// present -- see `remote.rs`, docs/remote-merge.md) or translates the
/// match into canonical long-form argv and hands it to the emerge
/// codepath -- `pretend::run`, the exact function the `emerge` applet
/// runs, so resolution output and exit codes are literally emerge's.
/// clap keeps its own parser, help, and exit behaviour (help -> 0, usage
/// error -> 2); everything else is the emerge codepath's.
pub fn run(args: &[String]) -> ExitCode {
    let bin = "mrg";
    let joined = join_optional_values(args);
    let argv = std::iter::once(bin).chain(joined.iter().map(String::as_str));
    match command().try_get_matches_from(argv) {
        Ok(matches) => match crate::remote::check_remote(&matches) {
            Ok(Some(ctx)) => crate::remote::run_preflight(&ctx),
            Ok(None) => crate::pretend::run(&to_emerge_argv(&matches)),
            Err(message) => {
                eprintln!("{message}");
                ExitCode::from(2)
            }
        },
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

    /// `-pv pkg` bundles two flags exactly like real argparse does, and
    /// forwards independently to the emerge codepath.
    #[test]
    fn bundled_shorts_parse_independently() {
        let m = parse(&["-pv", "cat/pkg"]).unwrap();
        assert!(m.get_flag("pretend"));
        assert!(m.get_flag("verbose"));
        let atoms: Vec<_> = m.get_many::<String>("package").unwrap().collect();
        assert_eq!(atoms, vec!["cat/pkg"]);
        let argv = to_emerge_argv(&m);
        assert_eq!(argv, ["--pretend", "--verbose", "cat/pkg"]);
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

    /// `--solver=` (portuale-only) accepts exactly the three solvers and
    /// forwards `--solver=<value>` to the emerge codepath; anything else
    /// is a clap usage error, and omitting it forwards nothing (the
    /// codepath defaults to portage).
    #[test]
    fn solver_value_forwards_to_the_emerge_codepath() {
        for value in ["portage", "pubgrub", "resolvo"] {
            let m = parse(&["--pretend", &format!("--solver={value}"), "cat/pkg"]).unwrap();
            assert_eq!(
                m.get_one::<String>("solver").map(String::as_str),
                Some(value)
            );
            let argv = to_emerge_argv(&m);
            assert_eq!(argv, ["--pretend", &format!("--solver={value}"), "cat/pkg"]);
        }
        assert!(parse(&["--pretend", "--solver=sat", "cat/pkg"]).is_err());
        let m = parse(&["--pretend", "cat/pkg"]).unwrap();
        assert_eq!(m.get_one::<String>("solver"), None);
        let argv = to_emerge_argv(&m);
        assert_eq!(argv, ["--pretend", "cat/pkg"]);
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

    /// Nothing is required: `mrg` with no args parses cleanly and
    /// forwards an empty argv (the emerge codepath then reports "no
    /// targets given", exactly like `emerge` with no args).
    #[test]
    fn empty_invocation_forwards_nothing() {
        let m = parse(&[]).unwrap();
        assert!(to_emerge_argv(&m).is_empty());
    }

    /// A `Flag` or toothless option forwards BARE: no `=y` / `=True`
    /// spelling is ever invented for the emerge codepath.
    #[test]
    fn flags_forward_bare() {
        let m = parse(&["-pv", "--update", "cat/pkg"]).unwrap();
        assert_eq!(
            to_emerge_argv(&m),
            ["--pretend", "--update", "--verbose", "cat/pkg"]
        );
    }

    /// Implemented `Value` options forward `--long=<value>`; ones the
    /// emerge codepath does not implement yet forward BARE so it can name
    /// them honestly ("a real emerge option, but is not yet implemented").
    #[test]
    fn value_options_carry_or_shed_their_value() {
        let m = parse(&["--color", "n", "--with-bdeps", "y", "cat/pkg"]).unwrap();
        assert_eq!(
            to_emerge_argv(&m),
            ["--color=n", "--with-bdeps=y", "cat/pkg"]
        );

        let m = parse(&["--root=/x", "--fetchonly", "cat/a"]).unwrap();
        assert_eq!(to_emerge_argv(&m), ["--fetchonly", "--root", "cat/a"]);
    }

    /// Implemented `Append` options forward one `--long=<value>` per
    /// occurrence; unimplemented ones forward BARE exactly once.
    #[test]
    fn append_repeats_per_occurrence() {
        let m = parse(&["-X", "cat/a", "--exclude", "cat/b", "cat/pkg"]).unwrap();
        assert_eq!(
            to_emerge_argv(&m),
            ["--exclude=cat/a", "--exclude=cat/b", "cat/pkg"]
        );

        let m = parse(&["--getbinpkg-exclude", "cat/a", "cat/b"]).unwrap();
        assert_eq!(to_emerge_argv(&m), ["--getbinpkg-exclude", "cat/b"]);
    }

    /// The `OptionalValue` bare/inserted-literal and `-j y|n` unlimited
    /// forms forward BARE (the emerge codepath's strict `=` validation
    /// would reject `--deep=True` / `--jobs=y`); numeric values forward
    /// `--long=N`. This makes `-D`/`-j`/`-j y` mean what real emerge
    /// means, and `-D 2`/`-j 4` explicit.
    #[test]
    fn optional_values_bare_or_numeric() {
        let m = parse(&["-D", "-j", "y", "-l", "2.5", "cat/pkg"]).unwrap();
        assert_eq!(
            to_emerge_argv(&m),
            ["--deep", "--jobs", "--load-average=2.5", "cat/pkg"]
        );

        let m = parse(&["-D", "2", "-j", "4", "cat/pkg"]).unwrap();
        assert_eq!(to_emerge_argv(&m), ["--deep=2", "--jobs=4", "cat/pkg"]);
    }

    /// `--remote-*` parses into `ArgMatches` the remote executor reads:
    /// hostname alone selects remote mode, every companion option rides
    /// along, and the choice option rejects anything outside its set.
    #[test]
    fn remote_options_parse_for_the_remote_executor() {
        let m = parse(&[
            "--remote-hostname",
            "client.invalid",
            "--remote-user",
            "root",
            "--remote-port",
            "2222",
            "--remote-key-file",
            "/tmp/key",
            "--remote-timeout",
            "5",
            // A value starting with `-` needs the `=` form (same rule as
            // every other `Value` option: clap never swallows a following
            // flag-looking token).
            "--remote-ssh-args=-o Foo=yes",
            "--remote-strict-host-key-checking=no",
            "--remote-max-clock-skew=60",
            "--remote-root",
            "/target",
            "--remote-workdir",
            "/tmp/work",
        ])
        .unwrap();
        let get = |id: &str| m.get_one::<String>(id).map(String::as_str);
        assert_eq!(get("remote_hostname"), Some("client.invalid"));
        assert_eq!(get("remote_user"), Some("root"));
        assert_eq!(get("remote_port"), Some("2222"));
        assert_eq!(get("remote_key_file"), Some("/tmp/key"));
        assert_eq!(get("remote_timeout"), Some("5"));
        assert_eq!(get("remote_ssh_args"), Some("-o Foo=yes"));
        assert_eq!(get("remote_strict_host_key_checking"), Some("no"));
        assert_eq!(get("remote_max_clock_skew"), Some("60"));
        assert_eq!(get("remote_root"), Some("/target"));
        assert_eq!(get("remote_workdir"), Some("/tmp/work"));

        // The choice option rejects anything outside accept-new/yes/no.
        assert!(
            parse(&[
                "--remote-hostname",
                "h",
                "--remote-strict-host-key-checking=sometimes"
            ])
            .is_err()
        );
        // Absent entirely: local mode, no remote keys set.
        let m = parse(&["--pretend", "cat/pkg"]).unwrap();
        assert_eq!(m.get_one::<String>("remote_hostname"), None);
    }

    /// Remote options never reach the emerge codepath's argv: remote mode
    /// runs the remote executor, not `pretend::run` (so `emerge_handles`
    /// needs no `--remote-*` arm and `emerge --remote-hostname` stays an
    /// unknown-option error on that applet).
    #[test]
    fn remote_options_are_never_emerge_argv() {
        assert!(!emerge_handles("--remote-hostname"));
        assert!(!emerge_handles("--remote-user"));
    }

    /// `check_remote` validation: hostname selects remote mode with
    /// documented defaults; companions without it, `user@host`, an empty
    /// hostname, and unparsable numerics are usage errors.
    #[test]
    fn remote_consistency_and_defaults() {
        use crate::remote::{StrictHostKeyChecking, check_remote};

        let m = parse(&["--remote-hostname", "client.invalid"]).unwrap();
        let ctx = check_remote(&m).unwrap().expect("remote mode");
        assert_eq!(ctx.hostname, "client.invalid");
        assert_eq!(ctx.user, None);
        assert_eq!(ctx.port, 22);
        assert_eq!(ctx.timeout_secs, 10);
        assert_eq!(
            ctx.strict_host_key_checking,
            StrictHostKeyChecking::AcceptNew
        );
        assert_eq!(ctx.max_clock_skew_secs, 900);
        assert_eq!(ctx.root, "/");
        assert_eq!(ctx.workdir, "/var/tmp/portage-remote");

        let m = parse(&["--pretend", "cat/pkg"]).unwrap();
        assert!(check_remote(&m).unwrap().is_none());

        let m = parse(&["--remote-user", "root"]).unwrap();
        assert!(check_remote(&m).unwrap_err().contains("--remote-hostname"));

        let m = parse(&["--remote-hostname", "root@client.invalid"]).unwrap();
        assert!(check_remote(&m).unwrap_err().contains("--remote-user"));

        let m = parse(&["--remote-hostname", "h", "--remote-port", "99999"]).unwrap();
        assert!(check_remote(&m).unwrap_err().contains("--remote-port"));

        let m = parse(&["--remote-hostname", "h", "--remote-timeout", "soon"]).unwrap();
        assert!(check_remote(&m).unwrap_err().contains("--remote-timeout"));

        let m = parse(&["--remote-hostname", "h", "--remote-max-clock-skew", "x"]).unwrap();
        assert!(
            check_remote(&m)
                .unwrap_err()
                .contains("--remote-max-clock-skew")
        );
    }
}
