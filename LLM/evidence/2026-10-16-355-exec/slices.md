# S1 slices — plan `LLM/02.355-reinstall-unmerge-parity.opus.md`, slice S1 (#358)

Branch (both repos): `backlog/355-358-unmerge-parity`.
Oracle rule: expected values from Portage 3.0.82.2 captures, never from portuale output.

## What changed

pmtest (bed side, no product code):

- `differential-test-bed/layers/l1/consume.sh`
  1. Pre-merge CONTENTS snapshot: right after `installed-before.txt`, every
     `/var/db/pkg/*/*/CONTENTS` is copied to `$OUT.pre-contents/<cat>/<pf>/CONTENTS`
     (before the vdb convert/mount blocks rewrite `/var/db/pkg`).
  2. Path list: for every cpv in `installed-before` that is gone in
     `installed-after` (replaced) or listed in `merged-cpvs` (reinstalled),
     the pre-merge CONTENTS paths are emitted too. Always on (observation only).
  3. Both extractions use a new `contents_paths()` shell function following
     Portage's `_contents_re` (`obj` strips ` <md5> <mtime>` from the right,
     `sym` cuts at the last ` -> `, `dir`/`dev`/`fif` take the rest; bad lines
     skipped). This is #357's rule on the bed side; replaces both
     `awk '{print $2}'` splits.
  4. New opt-in `L1_CONSUME_INSTALL_MASK`: when set, appended to
     `/etc/portage/make.conf` as `INSTALL_MASK="${INSTALL_MASK} <value>"`
     before the merge, in both consumers.
- `differential-test-bed/run/l1-merge-from-binpkg.sh`: forwards
  `L1_CONSUME_INSTALL_MASK` into both consumers; header documents it.
- `differential-test-bed/README.md`, `USAGE.AGENTS.md`: the new variable and
  the pre-merge CONTENTS paths.
- `differential-test-bed/findings/l1.md`: appended (see §Proof).

portuale (docs only, no code):

- `LLM/agent-context.md` §"Merge-path safety gate": third invocation
  (masked reinstall) documented as required for any change to the unmerge
  half of the merge path.

`bash -n` on both scripts: OK.

## Proof runs (G4)

All from `../pmtest`, one at a time, no cargo build during any bed run.
`GENTOO_MIRRORS=http://10.48.0.229:8080` exported (nothing needed fetching;
`L1_SKIP_BUILD=1` reused the warm pkgcache). Expected values from Portage
3.0.82.2 in the same runs, never from portuale output.

1. **Masked gate at HEAD** (`a530fbdc`, binary built from the slice tree):
   `L1_SKIP_BUILD=1 L1_CONSUME_REINSTALL=1
   L1_CONSUME_INSTALL_MASK="/usr/share/doc /usr/share/info /usr/share/man"`
   → `differential-test-bed/logs/l1-20261016T215952Z`: **0 hard /
   0 unexplained** (3934 files snapshotted, merge_rc=0 both PMs). The mask
   bit: pre-merge CONTENTS lists e.g.
   `obj /usr/share/doc/glibc-2.43-r2/ChangeLog.old/ChangeLog.1.bz2`;
   post-merge both PMs deleted the whole masked trees (only the emptied
   parent dirs and install-info's regenerated `dir` index remain).
2. **Masked gate on the pre-#354 tree** (parent of `8772601e` =
   `5e4b36b5`, via `git worktree add /var/tmp/355-s1-pre354` +
   `cargo build --release -p portuale` there — 1m17s, no fallback needed —
   and a temporary `portuale-pre354` entry in `managers/managers.yaml`,
   run with `PMTEST_PM=portuale-pre354 PMTEST_NO_BUILD=1`):
   → `logs/l1-20261016T220455Z`: **75 hard findings, all
   `[MISSING] … present for portuale, absent for portage`**, every one
   under `/usr/share/doc/{glibc-2.43-r2,bash-5.3_p15}`,
   `/usr/share/info`, `/usr/share/man` (rc=1, as expected for the buggy
   tree). The cell sees exactly the stale-file bug class. Entry reverted
   and worktree removed afterwards (`git worktree list` shows only the
   main checkout; `managers.yaml` has no diff).
