# Readability refactor (#336): decisions taken while the owner was away

Grant (owner, 2026-10-10): "commit locally, no push"; at checkpoints "decide, log,
continue"; Phase 6 (giant-file / giant-function split) waits for the owner.
Every entry can be overridden by a follow-up commit. Newest last.

## 2026-10-10, after the plan review (`2026-10-10-readability-refactor.review.md`)

1. **The plan is rewritten as v2** to absorb the review: D1-D12, G1-G11, P1-P7.
   The review stays next to it as the record.
2. **`helpers/` stays where it is, untracked** (G1). *Owner review 2026-10-10: right, `helpers/` is temporary and will be removed; anything agents need moves to `LLM/`. Done: see item 28.* It is gitignored local
   scratch (`.gitignore:24`), not part of the repo, so "move LLM-only files to
   `LLM/`" does not apply to it. Moving it into tracked `LLM/` would start
   publishing private notes. `oc.sh`/`scoperun.sh` paths and memories stay valid.
3. **No parallel worktrees** (G2). Every phase runs serially in the main checkout:
   the fixtures symlink, the pmtest `../portuale` binding and fixture pollution
   make parallel gates unsafe. Helpers get file-scoped edit jobs; I build and
   run the gate.
4. **"real" in comments is handled in Phase 7, not Phase 3** (P2). Phase 3 renames
   identifiers, portuale's own user-visible strings, live entry-point docs,
   `.claude/agents` and `AGENTS.md`. Code comments are rewritten once, in
   Phase 7, under the rule "kept comments and code-notes say Portage".
5. **History docs are not rewritten** (G5, AGENTS.md rule 7): `what-this-proves.md`,
   backlog files, plans, evidence and lessons/recap. They get a one-line header
   pointing at `LLM/renames.tsv` (old path/identifier → new), which every phase
   appends to. Live entry points are rewritten: README, AGENTS.md,
   agent-context, `.claude/agents`, `bin/README.md`, `3rdparty/*`, human `docs/`.
6. **FROZEN names** (D8) are never renamed:
   - `PortagePackage::Real` (external crate)
   - `real_quick_ratio` (Python difflib API)
   - the env value `PORTUALE_PYTHON_HELPERS=real` (read by the bed and `bin/`)
   - vendored Portage text in `bin/` and upstream `man/` pages (D10)
   - pmtest fixtures and corpus keys
7. **The `real-portage-oracle` agent is renamed `portage-oracle`** (G8). It is
   Portage-sense "real" in a live entry point. Its references in
   `.claude/agents/README.md` and AGENTS.md follow.
8. **Phase 5 lint policy** (P1, G9):
   - `[workspace.lints.clippy]` sets `too_many_arguments` and
     `fn_params_excessive_bools` to warn.
   - `clippy.toml` sets `too-many-arguments-threshold = 5` and
     `max-fn-params-bools = 2`.
   - The existing `#[allow]`s stay, and each commit removes the allows it fixes,
     so every commit's clippy is clean.
9. **The rust-skill guidance is adopted selectively** (G8). The adopt/reject table
   is in plan v2 §"Rules adopted". The main rejections:
   - `thiserror`/`anyhow` would be new dependencies (constraint: none without
     the owner).
   - "comprehensive rustdoc everywhere" contradicts the owner's "shorten
     comments"; rustdoc stays on public items only, concise.
10. **Backlog number #336** (P3) is filed for this work. Residues:
    - #337: pmtest's own real→Portage pass.
    - #338: dupes groups skipped in Phase 4.
    - #339: giant-function split beyond what Phase 6 covers.
11. **`docs/agent-context.md` moves to `LLM/`** (G8). Phase 8 writes a short human
    `docs/architecture.md` (goals, crate map, data flow) so `docs/` still has an
    architecture entry point.
12. **`docs/Paragone_solver_portage.md`** (P6): an Italian LLM chat transcript, so
    it is an LLM artefact under the plan's rule and moves to `LLM/`.
13. **`.claude/skills/rust-refactor-pro/SKIL.md`** (typo; the owner's untracked
    file): not touched. *Owner review 2026-10-10: the owner renamed and committed it (`037bb3f5`).* Read as guidance anyway. The owner may want to rename
    it to `SKILL.md`.

