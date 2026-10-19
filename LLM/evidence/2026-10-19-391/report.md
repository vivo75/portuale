# #391 report: package moves must not rewrite ebuild dependencies (2026-10-19)

Branch (both repos): `batch/2026-10-18`. No commits, no pushes, no bless.
portuale HEAD `9a909749`, pmtest HEAD `07ef582` + one shared-tree caveat below.

## Oracle verdict: DIVERGED pre-fix, AGREE post-fix (verify-first passed step 1, fix applied)

Staged fixture config (`../pmtest/fixtures` via
`l0-fixture-oracle/stage.sh` adapted for the host, fresh stage per cell;
host Portage 3.0.82.2; full argv/stdout/stderr/rc in
`portuale/LLM/evidence/2026-10-19-391/`):

| cell (moves on = default) | Portage 3.0.82.2 | portuale before | portuale after |
|---|---|---|---|
| `-pv --deep dev-libs/movedepconsumer` | rc 1, `no ebuilds to satisfy "dev-libs/oldmovepkg"`, backtrack 1/20 | rc 0, `N newpkg` + `N movedepconsumer` | rc 1, same error text (modulo the ` for <root>` suffix and backtrack rendering — pre-existing #395/determinism cuts) |
| `--pretend dev-libs/movedepconsumer` | rc 1, same error | rc 0, `N movedepconsumer` | rc 1, same error |
| moves-off controls (`--package-moves=n`, both argv) | rc 0, `N movedepconsumer` | rc 0, `N movedepconsumer` | unchanged |

Portage grounding (pinned checkout `3rdparty/portage`):
`_global_updates.py:59-280` rewrites the world file, the vardb
(`move_ent`/`move_slot_ent` + `update_ents`, `:626-683`, `:383-445`) and
the bintree, plus config files — never `portdb`. Installed-package aux
dicts get the same treatment at depgraph load
(`FakeVartree.py:191,336 perform_global_updates`). Ebuild `*DEPEND`
stays verbatim, so the dep has no ebuild and the installed entry was
renamed away → rc 1.
Mechanism of the old portuale rc 0: `read_md5_cache` rewrote
`RDEPEND=dev-libs/oldmovepkg` → `newmovepkg`; the dep then matched the
installed `oldmovepkg-1.0` dir (cp-sources fallback) with no row, while
`--deep` walked the live ebuild's own `RDEPEND=dev-libs/newpkg` → the
spurious `N newpkg` row.

## Files changed (portuale repo; pmtest repo below)

- `crates/portage-repo/src/metadata_cache.rs` — removed the on-read
  `*DEPEND` rewrite from `read_md5_cache` (ebuild metadata now verbatim);
  doc comment updated. Single ebuild rewrite site: every ebuild read
  (`portuale`, `mrg-director`, resolver) flows through
  `repo_aux_metadata` → `read_md5_cache`; the depcachedir/`depend`-phase
  rung never rewrote.
- `crates/portage-repo/src/updates.rs` — `installed_cp_sources` (+
  `_with` seam) now drops the queried name itself when a `move` chain
  renames it away (new shared core `installed_cp_sources_core`), i.e.
  the read-time equivalent of `move_ent`'s directory rename. Queries for
  the new name still scan the stale dir; slotmove-only names unchanged;
  moves-off (empty chain map) unchanged by construction.
- `crates/portage-repo/src/tests/updates.rs` — 3 new unit tests (single
  move, chain `a→b→c`, slotmove-only); the old cycle pin's second half
  now asserts `[]` (sequential `update_dbentries`/`move_ent` renames the
  second command's old name away; source-grounded, no fixture).
- `LLM/code-notes/portage-repo/{updates,metadata_cache,installed}.md` +
  index regen (rc 0). `fact_coverage.py HEAD` on both source files: 0 lost.
- pmtest `test_emerge_pretend_contract.py`: new `test_391_*` (3 argv),
  2 CASES rows appended at END; 2 moves-on rows removed from the harvest
  parametrize with explicit `ids=` preserving harvest ordinals for the 8
  survivors (corpus keys embed `[argsN]`; renumbering orphans them,
  #405a); harvest-test docstring narrowed to installed/binary deps.

Kept as Portage does: installed rewrites (`installed_meta.rs` vdb
snapshot arms, `installed.rs` cp/slot identity + sources fallback),
world + command-line atom rewrites (`expand_targets.rs`,
`pretend/mod.rs`, `problem_reports.rs`). Binary entries: portuale has
no read-time binary-dep rewrite and no fixture covers a moved binary dep
(`binhost`/`pkgdir`/`quickpkgroot` clean of old/newmovepkg) — unverified,
unchanged.

## Tests failing-then-passing

- Rust (`portage-repo`): `moved_away_cp_sources_drop_the_stale_pre_move_dir`
  + `chained_move_sources_land_only_on_the_final_name` FAILED pre-fix,
  pass post-fix (`slotmove_only_*` passes throughout).
- pmtest: `test_391_movedepconsumer_with_moves_on_aborts_like_portage`
  + 2 new CASES entries FAILED pre-fix (rc 0 vs 1), pass post-fix.
- Suites: `cargo fmt --check` clean; `cargo clippy --release
  --all-targets` zero warnings; `cargo test --release -p portage-repo`
  1129 passed; `-p portuale --bins` 1161 passed; `-p mrg-director` 17
  passed; pmtest `-k` selections (391/harvest/moves/updates/slotmove,
  fresh basetemps under `/var/tmp/pt-391`) green; invariants over CASES green.

## Pin changes with justification

- Removed from harvest-strict pinning (would need a bless, forbidden):
  `["-pv","--deep",movedepconsumer]` (harvest args2, was rc 0 N+N) and
  `["-p","--tree","--deep",movedepconsumer]` (args4, was rc 0 N+N) —
  Portage capture says rc 1 abort; now covered by `test_391_*` with the
  verbatim Portage stderr lines (suffix-free form per #395 fixture
  convention).
- Unchanged and re-verified green: all 8 surviving harvest rows
  (new/old/slotmovepkg × moves on/off, incl. args8
  `-pv --package-moves=n --deep` = rc 0 `N movedepconsumer`, i.e. the
  moves-off Portage capture — the "must not move" pin HELD).
- Note: task brief says that corpus pin is "rc 1 today"; the harvest
  actually holds rc 0 and HEAD already produced rc 0 before this slice
  (S0's rc 1 was the Oct-15 binary; the flip predates #391 — args8
  passing post-fix proves this slice didn't move it). Moves-off rc 0 vs
  Portage rc 0 agree; the remaining moves-off shape (plain `-p`, no
  `--update`) is #360 territory, out of scope.

## Corpus drift: YES (intended, unblessed)

9 `expanded` keys, all `dev-libs/movedepconsumer` with moves on
(`--pretend`, `-pv`/`-p` variants, `--tree/--quiet/--verbose/--json/
--emptytree/--update/--update --deep/--update --deep --newuse`):
rc 0→1, matching the Portage oracle. `test_harvested_corpus` passes with
9 `CorpusDrift` warnings; nothing else drifts. No `PORTUALE_CORPUS_BLESS`
run. The 2 orphaned `contract` entries (old args2/args4) are never
replayed (no warning, no failure); a future harvest GCs them (#405a).

## Residue

1. Moves-off `-pv --deep`/`--pretend` rc 0 relies on the #360
   installed-only fallback applying outside `--update` — matches the
   Portage moves-off capture here, but that fallback's general scope is
   #360's, not this slice's.
2. Binary-entry dep rewrites (`bindb.update_ents`): no portuale
   equivalent at read time and no fixture — needs its own cell design
   if ever pinned.
3. Shared-tree caveat: the tree also holds the parallel #381 agent's
   uncommitted WIP (`crates/portuale/src/ebuild_unmerge.rs`,
   `LLM/evidence/2026-10-19-381/`, `t381-*` pmtest tests + fixtures,
   `LLM/code-notes/portuale/ebuild_unmerge.md`) — untouched by this
   slice; the `portuale --bins` green run (1161) and workspace clippy
   green include their files.
4. Host-oracle method note: host `emerge` must run WITHOUT
   `PORTAGE_REPOSITORIES` inlined (it breaks the host running-root
   config's `gentoo:` parent); each config then reads its own
   `repos.conf`. Recorded in the evidence README.

Status: DONE (no commit per brief; docs edits limited to code-notes +
evidence + report — backlog and what-this-proves untouched).
