# crates/portuale/src/emerge_build/tests/resume.rs
Resume-list unit tests.

## fn resume_cpv_maps_source_and_binary_and_skips_non_merges
- Ports Portage's `Scheduler._save_resume_list` `operation == "merge"` filter.

## fn resume_binary_repo_comes_from_the_binary_pool_not_the_ebuild_repos
- Backlog #186: Portage's `depgraph.py::_loadResumeCommand` re-resolves a `["binary", root, cpv, "merge"]` resume item from the bintree. A record without `REPO` is `"__unknown__"` (what a fresh binary merge shows for it); `None` means the caller keeps the ebuild fallback.

## fn resume_entry_uses_the_binary_repo_for_binary_entries_only
- Backlog #186: `binary_repo` wins for a `Binary` entry even when an ebuild repo would resolve (here there are no repos at all, so the fallback would be `None` either way and the win is exact).

## fn run_merge_loop_shrinks_the_saved_resume_list_per_successful_merge
- Backlog #168 through the production serial loop: each successful merge removes exactly that entry (Portage `Scheduler.py:1595-1602`).
