# Backlog Tier 1, sliced: what is still open and how to finish it

> **Historical planning/investigation doc, retired 2026-09-15.** Its outcome is `docs/backlog-tasks.md`'s status line for this item; the extracted ground truth, gotchas and dead ends are in [`../recap-of-backlog-ops-2026-09-15.md`](../recap-of-backlog-ops-2026-09-15.md). Kept verbatim below for citation/provenance only.


Status: proposed. Written 2026-09-14 against `main` @ `5a1f329`.

**Progress 2026-09-14:** Track F committed on `main`: F1a, F1b, F2, F3
steps (1)+(2), F4 and F5 done; owner decisions registered D3 =
deterministic suffix (F3 step (3), landed in `9f0e477`), D4 = shuffle stays
a deliberate cut; F4's free-space check is `79ce808`. F5's oracle (real
`fetch(..., listonly=1)` on this host) showed a file's literal URIs are
tried **last-listed first**. **Track B complete (B0–B5):** `vivo75/brush`
`main` was rebuilt on upstream `25bffd54` (`b9524ad5`) with five per-bug
branches force-pushed (`bc99e6c1`, `df830c59`, `962051c9`, `dfbca97c`,
`2073877d`); portuale is re-pinned and verified (workspace
`cargo test --release`, full pytest, brush compat 0 unexpected failures,
eclass sweep 0 failures incl. quoted-tag synthetics, #38 G3 smoke non-empty
under `--shell brush`). B6 (open the five PRs) stays user-owned; B7 is
upstream-blocked.

