# Implicit state machines in portuale

Six lifecycles that the code implements without naming them as state machines:
no `enum State`, the state is carried by constants, stamp files, queue
contents or a saved list. Found with a read-only scan on 2026-11-26 at commit
`2055f4dc`, one trace per machine, each claim with a `file:line`. Diagrams are
in [`archify/`](archify/) (`state-*.html`, candidate JSON in `archify/src/`).

**How far to trust this.** Every report was spot-checked against the source
for its key claims, not line by line. Items marked *unverified* were listed as
such by the tracing agent and have not been read since. Names of states are
groupings made for the diagrams, not types in the code, except where noted.

| # | Machine | Diagram | Where |
|---|---|---|---|
| 1 | Ebuild phase lifecycle | [state-phases.html](archify/state-phases.html) | `crates/portuale/src/ebuild_phases/`, `bin/phase-functions.sh` |
| 2 | Backtracking search | [state-backtracking.html](archify/state-backtracking.html) | `crates/portage-repo/src/resolve/driver.rs`, `backtracker.rs` |
| 3 | Resolver pass | [state-resolver-pass.html](archify/state-resolver-pass.html) | `crates/portage-repo/src/resolve/pass/` |
| 4 | Merge-list entry and resume | [state-resume.html](archify/state-resume.html) | `mtimedb.rs`, `pretend/resume.rs`, `emerge_build/source_merge.rs` |
| 5 | Phase process cancellation | [state-process-cancel.html](archify/state-process-cancel.html) | `ebuild_phases/scheduler_registry.rs`, `signal_exit.rs` |
| 6 | Merge path of one package | [state-merge-path.html](archify/state-merge-path.html) | `ebuild_merge/after_install.rs`, `tree.rs`, `portage-vdb` |

Not machines: `Kind`, `Mode`, `Stage`, `SolverKind` and similar enums are
fixed classifications. `EBUILD_PHASES` in `elog.rs` only orders log messages;
the real chain is `CHAIN` in `ebuild_phases/mod.rs:125`.

## 1. Ebuild phase lifecycle

Chain: `pretend, setup, unpack, prepare, configure, compile, test, install`
(`ebuild_phases/mod.rs:125-134`); asking for a phase runs the ones before it.
Per phase, `commands.rs:256-274` skips in order: scheduler already ran
`pretend`; no-op source phase (`SKIPPABLE_SRC_PHASES`); stamp file present.
State lives on disk: bash writes `.<phase>ed` after the phase body
(`bin/phase-functions.sh`), Rust only reads (`stamp_skip.rs:42`). Under
`noauto`, `install` ignores its stamp (`stamp_skip.rs:43`,
`phase-functions.sh:665`). A non-zero status ends the chain
(`commands.rs:286`); after `install` come QA, metadata and soname deps
(`commands.rs:297`). `package` is the install chain plus a packaging tail;
`qmerge` requires `.installed`; `config`, `prerm`, `postinst` and friends run
standalone and consult no stamp.

Unverified: which scheduler call sets `skip_noop_src_phases` on the
source-merge path; readers of `.instprepped`, `.packaged`, `.ebuild_changed`;
that a failed body writes no stamp (inferred from write order); `depend`.

## 2. Backtracking search

Driver `backtrack_search` (`driver.rs:48`), loop `driver.rs:68-312`, node
store `Backtracker` (`backtracker.rs:21`). Each iteration pops a node, runs a
pass, then the decision is Settle, DeadEnd or Feedback (`driver.rs:224`).
Feedback kinds (`backtrack_params.rs:306`): `Config`, `RevDep`, `Circular` add
no mask step; `SlotConflict` and `MissingDep` add one and give non-terminal
children. `add_node` (`backtracker.rs:110`) drops a child on, in order: runtime
mask validity, `mask_steps > max_depth`, params equal to an existing node.

Two caps: mask steps, `max(1, ceil(backtrack_max/2))` (`driver.rs:53`), and
restarts, `restarts >= backtrack_max` (default 10). When the stack empties or
the restart cap hits, the best-run fallback re-runs the deepest terminal node
(`driver.rs:313`); the root is terminal, so a result always exists.
`--backtrack=0` settles the root pass, except `RevDep`, which re-runs as a
feed pass without counting a restart (`driver.rs:260-268`).