3. **Plain-gate smoke at HEAD** (no mask):
   `L1_SKIP_BUILD=1 L1_CONSUME_REINSTALL=1`
   → `logs/l1-20261016T220225Z`: **0 hard / 0 unexplained** (4010 paths,
   4007 files). Pre-S1 like-for-like baseline `l1-20261014T091928Z`
   (gate reinstall): 4006 files. The path list grew and the verdict did
   not change. The +1 file is real coverage: the old post-merge-only list
   never looked at `obj /usr/share/info/bash.info.bz2` /
   `sym /usr/share/info/bashref.info.bz2` (present in pre-merge CONTENTS,
   dropped by the reinstall image); both PMs deleted both and agree.

No pytest CASES (bed shell only, per plan). Full pmtest suite not re-run:
no Python files touched (`bash -n` OK on both scripts instead).

## Anything surprising / left undone

- Nothing blocking. The smoke run independently confirmed the #354 fix
  holds on identical-image reinstalls for `/usr/share/info` files dropped
  between the image build and the binpkg build (both PMs delete, 0/0).
- Left undone (per plan, not this slice): S2–S5 product fixes; the sqlite/redb
  reinstall gate lines (come after S3); `l1-porttest.txt` runs (after S4/S5).
- No commits made (coordinator reviews and commits). No allowlist entries
  added. No hard/unexplained rows at HEAD to report.

## Continuation verification (2026-10-16, second worker)

