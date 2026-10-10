//! The autounmask suggestion blocks and the --autounmask-only / --autounmask-continue exits.

use crate::pretend::*;
use portage_profile::Config;
use portage_repo::GraphResult;
use portage_repo::RepoConfig;

/// The values [`show_autounmask`] reads from its caller.
pub(crate) struct ShowAutounmaskArgs<'a> {
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) color: &'a Colorizer,
    pub(crate) result: &'a GraphResult,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) config: &'a Config,
    pub(crate) has_autounmask_changes: bool,
}

/// The autounmask suggestion blocks and the --autounmask-only / --autounmask-continue exits.
pub(crate) fn show_autounmask(args: ShowAutounmaskArgs<'_>) -> Option<ExitCode> {
    let ShowAutounmaskArgs {
        opts,
        color,
        result,
        repos,
        root,
        config,
        has_autounmask_changes,
    } = args;
    let &EmergeOpts {
        autounmask_only,
        pretend,
        autounmask_continue,
        ..
    } = opts;
    // Real `depgraph.py::_display_autounmask` (`:10625`), the
    // `unstable_keyword_msg` half: `--autounmask` accepted a
    // `KEYWORDS`-alone mask to make the graph resolve, so the implicit
    // `package.accept_keywords` change is reported after the merge list.
    // Real `_writemsg`: `\nThe following <BAD>keyword changes</BAD> are
    // necessary to proceed:\n (see "package.accept_keywords" in the
    // portage(5) man page for more details)\n`; then `format_msg`
    // (`#`-prefixed dep-chain comment lines stay plain, the `=<cpv>
    // <kw>` line is `INFORM`-coloured). One header covers every change.
    // Real portage does NOT print the "Use --autounmask-write" hint
    // under `--pretend` (`:11084` `not pretend`), and `emerge --pretend`
    // still exits 0 (real `actions.py:563` `return os.EX_OK`).
    let print_autounmask_block =
        |reason: &str, file: &str, changes: &[portage_repo::AutounmaskChange]| {
            if changes.is_empty() {
                return;
            }
            eprintln!(
                "\nThe following {} are necessary to proceed:",
                color.c("BAD", reason)
            );
            eprintln!(" (see \"{file}\" in the portage(5) man page for more details)");
            for change in changes {
                for line in &change.dep_chain {
                    eprintln!("# {line}");
                }
                let line = if change.token.is_empty() {
                    change.atom.clone()
                } else {
                    format!("{} {}", change.atom, change.token)
                };
                eprintln!("{}", color.c("INFORM", &line));
            }
        };
    // Real `_display_autounmask` `_writemsg` order: keyword, mask, USE,
    // license. The `atom` field already carries its own op prefix (`=`
    // for keywords/masks, `>=`/`>=…:slot`/`=` for USE/license).
    print_autounmask_block(
        "keyword changes",
        "package.accept_keywords",
        &result.autounmask_keyword_changes,
    );
    print_autounmask_block(
        "mask changes",
        "package.unmask",
        &result.autounmask_mask_changes,
    );
    print_autounmask_block("USE changes", "package.use", &result.autounmask_use_changes);
    print_autounmask_block(
        "license changes",
        "package.license",
        &result.autounmask_license_changes,
    );

    // `--autounmask-only`: real `actions.py:456` returns 0 right after
    // `display_problems()` -- nothing below (the abi-rebuild info, the
    // changed-deps report, and any real merge) runs. Real still calls
    // `post_emerge` after (`run_action`'s unconditional tail), so a
    // `--pretend` run prints the end notice here too.
    if autounmask_only {
        pretend_end_news_notice(pretend, repos, root, config, color);
        return Some(ExitCode::SUCCESS);
    }

    // Real `action_build`: `backtrack_depgraph` returns `success=False`
    // whenever `_have_autounmask_changes()` (i.e. autounmask had to
    // touch `package.use`/`.accept_keywords`/`.unmask`/`.license` to make
    // the graph resolve), and then `if not success:
    // mydepgraph.display_problems(); return 1` -- BEFORE any merge, and
    // under `--pretend` too (`--pretend` does not exempt it). The merge
    // list and the change blocks just printed ARE `display_problems()`'s
    // output; the run then fails so a caller's `$?` check sees that the
    // config still needs editing. The one exception is a real (non-
    // `--pretend`) `--autounmask-continue`, which writes the changes and
    // proceeds (real `_resolve`'s own `'--pretend' not in myopts` guard).
    // (`has_autounmask_changes` is bound once, above, next to the
    // `--autounmask-only` re-show gate -- backlog #271.)
    let autounmask_continue_active = !pretend && autounmask_continue == Some(true);

    // Real `_display_autounmask`'s tail (`depgraph.py:11093`, gated on
    // `_dynamic_config._autounmask_backtrack_disabled`): with
    // `--autounmask-backtrack` off (the default), the resolver stops
    // after the first autounmask batch instead of re-driving, and says
    // so -- `--pretend` included. `--autounmask-continue` implies
    // backtrack=y, so the notice is suppressed there. Backlog #217:
    // the flag is set at exactly one site (`need_config_change:11752`),
    // reached only when the early return (`:11713-11717`) did NOT fire
    // -- i.e. NOT when `_success_without_autounmask` holds (the
    // `_resolve` tail `:5793`, "reserved for cases where there are
    // *zero* other problems": the resolve succeeds once the autounmask
    // changes are applied, no other failure). A lone autounmask change
    // (oracle: `abort-au-plain`, `fixtures/abort-captures/`) therefore
    // prints the change block with NO notice; the notice stays exactly
    // when another failure coincides (a cycle: `abort-au-cycle`; a
    // restart-worthy flip on an already-graphed package: `aucasctop`;
    // a post-failure parent rescue). `GraphResult::
    // autounmask_backtrack_disabled` ports the flag, including real's
    // `_allow_backtracking` conjunct (`--backtrack=0` never prints it).
    if result.autounmask_backtrack_disabled(config.autounmask_backtrack) {
        eprintln!();
        for line in [
            "In order to avoid wasting time, backtracking has terminated early",
            "due to the above autounmask change(s). The --autounmask-backtrack=y",
            "option can be used to force further backtracking, but there is no",
            "guarantee that it will produce a solution.",
        ] {
            eprintln!(" {} {line}", color.c("WARN", "*"));
        }
    }

    if has_autounmask_changes && !autounmask_continue_active {
        pretend_end_news_notice(pretend, repos, root, config, color);
        return Some(ExitCode::from(1));
    }
    None
}
