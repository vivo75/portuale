# Backlog tasks

One line per open task. Each has a code/doc pointer so a fresh model can
start without a full context load. Source of truth for detail:
[`scope-backlog.md`](scope-backlog.md) (section letters in brackets),
`what-this-proves.md` (what already shipped), `git log`, and the memory
notes under
`.claude/projects/-home-vivo-repo-PORTUALE-portuale/memory/`.

Rules that apply to every task: ship Rust + `python/emerge_pretend_reference.py`
in one commit, verify byte-identical via `tests/test_emerge_pretend_contract.py`,
run the full verification pass (`AGENTS.md` step 8). Real-execution-only
features have no Python mirror.

---

## Tier 1 — focused slices (scoped, ~one sitting each)

1. **Release-profile `lto`/`codegen-units`** — set `lto="thin"` + `codegen-units=1` in `rust/Cargo.toml` `[profile.release]`; measure vs `-puD --getbinpkg`. See `performances-tuning.md` §5.
2. **`--solver=` module-doc refresh** — the "v1 cuts" list in `rust/portage-repo/src/solver_bridge.rs` is partly stale (blockers/ABI/merge-order edges now wired). Doc-only.
3. **`--solver=` forced-flag markers** — bridge output drops the `( )` USE markers real prints for forced/masked flags. Fix in `solver_bridge.rs` render path. [J]
4. **`--solver=` notice fields** — `slot_conflicts` / `autounmask_*` / `circular_deps` are hard-coded empty under `--solver=`. Wire them from the bridge result in `solver_bridge.rs`. [J]
5. **brush PRs: submit staged fixes** — `docs/brush-pr/{01,02,03}` (nested-heredoc tokenizer, heredoc AST serialization, pipeline-function deadlock) are prepared vs upstream; open them on `reubeno/brush`. [G]
6. **brush re-pin** — periodic bump of the `vivo75/brush` pin to upstream `main`; follow `brush-pin.md` checklist, drop cherry-picks that landed. [G]
7. **`FEATURES=compress-build-logs`** — compress the captured build log after a build. Hook in `rust/portuale/src/emerge_build.rs` log-capture path. [B]
8. **elog `mail` / `mail_summary`** — add the two mail elog modules alongside syslog/custom in the elog code. [B]
9. **`env.d` eroot path** — `portage-profile` reads `env.d` relative to `config_root`; real uses `eroot`. Only differs on a split config. [C]
10. **`package.env`/`env.d` `${VAR}` expansion** — no per-file variable-expansion map for `package.env` / `env.d` values in `portage-profile` config resolution. [C]
11. **`depend`-phase RESTRICT/PROPERTIES reduction** — `PORTAGE_RESTRICT`/`PROPERTIES` are reduced on the empty-USE set for the `depend` phase in `ebuild_phases.rs`. [D]
12. **Standalone `package.env`** — `ebuild <file> <phase>` doesn't apply per-package `package.env` (needs a resolved graph entry to atom-match). `ebuild_phases.rs`. [D]
13. **Resumed binary from `$PKGDIR`** — a resumed binary merge entry should always resolve from local `$PKGDIR`; currently may re-hit the binhost. Resume path in `emerge_getbinpkg.rs`. [B]
14. **`RESTRICT=primaryuri` / fetch order** — fetch tries mirrors in a deterministic order; real honours `primaryuri` / mirror selection. `portage-fetch` module doc has the cut. [E]
15. **`:=` literal-bound atom validation** — real masks a bound `:=` atom written in an ebuild ("improper context for slot-operator built atom syntax"); `portage-dep` accepts it. Add the validation. [A]
16. **slot-conflict `--color y` marker alignment** — operator/USE-token colorization is cut because it reproduces an upstream `highlight_violations` marker-drift bug; decide + implement in the slot-conflict renderer (`lib.rs`). [A]

## Tier 2 — larger / needs design first

