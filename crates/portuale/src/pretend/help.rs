//! `emerge --help`.

/// Whether `--help`/`-h` appears anywhere in `args`, including as one
/// character of a short-flag bundle -- see the module doc comment on why
/// this wins unconditionally, checked before anything else.
pub(crate) fn wants_help(args: &[String]) -> bool {
    args.iter().any(|arg| {
        arg == "--help"
            || arg == "-h"
            || (arg.starts_with('-') && !arg.starts_with("--") && arg[1..].contains('h'))
    })
}

/// The `emerge --help` / `-h` text: a grouped tour of every action and
/// option portuale actually implements. NOT a port of real emerge's
/// own `_emerge/help.py` (157 lines of colorized syntax for its full
/// ~130-flag surface -- see the module doc comment). The contract suite
/// pins it in full.
pub(crate) fn print_help() {
    print!("{HELP_TEXT}");
}

pub(crate) const HELP_TEXT: &str = r#"emerge: command-line interface to the Portuale package manager

Portuale is a drop-in Rust reimplementation of Portage: same behaviour,
verified against the Python original by a shared test suite. Any Portage
emerge option or action not listed below is recognized by name
(lib/_emerge/main.py) -- using one reports that it is not yet implemented
in portuale, rather than a generic error.

Usage:
  emerge [options] <target> ...            build and merge the targets, resolving dependencies
  emerge --pretend [options] <target> ...  show what would be merged; change nothing
  emerge --unmerge <atom> ...              remove matching packages
  emerge <action> [options]                run one of the actions listed below
  emerge --help
  A <target> is an atom, an @set, or an installed file / ebuild / tbz2 / gpkg.

Actions (with none of these, the targets are built and merged):
  -C, --unmerge              remove matching packages with no dependency check (CLEAN_DELAY countdown)
      --rage-clean           like --unmerge, with CLEAN_DELAY=0
  -c, --depclean             remove packages that nothing explicitly installed still needs
  -P, --prune                remove all but the highest installed version of a package
      --clean                remove all but the most recently installed version in each slot
      --config               run pkg_config for an installed package
  -W, --deselect[=y|n]       drop atoms / sets from the world favourites (implied by the removals above)
  -s, --search               search package names; -S / --searchdesc also matches DESCRIPTION
      --info                 print the configuration / build-environment block for bug reports
      --list-sets            list the available package sets
      --check-news           report how many unread GLEP 42 news items there are
      --regen                regenerate every repo's metadata/md5-cache (runs each depend phase)
      --metadata             no-op here (md5-cache is read directly); prints Portage's header only
  -r, --resume [--skipfirst] replay the merge list saved by the last failed run (--skipfirst drops entry 1)
  -h, --help                 show this message and exit

Dependency and target selection:
  -u, --update               pull a newer visible version even if the installed one satisfies the atom
  -D, --deep[=N]             also recurse through installed packages' dependencies (optionally N levels)
  -N, --newuse               reinstall an installed package whose USE settings changed
  -U, --changed-use          like -N, but ignore flags newly added to or removed from IUSE
  -e, --emptytree            rebuild the whole dependency tree, treating nothing as installed
  -n, --noreplace            leave a directly named, still satisfied installed atom alone
      --selective[=y|n]      same as --noreplace; =n cancels it
  -1, --oneshot              merge without recording the target in world / world_sets
  -o, --onlydeps             merge the targets' dependencies but not the targets themselves
      --onlydeps-with-rdeps[=y|n]  include runtime deps under --onlydeps (default y)
      --onlydeps-with-ideps[=y|n]  include install-time deps under --onlydeps --onlydeps-with-rdeps=n (default n)
  -O, --nodeps               ignore dependencies entirely
  -X, --exclude ATOMS        never act on a matching package (repeatable, space separated)
      --newrepo              reinstall if the package would now come from a different repo
      --changed-deps[=y|n], --changed-deps-report[=y|n]  react to a *DEPEND differing from the vdb record
      --changed-slot[=y|n]  reinstall if the ebuild's SLOT differs from the vdb record
      --with-bdeps <y|n>, --with-bdeps-auto <y|n>  keep DEPEND/BDEPEND when --deep walks installed packages
      --with-test-deps[=y|n]  also pull a target's test?-gated dependencies
      --root-deps[=rdeps|True]  resolve build dependencies against the running root
      --reinstall-atoms ATOMS  force-reinstall matching installed packages
      --rebuild-if-unbuilt, -new-rev, -new-ver, -new-slot  rebuild an installed package when a build dep is merged
      --rebuild-exclude ATOMS, --rebuild-ignore ATOMS  keep packages out of the rebuild triggers
      --complete-graph[=y|n], --complete-graph-if-new-use, --complete-graph-if-new-ver  force a full deep graph walk
      --ignore-world[=y|n]  ignore the @world set and its dependencies (complete-graph walks args only)
      --dynamic-deps[=y|n]  walk the ebuild (y, default) or the vdb snapshot (n) during --deep
      --backtrack N         maximum resolver backtracking passes (default 20; 0 disables)
      --package-moves[=y|n]  apply profiles/updates/ package moves (default y)
      --misspell-suggestions[=y|n]  suggest close names for a missing cat/pkg
      --implicit-system-deps[=y|n]  order as if @system packages were implicit deps (default y)

