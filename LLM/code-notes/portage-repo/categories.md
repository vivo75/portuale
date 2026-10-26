# crates/portage-repo/src/categories.rs
The repo-traversal category allow-list (`settings.categories`).

## fn repo_categories
- Portage `config.py:948-960` builds `settings.categories` as the incremental union of one `categories` file per location in `LocationsManager.profile_and_user_locations`, filtered by `dbapi._category_re`. Those locations are `[PORTDIR/profiles]` + each overlay's `profiles/` dir + the user config dir (`LocationsManager.py:420-440`). The selected profile chain is NOT consulted: a `categories` file inside the chosen profile dir is never read for this set (relevant to Task 12, which consumes this function for the broader allow-list).
- Source order here mirrors Portage: the main repo (`is_main`, i.e. Portage's `PORTDIR`) first, then the other repos in `find_repos` order, then `<config_root>/etc/portage/categories` last so user `-` removals win. Portage orders overlays by `PORTDIR_OVERLAY`; portuale keeps `find_repos`' `(priority, name)` order instead — only observable with cross-repo `-` removals, which no shipped repo uses.
- The user file always stacks: the old `local_config` parameter (which skipped it, modelling Portage's `local_config=False` repoman/emirrordist-style configs) is removed — every product path passed `true`, so the flag carried no behaviour (decided YAGNI, #571 follow-up).
- Each file is read with `grabfile` semantics (`util/__init__.py:171-200`: whitespace-split tokens, `#` cuts the line, missing file contributes nothing); stacking is `stack_lists(incremental=1)` (`-*` resets, `-x` removes, unmatched removals silent); the name filter is `dbapi._category_re` (`^\w[-.+\w]*$`, ASCII — leading digits allowed, leading dots rejected).

## struct Categories
- The run-shared `settings.categories`: the resolved `set` plus the once-per-cp `!!!` record (`warned`). Built once per run (`Categories::load`, called by `pretend::run`) and shared by every gate — resolve walks (via `ResolveRequest::categories`, cloned with the request across the best-run re-pass) and display gates alike — so a backtracking run prints the unlisted-category notice once. Portage memoises `cp_list` once frozen (`xcache["cp-list"]`, `porttree.py:1138-1151`). Callers without a run (unit tests, other binaries) get a fresh one per walk from `ResolveCtx::new`/`LazyRepo::build`.
- `CategoryGate::new` intentionally takes the repo list separately: `Categories` owns no repos, so one set can gate any repo slice (tests reuse the miniature pair).

## fn read_categories_file
- `grabfile` without `remember_source_file`: one file's whitespace-separated tokens with inline-`#` comments cut. A missing/unreadable file yields nothing (Portage `grablines` returns `[]` on `IOError`).

## fn stack_categories
- `stack_lists(lists, incremental=1)`: ordered dedup with `-` removal and `-*` reset across the whole source stack. The set is filtered by `category_name_ok` only after the full fold, exactly like Portage filters after stacking.

## fn category_name_ok
- `dbapi._category_re = re.compile(r"^\w[-.+\w]*$", re.ASCII)` as a predicate: first char ASCII alnum or `_`, rest ASCII alnum or `_`, `.`, `-`, `+`.

## tests: repo_categories_unions_repo_and_user_files
- Three sources (main repo, overlay, user file) union with comment/blank tolerance. The overlay sorts before the main repo by name on purpose, pinning that `is_main` — not sort order — decides precedence.

## tests: repo_categories_stacks_removals_incrementally
- An overlay `-` removal drops a main-repo entry, an unmatched removal is silent, and a user-file `-*` resets the whole stack (Portage `stack_lists` semantics).

## tests: repo_categories_tolerates_missing_files_and_filters_names
- Absent repo and user files yield the empty set (a categories-less repo contributes nothing — the shape every synthetic `--regen` test repo had before gaining its file). Name filter pins: leading digits kept, leading dots dropped, trailing comments cut.

## tests: repo_categories_stacks_the_user_file
- The user file's entries join the repo files' union.