17. **cluster I merge-order timing** — L0 order-parity is at 22 findings; correct package set, slightly-off sequence. Needs per-node drain tracing across ~400 nodes in `merge_order.rs`. See `TEST/findings/l0.md` "## I" + memory `cluster-i-merge-order.md`. [A]
18. **`_FrontierDigraph` perf layer** — port real `_emerge/_serialize_frontier.py` (pure perf optimization over `merge_order.rs`; no behaviour change). [A]
19. **DFS-partial merge-list truncation** — reproduce real's short merge list when a resolve is abandoned. In progress on `backlog/019-DFS-partial` per `docs/019_DFS-partial.plan.md`: Slice 1 oracle (`docs/abort-path-spec.md`), Slice 2 outcome model + `PORTUALE_ABORT_PATH` gate, Slice 3 membership/order (`abort_outcome`, exit 1 on abort). Oracle verdict: no DFS walk needed — the "partial" list is `_serialize_tasks`' stuck remainder (already `cycle_display`) or nothing at all. Remaining: Slice 4 rendering (partial list + counters + `--json`), Slice 5 error-block wiring + docs closure, Slice 6 L0. Real-tree plasma-meta/podman residue is a `||`-selection gap (spec §4d), not this task. [A]
20. **Masked-dependency disclosure on backtracking** — `gcr[gtk]` / `xwayland[libei]` "All ebuilds masked" for a *dependency* atom. Same DFS-walk gap as #19. `TEST/findings/l0.md` cluster A/R. Separate, narrower USE-dep-shape residue enshrined by the `unsatuseor`/`unsatuseinstconsumer` `--autounmask-use=n` contract pins (task 22): a `[use]`-dep dependency atom that genuinely can't be satisfied (no autounmask flip available) should print real's "there are no ebuilds built with USE flags to satisfy …" block and exit 1; portuale prints the bare `!!! no visible ebuild for dependency` line and exits 0. [A]
21. **Elementary-cycle enumeration + cycle `--tree`** — `large_cycle_count` and the cycle-only `--tree` re-display need real's richer multi-priority digraph, a different representation than portuale keeps. Circular-dep code in `portage-repo`. [A]
22. **DONE 2026-09-11 — `dep_zapdeps` finer choice bins.** `all_available`/`all_use_satisfied` split, `unsat_use_*` bins with the bug-515584 unmask gate, `all_installed_slots`, in-bin upgrade-preference ordering (slice 2, `use_reduce_flat_disjunctive`'s `tie_break` + `portage-repo::promote_tied_alternative`), and the `other_*` bins + `allow_masked` two-pass return (slice 4, `portage-repo::atom_matches_installed`) all shipped, both languages, one shared probe + tie-break pair. Formally cut, documented in `disjunction_preference`'s own doc comment (slice 5): `conflict_downgrade`/`installed_downgrade` (needs `downgrade_probe` + a live mutating `graph_db` neither language has — moved to #23, which already owns this backtrack-loop territory) and `circular_atom` (needs a `circular_dependency` dict from an earlier real pass, plus `--onlydeps`, neither threaded to this call depth); `minimize_slots` (needs `_overlap_dnf` DNF detection, not done) and `want_update` (needs the `--update`/`--newuse` "wanted" set, pinned `false`) stay cut per slice 2. Real `lib/portage/dep/dep_check.py::dep_zapdeps`. Code: `portage-repo` `disjunction_preference` + `promote_tied_alternative` + `portage-use-reduce` `resolve_disjunctions`; `solver_bridge.rs` is #34, not #22. Design brief: `docs/022-agent-task-22-zapdeps.fable.md`. Detail: `what-this-proves.md`'s 2026-09-11 `||`-resolution paragraphs. [A]
23. **`_slot_conflict_backtrack` mask-target analysis** — richer choice of which version to mask when reconciling a slot conflict (real `depgraph.py`). Portuale's `'backtrack` loop. Also inherits #22's cut `conflict_downgrade`/`installed_downgrade` guards (`dep_check.py` soft 476-521, bug 531656) if a live, mutating slot-conflict `graph_db` + `downgrade_probe` are ever modeled. [A]
24. **Slot-operator rebuild undo path** — add backtracking for a rebuild that shifts another sub-slot, re-bind the rebuilt consumer's own `:=` deps, and the `--changed-slot` interaction (one missing piece, not three). `slot_operator_rebuild_entries`; citations in `history/scope-backlog-2026-09-05.md`. [A]
25. **`_complete_graph` installed nomerge nodes** — real carries every `@world`/`@system`-reachable installed package as a graph node; portuale keeps only their reverse-dep atoms. Only matters for a position-dependent divergence. [A]
26. **`--dynamic-deps=n` walk source** — the `dynamic_deps_picks_ebuild_vs_vdb_deps` unit test disagrees with the CLI path; both languages currently walk ebuild deps. Unexplained — needs owner investigation. `portage-repo`. [A]
27. **`need_rebuild` slot-conflict trailer** — renderer code is landed but dormant; build a fixture that triggers it (blocked on the vdb reverse-dep walk). [A]
28. **mrg-director: wire the scaffolded slots** — 6/8 slots (`PackagesDb`, `RepoCache`, `Fetcher`, `MergeEngine`, `BinpkgIndex`, `NewsSet`) are conformance markers, not on any real path. The decomposition they scaffold hasn't started. `rust/mrg-director/src/lib.rs`. [H]

## Tier 3 — container test bed (L2–L5, not started)

29. **L2 — portuale as builder** — `emerge -b` the L1 set from source, `.gpkg.tar` structural checks (`gpkg-structure.sh`), cross-install portuale↔portage archives. `TEST/`; plan in `history/real-world-testing.md` §5. [I]
30. **L3 — source-build parity** — both PMs build `@system` / a desktop `@world` from source with `SOURCE_DATE_EPOCH` + `-j1`; diff VDB metadata + CONTENTS structure. `TEST/`. [I]
31. **L4 — `mrg` remote merge over SSH** — differential test of the remote binary-merge path; design in `remote-merge.md` §6. [I]
32. **L5 — lifecycle & fault injection** — `-C` / `--depclean` diffs, soname bump → preserved-libs, `CONFIG_PROTECT`, `--resume` after SIGKILL, disk-full / corrupt-archive / binhost-500. `TEST/`. [I]

## Tier 4 — `--solver=` real-tree correctness (pubgrub / resolvo)

33. **`--solver=resolvo` cycle linearization** — `install_order` can't order any closure with a toolchain cycle (glibc↔gcc↔perl); `solver_bridge.rs` prints raw ids. Non-functional on real targets. `solver-backends-analysis.md`. [J]
34. **`--solver=pubgrub` over-merge** — feeds pubgrub the over-approximated `flag?()` reachability closure as the real graph (`nodejs`: 48 vs 8). Feed the resolved graph instead. `solver_bridge.rs`. [J]

---

## Deliberate cuts — do NOT pick these (documented non-goals)

- Backtrack **timing** report line `Dependency resolution took X s (backtrack: N/M)` — non-deterministic, portuale is a deterministic tool. [A]
- `soname` slot-conflict reason key — unreachable: `portage-dep` can't parse soname atoms, dep flattening drops them. [A]
- `--autounmask-write` / any config-writing autounmask mode — "never writes `/etc/portage`". [Part 3]
- PyO3 / in-process FFI; EAPI 0/1/2/3/4/6; `bsd_chflags`; RPM; `emerge --sync`; GLSA/`@security`; `equery`/`portageq`; `clap` for the emerge/ebuild parsers; `os.listdir()`-order directory merge. [Part 3]
- `--metadata` (portuale reads `md5-cache` directly); `xpak` for `mrg`. [Part 3]