Scope: the open entries of **Tier 1** in
[`backlog-tasks.md`](backlog-tasks.md) ("focused slices, ~one sitting
each"). An LLM-oriented mirror of this file is
[`backlog_tier_1_sliced.babeltele.md`](backlog_tier_1_sliced.babeltele.md).
If the two disagree, this file wins.

**Read first:** `AGENTS.md` (the rhythm; step 8 is the verification
pass), [`agent-context.md`](agent-context.md),
[`brush-pin.md`](brush-pin.md), [`brush-pr/README.md`](brush-pr/README.md),
and the `rust/portage-fetch/src/lib.rs` module doc.

Line numbers drift. **Find things by symbol name before you trust a
line number.**

Model tiers follow the #22–#38 plans:

| Tier | Meaning | Examples |
|---|---|---|
| **F** | frontier | Claude Opus 5 / Fable 5.1 |
| **M** | mid | Claude Sonnet 5 |
| **S** | small | Claude Haiku 4.5 |

"F review" means a frontier model reads the full diff before the user
is asked to commit, whoever wrote it.

---

## 1. Where Tier 1 stands

Tier 1 had 16 entries. The 2026-09-13 audit found that commit
`67f3b0e` had shipped most of them without updating the list. I
spot-checked the "DONE" claims against the code again today, and they
hold. For example, `rust/Cargo.toml` has `lto = "thin"` and
`codegen-units = 1`, `expand_package_env_files`,
`slot_conflict_caret_idx`, `colorize_marked_spans` and
`find_hard_cycles` all exist, and `elog.rs` carries `mail_summary`.

| # | Entry | State |
|---|---|---|
| 1–4, 7–13, 16 | lto, solver docs/markers/notices, compress-build-logs, elog mail, env.d eroot, `${VAR}` expansion, depend-phase RESTRICT, standalone package.env, resumed binaries, slot-conflict colour | **done** |
| 15 | `:=` literal-bound atom validation | **not a gap** (decided 2026-09-10) |
| **5** | brush: submit the staged upstream fixes | **open** |
| **6** | brush: recurring re-pin, plus the brush `src_compile` no-op found in #38 G3 | **open** |
| **14** | fetch: remaining `RESTRICT=primaryuri` / candidate-order cuts | **open** |

Three items are left, and they fall into two independent **tracks**:

- **Track B (brush)** covers #5 and #6. They are the same work: bring
  the embedded shell to a state where the fixes can go upstream and
  the pin can move.
- **Track F (fetch)** covers #14.

Neither track touches the resolver, so neither needs a Python mirror
or `CASES` entries. Both are real-execution only (AGENTS.md step 4
carve-out).

---

## 2. What investigating these items turned up (read before slicing)

The backlog wording undersells both tracks. Four findings change the
plan. Each one was reproduced today, and the commands are included so
anyone can re-check them.

### 2.1 The brush `src_compile` no-op has a root cause: a fourth `declare -f` bug, in our own staged patch

`#6` records that `porttest/splitdebug` builds an **empty image with
rc 0** under `--shell brush`. The cause is not `tc-getCC` or the
toolchain eclass. It reproduces with a copy of the ebuild that inherits
nothing (`inherit` dropped, `$(tc-getCC)` replaced by `${CC:-gcc}`):

```
$ ebuild --shell bash  <sdnoinherit-1.0.ebuild> compile   # work/ has pt-splitdebug, libptsd.so.0.0.0
$ ebuild --shell brush <sdnoinherit-1.0.ebuild> compile   # work/ empty, rc 0
error: …/temp/environment: unterminated here document sequence; tag(s) ['EOF'] …
```

Here is what happens. The phase saves the ebuild environment with
`declare -f`. brush then writes the here-document terminator of a
**quoted** tag with its quotes still on:

```
$ brush --norc --noprofile -c "$(printf 'f() {\n\tcat > a.c <<-'"'"'EOF'"'"'\n\t\tint x;\n\tEOF\n}\ndeclare -f f')"
f ()
{
    cat > a.c <<-'EOF'
int x;
'EOF'          <- bash prints EOF here
}
```

The next phase's `source "${T}/environment"` cannot parse that file.
The ebuild's own `src_compile` is lost and the `default` one runs,
which does nothing. The saved env afterwards shows
`src_compile () { default }`.

The bug is **in our own staged fix 02**
(`fix/declare-f-heredoc-serialization`, `3d2bde47`). The new
deferred-body code emits `here_doc.here_end.value`, which is the raw
tag word with quotes included, where it should emit the
quote-removed delimiter (`brush-parser/src/ast.rs`, the
`block.push_str(&here_doc.here_end.value)` line, ~1733 in the pin).
The 211-eclass / 1843-function round-trip sweep did not exercise a
quoted tag. **Patch 02 must be fixed before anyone opens its PR.**

### 2.2 A broken saved environment does not fail the phase under brush

With bash, `source` of an unparseable file returns 2, so real
`bin/ebuild.sh`'s `source "${T}"/environment || die "error sourcing
environment"` dies. The same situation under brush looks different:

- standalone `brush -c 'source bad.sh || echo FAILED; echo continued'`
  aborts the whole script (rc 2). The `||` branch never runs.
- inside portuale's embedded runner (`run_one_phase_brush`), the phase
  **continues and exits 0**. That runner only maps `Err` from
  `run_string` / `source_script` and ignores a non-zero
  `ExecutionResult`.

So 2.1 turned into silent wrong output where it should have been a
loud failure. That is a problem of its own, independent of 2.1.

### 2.3 The `env: ''` and `readonly variable` noise under brush also has a cause

`bin/phase-functions.sh`'s `__filter_readonly_variables` finds bash's
special variables by running `env -i -- "${BASH}" -c 'printf …'`. The
brush smoke log shows `env: '': No such file or directory`, which
means `$BASH` is empty in the embedded shell. The special-variable
list therefore comes back empty, `BASHOPTS` / `EUID` / `PPID` /
`SHELLOPTS` / `UID` are saved into `${T}/environment`, and every
re-source prints `declare: cannot mutate readonly variable`. The
standalone brush binary does set `BASH`. The embedded
`brush_core::Shell` used by portuale apparently does not. Step 1 of B3
confirms this.

### 2.4 The `GENTOO_MIRRORS` fallback does not work against the real Gentoo mirrors

The `portage-fetch` module doc says real portage's `layout.conf`
negotiation can be skipped because flat is "what the real, well-known
`GENTOO_MIRRORS` entries … actually use". **That is false today:**

```
$ curl -s http://distfiles.gentoo.org/distfiles/layout.conf
[structure]
0=filename-hash BLAKE2B 8

$ curl -sI …/distfiles/which-2.23.tar.gz      -> 404   (portuale's flat URL)
$ curl -sI …/distfiles/80/which-2.23.tar.gz   -> 200   (real's filename-hash URL)
```

This host's own `/var/cache/distfiles/.mirror-cache.json`, written by
real portage, records `filename-hash BLAKE2B 8` for all four mirrors it
has used. So portuale's public-mirror fallback, and therefore every
`mirror://gentoo/…` and every dead-upstream download, can never
succeed against a stock `GENTOO_MIRRORS`. #14 files the `layout.conf`
negotiation as a harmless cut. **It is the most important functional
gap in Tier 1**, and slice F1 fixes it first.

Two smaller facts from reading `fetch_src_uri` against real
`fetch.py::fetch`:

- **Duplicate attempts.** For a `mirror://` URI, `assemble_candidates`
  lists the third-party expansions twice (inline, then again in the
  primary-uri tail). Real does the same but skips repeats with
  `tried_locations`. portuale has no such set, so every unreachable
  third-party mirror is tried twice.
- **Local-path `GENTOO_MIRRORS`.** Real puts `/`-rooted
  `GENTOO_MIRRORS` entries in `fsmirrors` (copied with
  `shutil.copyfile`). portuale's `gentoo_mirror_fallback` does not
  filter them, so it hands `wget` a bare filesystem path such as
  `/mnt/mirror/distfiles/<f>` as a URL. `assemble_candidates` filters
  `/` only for `custommirrors["local"]`.

---

## 3. Track B — brush (#5 + #6)

**Goal:** the brush backend builds a compiled ebuild the same way bash
does, a broken saved environment fails loudly, the four upstream fixes
are correct and rebased on current upstream, and the PRs can be opened.
Flipping `--shell`'s default back to `brush` is **out of scope**. That
stays a separate owner decision after the PRs land.

**Invariants:**

- Fix brush bugs **in brush** (`3rdparty/brush`, the per-bug `fix/*`
  branches), never by working around them in portuale.
- The only portage-tree workaround pattern allowed is the existing
  "brush strategy #2" (rewrite a `bin/*.sh` construct), and only with
  user sign-off.
- Every slice leaves `--shell bash` (the default) byte-unchanged.
- Opening PRs, pushing to `vivo75/brush`, and commenting on
  `reubeno/brush#1276` are outward-facing actions. **The user does
  them, or explicitly authorises each one.**

### B0 — Fix the stale pin records (S, ~30 min, docs only)

The pin moved to `vivo75/brush@67c301a7` on 2026-09-10, but five places
still describe older states:

| Where | Says | Should say |
|---|---|---|
| `rust/portuale/Cargo.toml` comment above the `brush-core` line | "The fork (`vivo75/brush`) is gone" | thin fork = upstream + staged fixes (as `brush-pin.md`) |
| `3rdparty/repos.toml` `[brush]` comment | "No fork: plain upstream `main`" | same |
| `docs/brush-pr/README.md` | "portuale is pinned to … `5af3f6c1` (commit `8184c11`)" | `67c301a7`; base of each `fix/*` branch |
| `docs/brush-pin.md` "Root-caused + fixed 2026-09-05" | "All three are now in the pin (`vivo75/brush@5af3f6c1`)" | point to "Current pin" |
| `tests/test_portuale.py` `test_ebuild_install_does_not_deadlock_on_a_large_eclass_scope` docstring | "since fixed in the pinned fork" | name the staged fix 03 / upstream #1276 |

**Accept:** a grep for `5af3f6c1` and for "fork is gone" / "No fork"
turns up only history. No code changes.

### B1 — Fix bug 4: `declare -f` quoted here-doc terminator (M, F review, 2–4 h)

1. **Confirm which revision has the bug.** Build `fix/declare-f-heredoc-serialization`
   (`3d2bde47`) on its own, and the pin `67c301a7`. Run the minimal
   repro from §2.1 on both. The expected result is that both are
   broken, because the code comes from 3d2bde47. If only the pin is
   broken, it is a merge regression. In that case, stop and re-plan
   with the user.
2. Fix in `brush-parser/src/ast.rs`: the deferred terminator line
   must be the **quote-removed** delimiter (bash rule: any quoting in
   the tag, `'EOF'` / `"EOF"` / `\EOF` / `E"O"F`, is removed for the
   terminator, and the command line keeps the quoted form). Put the
   change in a new commit on the same branch, or amend it (user's
   call, since the branch is already public on `origin`).
3. Add `brush-compat-tests` cases in
   `brush-parser/tests/cases/compat/builtins/declare.yaml`. Cover
   `<<'EOF'`, `<<"EOF"`, `<<\EOF` and `<<-'EOF'` with a tab-indented
   body. Each case asserts `declare -f` equals bash **and** that
   `declare -f | source` round-trips idempotently.
4. Extend the eclass sweep. Add a synthetic function per quoting form,
   since real eclasses apparently have none, then re-run the
   211-eclass sweep.
5. Cherry-pick onto `vivo75/brush` `main`, re-pin portuale to it
   (checklist in `brush-pin.md`), and refresh
   `docs/brush-pr/patches/02-*.patch` and `02-*.md`.

**Accept:** the §2.1 repro prints `EOF`. brush's compat suite has
0 new failures. `ebuild --shell brush` on the inherit-free splitdebug
copy produces `pt-splitdebug` and `libptsd.so.0.0.0` (the portuale
end-to-end pin lives in B4).

### B2 — Make a failed `source` of the saved env fail the phase (F, 3–6 h)

1. Characterise brush's behaviour when a `source`d file has a parse
   error, in three places: at top level, inside a function, and on
   the left of `||`. Compare with bash (returns 2, and `||` runs).
   This is an upstream brush divergence. Write it up the same way as
   `brush-pr/0N-*.md`.
2. Decide where to fix it. There are two options, and both may be
   needed:
   - **(a) upstream:** brush's `source` returns a non-zero status on a
     parse error instead of aborting the enclosing script. This makes
     real `ebuild.sh:580`'s `|| die` fire, which is the faithful fix.
   - **(b) portuale defence:** `run_one_phase_brush` treats a non-zero
     `ExecutionResult` from `run_string(setup)` and
     `source_script(ebuild.sh)` as a phase failure instead of carrying
     on to `invoke_function("__ebuild_main")`.

   Recommendation: do (a) as the fix-5 branch and (b) now, because (b)
   is a correctness guard that stays valid after (a) lands. Check with
   bash that (b) cannot fire on a healthy run.
3. Add a portuale regression test. Run `ebuild --shell brush <fixture>
   unpack`, corrupt `${T}/environment` (for example with an
   unterminated here-doc), then run `compile`. **Both** shells must
   exit non-zero with real's "error sourcing environment" die. With
   bash that test should already pass, so it acts as the control.

**Accept:** the regression test is green for both shells, and no brush
phase test changes behaviour on a healthy environment.

### B3 — Give the embedded shell a `$BASH` so `__filter_readonly_variables` works (M, 2–3 h)

1. Confirm the cause: print `${BASH}` from a brush phase (e.g. a
   fixture `pkg_setup` that `einfo`s it). The expected output is
   empty.
2. Choose what `BASH` should be. `__filter_readonly_variables` wants a
   "hygienic instance of bash" to list the special variables. The
   options are:
   - (i) set `BASH` to the real `bash` path. That lists **bash's**
     specials, which brush emulates.
   - (ii) set it to the portuale binary, if there is a
     brush-as-subprocess entry point.

   (i) matches real portage's intent and is the recommendation.
   `BASH` is set by the shell, not taken from the environment, so this
   happens inside `run_one_phase_brush`'s setup, not through
   `phase_setup_script`'s env export. If brush lets `BASH` be set,
   also consider an upstream issue: an embedded `Shell` has no `$BASH`.
3. Test: after `ebuild --shell brush <phasepkg> install`, the saved
   `${T}/environment` contains no `declare -r`/`declare --` lines for
   `BASHOPTS`, `EUID`, `PPID`, `SHELLOPTS` or `UID`, and stderr has no
   `env: ''` or `cannot mutate readonly variable` lines.

**Accept:** the `_l2-brush-smoke` noise lines are gone. The bash path
is unchanged.

### B4 — End-to-end pin: a compiled here-doc ebuild under both shells (S/M, 1–2 h, after B1–B3)

Add a fixture to `fixtures/repo` shaped like the inherit-free
splitdebug copy. It needs a `src_compile` with a `<<-'EOF'` heredoc
that compiles a C file with `${CC:-gcc}` (skip with an explicit reason
if no compiler is present). Extend
`test_ebuild_shell_bash_and_brush_produce_the_same_real_result`, or add
a sibling test, that asserts the same `image/` file set for both
shells. `git add` the fixture before any `git clean -fdq fixtures/`
(see the memory note). Then, in the L2 container, re-run the #38 G3
smoke (`emerge --shell brush --buildpkgonly porttest/splitdebug`) and
append the result to `TEST/findings/l2.md` "#38 S2" and to
`brush-pin.md` "What is *not* tracked here" (mark it resolved).

**Accept:** the new test is green, the container smoke shows a
non-empty image, and the `brush-pin.md` 2026-09-13 bullet is closed
with evidence.

### B5 — Rebase the fix branches onto upstream `main` + re-pin (#6's recurring bump) (M, 2–4 h)

Upstream `reubeno/brush` `main` was `25bffd54` on 2026-09-14
(`git ls-remote`). The pin's upstream base is `812336dd`, and every
`fix/*` branch is still based on `a250b84e`, which is far behind.

1. `git fetch upstream` in `3rdparty/brush`. For each of the four or
   five fix branches (tokenizer 01, declare-f 02 with B1 in it,
   deadlock 03, and source-status 05 if B2(a) was done), rebase onto
   `upstream/main` and resolve conflicts.
2. Check each branch against upstream in isolation: `cargo test -p
   brush-parser -p brush-core`, `cargo clippy`, and compat tests with
   0 new failures.
3. Rebuild `vivo75/brush` `main` as upstream plus the rebased fixes,
   then re-pin portuale (Cargo.toml both crates, `Cargo.lock`,
   `repos.toml`, `brush-pin.md` "Current pin"). Run the checklist:
   fmt, clippy with 0 warnings, `cargo test --release -p portuale`
   including the deadlock guard and B2/B4, plus the pytest brush tests.
4. Re-export `docs/brush-pr/patches/*.patch` and update
   `brush-pr/README.md` (bases, commit ids, verification numbers).

**Accept:** each fix is exactly one commit on current `upstream/main`
and passes brush's own suite alone. portuale is green on the new pin.
**Force-pushing `origin` branches needs the user's go-ahead.**

### B6 — Open the upstream PRs (user-owned; agent prepares, ~1 h agent + user time)

This is blocked on B1–B5 and on the #1276 decision (§6 D1). The agent
prepares one `gh pr create -R reubeno/brush --head vivo75:<branch>`
command per fix, each with a body adapted from `brush-pr/0N-*.md`
(root cause, minimal repro, bash comparison, tests added). The user
reviews and runs them. #1276 (OPEN, from the old
`fix/pipeline-function-stage-deadlock2` head) is either updated or
closed as superseded by fix 03, per D1.

