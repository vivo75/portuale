# crates/portuale/src/pretend/display/counters.rs
The `Total:` summary line for `emerge -pv`. Context moved out of the code comments (#336 Phase 7); the code keeps only the short why.

## struct PackageCountersSummaryArgs
- `orphans` (backlog #80): unresolved rows with no owner entry count in `Conflict:` exactly like entry-owned rows (Portage counts every `Blocker` node in the merge list).
- `tree` (backlog #81): in tree mode the solved-stuck rows count too (C0 p2b: `Conflict: 1 block (all satisfied)` under `--tree -v`).
- `circular` (backlog #221, I5): the settling pass's recorded cycle edges, threaded into the blocker-row counters below.

## fn package_counters_summary
- Ports Portage's `_PackageCounters.__str__` (`output_helpers.py`), which `output.py::print_verbose` emits via `writemsg_stdout(f"\n{self.counters}\n")` -- gated, in Portage too, on `verbosity == 3` (i.e. `-v`), never plain `-p`.
- `Size of downloads:` comes from Portage's `_calc_size`/`counters.totalsize`, via `GraphEntry::download_files`, deduped by filename across the graph like Portage's `myfetchlist`.
- The `Fetch Restriction: N package[s][ (M unsatisfied)]` line comes from `GraphEntry::fetch_restrict` / `fetch_restrict_satisfied`.
- The `Conflict:` suffix is classified per `BlockerConflict::unsolvable` (#68 S2/S3, which gave the rows a satisfied state); a blocked installed package Portage could neither replace nor unmerge is an `_unsolvable_blocker`.
- A top-level package suppressed by `--onlydeps` isn't in Portage's merge list at all (`pkg_info.ordered`), so it isn't counted here either.
- Blocker counting (backlog #72 B1/B2): exactly the rows the merge list carries (`count_blocker_rows`), the same disposition split the line printer uses -- plus the #80 orphans, which print in the same trailing group (unsolvable ones are never suppressed anywhere).
- An ordered uninstall node (backlog #72 B4, Portage `set_pkg_info`, `output.py:619-621`) counts too; it renders in the `Total:` details after `binary`, before `interactive`, and is not part of `total_installs`.
- `interactive` uses `colorize("WARN", "interactive")` -- only the word, not the count.
- Portage `_PackageCounters.__str__` (`output_helpers.py`), the trailing `Total: …` summary line Portage `output.py::print_verbose` emits via `writemsg_stdout(f"\n{self.counters}\n")` -- gated, in Portage portage too, on `verbosity == 3` (i.e. `-v`), never plain `-p`. Now includes `, Size of downloads: …` (Portage `_calc_size`/`counters.totalsize`, via `GraphEntry::download_files`, deduped by filename across the graph like Portage `myfetchlist`) and the `\nFetch Restriction: N package[s][ (M unsatisfied)]` line (from `GraphEntry::fetch_restrict` / `fetch_restrict_satisfied`). The `Conflict:` line's own `(N unsatisfied)`/`(all satisfied)` suffix is classified per `BlockerConflict::unsolvable` (#68 S2/S3); the resolution is still report-only, never an enforcement, as `resolve_pretend_graph`'s doc comment (portage-repo) explains. A top-level package suppressed by `--onlydeps` isn't in Portage's merge list at all (`pkg_info.ordered`), so it isn't counted here either.
- Portage `total_installs = upgrades + downgrades + newslot + new + reinst`.
- Portage `__str__`: `f", Size of downloads: {localized_size(self.totalsize)}"` -- appended to the `Total:` line unconditionally.
- Portage `_PackageCounters.__str__`: `bad(f" (N unsatisfied)")` -- the whole parenthetical is red (`bad` = `BAD` = red).
- Portage `_PackageCounters.__str__`: `bad(f" ({N} unsatisfied)")` when the resolver left any `_unsolvable_blockers` -- a blocked installed package it could neither replace nor unmerge -- and `good(" (all satisfied)")` when every block resolved since #68 S2/S3 gave the rows a satisfied state.
