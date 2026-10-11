# crates/portage-repo/src/tests/complete_graph.rs
Complete-graph mode tests: the missing deep dep is never merged.

## fn complete_graph_never_merges_a_missing_deep_dep_of_an_installed_pkg
- deeppkg (installed) -> RDEPEND deeppkg2 (installed) -> RDEPEND newpkg (not installed). `-D` deep-walks and merges newpkg.
- Portage's `complete` mode (`_complete_graph` 8662) also deep-walks but swaps to `_select_pkg_from_graph` (graph-or-installed, never a new merge), so the CLI's 2-pass (phase 1 non-complete, phase 2 complete with phase 1's merge set locked) leaves newpkg accounted-for but unmerged.
- The low-level `complete=true` with no locked set is the pre-2-pass fallback: a plain deep walk.
- Unlike `-D`, complete mode doesn't force the deep walk at all unless `--changed-deps-report` needs the wider scan (portuale's deep walk is full resolution per node, far heavier than Portage's).
- Auto-enable: `emerge -u completegraphpkg` is an Upgrade (a version change), so `complete_graph_auto_enable` re-walks when `complete_if_new_ver` is on but not when every trigger is off. After the re-walk the missing deep dep is still not merged.

## fn complete_graph_auto_enable_reasonless_reinstall_with_use_drift
- Backlog #222: a bare reasonless `[ebuild R]` for an explicitly-requested installed package carries no `changed_flags` (only computed under `--newuse`/`--changed-use`), yet Portage still enables complete mode when the merge node's own USE differs from the vdb's (`depgraph.py:8620-8637`).
- Fixture: `dev-libs/cgp0x-2.8.0` (IUSE `+icu`, installed `USE=""`); such a reinstall enables on `if_new_use` even with empty `changed_flags`.