**Accept:** PR URLs are recorded in `brush-pr/README.md` and
`brush-pin.md`. Backlog #5 becomes "PRs open, awaiting upstream".

### B7 — After upstream merges: drop the thin fork (S, 1 h, externally blocked)

Once all fixes are in `reubeno/brush` `main`, re-pin to upstream
directly, delete the thin-fork wording from the five B0 locations, and
close #5. Then file the `--shell` default flip as its own owner
decision. It is **not** part of this plan. #6 stays a recurring entry.

---

## 4. Track F — fetch (#14)

**Goal:** portuale's distfile candidate list and retry behaviour match
real `fetch.py::fetch` on every *deterministic* axis, and the public
mirror fallback actually works against real Gentoo mirrors.

**Invariants:**

- Real-execution only: no Python mirror, no `CASES`.
- Keep downloads in `portage_fetch::download_via_wget`, a real `wget`
  subprocess. Never an in-process HTTP client.
- **Expected orders come from real portage**, not from reading
  `fetch.py`. The oracle is real `emerge -pf <atom>` (real `listonly`
  prints every candidate URI in try-order) or a direct
  `portage.package.ebuild.fetch.fetch(..., listonly=1)` call, both on
  the container image. To keep comparisons deterministic, use
  `thirdpartymirrors` entries with **one** URL each so the shuffle
  cannot reorder them.
