# Readability refactor (#336): decisions taken while the owner was away

Grant (owner, 2026-10-10): "commit locally, no push"; at checkpoints "decide, log,
continue"; Phase 6 (giant-file / giant-function split) waits for the owner.
Every entry can be overridden by a follow-up commit. Newest last.

## 2026-10-10, after the plan review (`2026-10-10-readability-refactor.review.md`)

1. **The plan is rewritten as v2** to absorb the review: D1-D12, G1-G11, P1-P7.
   The review stays next to it as the record.
2. **`helpers/` stays where it is, untracked** (G1). It is gitignored local
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
    file): not touched. Read as guidance anyway. The owner may want to rename
    it to `SKILL.md`.