## 2026-10-10, Phase 0/1 findings

14. **Hidden test-order dependency (pre-existing).** About 300 unit tests in
    `crates/portuale` exec `target/release/portuale`, which `cargo test` does
    not build. The Phase 0 baseline passed only because the old `rust/target`
    still held a binary from earlier builds. The first run in the fresh root
    `target/` failed 298 of them until the binary existed. `LLM/tools/gate.sh`
    now runs `cargo build --release` before `cargo test`. Not a Phase 1
    regression: with the binary present the result is 2477/2478, the same as
    the baseline. Worth a real fix later: have those tests build or locate
    the binary themselves.
15. **Load-sensitive tests.** Both pass alone 3/3 and fail only under the
    full-workspace parallel run.
    - `pretend::tests::ask_config_select_sigint_…` failed in Phase 0.
    - `emerge_build::tests::a_hard_failure_kills_still_running_builds_…`
      failed in Phase 1: run_source_merge took 121 s, so the slow sibling's
      `sleep` was not killed under load.

    The second may be a real kill race, not just slowness. Filed as residue
    #340.
16. **Phase 3 inventory.** The subagent classification is in
    `2026-10-10-real-inventory.tsv`: 270 identifiers, of which 139 get
    renamed. I settled the doubtful rows:
    - `PORTUALE_REAL_PORTAGEQ` → `PORTUALE_COMPARE_PORTAGEQ`;
    - `exec_real_chmod_lite` kept (real-mode family of the frozen
      `PORTUALE_PYTHON_HELPERS=real` value);
    - `getlibpaths_…_real_defaults` and `sandbox_…_real_addrconfig_…` kept as
      ambiguous;
    - `graph_real*` / `resolve_real*` fixture helpers kept, because they mean
      "the fixture's actual config" (contrasted with `*_empty`).
17. **Gate basetemp.** A reused `--basetemp` breaks the next contract run.
    A privileged merge test leaves a root-owned directory in the basetemp,
    and pytest's cleanup at the start of the next run fails with
    `PermissionError`, which turned all 2285 tests into setup errors. The
    product is not involved. `gate.sh` now uses a fresh `$OUT/pytest` per
    run. Old gate dirs need `sudo rm -rf`.

## 2026-10-10, Phase 4 scope

18. **`cargo dupes` must run with `-p crates`.** From the repo root it also
    scans the vendored `3rdparty/` checkouts (rsync, coreutils, …), which
    inflated the report to 39 exact groups. `LLM/tools/dupes.py` wraps it.
    Over `crates/` the function-level picture is unchanged from Phase 0: 5
    exact and 6 near groups. Production code alone has 0 exact groups and
    1 near pair (`portage-fetch::varexpand` / `portage-profile::substitute`,
    0.974).
