# Review of `LLM/2026-10-18-batch-plan.md` (plan, before S1)

This review is the only file written. No product code, fixtures, docs, plan, backlog or pins were modified. Nothing was committed.

- **Audited HEAD:** portuale `02614272` on `main`. The worktree is clean apart from the untracked plan itself. pmtest is on `main` with untracked `fixtures/repo/dev-libs/pg359-*` fixtures and their md5-cache entries. These are unrelated #359 leftovers; I did not look at them, but they will ride into the first `batch/2026-10-18` commit unless they are dealt with first. Neither repo has a `batch/2026-10-18` branch yet, which is correct for a plan in `proposed` state.
- **Portage reference:** host `/usr/lib/python3.14/site-packages/{portage,_emerge}`, `portage.VERSION == 3.0.82.2`.
  - Byte-identical to `3rdparty/portage/lib/…`: `vartree.py`, `config.py`, `doebuild.py`, `LocationsManager.py`, `process.py`, `SpawnProcess.py`, `Scheduler.py`, `actions.py`, `EbuildBuild.py`, `main.py`, `depgraph.py`.
  - `AbstractEbuildProcess.py` and `bin/pid-ns-init` differ from the vendored copy only by install-time substitutions (`@IPC@`, the shebang).
  - So the line numbers below hold for both trees.
