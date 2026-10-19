# Batch 2026-10-18 — open backlog plan

> **Oracle before code.** Every expected value comes from Portage 3.0.82.2 (container bed `../pmtest/differential-test-bed/`, host `emerge`, or `lib/portage/tests/resolver/`), never from portuale's own output, the harvested corpus, or this plan's source reading. Each slice's test docstring says which. Host Portage files are byte-identical to `3rdparty/portage/lib` for the files cited here (checked in the review), so the line numbers hold for both.

Status: done 2026-10-19 (branch `batch/2026-10-18`, S1–S11 executed; #394 scoped to a design only, #406 not re-opened; commits S8 `021afe61`/`c7f2de0`, S9 `971186f4`/`964fb44`, S10 `9a909749`, `3dd86926`, S11 `719c7f31`, `50de74f2`). Originally: proposed 2026-10-18, revised the same day after `LLM/2026-10-18-batch-plan.review.md` (review item ids D*/G*/P* below). Owner answers to §6 recorded 2026-10-18. Owner priorities, reviewed with most care: **#408, #409, #401, #383, #384, #372, #405**. Difficulty: F ≈ 1 h, M = one slice with an oracle, A = several slices or research. Ratings now follow the backlog's `[A]` marks where the review showed more work (G12).

## 0. Branch rule and step 0

**Done 2026-10-18 (owner Q5):** the untracked `fixtures/repo/dev-libs/pg359-*` and md5-cache leftovers from #359 (P4). They are used by committed pins (`CASES` rows, `test_pg359_*`), so they were committed standalone as pmtest `417bb17`; the tree is clean. Then open `batch/2026-10-18` in **both** repos. One paired commit per slice, pmtest first, each message quoting the other. Commit/push only when asked.

## 1. Ground truth (checked 2026-10-18, review §"What was checked")

| # | Claim | Checked |
|---|---|---|
| 408 | the freeze is caused by stdin | **Wrong (D1).** Probe under a real controlling pty: `bash -i -c true` in a `process_group(0)` child stops with SIGTTIN **even with stdin `/dev/null`**; it finishes in the parent's group or under `setsid`. `bash -i` opens `/dev/tty` or uses stderr, never fd 0. Portage never creates a process group: pid-sandbox phases run under `bin/pid-ns-init:96` (`os.setsid()`), `config/preinst/prerm/postinst/postrm` (`doebuild.py:141`, `:253-255`) stay in emerge's foreground group. `stdin=/dev/null` for non-interactive phases (`AbstractEbuildProcess.py:140-146`) is still parity but a separate change. Callers of `spawn_trackable`: `run_phase.rs:56,257`, `misc_functions.rs:202`; `run_phase_command` does not exist. |
| 409 | `\`+newline in a double-quoted `package.env` value | Owner confirmed with `/etc/portage/env/llvm-lto` (`RUSTFLAGS="… \<nl>-Clinker=clang …"`). Reader: `expand_package_env_files`, `portage-profile/src/resolve/package_env.rs:44-47`. Portage: `getconfig` (shlex). |
| 383 | one FEATURES gate | **Four sites (D3, D4):** `vartree.py:2053` (features), `:4541-4545` + `:4560-4564` (`treewalk`: RESTRICT of the **replaced installed** instances clears `_preserve_libs`; the new package's RESTRICT is never read), `:2622-2626` (`unmerge`: RESTRICT of the package being unmerged), `:3667` (`_find_libs_to_preserve`; `:3625-3644` is `_linkmap_rebuild`, a different condition). Ungated in portuale also: `ebuild_unmerge.rs:1080-1090`, `pretend/removal/depclean.rs:251-253`; Portage depclean `actions.py:816-818`, `:1361-1382`. |
| 384 | make.globals read from the config root | `scalar_layers.rs:150` vs `config.py:467-474`. `LocationsManager.py:413-417` prefixes `GLOBAL_CONFIG_PATH` with `portage.const.EPREFIX` (= `PORTAGE_OVERRIDE_EPREFIX`, `const.py:271-272`), which also moves EROOT/BROOT/default config root (D6). portuale has no such variable (only `environ_whitelisted`, `phase_env.rs:176` hard-codes `EPREFIX=""`). No embedded `make.globals` exists; the only copy is `3rdparty/portage/cnf/make.globals`, which differs from the installed file (`xattr` appended). Reading the host's file makes hermetic tests host-dependent (D7). |
| 372 | nobody sets PORTAGE_BASHRC | Confirmed (`doebuild.py:492,535-537`; `ebuild.sh:429-437`). Also unset: `PM_EBUILD_HOOK_DIR` (`doebuild.py:538-540`, `ebuild.sh:439-441`, G5). Depend phase sources the user bashrc too (`ebuild.sh:615-620`, G6). |
| 405 | sites for (c) | Cited lines hold; nine more `requires an argument` sites and two more (D9): `args.rs:1261,1304,1912,1982,2023,2275,2304,2334,2404`, plus `:1775` (`invalid -j parameter`), `:1789` (`--load-average`). |
| 401 | `-f`/`-F` go down the merge path | **False (D10).** `pretend/mod.rs:127-145` refuses them: `option "--fetchonly" is a Portage emerge option, but is not yet implemented in portuale …`, exit 2. Portage has **no `[fetch ]` marker** (D8). |

## 2. Gates

Per slice (AGENTS.md step 8): `cargo fmt --check`; `cargo clippy --release --all-targets` (0 warnings); `cargo build --release`, then `cargo test --release`; then, not concurrently, `python3 -m pytest pytests-contract-suite -q` in `../pmtest`, failures compared by test **name** against a baseline taken at the branch point. Corpus drift: review, bless only in the pmtest commit and only when the new output equals a Portage capture. Never rebuild during a bed run. Record every bed cell in `../pmtest/differential-test-bed/findings/*.md` (P2); evidence for this batch lives under `LLM/evidence/2026-10-18-<item>-*/` (one root, P2).

**Merge-path gate (G8):** slices touching the merge or unmerge half (S4 #383, S9 #380, S10 #381/#382, and S5 #384 whenever `PORTAGE_CONFIGROOT≠/`) run `l1-merge-gate.txt` with `L1_CONSUME_REINSTALL=1`, and for any change to which old files survive the replace loop also the third run with `L1_CONSUME_INSTALL_MASK` (`LLM/agent-context.md:398-430`), plus successful test merges of `sys-libs/glibc` and `app-shells/bash`.

**Phase-spawn gate (G9):** S1, S2, S3 are checked on `l2-portuale-builder.sh` (with `l1-porttest.txt`) or `l3-source-parity.sh`, not `l1-merge-from-binpkg.sh` (which compiles nothing). Neither bed has a tty, so they are regression checks only; #408 itself needs its own pty test and Portage capture (S1.0/S1.2).

**Resolver gate (G10):** S8 (#375, #396), S10 (#391, #394, #402) also run `differential-test-bed/run/l0-resolver.sh` and `l0-fixture-oracle.sh`: no new UNEXPLAINED rows.

## 3. Order

| Wave | Slice | Item | Diff. | Note |
|---|---|---|---|---|
| 1 | S1 | #408 | A | Process-group arrangement, not stdin (D1–D2). |
| 1 | S2 | #409 | F/M | Blocking bug; no merge path; independent of S1. |
| 1 | S3 | #372 | A | Touches `phase_env_vars` only; not dependent on S1 (P3). |
| 1 | S4 | #383 | A | Needs the synthetic base to carry `preserve-libs` (D5). |
| 1 | S5 | #384 | M | Owner Q2 answered: embedded copy of `make.globals` (see S5). |
| 2 | S6 | #405 (b)(c) | F/M | |
| 2 | S7 | #401 | A | S7.0 oracle first, then 2 slices. |
| 3 | S8 | #371, #375, #395, #396, #392, #393, #373 | F each | One commit each. |
| 4 | S9 | #380, #406 | F/M | #406 only if the owner re-opens it. |
| 5 | S10 | #402, #391, #381, #382, #394 | M | Own oracle cell design each. |
| 6 | S11 | #385, #399 | verify | May close with no change. |
| — | — | #390, #363–#365, #368 | A | Not planned here; Tier 4, own plans. |

## 4. Slices

### S1 — #408 phases must not be a background process group of the terminal (A)

**Files:** `ebuild_phases/scheduler_registry.rs:84-110` (`spawn_trackable`), `ebuild_phases/commands.rs:549-600` (`spawn_bash_script`), callers `run_phase.rs:56,257`, `misc_functions.rs:202`, `emerge_build/status.rs:94-116` (stale "no `interactive` PROPERTIES" comment; `GraphEntry::interactive` exists, `portage-repo/src/resolve/entry.rs:86`), tests, code note `LLM/code-notes/portuale/ebuild_phases/…`. Backlog #408 root-cause steps (2)–(3) and `docs/evidence/2026-10-18-sigttin/` conclusions are corrected in the same slice.

**Interfaces:** `SpawnBashScriptArgs` gains `placement: PhasePlacement { NewSession, ParentGroup }` and `interactive: bool`. `spawn_trackable` does `setsid()` via `pre_exec` for `NewSession` (pgid == pid, so the registry's `kill(-pgid)` still works) and nothing for `ParentGroup`.

- [ ] **S1.0 Oracle (G2).** On the bed or host, with Portage, on a real pty: a fixture ebuild whose `src_test` runs `bash -i -c true`, and one whose `pkg_config` does `read`; each with `FEATURES=pid-sandbox` on and off. Record in `findings/l2.md` and `LLM/evidence/2026-10-18-408-s0/`. Re-run the review's probe (`script -qec "python3 -I probe.py" /dev/null`, cases A, C, E, F, J, K) and keep the output.
- [ ] **S1.1 Audit all spawns** (G3): `grep -n "Command::new\|spawn_trackable\|process_group" crates/portuale/src`. List each with placement and stdin. Include `--config`, merge-time `preinst/postinst/prerm/postrm`, hooks, `pkg_pretend`/`pkg_setup`, `FETCHCOMMAND`. The `mrg.rs:2654/2759/2951` lines are test code (D10) and prove nothing. Decide the scheduler's interactive handling (G3): Portage forces foreground, `--jobs=1` and prints "Sending package output to stdio due to interactive package(s)" (`Scheduler.py:483-519`); `scheduler_status_mode` (`status.rs:116-`) ignores it. Implement or file with a real number.
- [ ] **S1.2 Failing test (G1).** The test binary re-executes itself in a child that calls `setsid()` and `ioctl(TIOCSCTTY)` on a pty slave (an `#[ignore]`d inner test selected by a filter). Inside: `spawn_bash_script` of a script `bash -i -c true` with stderr on a pipe must finish within 20 s (today: stops, case A); a `config`-phase script doing `read x` must get data written to the pty master (case J today, K expected). Use `timeout`, never `pkill`.
- [ ] **S1.3 Implement placement** mirroring Portage: pid-sandbox phases ⇒ `NewSession`; `config/preinst/prerm/postinst/postrm` ⇒ `ParentGroup` (needs a pid-based kill path since `kill(-pgid)` would hit portuale itself; or knowingly `setsid` them and record the divergence: owner Q1). Do not tie the choice to whether portuale itself unshares a pid namespace.
- [ ] **S1.4 stdin** (separate commit-worthy change): `stdin(Stdio::null())` unless phase `config` or the package's **raw** `PROPERTIES` string split contains `interactive` (Portage `AbstractEbuildProcess.py:143` tests the raw metadata, so `foo? ( interactive )` counts even with `foo` off; D11). Test with USE-conditional `PROPERTIES`.
- [ ] **S1.5 Signals (G4).** With `setsid`, `^C` reaches portuale only. Test: SIGINT to portuale during a phase terminates the phase group via the registry; document `^Z`/SIGTSTP (Portage forwards SIGINT, SIGTERM, SIGHUP, SIGTSTP, SIGCONT in `pid-ns-init:27-33`).
- [ ] **S1.6 Live check:** `portuale emerge -1a app-shells/bash` with `FEATURES=test` in a tmux pane; `make check` completes. Evidence `LLM/evidence/2026-10-18-408-after.md`. Phase-spawn gate (§2).
- [ ] **S1.7 Docs:** what-this-proves paragraph, backlog #408 → DONE, fix `status.rs:94-95`, commit.

### S2 — #409 backslash-newline in quoted values (F/M)

**Files:** `portage-profile/src/resolve/package_env.rs` (`expand_package_env_files`, `:44-47`) and the shared make.conf-style reader it calls (find with `graft callers expand_package_env_files --direction out`), tests `portage-profile/src/tests/`, pmtest `fixtures/etc/portage/env/…` fixture.

- [ ] **S2.0 Oracle.** Run Portage's `portage.util.getconfig` on the owner's `llvm-lto` file (copy into the scratchpad) and on a `make.conf` with the same continuation; capture the dict. Cases: `\<nl>` in double quotes (removed), in single quotes (kept literally), unquoted, at end of value, `\\<nl>`. Record in `LLM/evidence/2026-10-18-409-s0/`.
- [ ] **S2.1 Failing test:** unit test with the owner's file: `package_env_vars` has `RUSTFLAGS` = `-C target-cpu=znver3 -C strip=debuginfo -C opt-level=3 -Clinker=clang -Clinker-plugin-lto -Clink-arg=-fuse-ld=lld` (exact value from S2.0) and `CFLAGS` expanded from `${COMMON_FLAGS}`. Same continuation in `make.conf`; expect the same fix or file a sibling item.
- [ ] **S2.2 Implement** in the shared reader; keep one implementation.
- [ ] **S2.3 pmtest:** `emerge --info` pin for a `package.env` fixture with a continued value; expected value from Portage.
- [ ] **S2.4 Live check:** the owner's `emerge -1 --nodeps =app-containers/aardvark-dns-2.1.0::gentoo` (needs the owner's env file; use a stub `rustc` wrapper that fails on a lone `\` otherwise). The `Unable to unshare` line is a separate residue to check against Portage's text.

### S3 — #372 `PORTAGE_BASHRC`, `PORTAGE_BASHRC_FILES`, `PM_EBUILD_HOOK_DIR` (A)

**Files:** `ebuild_phases/phase_env.rs` (`phase_env_vars`), `depend_phase.rs:58-61`, `misc_functions.rs` spawn, `portage-profile` (per-profile `profile.bashrc`, `package.bashrc`), `atom_lines.rs:74-90` and `profile_chain.rs:76` (per-repo `profile-formats`; `atom_lines.rs:271-275` is only the user-profile constant, D10), `LLM/code-notes/portuale/ebuild_phases/mod.md:18` and its long duplicate at `:23`.

- [ ] **S3.0 Oracle with the exact order pinned (G7).** `_pbashrc`: for each node of `profiles_complex` (user profile `etc/portage/profile` last, `LocationsManager.py:170-189`), `profile.bashrc` if the file exists regardless of `profile-formats` (`config.py:850-853`); then that node's `package.bashrc` matches by atom specificity, read only when the node has `profile-bashrcs` (`config.py:921-947`, `:2008-2022`); files resolve under `<profile>/bashrc/<name>`. `PORTAGE_BASHRC` = `PORTAGE_CONFIGROOT` + `etc/portage/bashrc` (`doebuild.py:535-537`, `const.py:38`). `PM_EBUILD_HOOK_DIR` (`doebuild.py:538-540`, `ebuild.sh:439-441`). Capture a probe bashrc echoing `${EBUILD_PHASE}` for `/etc/portage/bashrc`, a `profile.bashrc`, and an `/etc/portage/env/<cat>/<pn>` hook, on Portage.
- [ ] **S3.1 Failing tests:** fixture config root: marker written in `pre_src_compile` by `/etc/portage/bashrc`; a profile `profile.bashrc`; an env hook file. All absent today, silently.
- [ ] **S3.2 Implement** the three variables in `phase_env_vars`, in the misc-functions spawn, and in the depend phase (user bashrc is sourced there; profile bashrcs are skipped, `ebuild.sh:428`, `:615-620`; G6).
- [ ] **S3.3 Bed cell** (phase-spawn gate): marker sequence compared on both PMs, recorded in `findings/l2.md`; pin in pmtest.
- [ ] **S3.4** Fix both stale lines in `mod.md`; backlog #372 DONE.

### S4 — #383 preserve-libs gating (A)

**Files:** `ebuild_merge/plib_merge.rs:254` (`find_preserve_paths_for_merge`), callers `after_install.rs:195`, `from_binpkg.rs:481`; `ebuild_unmerge.rs:1080-1090`; `pretend/removal/depclean.rs:251-253`; `ebuild_merge/options.rs:175-188` (`set_resolved_features`); `portage-profile/src/resolve/scalar_layers.rs:162-175` (synthetic base); pattern `ebuild.rs:513-521`; tests `ebuild_merge/tests/plib_merge.rs:184`.

**Interfaces:** `FindPreservePathsForMergeArgs` gains `preserve_libs: bool` computed as `"preserve-libs" in FEATURES && no replaced installed same-slot instance has RESTRICT=preserve-libs` (vdb `RESTRICT`, USE-reduced as `setcpv` does). A separate `preserve_libs_for_unmerge(features, own_restrict)` for the unmerge arm.

- [ ] **S4.0 Prerequisite (D5).** Add `preserve-libs` (`make.globals:82`) to the synthetic base beside `unmerge-orphans`, **in the same commit**, so hermetic roots (no `make.globals`) keep preserving. Assert `pmtest test_portuale.py:7235 test_emerge_preserved_libs_advisory_and_rebuild_set` unchanged.
- [ ] **S4.1 Oracle:** rerun #354 matrix row `f0` (`LLM/evidence/2026-10-14-354-s0/matrix.md`); add rows: (a) old instance with `RESTRICT=preserve-libs`, new without (expect removed); (b) new with `RESTRICT=preserve-libs`, old without (expect **preserved**, `treewalk` reads only the old instances); (c) `-preserve-libs` on `emerge -C` of a lib still linked (expect removed); (d) depclean with `-preserve-libs` (`actions.py:816-818`, `:1361-1382`).
- [ ] **S4.2 Failing tests** for (a), (b), (c), (d) and the merge-time features case.
- [ ] **S4.3 Implement** merge, unmerge and depclean arms with one shared helper, or file the unmerge/depclean halves with real backlog numbers at close-out (D4); do not leave an unmentioned cut.
- [ ] **S4.4 Merge-path gate** (§2): three L1 runs, glibc and bash test merges; the default-on path must be byte-identical.

### S5 — #384 locate `make.globals` like Portage (M; owner Q2: portuale carries its own copy)

Decision (owner, 2026-10-18): portuale brings its own `make.globals`. This is feasible and solves D7: `crates/portuale/build.rs` already embeds the repo's top-level `cnf/` directory at build time (`walk(&repo.join("cnf"), "cnf", …)`; today it holds only `cnf/sets/`), and `3rdparty/portage/cnf/make.globals` is the upstream file. Host `/usr/share/portage/config/make.globals` differs from it by exactly one line (`FEATURES="${FEATURES} xattr"`, appended by the Gentoo ebuild, line 177). Hermetic runs read the embedded copy, so results no longer depend on the host.

- [ ] **S5.1 Oracle:** `config.py:462-490` and `LocationsManager.py:413-417`; decide on the `xattr` line (recommended: the embedded copy includes it, since it is what every Gentoo Portage reads; record the choice). Capture Portage `emerge --info` FEATURES with `PORTAGE_CONFIGROOT` pointing at a fixture root.
- [ ] **S5.2** Copy the file to `cnf/make.globals`; in `scalar_layers.rs:147-175` stop reading `<config_root>/usr/share/portage/config/make.globals` (Portage never reads it from the config root); the embedded copy is the base layer; the synthetic `unmerge-orphans`/`preserve-libs` base (S4.0) becomes redundant and is removed here. Warn on `<config_root>/etc/make.globals` as `config.py:475-490` does.
- [ ] **S5.3** Failing test: a hermetic root carrying a *different* `usr/share/portage/config/make.globals` is ignored; `FEATURES` has `preserve-libs` and `unmerge-orphans` from the embedded copy.
- [ ] **S5.4** Drift review against Portage captures (expect drift where hermetic roots had no globals); merge-path gate (§2). Test that the embedded file equals `3rdparty/portage/cnf/make.globals` plus the declared `xattr` line, so a Portage bump is noticed.
- [ ] **S5.5** Residue: a host whose installed `make.globals` was patched by the distro differs from the embedded copy (accepted divergence, documented in the code note).

### S6 — #405 (b)(c) option errors (F/M)

**Files:** `pretend/args.rs` at `:604,735,749,763,778,1018,1032,1047,1061,1076` **and** `:1261,1304,1912,1982,2023,2275,2304,2334,2404`, plus `:1775` (`invalid -j parameter`) and `:1789` (`--load-average`).

- [ ] **S6.1 Oracle:** `main.py:263-288` for `-jx`; capture stderr+rc of Portage for every argv above (argparse `choices` options and `insert_optional_args`-defaulted options give different texts; record `python3 -V`). One capture per site, no generalising.
- [ ] **S6.2** One pinned test per distinct text; drive sites from one table in `args.rs` only if smaller than the sites it replaces.
- [ ] **S6.3** Bless corpus drift in the pmtest commit after review.

### S7 — #401 `--fetchonly` / `--fetch-all-uri` (A)

- [ ] **S7.0 Oracle first (portage-oracle agent), corrected checklist (D8, D10, G11).** Write `LLM/evidence/2026-10-18-401-s0/oracle.md` with captures of `emerge -pf`, `-af`, `-f`, `-F` (host or bed, proxy mirror): header "These are the packages that would be fetched" (`depgraph.py:12096-12097`); prompt "Would you like to fetch the source files for these packages?" (`actions.py:522-523`); `-p -f` is *not* terminal after display (`actions.py:541-543`), the Scheduler runs `EbuildFetchonly(pretend=True)` → `doebuild(…,"fetch",listonly=1)` and **prints the URI list** (`EbuildBuild.py:153-175`, `EbuildFetchonly.py:20-36`); `-F` implies `-f` (`actions.py:3910-3911`), `FEATURES=mirror` implies `fetch_all_uri` (`Scheduler.py:192-193`); mtimedb read-only under fetch-only (`actions.py:271-272`, `mtimedb.py:46,94-95`): resume data is neither cleared nor written; and the G11 list: scheduling continues after a failure (`Scheduler.py:1918-1923`), no `--keep-going` loop (`:1279`), blockers ignored (`:77-79`, `depgraph.py:9268`), `pkg_pretend` skipped (`:1744`), strict-manifest pre-check skipped (`:793-796`), parallel-fetch off (`:324-327`), no background output (`:81`), no temp-dir check or build-dir lock (`EbuildBuild.py:45`), failed fetch runs `pkg_nofetch` without a build dir and returns 1 (`EbuildBuild.py:231-256`), world untouched (`:2555-2562`), no vardb/bindb write handle (`actions.py:571-578`). One capture row per fact. There is **no `[fetch ]` marker**: do not invent one.
- [ ] **S7.1 Pretend/ask half:** remove the exit-2 refusal in `pretend/mod.rs:127-145`; header, prompt, URI listing, favorites gate (`pretend/merge_setup.rs:155`); pins from S7.0 captures. Correct backlog #401 text ("goes down the merge path" is false).
- [ ] **S7.2 Execution half:** fetch-only job path in `emerge_build` per S7.0; assertion that `var/db/pkg` is byte-identical; exit codes and `pkg_nofetch`.
- [ ] **S7.3 Gate:** L1 bed cells for both options, recorded in `findings/l1.md`; merge-path gate only if `ebuild_merge` is touched.

### S8 — small divergences (F each, one commit per item, capture first)

**#371** `Total: 0 packages` rc 0 for an empty set; re-ground `test_world_missing_file_expands_to_system_not_an_error` and `test_preserved_rebuild_set_is_empty_when_the_registry_is_empty` from Portage captures. **#375** fold `--update-if-installed` into `req.update` (`create_depgraph_params.py:139-143`); cell `pg367-top -pvD --update-if-installed`. **#395** ` for <root>` suffix (`depgraph.py:6502-6503`; the `PORTAGE_RUNNING_ROOT` hook is at `portage-repo/src/env_roots.rs:72-79`, owner Q3: the hook is kept; removing it is not under 15 minutes (three test setups plus every fixture run), and printing the suffix is its own slice since about 53 `no ebuilds to satisfy` test sites compare modulo it). **#396** `--usepkgonly=n` drops the ebuild db (`depgraph.py:809`; probe a NEW package). **#392** `USE="-flag"` on `NS` without `-v`. **#393** `--update --package-moves=n` same-version slot mismatch. **#373** `usepkg-exclude/include` in repos.conf (`repository/config.py:196,269-282`, `depgraph.py:384-392`). Resolver gate applies.

### S9 — #380 (F/M), #406 (conditional)

**#380** capture Portage's refusal text for an image path with `\n`/`\r` (`vartree.py:4616-4706`, `paths_with_newlines`), refuse before copying, unit + L1 cell; merge-path gate. **#406** only if the owner re-opens it (02.346 Q6).

### S10 — medium, each with its own oracle cell design

#402 (cpv-deduped masked row; `resolve/pretend.rs` `binary_candidates.retain(binpkg_respect_use_ok)` vs `masked_rows_for_pool`), #391 (pin the moves-off capture first), #381 (`_match_contents` full form; symlinked-parent cell), #382 (`merge_flow.sh` calls the shipped Rust unmerge, owner Q4 decided; failing test first: a recorded path with a space is removed; remote client binary must be present, cross-arch unsupported), #394 (re-walk after `_rebuild_config`). A separate `LLM/02.<n>-*.opus.md` only if S0 shows more than one slice of work. Resolver gate for #391/#394/#402; merge-path gate for #381/#382.

### S11 — verify first

**#385:** `dodir`/`keepdir` cells (EAPI 7/8, with/without `strict-keepdir`, image and `CONTENTS` diff, `.keep_<cat>_<pkg>-<slot>`); close as no divergence if both agree. **#399:** rerun the four `blk0*` rows on the current binary; bisect from `3afc6bf0` only if they persist.

## 5. Not planned here

#390, #363–#365, #368 (pubgrub, Tier 4, A): each needs its own plan.

## 6. Owner decisions (2026-10-18)

1. **Q1 (S1): use Portage's behaviour.** `config/preinst/prerm/postinst/postrm` stay in portuale's own foreground group (pid-based kill path instead of `kill(-pgid)`); pid-sandbox phases get `setsid`. No recorded divergence.
2. **Q2 (S5): portuale carries its own `make.globals`**, via the existing `cnf/` embedding.
3. **Q3 (S8 #395): keep `PORTAGE_RUNNING_ROOT`** (removal is not under 15 minutes).
4. **Q4 (S10 #382): one path, the Rust unmerge (option A).** Already decided earlier, in `docs/remote-merge.md` §5: before a remote merge the server installs its own binary on the client (`remote/client_binary.rs`: `/opt/bin` or `/usr/local/bin`, named `portuale` or `portuale-<hash>` by SHA-256, `--remote-portuale-binary`, `--remote-portuale-dir`) and remote phases already get `PORTUALE_BIN` (#313). So `merge_flow.sh` removes the old instance by calling that binary's unmerge, and the shell copy (`:31-34`, `:177-200`) is deleted, not ported. **Out of scope, unchanged:** cross-architecture use (for example an x86-64 server and an aarch64 client). The existing gate keeps refusing it: the shipped binary's ELF `e_machine` is compared with the client's `uname -m` and an arch mismatch aborts before anything ships (`client_binary.rs`, `docs/remote-merge.md` §5/§8). S10 adds no cross-arch support and states it in the code note.
5. **Q5 (step 0): done**, pmtest `417bb17`.

## 7. Close-out (AGENTS.md 7, 10, 11)

Each slice: one `what-this-proves.md` paragraph with a live-run example, backlog entry → `DONE <date>` with reachable commits, code notes under `LLM/code-notes/…` plus `python3 LLM/tools/code_notes_index.py`. Last slice: flip this file's `Status:` to `done <date>` in the same commit. Residues (e.g. the unmerge/depclean halves of #383, the scheduler's interactive handling, the `Unable to unshare` text) get numbers after scanning all tiers (`grep -nE '^[0-9]+\. \*\*' LLM/backlog-tasks.md LLM/backlog-tasks-2026-10.md`); the last taken is #409.
