//! `mrg`: a portuale-only applet (there will never be a Portage counterpart)
//! offering Portage's `emerge` option surface -- every short and long option
//! keeps Portage's spelling -- over portuale's own emerge codepath. A clap
//! front end parses the options, translates the match into canonical
//! long-form argv (see `to_emerge_argv`), and hands it to `pretend::run`,
//! the exact function the `emerge` applet runs. Only the option *names* are
//! shared; semantics beyond the names may differ. Exit codes follow argparse
//! (0 for success/help, 2 for usage errors) via clap's own rendering.
// Portage: _emerge/main.py (options, shortmapping, longopt_aliases, argument_options, actions)

use clap::builder::PossibleValuesParser;
use clap::{Arg, ArgAction, ArgMatches, Command};
#[cfg(any(feature = "vdb-sqlite", feature = "vdb-redb"))]
use portage_vdb::InstalledDb;
use std::process::ExitCode;

/// How one emerge option is modelled. Drives the clap `Arg` build (in
/// `command()`) and the `to_emerge_argv` forward translation, so the two
/// can never drift apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Plain boolean: `ArgAction::SetTrue`. (The y/n optional-value family
    /// is modelled as flags too; see the notes.)
    Flag,
    /// Required value: `ArgAction::Set`.
    Value,
    /// Optional numeric value (`--deep`/`--jobs`/`--load-average`):
    /// `ArgAction::Set`, `num_args(0..=1)`, bare -> `missing`.
    OptionalValue,
    /// Repeatable: `ArgAction::Append`.
    Append,
}

struct Opt {
    /// clap arg id (the dest), also the `to_emerge_argv` lookup key.
    id: &'static str,
    /// Canonical long option, exactly as Portage spells it.
    long: &'static str,
    /// Secondary long name for the `longopt_aliases` pair
    /// (`--cols`/`--skip-first`); `None` for everything else.
    alias: Option<&'static str>,
    /// Short option, exactly as Portage's `shortmapping` /
    /// `argument_options` spells it; `None` when Portage has none.
    short: Option<char>,
    kind: Kind,
    /// Value choices for required-value options (empty = accept any string).
    choices: &'static [&'static str],
    /// Bare-flag default for `OptionalValue` options.
    missing: &'static str,
    /// One-line help shown by `mrg --help` (short, honest, portuale's own
    /// wording).
    help: &'static str,
}

/// Splits `OPTIONS` into "Actions" and everything else, for clap's help headings.
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

/// The `actions` frozenset + `options` list + y/n optional-value family
/// (the non-value options). Every entry keeps Portage's spelling.
const OPTIONS: &[Opt] = &[
    // --- The `actions` frozenset: a standalone action replaces the
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
    // --- The `options` list (plain boolean store_true options), with
    // `shortmapping` shorts where Portage defines one.
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
    // --- The y/n/True optional-value family (`default_arg_opts`),
    // modelled as plain flags (see the notes for why). Short aliases are
    // Portage's own.
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
        id: "read_news",
        long: "--read-news",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "with --ask: offer to read unread news via eselect",
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
    // --- Required-value options. `choices` non-empty = the closed choice
    // set; empty = accept any store value like the `action: "store"` case.
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
    // Portuale-only (Portage has no `--solver`): which dependency
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
    // --- The optional-value numeric options. Bare form inserts Portage's
    // own literal default "True"; `join_optional_values` + `require_equals`
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
    // --- `action: "append"` options (repeatable).
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
    Opt {
        id: "remote_transport",
        long: "--remote-transport",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &["ssh", "local"],
        missing: "",
        help: "how driver scripts and files reach the client (default ssh)",
    },
    Opt {
        id: "remote_binpkg",
        long: "--remote-binpkg",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "bundle, stream and unpack one explicit binpkg file (bypasses resolution)",
    },
    Opt {
        id: "remote_config_protect",
        long: "--remote-config-protect",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "space-separated CONFIG_PROTECT list for the client merge (default /etc)",
    },
    Opt {
        id: "remote_config_protect_mask",
        long: "--remote-config-protect-mask",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "space-separated CONFIG_PROTECT_MASK list for the client merge (default /etc/env.d)",
    },
    Opt {
        id: "remote_etc_portage",
        long: "--remote-etc-portage",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "where /etc/portage resolves from: server:<path> or client:<path> (default client:/etc/portage)",
    },
    Opt {
        id: "remote_vdb",
        long: "--remote-vdb",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "where the client installed-db lives: server:<path> (stateless: no old hooks, fail-closed collisions) or client:<path> (default client:<root>/var/db/pkg)",
    },
    Opt {
        id: "remote_edb",
        long: "--remote-edb",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "binhost-index cache side, informational: server:<path> only (default server:<root>/var/cache/edb); the client has no binhosts",
    },
    Opt {
        id: "remote_ledger_dir",
        long: "--remote-ledger-dir",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "server ledger directory override (default <placed-PKGDIR>/remote-ledger)",
    },
    Opt {
        id: "remote_require_ledger_match",
        long: "--remote-require-ledger-match",
        alias: None,
        short: None,
        kind: Kind::Flag,
        choices: &[],
        missing: "",
        help: "enforce that the client's ledger provenance matches the server's record for this hostname (abort on mismatch, before anything merges)",
    },
    Opt {
        id: "remote_portuale_binary",
        long: "--remote-portuale-binary",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "portuale binary to install and verify on the client (default: this binary); preflight compares its SHA-256 and ELF machine with the client's, and the install streams this file",
    },
    Opt {
        id: "remote_portuale_dir",
        long: "--remote-portuale-dir",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "absolute client directory holding the portuale binary (default: first writable of /opt/bin, /usr/local/bin); a non-root login needs this pointing at a directory it can write",
    },
    // mrg-only: which installed-package database backend the run reads.
    // Never reaches `to_emerge_argv` (ids start with `vdb_`) and is not an
    // `emerge` option.
    Opt {
        id: "vdb_backend",
        long: "--vdb-backend",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &["files", "sqlite", "redb"],
        missing: "",
        help: "installed-package database backend: files (default, the classic VDB tree), sqlite or redb; also PORTUALE_VDB_BACKEND (environment, then make.conf)",
    },
    Opt {
        id: "vdb_path",
        long: "--vdb-path",
        alias: None,
        short: None,
        kind: Kind::Value,
        choices: &[],
        missing: "",
        help: "database file for sqlite/redb (default <ROOT>/var/lib/portage/vdb.sqlite or vdb.redb); also PORTUALE_VDB_PATH; ignored with files",
    },
];