- Unit tests must not touch the network. Use the existing
  `serve_once` / `closed_port` local-server helpers and a pre-seeded
  `.mirror-cache.json`.

### F1 — Mirror `layout.conf` support (the functional gap from §2.4)

Split in two so the pure part can land and be reviewed alone.

**F1a — layout math (M, 2–3 h), `rust/portage-fetch`.**
Port `FlatLayout`, `FilenameHashLayout` and `ContentHashLayout`
(`get_path`, `verify_args`) plus `MirrorLayoutConfig` (parse the
`[structure]` `0=`, `1=`, … keys, `validate_structure`, and
`get_best_supported_layout` with its flat fallback). `BLAKE2B` and
`SHA512` are already dependencies (`blake2`, `sha2`). Hash names follow
real `checksum_str`. `content-hash` needs the file's Manifest digest,
which `DistfileDigests` already has. Oracle values come from real
Python, e.g.
`FilenameHashLayout('BLAKE2B','8').get_path('which-2.23.tar.gz')` →
`80/which-2.23.tar.gz`. Pin several filenames and cutoffs (`8`, `8:8`,
`16`), plus an invalid structure (cutoff not a multiple of 4, unknown
algorithm) that must fall back to flat.

**F1b — negotiation + cache (F, 4–6 h), `rust/portuale/src/fetch.rs`.**
Port `async_mirror_url`:

