# crates/portuale/src/pretend/json.rs
`--json` output.

## fn json_escape
- Was hand-rolled rather than a JSON crate: `--json`'s own output is a small flat shape, and portuale has no other dependency beyond `regex` anywhere in the workspace. `--json` is NOT a port of any Portage emerge behavior (Portage has no structured-output mode for `--pretend`); a portuale-specific convenience, requested directly by name, not routed through `emerge_options.rs`'s CLI-surface tables. The contract suite and `tests/output_invariants.py` read this output; the byte shape is pinned by the harvested corpus (`tests/corpus/`).

## fn entry_to_json
- `source` mirrors `entry.source` / the plain-text loop's `bracket` variable (`"binary"`/`"ebuild"`, Portage `RootConfig.py`'s `pkg_tree_map`-driven `type_name`) -- until the binary-package slice (`--usepkg`/`--usepkgonly`, `portage-repo`) this was always `"ebuild"` unconditionally.
- `provenance` (alongside `source`, so also absent for `NoVisibleCandidate`) mirrors `entry.provenance` / `VisibilityProvenance` (portage-repo): which `package.mask`/`.unmask`/`package.accept_keywords` entries, if any, were actually load-bearing for this candidate to be visible at all. Each of its three fields is `null` rather than omitted when not applicable -- there is no `verbose` gate here (exactly the kind of detail `--json` exists to expose unconditionally) and a consumer scripting against this output should not have to branch on whether the key is even present. `keyword_suggestion` is `provenance`'s mirror image -- present (as `{"version", "keyword"}` or `null`) only for `NoVisibleCandidate` entries, since that is the one outcome with nothing visible to trace provenance for and something to suggest instead. See `entry.keyword_suggestion` (portage-repo).
- `requested`: whether this exact category/package was one of `atoms` directly, as opposed to reached only via a dependency string. `required_by`: which package(s) pulled it in -- see `GraphEntry::required_by` (portage-repo) for how it is tracked through the BFS.
- Backlog #72 B3/B4: `--json` filters `Uninstall` entries out (see `print_json`); the tag exists for B4's outcome string.
- One JSON object per `GraphEntry` -- a structured mirror of the plain- text `[ebuild ...]`/`[binary ...]`/"already installed"/blocker lines above, plus two fields no plain-text line carries at all: `requested` (was this exact category/package one of `atoms` directly, as opposed to reached only via a dependency string) and `required_by` (which package(s), if any, pulled it in that way -- see `GraphEntry::required_by`'s own doc comment, portage-repo).
- The entry's 0-based position in Portage's dependency-first merge order (`portage_repo::topological_merge_order`). The `entries` array is already emitted in this order -- the field is here so a consumer that re-sorts or filters the array keeps the schedule.
- Portage `output.py`'s own `S` bracket column, exposed unconditionally (like every other `--json` field, see this module's own doc comment): `true` for a `New` into a slot the package isn't installed in while another slot of it is (`GraphEntry::new_slot`).
- Portage `output.py:833`'s own `I` bracket column, exposed unconditionally: `true` for a merge-bound entry whose evaluated `PROPERTIES` contains `interactive` (`GraphEntry::interactive`).
- Portage `output.py:633`'s own `f`/`F` fetch-restrict column (`GraphEntry::fetch_restrict` / `fetch_restrict_satisfied`).
- `--root-deps`'s own running-root build entries (see `root_suffix`'s own doc comment and `GraphEntry::targets_running_root`): the same `to <root>` distinction the plain-text output carries, as an explicit field -- the running-root path string for such an entry, `null` for every ordinary `ROOT`-targeted one.

## fn print_json
- `backtrack_restarts`: backlog #59 S2 (K1 option (a)) -- the deterministic `(backtrack: N/M)` count Portage's `_show_resolution_report` prints on every run.

## fn autounmask_change_to_json
- The whole `--json` output: `{"entries": [...], "slot_conflicts": [...]}`, one line, no pretty-printing (a portuale-specific convenience format, not a stable schema -- see the module doc comment).

## fn abort_outcome_to_json
- Backlog #19 Slice 4: the `--json` `aborted` field for a resolve outcome — `null` on the complete path, `{"reason":…}` on an abort.

## fn print_removal_json
- Charter property 4 (health checker, 2026-11-07): the pretend removal paths (`-C`, `--rage-clean`, `--depclean`, `--prune`, `--clean`) printed no `--json` document at all; this one line is printed after the text display, so the text output is unchanged and `--json` stays additive. Both display tails call it (`run_unmerge_pretend` and `run_prune_nodeps_or_clean`), which is why `json` is threaded through every removal args struct.
- `needed_libraries` comes from `ebuild_merge::libs_still_linked` (the same `_find_libs_to_preserve` machinery the real unmerge uses); `preserve_libs` is the same gate `ebuild_unmerge.rs` applies (`FEATURES=preserve-libs` and the installed `RESTRICT`), so `preserved` is what the real removal keeps. The checker (`pmtest/pytests-contract-suite/health_checker.py`, `check_removal_plan`) does not trust `needed_libraries`: it recomputes linkage from `NEEDED.ELF.2` and the disk and only reads `preserved`.

## fn print_json (request)
- `request` (with `-v`, next to `kept`): the raw command-line atoms and sets, so the property-3 checker can tell a version the user asked for (`=cat/pkg-1.2`) from one the resolver chose. Gated on `-v` like `kept` to keep the non-verbose pins unchanged.