Autounmask (read-only: prints the required changes and stops -- never writes config):
      --autounmask[=y|n], --autounmask-use[=y|n], --autounmask-keep-keywords[=y|n]
      --autounmask-license[=y|n], --autounmask-keep-masks[=y|n]
      --autounmask-only[=y|n]  resolve, print the merge list and the change block, and exit 0
      --autounmask-backtrack<y|n>  keep re-resolving after autounmask changes (off by default)
      --autounmask-continue[=y|n]  recognized; implies --autounmask-backtrack=y

Binary packages:
  -b, --buildpkg[=y|n]       also build a binary package for each merged package
  -B, --buildpkgonly         build binary packages only; never merge
      --buildpkg-exclude ATOMS  skip the binary package for matching packages
  -k, --usepkg / -K, --usepkgonly  use a usable binary package when one exists (-K: only, never build)
  -g, --getbinpkg / -G, --getbinpkgonly  also fetch binary packages from a remote binhost (-G: only)
      --usepkg-exclude ATOMS, --usepkg-include ATOMS  narrow which packages may come from a binary
      --binpkg-respect-use[=y|n]  reject a binary package built with the wrong USE
      --binpkg-changed-deps[=y|n]  reject a binary whose recorded deps differ from the ebuild (auto unless -K)
      --use-ebuild-visibility[=y|n]  apply ebuild visibility checks to built packages even under -K
      --rebuilt-binaries[=y|n], --rebuilt-binaries-timestamp N  prefer / bound rebuilt binary packages
      --useoldpkg-atoms ATOMS  prefer an existing binary package for matching atoms
      --quickpkg-direct[=y|n], --quickpkg-direct-root DIR  reuse another root's installed packages as binaries

Build scheduling:
  -j, --jobs[=N]             run up to N package builds in parallel
  -l, --load-average N       hold new builds while the load average exceeds N
  -a, --ask[=y|n]            prompt for confirmation before a real merge or removal
      --ask-enter-invalid    with --ask: a bare Enter is not accepted as Yes
      --read-news[=y|n]      with --ask: offer to read unread news via eselect
      --ignore-default-opts  ignore the EMERGE_DEFAULT_OPTS variable for this run
      --keep-going           on a build failure, drop that package's dependents and carry on
      --quiet-build[=y|n]    redirect a build's phase output to ${T}/build.log (implied by -j >1 and -q)

Output:
  -p, --pretend             resolve and print the merge list; do nothing
  -v, --verbose[=y|n]       add the USE="..." column to each [ebuild ...] line
  -q, --quiet[=y|n]         verbosity level 1: drop the mask column and the USE line
  -t, --tree                show the merge list as a dependency tree
      --columns             lay the merge list out in columns (not together with --tree)
      --unordered-display, --alphabetical  merge-list ordering variants
      --color <y|n>         force colour output on or off
      --verbose-slot-rebuilds[=y|n]  show the atoms forcing a slot-operator rebuild
      --verbose-conflicts   list every parent of a slot conflict, not one per collision reason
      --ignore-built-slot-operator-deps[=y|n]  ignore recorded := slot-operator dependencies
      --depclean-lib-check[=y|n]  with --depclean/--prune: scan for soname breakage (default y)
  -d, --debug               PORTAGE_DEBUG=1 in ebuild phases; resolver trace under --pretend

Portuale extensions (not Portage emerge options):
      --json                dump the resolved graph as one JSON line instead of the display
      --shell <bash|brush>  which real shell runs a merge / unmerge / --config phase chain (default bash)
      --solver <solver>     which dependency solver resolves the graph: portage (default), pubgrub or resolvo

emerge --sync is a permanent non-goal: it prints
"Functionality has moved to `emaint sync`." and exits 1.
See README.md and emerge(1) for the full picture.
"#;