- **Cache.** Read `${DISTDIR}/.mirror-cache.json`, which is
  `{mirror_url: [ts, [[layout args…]…]]}`. If the entry is younger
  than 86400 s, use it. Otherwise fetch
  `<mirror>/distfiles/layout.conf` into `${DISTDIR}/.layout.conf.<host>`
  with the same wget path and no mirrors. A `/`-rooted mirror instead
  reads `<dir>/layout.conf` directly. On success, write the cache
  atomically. On any failure, use flat and write nothing.
- **Shared file.** The cache is used by real portage in the same
  `DISTDIR`, so the JSON must stay **format-compatible in both
  directions**. Test that portuale reads a real-written file (like
  this host's) and that real reads a portuale-written one (container).
- **Quoting.** URL-quote the path for `ftp`/`http`/`https` mirrors
  only (real `urlquote`), not for others.
- **Where it applies.** `local_mirrors` and `public_mirrors`
  candidates (steps 1–2 of `assemble_candidates`) become
  layout-resolved. `mirror://` expansions and literals do not change.
  Real resolves lazily (`functools.partial`) only when a candidate is
  reached. Mirror that, so a file fetched from its first candidate
  never triggers a `layout.conf` download.
- **Time.** The TTL is wall-clock based. Put `now` behind a parameter
  so tests are deterministic, and only the call site reads the clock.
- **Docs.** Correct the false "flat is what `GENTOO_MIRRORS` use" text
  in the `portage-fetch` module doc and the `fetch.rs` doc comments,
  citing §2.4's evidence.

**Accept:** unit tests cover the cache hit, the stale cache → local
server `layout.conf` → hashed path fetched, the unreachable
`layout.conf` → flat, and cache round-trips. A container check against
real `distfiles.gentoo.org`: with the literal `SRC_URI` made
unreachable, portuale fetches `which-2.23.tar.gz` from
`…/distfiles/80/…`, and real `emerge -pf` lists the same URL.

### F2 — Skip repeated candidates (`tried_locations`) (S, ~1 h, independent, can go first)

In `fetch_src_uri`'s candidate loop, skip a candidate already tried
for this file, exactly as real `if loc in tried_locations: continue`.
Keep `assemble_candidates`' list shape, duplicates included, because
real builds it that way too. Only the attempts get de-duplicated.
Test: a `mirror://` URI whose single third-party root is a
`closed_port` gets exactly one connection attempt, with the error list
naming it once.

### F3 — Checksum-failure semantics (M, 2–4 h, after F2)

Real `fetch.py` on a digest mismatch does three things:

1. It counts `checksum_failure_count`. At the 2nd failure
   (`checksum_failure_primaryuri = 2`) it appends the reversed
   primary URIs to the remaining list, which is "switch to primaryuri
   mode".
2. It stops after `PORTAGE_FETCH_CHECKSUM_TRY_MIRRORS` failures
   (default 5, and bad values are warned about and replaced with the
   default).
3. It **renames** the bad file via `_checksum_failure_temp_file`
   (`<file>._checksum_failure_.<random>` in `DISTDIR`) and prints
   `Refetching... File renamed to '…'`.

portuale currently deletes the bad file and tries every candidate.
Port (1) and (2), which are deterministic. For (3), see §6 D3
(**decided 2026-09-14: deterministic suffix**).
Real's temp name is random, so the recommendation is to port the
rename with a **deterministic** suffix and note that as the one
documented divergence, or to keep deletion as a cut. The user decides.

**Accept:** a local server that serves bad bytes on N candidates gives
the exact real attempt count and order. The primary-uri switch is
pinned with a fixture where the literal is only reached after the
switch.

### F4 — On-filesystem mirrors (`fsmirrors`) (M, 2–3 h, after F1a)

- **Copy step.** Before any remote candidate, and only when the file
  is absent and there is space, try each `fsmirrors` entry in real
  order: `custommirrors["local"]` `/` entries first, then `/`-rooted
  `GENTOO_MIRRORS` entries. Resolve the path with that directory's own
  `layout.conf` (F1a/F1b), copy with a plain file copy, print real's
  `Local mirror has file: <f>`, and stop at the first hit. The copied
  file still goes through normal digest verification.
- **Bug fix from §2.4.** `/`-rooted `GENTOO_MIRRORS` entries leave
  `gentoo_mirror_fallback` and never reach `wget`.

**Accept:** a unit test with a temp-dir mirror under `flat` and under
`filename-hash` layouts, and a test that a `/`-rooted `GENTOO_MIRRORS`
entry produces no `wget` candidate.

### F5 — Multi-URI-per-file grouping (M, F review, 3–5 h, after F2)

Real builds **one** list per *filename*
(`filedict: OrderedDict[filename → uris]`). The local and public
mirror lists are added once, when the filename is first seen. Each
`SRC_URI` entry naming that filename then adds its `mirror://`
expansions in order. Primary URIs are collected per filename,
**reversed** (`uris.reverse()`), have the third-party URIs appended,
and are then merged at the head (`primaryuri`) or the tail.

portuale loops per entry. With two entries for one filename, it
re-tries the shared mirror lists, and it gets the relative order of
the two literals wrong under both modes.

1. Build the per-filename grouping inside `fetch_src_uri` (or change
   `assemble_candidates` to take the group).
2. **Get the expected orders from the oracle,** i.e. real `emerge -pf`
   on a two-URI fixture, both with and without `RESTRICT=primaryuri`.
   Don't derive them from the `reverse()` comment, which is easy to
   misread.
3. Check that `A` (exported from `fetch_src_uri`'s returned filenames,
   `ebuild_phases.rs` `extra_env.push(("A", …))`) lists a
   multiply-sourced file once, as real does. If it lists it twice,
   fix it in this slice. Also cross-check against #45 (the vdb
   `environment` carries `A` with the distfile three times), because
   it may be the same root cause or a separate one.

