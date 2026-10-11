# crates/portuale/src/pretend/config_action.rs
`emerge --config` and the misspell-suggestion block. Context moved out of the code comments (#336 Phase 7); the code keeps only the short why.

## fn run_config_action
- Ports Portage's `_emerge/actions.py::action_config` (`emerge --config <atom>`): exactly one atom (Portage's `if len(myfiles) != 1`).
- Matching mirrors `--unmerge`: a bare name goes through a null-category vdb lookup (ambiguous is a hard error); `cat/pkg` goes via `installed_candidates` + `match_from_list`.
- Exactly one match runs `pkg_config` from that version's own vdb-stored `environment.bz2` + `<pf>.ebuild` (`ebuild_merge::run_vdb_saved_env_phase`, ungated -- Portage's `doebuild(ebuildpath, "config", ...)` runs it unconditionally), then a best-effort builddir clean on success (Portage's `doebuild(..., "clean")`).
- The multi-atom error goes to stdout with the string's own trailing newline plus `print`'s (Portage's `print(red("!!! ...\n"))`).
- Without `--ask`, multiple matches list and bail (`Please use a specific atom or the --ask option.`); the `--ask` branch is a numbered menu + `X) Cancel` (`uq.query("Selection?", responses=[1..N, X])`, `X` -> exit `128 + SIGINT`), then a `Ready to configure <cpv>?` prompt (No -> exit 130).
- Note: an older doc comment called `--ask` a v1 cut; the menu and confirm above are implemented, so that claim was stale and is dropped here.
- Portage `action_config`: with `--ask`, prompt `Ready to configure <cpv>?` (No -> exit 130); without it, print `Configuring pkg...`.
- Portage `action_config`: `doebuild(ebuildpath, "clean", ...)` on success -- drop the per-package builddir.

## fn misspell_suggestion_block
- Ports Portage's `depgraph.py:7034-7066` + `dbapi/_similar_name_search.py` for `--misspell-suggestions` (default on).
- Returns the block with no trailing newline -- the caller adds one.
- Portage `depgraph.py:7034-7066` + `dbapi/_similar_name_search.py` (`--misspell-suggestions`, default on): when `resolve_pretend_graph` aborts a top-level atom with `there are no ebuilds to satisfy "<atom>".` **and** that atom's `cat/pkg` genuinely doesn't exist in any repo (Portage `not cp_exists` -- a `cat/pkg` that exists but is only masked gets the autounmask notes instead, never this), print `difflib.get_close_matches` suggestions over every `cat/pkg` in the tree. Returns the block to append to the already-printed `emerge: <message>` (no trailing newline -- the caller adds one), or `None` when it doesn't apply.
- The message's first line is exactly `there are no ebuilds to satisfy "<atom-debug>".` -- pull the quoted atom out.