- **Verification:** none re-run (no cargo, no pytest, no bed). The last recorded slice paragraph is `LLM/what-this-proves.md:19568` (#403); the last recorded merge-path gate is the #355 one cited at `:19540`.
- **Oracle evidence:** I ran three small probes under a real pty with a controlling terminal (`script -qec`, bash 5.3.20). They are a discriminating capture of kernel and bash job-control behaviour, not of Portage. The script is inlined under D1 so it can be re-run. Every other finding rests on source reading and is marked **proven** (a discriminating read or a probe) or **likely**.

## What was checked and holds

1. **§0 #408, Portage side.**
   - `AbstractEbuildProcess._phases_interactive_whitelist = ("config",)` is at `:41`. The `/dev/null` assignment is at `:140-146`, gated on `0 not in self.fd_pipes`, phase not `config`, and `"interactive" not in settings["PROPERTIES"].split()`.
   - `SpawnProcess._start` (`:82-94`) also gives `/dev/null` to any background spawn, and otherwise `portage._get_stdin()`.
2. **§0 #408, portuale side.**
   - `spawn_trackable` does `cmd.process_group(0)` at `scheduler_registry.rs:88`.
   - `spawn_bash_script` (`commands.rs:549-600`) sets stdout and stderr only (`:577`, `:585`), never stdin.
   - Its only callers are `run_phase.rs:257` (`run_one_phase_bash`, every bash phase including `--config`, `preinst`, `postinst`) and `misc_functions.rs:202`.
   - The depend phase already uses `/dev/null` (`depend_phase.rs:61`).
3. **§0 #383 premise.**
   - `find_preserve_paths_for_merge` is at `plib_merge.rs:254` and takes no features input (`:241-248`).
   - Its callers are `after_install.rs:195` and `from_binpkg.rs:481`.
   - `vartree.py:2053` sets `self._preserve_libs = "preserve-libs" in mysettings.features`.
   - `preserve-libs` is default-on in `make.globals:82`.
   - The `ebuild.rs:513-521` pattern for `unmerge_orphans` exists as cited.
4. **§0 #384 premise.**
   - `scalar_layers.rs:150` joins `config_root` with `usr/share/portage/config/make.globals`.
   - `config.py:467-474` picks `PORTAGE_BASE_PATH/cnf/make.globals` when `portage._not_installed`, else `global_config_path/make.globals`.
   - `LocationsManager.py:413-417` prefixes `GLOBAL_CONFIG_PATH` with `portage.const.EPREFIX`, which `const.py:271-272` takes from `PORTAGE_OVERRIDE_EPREFIX`.
   - `config.py:475-490` warns about `<config_root>/etc/make.globals`.
   - The synthetic-base test is at `portage-profile/src/tests/env_layer.rs:74`.
5. **§0 #372 premise.**
   - No `PORTAGE_BASHRC` anywhere in `crates/`.
   - `doebuild.py:492` sets `PORTAGE_BASHRC_FILES = "\n".join(mysettings._pbashrc)`.
   - `:535-537` sets `PORTAGE_BASHRC = PORTAGE_CONFIGROOT + EBUILD_SH_ENV_FILE`, where `EBUILD_SH_ENV_FILE` is `etc/portage/bashrc` (`const.py:38`).
   - `ebuild.sh:429-437` consumes both.
   - `LLM/code-notes/portuale/ebuild_phases/mod.md:18` holds the stale "command not found" line; its long duplicate is in the same file at `:23`.
6. **§0 #401 premise.** The options appear only in `emerge_options.rs:44-45`, and `pretend/merge_setup.rs:155` and `emerge_build/status.rs:95` carry the comments. There is no fetch-only path in `emerge_build`.
7. **§0 #405 sites.** The cited lines `args.rs:604,735,749,763,778,1018,1032,1047,1061,1076` all say what the plan claims. The list is incomplete; see D9.
8. **S7/S8 citations spot-checked:** all match.
   - `create_depgraph_params.py:139-143` (`--update-if-installed` implies `--update`);
   - `depgraph.py:6502-6503` (` for <root>`);
   - `depgraph.py:809` (`--usepkgonly` keeps porttree);
   - `repository/config.py:196,269-272`;
   - `depgraph.py:384-392`;
   - `vartree.py:4616-4706` (`paths_with_newlines`).
9. **Numbering.** The highest item across both backlog files is #408. There are no duplicates (`grep -hoE '^[0-9]+\. \*\*' LLM/backlog-tasks*.md`), so the next free number is #409, as §5 says. Every item the plan schedules exists.
10. **Close-out rules.** §5 matches AGENTS.md steps 7, 10 and 11: plan `Status:` flip in the last commit, reachable hashes, code-notes index. The artefact is in English. Branch-in-both-repos and pmtest-first commit order are stated in the plan header.

## D — defects

### D1 — S1's fix (`stdin(Stdio::null())`) does not fix #408; the root cause is the new process group, not stdin (high, proven)

**Where.**
- Plan §0 row 408 ("Confirmed"), S1.3 and the S1.2 test.
- Backlog `LLM/backlog-tasks.md:119`, root-cause steps (2) and (3).
- Code: `crates/portuale/src/ebuild_phases/scheduler_registry.rs:88`.

**Facts.**
- bash's `initialize_job_control` does not use fd 0. For a forced-interactive shell (`bash -i`) it opens `/dev/tty` when stderr is not a tty, else it duplicates stderr. It then compares `tcgetpgrp()` with its own process group and sends itself SIGTTIN while they differ. The captured stack (`docs/evidence/2026-10-18-sigttin/stopped-emerge-portuale.info:79-86`) is in exactly that function (`jobs.c:4795`).
- Probe (pty with a controlling terminal, child spawned from Python), proven:

  | Case | Process group | stdin | stderr | Result |
  |---|---|---|---|---|
  | A | `process_group=0` | `/dev/null` | pipe | **stops (SIGTTIN)** |
  | B | `process_group=0` | `/dev/null` | tty | **stops** |
  | C | parent's | `/dev/null` | pipe | exits 0 |
  | D | parent's | tty | tty | exits 0 |
  | E | `process_group=0` | tty | pipe | **stops** |
  | F | `start_new_session` (setsid) | `/dev/null` | pipe | exits 0 ("no job control in this shell") |
  | I | `process_group=0` | `/dev/null` | — (`cat /dev/tty`) | **stops** |

  A and E differ only in stdin, and both stop. stdin is irrelevant; the process group decides.
- **Portage never creates a process group for a phase.** `grep -rn 'setsid\|setpgid\|setpgrp\|tcsetpgrp' portage _emerge` finds only a TODO comment (`SpawnProcess.py:87`). The one place a new session appears is `bin/pid-ns-init:96` (`os.setsid()`, "Isolate parent process from process group SIGSTOP (bug 675870)"). Portage interposes it for every phase when `FEATURES=pid-sandbox` is on (default, `make.globals:81`), the process is root on Linux (`doebuild.py:2066-2073`), and the phase is not in `_global_pid_phases = {config, preinst, prerm, postinst, postrm}` (`doebuild.py:141`, `:253-255`).
- So Portage runs a phase either in a **new session with no controlling tty** (pid-sandbox phases: `pid-ns-init:110-119` tries `TIOCSCTTY` on stdout and gets EPERM when it is the parent's terminal), or in **emerge's own foreground group** (`config`/`preinst`/`prerm`/`postinst`/`postrm`, or pid-sandbox off, or non-root). Neither case can get SIGTTIN.

**Divergence.**
- Inputs: `portuale emerge -1a app-shells/bash` with `FEATURES=test` on a terminal.
- Portage: `make check` runs `bash -i exec8.sub` in a session with no tty (pid-sandbox) or in the foreground group, and it completes.
- portuale, after the plan's S1.3: the phase is still in a background group of the session's controlling tty, so `bash -i` opens `/dev/tty` (stderr is the tee pipe) and the whole group still stops (case A).

**Fix direction** (owner decision; replaces §4 Q1). Mirror Portage's two arrangements:
- `setsid()` (via `pre_exec`) instead of `process_group(0)` for the phases Portage puts behind `pid-ns-init`. pgid still equals pid, so `SCHEDULER_REGISTRY`'s `kill(-pgid)` keeps working.
- For the `_global_pid_phases`, either stay in portuale's own group, which needs a pid-based kill path because `kill(-pgid)` would hit portuale itself, or knowingly also `setsid` them and record the divergence. Do not tie this choice to whether portuale itself unshares a pid namespace, unless the owner says so.
- `stdin(Stdio::null())` is still right as Portage parity (`AbstractEbuildProcess.py:140-146`), but it is a second, independent change, not the #408 fix.
- Correct backlog #408 root-cause steps (2)–(3) and §0 in the same slice.

Probe script (re-run with `script -qec "python3 -I probe.py" /dev/null`):

```python
import subprocess
def run(label, argv=["bash","-i","-c","true"], **kw):
    try: print(label, "rc", subprocess.run(argv, timeout=4, **kw).returncode)
    except subprocess.TimeoutExpired: print(label, "TIMEOUT (stopped)")
N, P = subprocess.DEVNULL, subprocess.PIPE
run("A pg0 null pipe", process_group=0, stdin=N, stdout=P, stderr=P)
run("C samepg null pipe", stdin=N, stdout=P, stderr=P)
run("E pg0 tty pipe", process_group=0, stdout=P, stderr=P)
run("F setsid null pipe", start_new_session=True, stdin=N, stdout=P, stderr=P)
run("J pg0 read tty", ["bash","-c","read x; echo got=$x"], process_group=0)
run("K samepg read tty", ["bash","-c","read x; echo got=$x"])
```

### D2 — the interactive path (`config`, `PROPERTIES=interactive`) is broken by the same process group, and S1.4 frames it wrongly (high, proven by probe; the `--config` consequence is likely)

**Where.** Plan S1.4 and §4 Q1 ("Portage does *not* `tcsetpgrp` … only add `tcsetpgrp`+SIGTTOU-ignore if Portage's group arrangement requires it").

**Facts.**
- Probe J/K: `read x` from the controlling tty stops when it runs in a `process_group(0)` child, and works in the parent's group (proven).
- portuale's `pkg_config` goes through `run_one_phase_bash` → `spawn_bash_script` → `spawn_trackable` (`run_phase.rs:56,257`). So today an interactive `pkg_config` that reads the terminal hangs (likely; not run against a real package).
- Portage runs `config` in emerge's foreground group: `config` is in `_global_pid_phases`, so there is no `pid-ns-init` and no new group.
- For a `PROPERTIES=interactive` package's `src_*` phases under pid-sandbox, Portage is in a new session where stdin is the tty but not the controlling terminal. Reads then work without job-control signals.

**Divergence.** S1.4 proposes keeping the tty only on the stdin side, plus an optional `tcsetpgrp`. Portage never calls `tcsetpgrp`; it avoids the background-group situation altogether.

**Fix direction.** S1.4 should follow D1's arrangement: `config` (and the other global-pid phases) in portuale's own group with stdin inherited, and interactive `src_*` phases in a new session with stdin inherited. Drop the `tcsetpgrp`/SIGTTOU option unless the owner explicitly prefers it over parity.

### D3 — S3.2's second test expects the wrong package's RESTRICT, and the §0 citations for the three gates are off (medium, proven)

**Where.** Plan §0 row 383, S3.1, S3.2 ("`RESTRICT=preserve-libs` of the *new* package ⇒ same").

**Portage.**
- `:2622-2626` is in `dblink.unmerge` (`def` at `:2482`). It checks the RESTRICT of the package **being unmerged**.
- The merge-time RESTRICT gate is in `treewalk` (`def` at `:4352`), at `:4541-4545`: for every same-slot **installed** instance (`settings_clone.setcpv(cur_cpv, mydb=vardb)`), `RESTRICT=preserve-libs` clears `self._preserve_libs`. `:4560-4564` then propagates the result to every `others_in_slot`.
- Nothing in `treewalk` reads the **new** package's RESTRICT for preserve-libs.
- `:3625-3644` is `_linkmap_rebuild`, not `_find_libs_to_preserve`. The latter starts at `:3652`, with its gate at `:3667` (`or not self._preserve_libs`). `_linkmap_rebuild`'s own gate is "`preserve-libs` not in features **and** registry empty", which is a different condition.

**Divergence.**
- Inputs: installed `a-1` without RESTRICT; merge `a-2` with `RESTRICT=preserve-libs`; a consumer still needs `a-1`'s library.
- Portage preserves the library (the old instance carries no RESTRICT).
- The plan's test would assert it is removed.

**Fix direction.**
- Correct §0 to "2053 (features); 4541-4545 + 4560-4564 (RESTRICT of the replaced installed instances, merge); 2622-2626 (RESTRICT of the unmerged package, unmerge); 3667 (`_find_libs_to_preserve`)".
- Rewrite the S3.2 RESTRICT test as "old installed instance has `RESTRICT=preserve-libs` ⇒ removed".
- Add a negative test: the new package's RESTRICT alone ⇒ still preserved.
- Read the old instance's RESTRICT from its vdb `RESTRICT` file (USE-reduced, as `setcpv` does).

### D4 — S3 covers only the merge path; Portage's gate also governs unmerge-time preservation and depclean's lib check (medium, proven by source read)

**Where.** Plan S3 "Files"/"Interfaces". The ungated sites in portuale:
- `ebuild_unmerge.rs:1080-1090` (`preserve_libs_on_unmerge_into`);
- `pretend/removal/depclean.rs:251-253` ("portuale has no preserve-libs feature").

**Portage.**
- Unmerge: `unmerge()` → `_prune_plib_registry(unmerge=True)` → `_find_libs_to_preserve(unmerge=True)` (`vartree.py:2435`), which returns early when `not self._preserve_libs` (`:3667`). `self._preserve_libs` comes from `:2053` and `:2622-2626`.
- depclean: `actions.py:816-818` (the `revdep-rebuild` advisory) and `actions.py:1361-1382` (the `--depclean-lib-check` decision) both read `"preserve-libs" in settings.features` and `pkg.restrict`.

**Divergence.** `FEATURES=-preserve-libs emerge -C <lib provider still linked>`:
- Portage removes the library;
- portuale preserves it, even after S3 as planned.

**Fix direction.** Either extend S3 to the unmerge path, reusing the same flag (and depclean's two branches), or file the unmerge and depclean halves as residues with real numbers (#409+). Do not leave them as an unmentioned cut.

### D5 — S3 as written turns preserve-libs **off** in every hermetic root, contradicting S3.4's "default-on path untouched" (medium, likely)

**Where.**
- Plan S3 interfaces ("from the resolved config like `ebuild.rs:513-521`") and S3.4.
- `MergeOptions::set_resolved_features` (`ebuild_merge/options.rs:175-188`) re-derives each token from the resolved `FEATURES`.
- With no `make.globals` under the config root, the only seeded default is `unmerge-orphans` (`scalar_layers.rs:162-175`).
- The fixture root has no `preserve-libs` anywhere: pmtest `fixtures/etc/portage/make.conf` has no `FEATURES`, and there is no `usr/share/portage/config/`.

**Divergence.**
- Inputs: any contract or Rust end-to-end test that merges or unmerges through the `emerge` path in the fixture root and expects a preserved library, e.g. pmtest `test_portuale.py:7235` `test_emerge_preserved_libs_advisory_and_rebuild_set` (an unmerge, so it is affected once D4 is fixed).
- Portage with its installed `make.globals` preserves.
- portuale after S3 does not, because resolved `FEATURES` lacks the token.

**Fix direction.** Either add `preserve-libs` to the synthetic base (`make.globals:82`, beside `unmerge-orphans`), or schedule S4 before S3. Reflect whichever is chosen in §1's order and in the S4.3 wording "keep the synthetic base only for …".

### D6 — S4's test lever `PORTAGE_OVERRIDE_EPREFIX` does not exist in portuale, and Portage gives it a much wider meaning (medium, proven)

**Where.** Plan S4.2 ("Use a fixture path override via `PORTAGE_OVERRIDE_EPREFIX` only (no new env var)") and S4.1.

**Facts.**
- `grep -rn PORTAGE_OVERRIDE_EPREFIX crates --include=*.rs` (excluding tests) finds nothing. The name appears only in the whitelist file `ebuild_phases/environ_whitelist.txt:103`, and `phase_env.rs:176` hard-codes `EPREFIX=""`.
- In Portage, `const.py:271-272` turns `PORTAGE_OVERRIDE_EPREFIX` into `portage.const.EPREFIX`. That value then feeds:
  - the default `config_root` (`LocationsManager.py:78`);
  - `eprefix`, and so `EROOT`/`ESYSROOT` (`:70-71`, `:402-411`);
  - `broot` (`:99`);
  - `global_config_path` (`:414-417`);
  - and it is exported back to the environment (`config.py:791-792`).

**Divergence.** Honouring the variable only for `make.globals` would itself be a new divergence: Portage with the variable set also moves `EROOT`, the default config root and `BROOT`. Honouring it fully is Prefix support, which is not an F/M slice.

**Fix direction.**
- Ask the owner (extend §4 Q2) how tests select the global config directory. Portage's own test suite gets hermeticity through `portage._not_installed` → `PORTAGE_BASE_PATH/cnf` (`config.py:467-470`), i.e. the source tree's `cnf/`.
- The closest portuale analogue is an explicit, test-only selection of a fixture or vendored `cnf/make.globals`.
- Note: the embedded `cnf/` tree (`crates/portuale/build.rs:27-32`) currently holds only `cnf/sets/portage.conf`, so there is no embedded `make.globals` to fall back to. S4.1's "the vendored copy of `cnf/make.globals`, if any" should say so: the only copy is `3rdparty/portage/cnf/make.globals`, and it differs from the installed file (the Gentoo ebuild appends `FEATURES="${FEATURES} xattr"`, installed `make.globals:177`).

### D7 — S4 makes every hermetic run read the host's installed `make.globals`: the contract suite and Rust tests become host-dependent (high, proven by source read; the drift size is likely)

**Where.** Plan S4.2/S4.4 ("expect corpus drift … review each drift against a Portage capture").

**Facts.**
- With S4 as specified, `PORTAGE_CONFIGROOT=<fixture>` no longer prevents `/usr/share/portage/config/make.globals` from being read.
- That file sets, among others:
  - `FEATURES` with `sandbox network-sandbox ipc-sandbox pid-sandbox preserve-libs protect-owned config-protect-if-modified merge-wait …` (`:77-84`);
  - `CLEAN_DELAY`, `EMERGE_WARNING_DELAY`, `DISTDIR`, `PKGDIR`, `PORTAGE_TMPDIR`, `BINPKG_FORMAT`, `CONFIG_PROTECT`, `ACCEPT_RESTRICT`, `AUTOCLEAN`, `FETCHCOMMAND`.
- On this host the file exists. On a host or container without Portage (the #326 runtime) it does not. The same suite would therefore produce different outputs on different machines.
- This is the AGENTS.md step-3 case ("if a slice conflicts with a hard constraint (e.g. contract-suite determinism), stop and surface it"). Re-blessing does not cure it.

**Divergence.** Not a Portage-output divergence but a determinism break. On a real system (`PORTAGE_CONFIGROOT=/`) today's code already reads the same path, so the user-visible gain of #384 is limited to `PORTAGE_CONFIGROOT≠/` runs.

**Fix direction.**
- Surface it to the owner before S4.1.
- Make the global-config directory an explicit input whose hermetic value is pinned by the harness (see D6).
- Weigh whether #384's real-world reach (`PORTAGE_CONFIGROOT≠/` only) justifies the size. The backlog already rates it `[A]` (`backlog-tasks-2026-10.md:200`), not F/M.

### D8 — S6.1's `[fetch ]` merge-list marker does not exist in Portage; the real `--pretend --fetchonly` behaviour is different (medium, proven)

**Where.** Plan S6.0 ("`[fetch ]` merge-list marker in `--pretend`/`--ask`") and S6.1. The phrase comes from backlog #401 (`LLM/backlog-tasks.md:117`).

**Portage.**
- `resolver/output.py` has no fetch-only marker: its `fetch` references are fetch-restrict columns and sizes (`:300-329`, `:633-645`).
- What `--fetchonly` changes on the pretend and ask side:
  - the header reads "These are the packages that would be **fetched**…" (`depgraph.py:12096-12097`);
  - the prompt reads "Would you like to fetch the source files for these packages?" (`actions.py:522-523`);
  - `-p -f` does **not** return after display (`actions.py:541-543` excludes fetchonly). It goes through the Scheduler, where `EbuildBuild._start` runs `EbuildFetchonly(pretend=True)` → `doebuild(..., "fetch", listonly=1)` and **prints the URI list** (`EbuildBuild.py:153-175`, `EbuildFetchonly.py:20-36`);
  - `--fetch-all-uri` implies `--fetchonly` (`actions.py:3910-3911`), and `FEATURES=mirror` implies `fetch_all_uri` (`Scheduler.py:192-193`).

**Divergence.** A slice pinning a `[fetch ]` marker would invent output.

**Fix direction.** Replace S6.1's target with the four items above and require S6.0 to capture `emerge -pf` and `emerge -af` (bed or host) before any pin.

### D9 — #405's site list for (c) is incomplete (low, proven)

**Where.** Plan §0 row 405 and S5 "Files".

**Fact.** `args.rs` also hand-writes `requires an argument` at:
- `:1261` (`--with-bdeps`) and `:1304` (`--with-bdeps-auto`);
- `:1912` (`--autounmask-backtrack`), `:1982` (`--autounmask-use`), `:2023` (`--autounmask-license`);
- `:2275` (`--rebuilt-binaries-timestamp`);
- `:2304` (`--quickpkg-direct`), `:2334` (`--quickpkg-direct-root`);
- `:2404` (`-X … can't be …`).

It also has `:1789` (`--load-average` "requires a positive number") and `:1775` (`invalid -j parameter`).

**Fix direction.** Add them to S5.1's argv list. Every one needs its own Portage capture, because argparse `choices` options and `insert_optional_args`-defaulted options produce different texts.

### D10 — stale or wrong references the executor would trip on (low, proven)

- `run_phase_command` (plan S1 "Files"; backlog #408) does not exist. The callers are `run_phase.rs:257` (`run_one_phase_bash`) and `misc_functions.rs:202`.
- `mrg.rs:2654/2759/2951 already null` (S1.1): all three are inside `#[cfg(test)] mod tests` (`mrg.rs:1828`). They spawn the portuale binary from tests and say nothing about production spawns.
- `env_roots.rs:41-45` (§4 Q3): the hook lives in `crates/portage-repo/src/env_roots.rs:72-79`; `:41-45` is `PORTAGE_TMPDIR` code.
- `atom_lines.rs:271-275 already parses the profile-bashrcs format` (S2.0): that is only the constant `USER_PROFILE_FORMATS` for the user-profile node. Per-repo `profile-formats` come from `atom_lines.rs:74-90` and `profile_chain.rs:76`, and nothing reads `package.bashrc`.
- S6.0 names `_sigcheck`, which does not exist in `_emerge/`. `Scheduler._add_prefetchers` is at `:835`, and `EbuildBuild._fetch_failed` at `:350`.
- S6.0 "`--fetchonly` clears resume data": wrong direction. `action_build` makes mtimedb read-only under fetch-only (`actions.py:271-272`; `mtimedb.py:46,94-95`), so existing resume data is left untouched and nothing new is written.
- Backlog #401 claims that "a build action with `--fetchonly` still goes down the normal merge path". This is false. `emerge -f` and `emerge --fetch-all-uri` are refused with `option "--fetchonly" is a Portage emerge option, but is not yet implemented in portuale …` and exit 2 (`pretend/mod.rs:127-145`; run on `target/release/portuale`, built 13:51 today). The plan's §0 is consistent with that, but should correct the backlog text, and `status.rs:95-96`'s "those modes never reach this code" is accurate for the same reason.
- `emerge_build/status.rs:94-95` says "portuale models no `interactive` PROPERTIES". This is stale (T13 shape): `GraphEntry::interactive` exists (`portage-repo/src/resolve/entry.rs:86`, computed at `resolve/pass/graph_entry.rs:156-164`). It matters for G4.

### D11 — S1 interactive predicate: Portage tests the raw `PROPERTIES` tokens, not a resolved value (low, proven)

**Where.** Plan S1 interfaces ("when the resolved `PROPERTIES` contains `interactive`").

**Portage.** `AbstractEbuildProcess.py:143` does `"interactive" not in self.settings.get("PROPERTIES", "").split()`, where `settings["PROPERTIES"]` is the package's raw metadata string (`config.py:2083-2087`). `foo? ( interactive )` therefore counts even with `foo` off. The Scheduler, by contrast, uses the USE-reduced `pkg.properties` (`Scheduler.py:539`).

**Fix direction.** State which one S1 mirrors. Parity for the stdin decision is the raw split.

## G — gaps

- **G1 — S1.2's test cannot discriminate as designed.**
  - Problem: "a pty slave as the parent's fd 0" is not enough. SIGTTIN needs the pty to be the session's **controlling terminal** (probe: only then do A, E and J stop), and `bash -i` reaches it through stderr or `/dev/tty`, not fd 0. Rewriting fd 0 inside the multi-threaded `cargo test` process is also racy.
  - To close: run the case in a child process that calls `setsid()` and `ioctl(TIOCSCTTY)` on the pty slave, e.g. the test binary re-executed with a filter on an `#[ignore]`d inner test. Assert that `bash -i -c true` with stderr on a pipe finishes (D1 case A today, case F after), and that a `config`-phase `read` gets the data (case J vs K).
- **G2 — S1 "no oracle needed beyond the cited Portage source"** (§1) contradicts the plan header and is how D1 slipped through. To close: an S1.0 capture of Portage on the bed or host building a fixture ebuild whose `src_test` runs `bash -i -c true`, and one whose `pkg_config` does `read`, both on a real pty, with pid-sandbox on and off. Record it in `../pmtest/differential-test-bed/findings/l2.md` (or `l3.md`).
- **G3 — the S1 audit list misses `--config`, merge-time `preinst`/`postinst`/`prerm`/`postrm`, and the scheduler's interactive handling.**
  - The first group all go through `spawn_trackable` (`run_phase.rs:56`), and Portage runs them in emerge's group (`doebuild.py:141`).
  - Portage also forces foreground mode, `--jobs=1` and the "Sending package output to stdio due to interactive package(s)" notice when any merge task is interactive (`Scheduler.py:483-519`). `scheduler_status_mode` (`status.rs:116-`) ignores interactive, so an interactive package under `-j`/`--quiet-build` would get a captured, invisible prompt.
  - To close: add these to S1.1, and either implement the Scheduler branch or file it with a number.
- **G4 — signal delivery after the change.** With `setsid()` (D1), a terminal `^C` reaches portuale only, not the phase. Portage under pid-sandbox has the same shape, with `pid-ns-init` forwarding SIGINT, SIGTERM, SIGHUP, SIGTSTP and SIGCONT (`pid-ns-init:27-33,121-122`). To close: a test that SIGINT to portuale during a phase terminates the phase group (the registry kill), plus a note on SIGTSTP/`^Z` behaviour.
- **G5 — S2 omits `PM_EBUILD_HOOK_DIR`.** `doebuild.py:538-540` sets it beside `PORTAGE_BASHRC`, and `ebuild.sh:439-441` sources `/etc/portage/env/<cat>/<pn>[:slot]` hook files from it. portuale sets neither (grep is empty), so those hooks are equally a silent no-op. To close: include it in S2.2 with a fixture `etc/portage/env/<cat>/<pn>` hook, or file it as #409.
- **G6 — S2 omits the depend phase.**
  - Facts: `EbuildMetadataPhase` calls `doebuild(..., "depend")` (`EbuildMetadataPhase.py:135`), which sets `PORTAGE_BASHRC`, and `ebuild.sh:615-620` sources the user bashrc during depend (profile bashrcs are skipped, `:428`). portuale's depend spawn is `depend_phase.rs:58-61`.
  - To close: decide and test whether metadata generation sources `/etc/portage/bashrc`. Portage does.
- **G7 — S2.0 should pin the exact `_pbashrc` order from the source** instead of leaving it to the oracle prose:
  - for each node of `profiles_complex` (the user profile `etc/portage/profile` last, `LocationsManager.py:170-189`), `profile.bashrc` if the file exists, whatever `profile-formats` says (`config.py:850-853`);
  - then that node's `package.bashrc` matches, by atom specificity, read only when the node has `profile-bashrcs` (`config.py:921-947`, `:2008-2022`);
  - files resolve under `<profile>/bashrc/<name>`.
- **G8 — the merge-path gate is under-specified.**
  - §1 says "merge glibc and bash". `LLM/agent-context.md:398-430` requires the `l1-merge-gate.txt` cell **with** `L1_CONSUME_REINSTALL=1`, and for any change to the unmerge half (S3 decides which old files survive the replace loop) also the third run with `L1_CONSUME_INSTALL_MASK`. Name all three for S3 and S8 #380.
  - S4 changes which `FEATURES`/`CONFIG_PROTECT`/`UNINSTALL_IGNORE` a merge sees whenever `PORTAGE_CONFIGROOT≠/`, so it needs at least the L1 gate.
- **G9 — wrong bed for S1/S2.**
  - `l1-merge-from-binpkg.sh` (§1) merges binary packages and compiles nothing, so it never exercises `src_*` spawns. The phase-spawn bed is `l2-portuale-builder.sh` (with `l1-porttest.txt`, as recorded in the #336 lessons) or `l3-source-parity.sh`.
  - Neither bed has a tty, so they are regression checks only. #408 itself needs G1's test and G2's capture.
- **G10 — resolver-touching items have no L0 bed run.** AGENTS.md step 8 requires `l0-resolver.sh` after resolver changes. #375, #396, #391, #394 and #402 (S7/S9) are resolver changes, but §1's gate omits L0.
- **G11 — S6.0 checklist is missing Portage facts S6.2 depends on.** Under fetch-only:
  - the Scheduler keeps scheduling after a failure (`Scheduler.py:1918-1923`);
  - the `--keep-going` resume loop does not apply (`:1279`);
  - blockers are ignored (`:77-79`, `depgraph.py:9268`);
  - `pkg_pretend` is skipped (`:1744`);
  - the strict manifest pre-check is skipped (`:793-796`);
  - parallel-fetch is off (`:324-327`);
  - output is never backgrounded (`:81`);
  - `_check_temp_dir` and the build-dir lock are skipped (`EbuildBuild.py:45`);
  - a failed fetch spawns `pkg_nofetch` without a build dir and returns 1 (`EbuildBuild.py:231-256`);
  - world is untouched (`Scheduler.py:2555-2562`);
  - neither vardb nor bindb is opened for write (`actions.py:571-578`).

  Each needs a capture row; the "vdb byte-identical" assertion alone does not cover them.
- **G12 — difficulty labels contradict the backlog.** #383, #384, #372 and #405 are `[A]` in `backlog-tasks-2026-10.md:199,200,206,216`, but the plan rates them F, F/M, M and F/M. After D3–D7, S3 and S4 are not F. Re-rate, or say why the backlog rating is wrong.

## P — process deviations

- **P1 — §0 marks #408 "Confirmed"** on a source read of one side only. The plan header's oracle-before-code rule and the review rule "a slice claiming a Portage expectation with no capture is a gap" both apply. The confirmation inherited the backlog's root cause, which D1 disproves.
- **P2 — evidence locations are inconsistent.**
  - S1 writes to `docs/evidence/2026-10-18-sigttin/after.md`, while S2 and S6 write to `LLM/evidence/2026-10-18-*/`.
  - Bed cells (S2.3, S3.1, S6.3) do not say they are recorded in `../pmtest/differential-test-bed/findings/*.md`.
  - Pick one evidence root for this batch and require the findings entry for every bed cell.
- **P3 — §1 orders S3 before S4 although S3's correctness depends on S4's decision** about the synthetic base (D5). Also, "S2 after S1, same spawn code" is not a real dependency: S2 touches `phase_env_vars`, not the spawn. The order is harmless, but the stated reason is wrong.
- **P4 — the pmtest worktree is not clean at the planned branch point** (untracked `pg359-*` fixtures). The plan's baseline-by-test-name rule needs a clean or explicitly stashed tree first. Add a step 0 to the branch rule.