**Accept:** the oracle-derived order is pinned for 2-URI and 3-URI
files in both modes, and `A` is de-duplicated.

### F6 — Third-party mirror shuffle: record the cut (S, 30 min, docs)

Real `random.shuffle`s the `thirdpartymirrors` expansions. portuale is
a deterministic tool, and the other determinism cuts are listed under
"Deliberate cuts" in `backlog-tasks.md` and in `scope-backlog.md`
Part 3. Move the shuffle there with its reason: load balancing only,
every candidate is still digest-verified, and try-order is not
observable in the result. Recommended unless the user wants an
opt-in, seeded shuffle (§6 D4). **Decided 2026-09-14: deliberate cut.**

---

## 5. Order, dependencies and effort

```
Track B:  B0 ─┐
              ├─ B1 ─┬─ B4 ─ B5 ─ B6 (user) ─ B7 (upstream-blocked)
              │  B2 ─┤
              │  B3 ─┘
Track F:  F2 ─ F3
          F1a ─ F1b ─ F4
          F2 ─ F5
          F6 (anytime)
```

- The two tracks don't interact. They can run in parallel in separate
  sessions or worktrees.
- B1, B2 and B3 are independent of each other. B4 pins all three
  together, so it waits for them. B5 must come after B1 and B2(a)
  because it rebases those commits.