The first worker's turn ended while a background build was pending, so a
second worker re-entered to finish S1 and found all three proof runs
already on disk with the findings note already appended to
`differential-test-bed/findings/l1.md`. Per the brief ("do NOT redo what
is already done") the bed runs were **verified, not repeated**:

- `bash -n` re-run on `consume.sh` and `l1-merge-from-binpkg.sh`: OK.
- `logs/l1-20261016T215952Z/l1-report.txt`: 0 hard / 0 unexplained,
  3934 files, merge_rc=0 both PMs. Pre-merge `CONTENTS` under
  `<pm>.pre-contents/` list e.g.
  `obj /usr/share/doc/glibc-2.43-r2/ChangeLog.old/ChangeLog.1.bz2`;
  both `paths.txt` hold 70 `/usr/share/doc` entries.
- `logs/l1-20261016T220455Z/l1-report.txt`: 75 hard, all
  `[MISSING] … present for portuale, absent for portage` (69 under
  `/usr/share/doc`, 2 `/usr/share/info`, 4 `/usr/share/man`); its
  `pm.json` stamps version `5e4b36b5`, which is `8772601e^` (checked with
  `git log` in portuale), so the buggy tree is genuine.
- `logs/l1-20261016T220225Z/l1-report.txt`: 0/0; both `paths.txt` are
  4010 lines vs 4007 in baseline `l1-20261014T091928Z` (4006 files there);
  `/usr/share/info/bash.info.bz2` is in the new paths and in neither
  baseline paths file.
- Revert state: `git worktree list` shows only the main checkout,
  `/var/tmp/355-s1-pre354` and `/var/tmp/t355*` do not exist, and
  `managers/managers.yaml` has no diff and no `pre354` reference.
- No commits made; no bed run was active while checking, and no cargo
  build was started. `/var/tmp` holds no S1 scratch.

---

# S2 — plan `LLM/02.355-reinstall-unmerge-parity.opus.md`, slice S2 (#355 fix)

Branch (both repos): `backlog/355-358-unmerge-parity`.
Oracle rule: expected values from Portage 3.0.82.2 captures (S0 cells P1/P1c/P2), never from portuale output; tests write "Portage", never "real".

## What changed

portuale (product + unit tests + code note):

- `crates/portuale/src/ebuild_merge/plib_merge.rs`
  - New pure `preserved_scan_input(preserved, exclude_cpvs,
    replacement_preserved)`: clones the registry map, drops the
    `exclude_cpvs` keys **first**, then merges the just-preserved feed
    untouched — Portage's `LinkageMapELF.rebuild`
    (`LinkageMapELF.py:180`: `preserve_paths` unconditional,
    `exclude_pkgs` registry-only). `linkage_owner_entries` uses it; the
    old inline order (merge, then remove) dropped the feed on a
    same-version reinstall, where feed key and excluded cpv are the same
    string. Callers passing empty excludes/feeds are behaviour-identical.
- `crates/portuale/src/ebuild_merge/tests/plib_unmerge.rs`: new
  `prune_keeps_a_lib_whose_only_consumer_was_just_preserved_on_a_same_version_reinstall`
  (feed keyed `dev-libs/newapp-2.0`, same string in `exclude_cpvs`;
  asserts the preserved lib is kept). Docstring cites Portage 3.0.82.2,
  S0 cell P1.
- `crates/portuale/src/ebuild_merge/tests/plib_merge.rs`: new
  `preserved_scan_input_excludes_the_registry_but_keeps_the_just_preserved_feed`
  (excluded registry key drops, same-key feed entry stays, unrelated
  feed keys added, input map not mutated).
- No `run_merge.rs` e2e: reproducing P1 in-process is not cheap, and the
  plan lets the pmtest pin cover it instead.
- `LLM/code-notes/portuale/ebuild_merge/plib_merge.md`: #355 paragraph
  under `## fn linkage_owner_entries` plus new `## fn
  preserved_scan_input`; `python3 LLM/tools/code_notes_index.py` run
  (index already current, no further diff).

pmtest (fixtures + pin, no CASES entry per plan):

- `fixtures/repo-355/` (new; collision-checked: no tracked or untracked
  `repo-355`/`t355-*` existed; other plans' untracked `pg359-*` left
  alone): `metadata/layout.conf` (S0 shape), `profiles/repo_name`
  (`t355`), `profiles/categories`, `app-misc/t355-{base,mid,app}` copied
  from the S0 `scripts/ebuilds` (base-1+2, mid-1, app-1; mid-2 belongs to
  the P2 control and is not pinned), and real md5-cache entries for all
  four ebuilds generated by host `egencache` (temp configroot under
  `/var/tmp`, removed afterwards).
- `pytests-contract-suite/test_emerge_pretend_contract.py`: `_t355_configroot`
  helper (mirrors `_t348_configroot`'s repos.conf/make.conf/make.profile
  shape with the `t355` overlay, except `ROOT` is a scratch root à la
  S0 `run.sh`, with the S0 `LDPATH` seed, hermetic `PORTAGE_TMPDIR`/
  `DISTDIR`, `PORTAGE_RUNNING_ROOT` pinned at that root, and
  `T355_V="1"` in make.conf) plus `_t355_merge`, and the pinned
  `test_t355_same_version_reinstall_keeps_a_lib_only_the_just_preserved_lib_needs`:
  real merges base-1, mid-1 (v1), app-1 (v1), base-2, then reinstall
  `=t355-mid-1` with `T355_V="2"` rewritten in make.conf; asserts
  `libt355mid.so.1` and `libt355base.so.1` on disk and both registry
  records (`base:0` under `base-2`, `mid:0` under `mid-1`, cpv + path
  lists; counters deliberately not pinned). Docstring names cell P1 and
  Portage 3.0.82.2. Skips if `gcc` is missing.

## Commands run and results

Failing-first (before the fix, unfixed tree):

- `cargo test --release -p portuale --bin portuale
  prune_keeps_a_lib_whose_only_consumer_was_just_preserved` → **FAILED**
  as predicted: `{"dev-libs/provider-1.0": {"/usr/lib/libA.so.1.0.0"}}`
  reported unneeded — the exact #355 mechanism (S0 P1-pl:
  `<<< !needed obj …/libt355base.so.1`). The pure-helper test could not
  run pre-fix (its symbol did not exist yet — compile-time, noted
  honestly); it passes post-fix.

