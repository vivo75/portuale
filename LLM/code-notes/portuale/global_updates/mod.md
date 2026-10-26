# crates/portuale/src/global_updates/mod.rs
Per-repo updates map for the `_do_global_updates` port (backlog #576): ordered repo lists with the `DEFAULT` alias.

## struct RepoMatch
- The `_do_global_updates` `repo_match` closure (`_global_updates.py:161-164`) in one place: `new` snapshots the map's repo names plus the main repo's name, and `matches(repo_name, repository)` answers the same-repo-or-main-covering-unmapped check. Used by the vdb moves and both config-files passes (world, `package.*`), each through a one-line closure adapter.
- Kept separate from `repo_update_map` because the map outlives the borrow: callers build the map, then the matcher from it.

## fn repo_update_map
- Ports `_global_updates.py:79-153`: one entry per repo that HAS an updates directory (missing dir means omitted, `:82`); repos sharing one directory share its list (`updpath_map`, `:85-87`); lists in `portdb.getRepositories()` order, i.e. the caller's `repos` order; the main repo's list aliased as `DEFAULT` (`:149-153`), appended last so iteration meets it after the named repos. Empty lists stay (Portage keeps them in `repo_map` and skips them at apply time, `:158`), so `repository not in repo_map` checks see every repo.
- Reads via `portage-repo`'s `grab_updates_for_repo` (no `prev_mtimes` filter yet -- the changed-file gating and the record return with the commit in a later piece). Ordering note: `grab_updates_for_repo` sorts quarter files `(year, quarter)`; Portage's `grab_updates` returns `scandir` order (arbitrary per filesystem). Chained moves across files could therefore apply in a different order; the fixture has one file so the capture cannot tell.