- F1a before F1b before F4 is a real dependency (layout math →
  negotiation → local-dir layouts). F2 is the cheapest win and makes
  F3/F5 attempt counts meaningful.

| Slice | Tier | Estimate | Container needed? |
|---|---|---|---|
| B0 | S | 0.5 h | no |
| B1 | M (F review) | 2–4 h | no |
| B2 | F | 3–6 h | no |
| B3 | M | 2–3 h | no |
| B4 | S/M | 1–2 h | yes (G3 smoke re-run) |
| B5 | M | 2–4 h | no |
| B6 | user | — | no |
| B7 | S | 1 h | no |
| F1a | M | 2–3 h | no (Python oracle on host) |
| F1b | F | 4–6 h | yes (real-mirror check + cross-read of cache) |
| F2 | S | 1 h | no |
| F3 | M | 2–4 h | optional |
| F4 | M | 2–3 h | no |
| F5 | M (F review) | 3–5 h | yes (`emerge -pf` oracle) |
| F6 | S | 0.5 h | no |

Total is roughly 27–45 agent-hours, plus the upstream wait for B6/B7.
Tier 1's "one sitting each" label no longer fits #14. Keep F1b in
Tier 1 because it is a functional bug, but consider moving F3 and F5
to Tier 2 if the owner prefers.