19. **Sub-function groups are not folded** (#338). cargo-dupes reports a
    sub-function member's *enclosing* function span: a group's members
    all read `pretend.rs:10081-16150`, which is all of `pretend::run`. So
    the duplicated blocks can be neither located nor ranked. They are
    small normalised AST shapes (guards, early returns, match arms) whose
    folding would add indirection, not remove knowledge. Large duplicated
    blocks surface as function-level groups anyway once Phase 6 splits
    the giant functions. Re-run after Phase 6.
20. **The `graph*` / `graph_result*` test-helper groups move to Phase 5.**
    Four of the eleven function groups exist only because
    `resolve_pretend_graph` takes ~44 positional arguments and each helper
    re-spells them all with one bool flipped. A `ResolveRequest` struct
    already exists, and its doc comment defers the call-site migration
    (85 calls, all in `portage-repo/src/lib.rs`). Migrating those calls to
    named-field `ResolveRequest` construction is Phase 5's explicit-
    arguments work, and it collapses the helper groups as a side effect.
    Phase 4 folds the rest: varexpand, `assert_same_reads`
    (sqlite/redb), the two overlay package.use tests, the `ask_*` pty
    test pairs, the changed_slot pair, and the two lib.rs closures.

## 2026-10-10, Phase 5 progress

21. **Lint limits: more than 5 arguments or more than 2 bool parameters**
    (Task 5.1, `clippy.toml`). That flags 286 functions, each tagged
    `#[allow(<lint>, reason = "#336 Phase 5 worklist")]`. The tag is
    greppable, and clippy stays clean while the worklist shrinks.
22. **How a positional-argument function is fixed** (5a, 5b): group the
    options into a named struct with a `Default` (or a `new`) that means
    "emerge with no option given". Callers use struct-update syntax and
    list only what they change. The function body destructures the
    struct into the old local names, so it is not edited. Production
    callers that copy a context get one mapping method, for example
    `ResolveCtx::select_options`. Calls are rewritten by scripts that map
    arguments positionally (`LLM/tools/migrate_*_calls.py`), never by
    hand.
23. **Done so far:**
    - `resolve_pretend_graph` (44 args) is deleted; its 55 calls build
      `ResolveRequest` with named fields.
    - `resolve_pretend` (29 args) takes `SelectSource` + `SelectOptions`.

    284 worklist functions remain. Biggest: `enqueue_dependencies` (41),
    `print_entry_line` (34), `print_tree` (24),
    `already_installed_or_reinstall` (23), `euf_flat` (23),
    `run_phase_from_saved_env` (22), `run_unmerge_pretend` (22).
24. **Phase 6 waits for the owner.** The split map is
    `2026-10-10-split-map.md`, with six open questions at its end.
25. **Phase 7 starts with a pilot**, `crates/portage-dep/src/lib.rs`. The
    owner reviews it as the style template before the comment pass runs
    over 229k lines. `LLM/tools/strip_comments_eq.py` proves each such
    commit is comment-only.
26. **`enqueue_dependencies` (23 args) is fixed the same way.**
    - It takes `&SelectSource` + `&DepOwner` + `&EnqueueOptions` (built by
      `ResolveCtx::enqueue_options`) + `EnqueueSinks` (the six `&mut`
      pass-state sinks).
    - `owner_key`/`owner_version` were always `(category, package)` and
      `version` again, so they are derived inside.
    - The parameter comments became field docs.
27. **The display functions in `pretend.rs` wait for Phase 6.**
    `print_entry_line` (24), `print_tree` (24) and the `run_*_pretend`
    family share ~18 per-run display settings. Four of the six
    `print_entry_line` calls sit in different branches of the 6,000-line
    `pretend::run`. A clean `DisplayCtx` belongs to the `run` split
    (`split-map.md`, the `Session` / `EmergeOpts` / `ResolvePlan`
    contexts). Doing it now would add four repeated 18-field literals
    that Phase 6 would rewrite.

## 2026-10-10, owner review of items 1-27

The owner approved items 1-27 and commented on two of them: item 2
(helpers/) and item 13 (SKIL.md).

28. **What agents need from `helpers/` now lives under git.** Moved:
    - the scope tooling (`oc*.sh`, `scope*.sh`) → `LLM/tools/scope/`,
      with `no-pkill.md` as its README;
    - `distfile-mirror.md`, `lu_zero.md` (→ `related-rust-portage-projects.md`)
      and `cleanroom.md` (→ `cleanroom-prompts.md`, the prompts behind
      `docs/cleanroom/`) → `LLM/reference/`;
    - `IDEE.md` → `LLM/ideas.md`;
    - `portuale_remote.md` → `LLM/evidence/2026-09-25-remote-vdb-location.md`;
    - the portage-cli probe plan and the readability guide (now
      `2026-10-10-readability-strategy.it.md`) → `LLM/plans/`;
    - the `devmanual/` checkout → `3rdparty/devmanual`, pinned in
      `repos.toml` like the other reference checkouts.

    Left in `helpers/`, all obsolete or kept elsewhere:
    - `oc.sh.orig-dir-flag` (pre-2026-10-02 `oc.sh`);
    - `migrate-to-new-server.sh` (done 2026-09-27);
    - `vdb_to_db.md` (the current version is `docs/vdb_to_db.md`) and
      `vdb_to_db.commit_message` (in git history);
    - `real_emerge/` (2026-09-25 captures from a `rust/target` binary);
    - `gentoo/` (2.4 GB, the old test bed; its content now lives in
      pmtest's `differential-test-bed/`).

    `agent-context.md` cited a `helpers/emerge_-1v_--debug_--getbinpkgonly__sys-fs--fuse.log`
    that no longer exists. Its pointer is dropped.