/// Builds the clap `Arg` for one `Opt` entry, keeping Portage's short/long
/// spellings.
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

/// The clap `Command` for the whole `mrg` surface: every emerge option
/// (grouped into clap's "Actions"/"Options" help sections) plus the target
/// package atoms as positionals.
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

/// Portage's `valid_integers` (`int(s) >= 0`).
fn valid_integer(s: &str) -> bool {
    let Ok(v) = s.parse::<i128>() else {
        return false;
    };
    v >= 0
}

/// The `--jobs` variant: Portage's `valid_integers_or_y_or_n`.
fn valid_integer_or_y_or_n(s: &str) -> bool {
    valid_integer(s) || s == "y" || s == "n"
}

/// Portage's `valid_floats` (`float(s) >= 0`). A non-finite result still
/// compares against 0 like Python's `float(s) >= 0` does.
fn valid_float(s: &str) -> bool {
    let Ok(v) = s.parse::<f64>() else {
        return false;
    };
    v >= 0.0
}

/// Portage's `insert_optional_args`, trimmed to what this parser needs:
/// `--deep`/`-D`, `--jobs`/`-j`, `--load-average`/`-l` consume the *separate*
/// following token as their value only when it passes Portage's own
/// validator, and never otherwise. Since the clap arg is
/// `require_equals(true)`, that join happens here: a bare `-D cat/pkg` stays
/// `-D` (bare, `"True"`) with `cat/pkg` as an atom, and `-j 4` becomes
/// `--jobs=4`. Attached short forms (`-j4`, `-D2`, `-l2.5`) are joined to
/// their explicit long form too. Every other token passes through untouched.
fn join_optional_values(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut iter = args.iter();
    'outer: while let Some(arg) = iter.next() {
        // Attached short forms (`-j4`, `-D2`, `-l2.5`): Portage expands
        // these itself, and clap's require_equals rejects the attached
        // spelling, so join them into the explicit long form here when the
        // suffix is a valid value.
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

// ---------------------------------------------------------------------
// VDB backend selection
// ---------------------------------------------------------------------

use portage_vdb::BackendKind;
use std::path::{Path, PathBuf};

/// Shared by the database backends: refuse a missing file (mrg never
/// creates one), then, after the open, print the stale-import warning
/// and one warning per entry an interrupted merge left behind.
#[cfg(any(feature = "vdb-sqlite", feature = "vdb-redb"))]
fn check_db_exists(path: &Path, root: &Path, kind: BackendKind) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    Err(format!(
        "the {kind} VDB {} does not exist (portuale never creates one implicitly); \
         convert first: portuale vdb convert --from files:{} --to {kind}:{}",
        path.display(),
        root.display(),
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<ArgMatches, clap::Error> {
        let joined = join_optional_values(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let argv = std::iter::once("mrg").chain(joined.iter().map(String::as_str));
        command().try_get_matches_from(argv)
    }

    /// Every table short option must be unique, so no Portage short spelling
    /// is ever shadowed by a clap-invented duplicate.
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

    /// `-j4` (attached), `-j 4`/`--jobs=4` (separate) all carry the value;
    /// a bare `-j` falls back to the inserted "True".
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

    /// A bare `-D` matches Portage: a following flag OR a following atom is
    /// NOT consumed as the (non-valid) value -- the option stays "True" and
    /// the token stays wherever it belongs.
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

    /// `--jobs y`/`--load-average 2.5` join the separate valid-value forms
    /// like `insert_optional_args` does.
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

    /// `--columns` and its `--cols` alias name the same flag.
    #[test]
    fn longopt_alias_columns() {
        for args in [&["--columns"][..], &["--cols"][..]] {
            let m = parse(args).unwrap();
            assert!(m.get_flag("columns"), "{args:?}");
        }
    }

    /// A required value missing (bare `--color`) is a clap usage error, and
    /// so is any short spelling Portage never defined.
    #[test]
    fn usage_errors_are_errors() {
        for args in [&["--color"][..], &["-I"][..], &["--bogus"][..]] {
            let err = parse(args).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{args:?}");
        }
    }

    /// A repeatable option collects its `action: "append"` list.
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
}