**Suggested first session:** B0 + F2 (both S, both near-zero risk),
then B1. B1 unblocks the brush smoke and gates the PR work.

---

## 6. Decisions for the owner

| # | Decision | Options | Recommendation |
|---|---|---|---|
| D1 | `reubeno/brush#1276` (OPEN, old deadlock fix) | (a) force-push fix 03 onto its head branch, (b) close it as superseded and open fix 03 fresh | **(b)**: fix 03 is a re-do on a different branch and base, and a fresh PR with the new write-up is cleaner for the reviewer |
| D2 | B1 on an already-pushed branch | amend `3d2bde47` vs add a follow-up commit | **amend** before the PR is opened (one commit per fix is the stated structure), which needs a force-push to `origin` |
| D3 | Checksum-failure rename (F3 step 3) | port with a random suffix / port with a deterministic suffix / keep deleting (cut) | **DECIDED 2026-09-14 (owner): deterministic suffix.** Keeps real's "evidence kept in `DISTDIR`" behaviour without non-determinism; the suffix is the one documented divergence |
| D4 | Third-party shuffle (F6) | deliberate cut / seeded opt-in | **DECIDED 2026-09-14 (owner): confirmed, the third-party mirror shuffle stays a deliberate cut** |
| D5 | Scope of #14 | keep all of F1–F5 in Tier 1 / move F3+F5 to Tier 2 | keep **F1, F2, F4** in Tier 1 (real bugs), move **F3, F5** to Tier 2 |

---

## 7. Housekeeping to fold into the first commit touching `backlog-tasks.md`

- Rewrite #14 around §2.4: it is a functional bug, not a cosmetic cut,
  and it has the F1–F6 slices.
- Rewrite #6: the `src_compile` no-op root cause is §2.1–2.3, owned by
  B1–B4.
- #12 cites `standalone_phase_env_layers_matching_package_env_build_vars`
  at `ebuild_phases.rs:4761` as if it were production code. It is a
  `#[test]` fn (currently ~:5670). Point at the production matcher
  instead.
- Outside Tier 1 but in the same file: Tier 5 lists **#40 twice**
  (DONE 2026-09-13 and an older open line). Delete the stale line.

## 8. Verification (every slice)

Run AGENTS.md step 8: `cargo fmt --check`,
`cargo clippy --release --all-targets` (0 warnings),
`cargo test --release`, and `python3 -m pytest tests -q`. Compare
failing test **names** against a clean-`main` baseline, not counts
(see the contract-suite pollution note). Build at the workspace root
if a `pub` signature in a `portage-*` crate changes. Track B also runs
brush's own `brush-compat-tests` in `3rdparty/brush`. Track F slices
that change what gets downloaded (F1b, F4, F5) also re-run the L2
porttest track, because L2/L3 builds fetch distfiles.