After the fix:

- `cargo fmt --check` → clean (one `cargo fmt` pass applied first).
- `cargo clippy --release --all-targets` → 0 warnings (one `unused_mut`
  from the refactor fixed).
- `cargo build --release && timeout 2400 cargo test --release --
  --test-threads=2` (whole workspace) → **all targets ok, 0 failed**,
  incl. the portuale bin target **1072 passed, 0 failed** (both new
  tests green; self-exe/db_merge/collision/run_merge/logs/broken-pipe
  tests all pass against the rebuilt binary).
- pmtest baseline (BEFORE any edit):
  `python3 -m pytest pytests-contract-suite -q
  --basetemp=/var/tmp/355-s2-base-pt -p no:cacheprovider` →
  **3939 passed, 37 skipped, 2 xfailed, 0 failed**, no corpus drift.
- pmtest final (with fix + fixtures + pin):
  `python3 -m pytest pytests-contract-suite -q
  --basetemp=/var/tmp/355-s2-pt -p no:cacheprovider` →
  **3940 passed, 37 skipped, 2 xfailed, 0 failed** (+1 = the new pin),
  no corpus drift, compared by test name against the baseline (no test
  changed verdict). The new pin alone: `1 passed`. Its leftover root
  verified by hand: registry JSON holds both records with the S0 P1-pg
  shape (even the counters `"3"`/`"4"` match the Portage capture), and
  `libt355mid.so.1`'s md5 equals the capture's.
- No `git clean` in pmtest fixtures (would delete other plans'
  `pg359-*` untracked files); post-run `git status` shows only the
  intended `M test_emerge_pretend_contract.py` and `?? fixtures/repo-355/`.
- Bed gate (glibc/bash merges) NOT run: the coordinator runs it per the
  task brief.

## Anything surprising

- Two full-workspace `cargo test` runs died with the harness killed by
  SIGTERM (~13 and ~30 min in), both times with only the two slow
  scheduler tests (`run_build_scheduler_single_failure…`,
  `a_hard_failure_kills…`) still running. The failing tests in those
  runs (105 FAILED incl. db_merge/collisions/logs/broken-pipe) were all
  the `self_exe.rs:56` staleness guard (`target/release/portuale` older
  than the edited source) — an invalid sequence on my part (`cargo test`
  without a preceding `cargo build --release`), not regressions: with
  the explicit build first, the third run completed TEST-RC=0 with
  everything green. The SIGTERM itself is unexplained (shared server;
  possibly external); it never recurred on the completing run, but the
  coordinator should know it happened twice.
- One backgrounded `cargo build --release && …` chain vanished silently
  (no log, no process); a foreground rerun finished in 0.09s. Same
  shared-machine flakiness class as above; no code impact.

## Left undone and why

- Merge-path safety gate (glibc + bash test merges): coordinator runs it
  (task brief forbids it here). S2 is therefore "code complete, gate
  pending", not "done" in the plan's Z sense.
- `run_merge.rs` e2e: skipped per the plan's own option (pmtest pin
  covers it).
- No CASES entry, no `what-this-proves.md` paragraph, no backlog/plan
  edits: all per the plan (CASES are pretend argv; docs/backlog belong
  to the Z close-out, which also owns the corpus-bless decision — none
  needed: no drift).
- No commits (coordinator reviews and commits).

---

# S3 — plan `LLM/02.355-reinstall-unmerge-parity.opus.md`, slice S3 (#357 shared CONTENTS parser)

Branch (both repos): `backlog/355-358-unmerge-parity`.
Oracle rule: expected values from Portage 3.0.82.2 captures (S0 cells S1, S2, S3), never from portuale output; tests write "Portage", never "real".

## What changed

portuale (product + unit tests + code note):