Observations: `params_equal` compares `circular_dependency` twice
(`backtracker.rs` around lines 42 and 54), harmless but redundant. Unverified:
that the `Settle`-with-conflict arm (`driver.rs:231-241`, increments restarts
with no cap check) always drains; that the `RevDep` feed loop under
`--backtrack=0` is bounded; the bodies of `collect_feedback`, `run_pass`,
`slot_conflict_handler_stops`.

## 3. Resolver pass

`run_pass` (`pass/mod.rs`): seed (`seed_queue.rs`), then a loop of
`pop_queue_item` + `step_queue_item`, then a disjunction phase when `queue` and
`held_blocks` are empty (`mod.rs:138-150`), then finish: `finish_required_by`,
`finish_slot_conflicts`, `collect_pass_blockers`, `assemble_pass_result`.
No state enum; the phase is which of `queue`, `held_blocks`,
`disjunctive_stack` is non-empty, plus `seeds_left` / `seed_split_at`
(`pass_state.rs:213-218`) which keep an earlier seed's subtree ahead of later
seeds. Per item: popped, prepared (dedup via `visited_atoms`), outcome resolved
(reuse, pin, or `resolve_pretend`), fallbacks rewrite a miss into
`AlreadyInstalled`/`Reinstall` (`outcome_fallbacks.rs`), then either an
unresolved/installed entry (no version) or candidate pool, existing-slot check
and a new graph node, whose deps are enqueued. A pass fails with `Err` only on
a top-level unsatisfied report or a REQUIRED_USE violation
(`assemble_pass.rs:15`); everything else is data in `PassResult`.

Caveat: `ControlFlow::Break` means both "dropped on purpose" and "fully
handled", so terminal-state names are inferred per call site. Unverified:
disjunction choice logic, parts of `top_level_unsatisfied.rs`, the tail of
`existing_slot.rs`, dep filtering in `enqueue_deps.rs`.

## 4. Merge-list entry and resume

State is derived, not stored: membership in `mtimedb["resume"]["mergelist"]`
plus `CONTENTS` on disk. The list is saved up front (`merge_run.rs:93-110`);
each success removes its own entry (`source_merge.rs:250-257`). A postinst
failure keeps the entry in the list (`source_merge.rs:259`). Without
`--keep-going` the loop stops at the first failure; with it, the failed cp's
transitive dependents are dropped (`source_merge.rs:276-288`). The final save
keeps every entry not merged (`resume.rs:64-78`). `--resume` always runs with
`keep_going: false` (`resume.rs:615`). Rotation: a fresh merge moves `resume`
to `resume_backup` only if it has more than one entry (`mtimedb.rs:903-917`).

Worth a look (unconfirmed as bugs): `read_resume_list` can promote the backup
and persist it, so a read writes (`mtimedb.rs:929-934`); an empty up-front save
writes a list that does not parse, so a backup behind it is promoted on the
next `--resume`; after a failing keep-going run the final save recomputes from
disk and re-includes failed and dropped entries, while `resume.rs:59-62`
documents only the postinst exception; a postinst-failed entry is replayed and
nobody traced whether the replay notices it already landed. Unverified: the
binary merge plan, the middle of the `-j` dispatch loop, locking around
`mtimedb`.

## 5. Phase process cancellation

Two independent kill paths. Registry (`scheduler_registry.rs`): per scheduler
call, thread-local, fires on a non-`--keep-going` failure and signals only
children of threads holding a guard; `terminated` is sticky and kills late
spawners (`:94-115`, `:150-161`). Signal watcher (`signal_exit.rs`):
process-wide `LIVE` map, fed unconditionally by `spawn_trackable`, fires on
SIGINT/SIGTERM, SIGTERMs every live child every 100 ms for up to 60 s, then
exits `128+sig` (`:37`, `:107-125`). `PhasePlacement`: `config`, `preinst`,
`prerm`, `postinst`, `postrm` stay in portuale's process group (signalled by
pid); every other phase gets its own session (`-pgid`); a demoted phase using
the pid namespace is forced to the parent group (`commands.rs:1241`).

