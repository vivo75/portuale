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

## fn qualified_suggestion_matches / bare_name_matches (#535)
- Backlog #535, Task 6 Step 1: the qualified branch used to match
  case-sensitively, so `app-misc/Cor` competed as `Cor` and lost to
  `app-misc/soreqb` for `app-misc/zznope1`. Portage lowercases every
  candidate and matches the lowered target, mapping winners back to
  original case (`dbapi/_similar_name_search.py`); over the same candidate
  set it answers `E, foo, Cor`, which is what portuale prints now (Rust
  `qualified_branch_matches_lowercased_and_restores_case`, oracle: live
  host probe of `similar_name_search` over the fixture+s511 set).
- The bare-name branch keeps Portage's `identical` nuance: a lowered name
  equal to the typed one is dropped only when every original-case owner is
  case-identical to it, so a `cat/foo`+`cat/Foo` pair is kept for `foo`
  (both orders observed stable across PYTHONHASHSEED 1,2,3,7; portuale's
  map order is deterministic).
- What was NOT fixed: live Portage answers `note, gnote, pet` in the 511
  probe env because its portdb always carries the implicit EPREFIX `gentoo`
  repo alongside the configured ones; portuale resolves configured repos
  only (hermetic fixture model). Same for which miss is named first in a
  multi-atom abort (resolver walk order; owner wontfix, #535).

## fn misspell_suggestion_block
- Ports Portage's `depgraph.py:7034-7066` + `dbapi/_similar_name_search.py` for `--misspell-suggestions` (default on).
- Returns the block with no trailing newline -- the caller adds one.
- Portage `depgraph.py:7034-7066` + `dbapi/_similar_name_search.py` (`--misspell-suggestions`, default on): when `resolve_pretend_graph` aborts a top-level atom with `there are no ebuilds to satisfy "<atom>".` **and** that atom's `cat/pkg` genuinely doesn't exist in any repo (Portage `not cp_exists` -- a `cat/pkg` that exists but is only masked gets the autounmask notes instead, never this), print `difflib.get_close_matches` suggestions over every `cat/pkg` in the tree. Returns the block to append to the already-printed `emerge: <message>` (no trailing newline -- the caller adds one), or `None` when it doesn't apply.
- The message's first line is exactly `there are no ebuilds to satisfy "<atom-debug>".` -- pull the quoted atom out.
- No similar-names block for an installed-only cp (#360 S4): Portage `cp_exists` is true via the installed db (`depgraph.py:7018-7040`), so the S0-backed `360-gone-pv` capture prints the bare line with no `Maybe you meant` block. The check reads the target-root vdb (`installed_candidates`); a genuinely unknown cp keeps the old block.