- `crates/portage-vdb/src/contents.rs` (new, re-exported from `lib.rs`):
  `ContentsLine<'a>` (`Obj { path, md5, mtime }`, `Sym { path, target,
  mtime }`, `Dir/Dev/Fif { path }`, plus `Bin { path }` for
  compatibility — Portage's regex rejects `bin`, plan §0.3 keeps it),
  `parse_line` (anchored like `_contents_re`: `obj` splits at the last
  two spaces with an all-digits mtime, `sym` strips ` <digits>` then
  cuts at the last ` -> `, `dir/dev/fif/bin` take the rest; a
  non-matching line is `None`/skipped; one trailing `\r` is a line
  ending), `paths(text)` yielding `(kind, path)`. Unit tests: spaces in
  each kind, the ` -> `-in-target greedy mis-split pinned exactly as
  Portage splits it (path keeps ` -> x`, with a do-not-"fix" comment),
  trailing-space path kept verbatim, malformed lines skipped,
  non-numeric mtime rejected, CRLF + `bin` + `paths()` order.
- Migrated every §0.3 site: `portage-vdb` `claim_paths`
  (`files.rs`), `derive_owner` (`sqlite/write.rs`), `owner_rows`
  (`redb_db/write.rs`); `portage-repo` `installed_contents_files`
  (`installed.rs`); `portuale` `parse_contents` (now a thin map),
  `being_unmerged`, `contents_text_owns`/`contents_text_node_type`/
  `owned_node_value_pf` (`installed_instance.rs`),
  `remove_from_contents_into`/`find_preserve_paths_for_merge`/
  `inject_preserved_libs_into_contents` (`plib_merge.rs`),
  `plib_unmerge.rs`, `after_install.rs`/`from_binpkg.rs`
  (`new_image_paths`), `stage_contents_for_quickpkg`
  (`ebuild_package.rs`, quickpkg from vdb now packs space paths),
  `contents_owned_paths` (`remote/binpkg_flow.rs`). Excluded with a
  one-line `//` why each, per plan: config-memory lines
  (`files_write.rs`, Portage `grabdict`/`writedict` split too) and
  `parse_filemeta_paths` (`binpkg_flow.rs`, FILEMETA is
  whitespace-free by construction). The corpus oracle helper
  (`corpus_round_trip.rs`) also uses the shared parser now.
- sqlite/redb owner re-derivation (Q4 default): new `meta`
  `contents_parser` key (`2` = shared parser; absent reads as old),
  seeded at creation, no schema bump. A writable `open` with an absent
  or older key runs one `WriteTxn::rebuild_index` and stamps the key
  in the same transaction (a crash leaves the old key, the next open
  retries; idempotent after). Read-only opens never write: they warn
  once on stderr and serve the stale rows. `portuale vdb
  rebuild-index` needs no change (`open` stamps first).
- Tests: conformance `owners_claim_paths_with_spaces` (S1 shapes: all
  three backends claim the full space paths, decoy and truncation own
  nothing); per-backend reopen tests
  (`reopening_an_old_split_database_rederives_owners_with_the_shared_parser`
  in `sqlite.rs`/`redb_db.rs`: hand-built old-split state — correct
  space rows deleted, bogus decoy rows inserted, stamp dropped —
  misses on read-only, heals on writable reopen with stamp `2`,
  second reopen changes nothing); `parse_contents` space test;
  `contents_owned_paths` space case.
- Rust e2e: `run_merge.rs`
  `a_contents_path_with_spaces_is_removed_and_the_prefix_decoy_is_kept`
  (S1 shape: merge v1, CONTENTS bytes, decoy, `unmerge_one_installed`,
  three entries gone, decoy kept; S2 shape: re-merge v1, reinstall a
  `T355_V=2` copy dropping `a b.txt`, replace loop removes it, decoy
  and kept entries survive) and the S1 shape through `db_kinds()` in
  `db_merge.rs` (sqlite + redb, incl. an `owners` check of the derived
  rows).
- `LLM/code-notes/portage-vdb/contents.md` (new; Portage citations,
  Q4 shape, the two deliberate non-migrations, the #380/#381/#382
  cuts); `python3 LLM/tools/code_notes_index.py` run.

pmtest (fixtures + pin):

- `fixtures/repo-355/app-misc/t355-space/t355-space-1.ebuild` (copied
  from the S0 `scripts/ebuilds`) + real md5-cache entry
  `fixtures/repo-355/metadata/md5-cache/app-misc/t355-space-1`
  generated by host `egencache --config-root=<tmp> --repo=t355
  --update` (temp configroot removed afterwards; existing entries
  untouched; `_md5_` verified against the ebuild bytes). Other plans'
  untracked `pg359-*` files left alone.
- `test_t355_space_paths_survive_merge_and_unmerge_as_portage_records_them`
  in `test_emerge_pretend_contract.py`: merges
  `=app-misc/t355-space-1` via `_t355_configroot`, asserts the three
  S1 CONTENTS lines, plants the decoy, `emerge -C`, asserts all three
  gone and the decoy kept. Docstring names cell S1 and Portage
  3.0.82.2.

## Commands run and results

- `cargo build --release` before every test run (rule (a)); one
  `cargo fmt` pass; `cargo fmt --check` → clean.
- `cargo clippy --release --all-targets` → 0 warnings.
- `cargo test -p portage-vdb --all-features` → all ok (lib 96,
  conformance 139 incl. the new spaces test on files/sqlite/redb,
  both new reopen tests green).
- `cargo test -p portage-repo` → 1109 passed, 0 failed.
- New e2e alone: `cargo test --release -p portuale --bin portuale
  a_contents_path_with_spaces` → 2 passed (files S1+S2, databases
  S1). (First attempt hit the `self_exe.rs` staleness guard because
  `cargo fmt` had touched sources after the build — rebuilt, reran,
  green; no scheduler hang this time.)
- Whole workspace `cargo test --release -- --test-threads=2` (nohup,
  `--test-threads=2`, polled; no #376 hang this time) → **2716
  passed, 0 failed** (portuale bin target 1075, incl. the 2 new e2e
  and the new `parse_contents`/shadow-parser space cases;
  portage-vdb + portage-repo as above).
- pmtest pin alone: `pytest test_emerge_pretend_contract.py -k
  "t355_space_paths or t355_same_version"` → 2 passed (the new S3 pin
  + the S2 pin, still green).
- Full pmtest suite (`python3 -m pytest pytests-contract-suite -q
  --basetemp=/var/tmp/355-s3-pt -p no:cacheprovider`) → **3941 passed,
  37 skipped, 2 xfailed, 0 failed**, no corpus drift. Compared by test
  NAME against the S2 baseline (3940 passed, same tree): +1 is the new
  S3 pin, no test changed verdict.
- `grep -rn "split_whitespace\|split(' ')" crates | grep -i contents`
  → empty (the two excluded sites carry their `//` why on comment
  lines, so the check is vacuous-clean).
- Bed gate (glibc/bash merges, incl. sqlite/redb reinstall lines):
  NOT run — coordinator runs it per the task brief.

## Anything surprising

- The `\r\n` CONTENTS in the existing conformance edge test
  (`obj /usr/share d 1\r`) forced one design point: `parse_line`
  strips a single trailing `\r` (Portage reads in text mode), else
  that long-standing test would newly reject the line.
- `RedbDb::open` on an old database migrates inside `open`, which
  takes the one write transaction briefly; no caller needed changes
  (`mrg` `select_vdb`, `vdb convert`, the tests all go through
  `open`).

## Left undone and why

- Merge-path safety gate (glibc + bash test merges, incl. the
  sqlite/redb reinstall lines): coordinator runs it (task brief
  forbids it here). S3 is therefore "code complete, gate pending",
  not "done" in the plan's Z sense.
- No `what-this-proves.md` paragraph (plan puts docs in Z), no
  backlog/plan edits (Z close-out), no commits (coordinator reviews
  and commits). No allowlist entries added; no corpus bless (the
  suite reported no drift).
- Scratch `/var/tmp/355-s3-*` (build/test/pytest logs and basetemps)
  deleted at handoff per the scratch rule.