Worth a look: neither path sends SIGKILL, so a phase that ignores SIGTERM holds
the failure path open (the signal path gives up after 60 s); `terminated` is
never set by ^C; pid reuse between the snapshot and the kill was not evaluated.
Unverified: the serial (`jobs <= 1`) path; what the caller does after
`Landed::Stop`; a race between the watcher's `exit` and the guard's drop.

## 6. Merge path of one package

Order on the `files` backend (`after_install.rs`): `instprep`, SLOT, install
mask, CRLF scan, collision and blocker checks; create `-MERGING-<pf>` (`:174`);
`pkg_preinst`; build-info, COUNTER tick under `VdbLock`, metadata; write the
image to ROOT and build CONTENTS (`:202`); select and inject preserved libs;
write config memory and seal CONTENTS; unmerge each same-slot old instance
(prerm, delete files, postrm, remove entry, `:264`); rename the new entry live
(`:280`); `pkg_postinst`; `env_update`, preserved-libs prune; `clean` phase.
On DB backends old rows are deleted in the same commit as the publish.

Invariants (the agent's reading): the merge is not transactional and no error
path removes the pending entry or ROOT files (only a manual `vdb sweep`); a
postinst failure keeps the merge and skips clean and the world-file update; a
destination file is never opened for write, a temp sibling is renamed over it;
the new image is written before the old instance is unmerged, with ownership
read from the pending CONTENTS; hook failures in the replace loop are not
fatal, unlike standalone `unmerge`. Unverified: recovery after a failed old
unmerge or failed publish on `files`; `merge_binpkg` running `clean`; world-file
order relative to publish.

### Documentation versus code

`AGENTS.md` asks for each inconsistency to be listed. Item 1 was confirmed by
reading both sides; the others come from the tracing agent and are unchecked.

1. `LLM/skills/portuale-ebuild-merge/SKILL.md:40-42` says the merge holds an
   exclusive VDB lock for its whole duration. Code: `VdbLock` is taken only in
   `next_counter` (`crates/portage-vdb/src/files_write.rs:465`) and documented
   as scoped to one counter tick.
2. SKILL.md:48-49 says category-dir and VDB-root mtimes are bumped around
   modifications; no such bump was found in the merge path.
3. SKILL.md:45-47 says to restore directory mtimes before the stamp; that
   exists only in the converter's `insert_entry`, while `seal_entry` appends
   `#dir_mtime=` afterwards.
4. SKILL.md:43-44 says delete the old entry, then rename the new one in; true
   on `files` (leaving a window with no entry), not on DB backends.
5. The merge-path gate's file list (`SKILL.md:21-24`, `LLM/agent-context.md`
   around 396-402, `pair_check.sh:24-27`) omits `files_write.rs`,
   `merge_engines.rs` and `env_update.rs`, which define the VDB write order.
6. `ebuild_merge/mod.rs:12-13` lists privilege dropping as a known gap, while
   SKILL.md:107-118 documents demoted-phase namespaces as implemented.
7. `ebuild_unmerge.rs:899-902` says the env-var read ignores other FEATURES
   tokens; `unmerge_orphans_from_env` (`:65-76`) now handles them.
8. `mrg-director/src/lib.rs:261-265` documents `MergeOutcome::Merged` as "vdb
   entry written", but a postinst failure lands the entry and maps to `Failed`.
9. `mrg-director/src/lib.rs:277-278` says the engine serializes the vdb-write
   half; its engines only return markers, serialization is in `emerge_build`.
10. `MergeUnit::replaces_same_slot` (`mrg-director/src/lib.rs:196`) is
    documented but always `None` and never read.
11. SKILL.md:73-76 says one shared CONTENTS parser lives in `portage-vdb`;
    `ebuild_unmerge.rs:36` still has its own `parse_contents`.
12. `ebuild_merge/mod.rs:2-3` gives the order as preinst, copy, vdb entry,
    postinst; the pending entry is created before preinst and published last.

Not checked in any trace: the sqlite and redb write paths, `needed_elf`,
`env_update`, and the glibc/bash gate, which is a pmtest bed run that nothing in
these crates enforces.
