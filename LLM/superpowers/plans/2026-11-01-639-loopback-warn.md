# #639 + #640 — Child-side loopback warning via pre_exec write(2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status:** done 2026-11-01 (Tasks 0–3; backlog #639 + #640; branch `slice/639-loopback-warn` in both repos — portuale `2d71328f`..`f82c3b38` plus this docs-only close-out, pmtest clean with no commit, no pretend-output change; no new residue)

**Goal:** A failed demoted loopback setup warns at the head of the phase log, gzip-safe and terminal-visible, via one raw `write(2)` from `pre_exec` — replacing the #638 parent-side pipe with a strictly simpler mechanism.

**Architecture:** S0 proved, for the `Command::stderr(File)` probe it ran, that fd 2 inside `pre_exec` is already the Command-configured stderr (dup2 runs before hooks on this toolchain) — the pipe-backed `Tee` and un-redirected `Inherit` legs extrapolate that ordering, low risk but not probed — so a child-side write lands where phase stderr goes: through the gzip pump as a proper member (fixes #639), at the head of the stream before any phase output (fixes #640 placement), and through the tee pipe to the terminal (fixes #640 visibility). The parent pre-builds one Portage-identical line per errno (1..=135) and moves the table into the closure; the child selects and writes a single line on first failure, then continues best-effort. The entire pipe/append/eprintln apparatus is deleted.

**Tech Stack:** Rust 2024 (`crates/portuale`), Portage 3.0.83 (`3rdparty/portage/lib/portage/process.py:867-871`), pmtest contract suite (`../pmtest`).

**Spec:** `LLM/backlog-tasks.md` #639 + #640, argued from the S0 probe (`LLM/evidence/2026-11-01-639-loopback-report/s0-fd2-preexec.log`: fd 2 in `pre_exec` == configured stderr target for a `stderr(File)` command; the pipe/tee and inherit legs extrapolate it), Portage's warn-then-abandon shape (`process.py:861-871`, warn text), and the #638 implementation being superseded (`commands.rs` pipe/read/append/eprintln block).

## Global Constraints

- **Expected behaviour comes from Portage, never a second portuale run.** Cite `3rdparty/portage` `file:line` in every code note (repo `// Portage:` convention: bare names).
- **Demote exactly Portage's phases under exactly Portage's gate** (unchanged — this slice touches neither).
- **Best-effort is preserved: no new `Err` from loopback reporting itself.** The pipe deletion removes the only spawn-failure mode the #638 reporting path added (pipe creation). A failed warning `write(2)` is ignored like every other best-effort step.
- **No printing (stdio), no allocation, no subprocess in `pre_exec`** (inherited #630 rule). One raw `write(2)` of parent-built bytes; `// SAFETY:` on every unsafe block. (Rationale: `write(2)` takes no locks, unlike stdio — this is why the reporting moves into the child while `println!`/`eprint!` stay forbidden there.)
- **Linux-only for the netlink path; the table builder is cross-platform pure Rust** (no cfg needed for it; the `pre_exec` call site keeps its existing `#[cfg(target_os = "linux")]` gate).
- **Corpus untouched** (no pretend-output change). New backlog numbers start at **641**.
- **Root-gated tests run via `sudo <test-binary> <filter>`** (image rebuild is an owner step); skip cleanly when `euid != 0` or the `portage` user is unresolvable.

## Review Focus

- The warning lands at the head of the phase log, not the tail — pinned by Task 1's updated test asserting `lines().next()` on the plain log (and the warning inside a decodable member on the `.gz` log; the reader is `MultiGzDecoder` over the whole concatenated file, since Python's `GzipFile` reads across members).
- The `.gz` stream stays decodable end-to-end (no raw tail) — pinned by Task 1's new `.gz` decode test (`MultiGzDecoder` over the full file, warning present).
- The abort-rest + fib_trie behavior is unchanged by the reporting move (only the *sink* for the message moved, not the decision logic) — pinned by re-running the #638 tests unmodified in intent (message-content assertions updated for head placement only).
- The phase still runs rc 0 and localhost behavior is unchanged (setup itself untouched — this slice only moves where the failure line goes) — pinned by the kept rc/connect assertions.
- No subprocess, allocation, or stdio in `pre_exec` after the change — pinned by code review of the diff: exactly one `libc::write` added to the child path, pipe creation/read/append deleted (reviewer greps for `pipe2`, `OpenOptions::append` in the report path, and `print!`/`eprintln!` in `pre_exec`).

---

### Task 0: Baseline (no product code)

**Files:** none (read-only checks; S0 evidence already recorded during planning).

- [x] **Step 1: Cut the branch pair and verify clean trees**

```bash
export PORTUALE_DIR=/home/vivo/repo/PORTUALE/portuale
export PMTEST_DIR=$(cd "$PORTUALE_DIR/../pmtest" && pwd)
git -C "$PORTUALE_DIR" checkout -b slice/639-loopback-warn
git -C "$PMTEST_DIR" checkout -b slice/639-loopback-warn
git -C "$PORTUALE_DIR" status --short; git -C "$PMTEST_DIR" status --short
```

Expected: both empty except the untracked plan file + untracked `LLM/evidence/2026-11-01-639-loopback-report/` (S0 probe + outputs, recorded during planning; committed by Task 2).

- [x] **Step 2: Rebuild and record the binary**

```bash
cargo build --release -p portuale && ls -l --time-style=full-iso target/release/portuale
```

Expected: success; note the mtime.

---

### Task 1: Child-side warning write, delete the pipe (the fix)

**Files:**
- Modify: `crates/portuale/src/ebuild_phases/commands.rs` (table builder + pre_exec write path; delete pipe creation/read/append/eprint block)
- Test: `crates/portuale/src/ebuild_phases/tests/sandbox.rs` (first-line assertion; new .gz decode test)

**Interfaces:**
- Consumes: demoted `pre_exec` (`commands.rs:780+`, `#[cfg(target_os = "linux")]` net block), `SpawnBashScriptArgs{droppriv, iso, log_file}`, `loopback_fail_at` env hook (unchanged semantics), existing fib_trie + no-egress + control assertions.
- Produces: `pub(crate) fn loopback_warn_table() -> Vec<Vec<u8>>` — index `errno-1`, entries `"Unable to configure loopback interface: {strerror}\n"` via `std::io::Error::from_raw_os_error` (parent-built, C-locale caveat: tests assert the stable prefix only); plus a `const` fallback byte string for out-of-range errnos (no allocation). Replaces `loopback_warn_msg` (delete it; single call site moves to the table lookup).
- `fn configure_loopback_netlink(has_ipv6: bool, fail_at: u8) -> (u8, u8)` stays as-is (early-return abort-rest untouched; only its *caller* changes how the result is reported).
- Delete: `pipe2` creation block, `loopback_read`/`loopback_write` bindings + move into closure, post-wait read loop, `OpenOptions::append` warning write, `Inherit`/`Foreground` `eprint!` arm for loopback (keep the arm's other uses if any — there are none today; delete the arm only if it becomes dead).
- Add (parent, before `Command::spawn`): `let loopback_warn = loopback_warn_table();` moved into the `pre_exec` closure alongside `hostname`/`has_ipv6`/`fail_at`.
- Add (child, in the net block after the sequence, replacing the 2-byte pipe write): on `step != 0`, one `libc::write(2, line.as_ptr(), line.len())` of `table[(errno-1)]` (bounds-checked, fallback const otherwise); `let _ =` ignored (best-effort). Comment cites the S0 result (fd 2 == phase stderr in `pre_exec` on this toolchain; graceful degradation noted: on a toolchain that reorders dup2 after hooks the warning lands on the parent stderr — still visible, never lost).

- [x] **Step 1: Write the failing tests**

```rust
// Update: demoted_loopback_failure_warns_and_continues asserts the warning
// is lines().next() (head, pre-exec) instead of lines().last().
#[test]
fn demoted_loopback_warning_survives_gzip_log() {
    // SKIP-gated identically (root + portage user).
    // Same injected failure (fail_at=linkup), Capture to a `build.log.gz`
    // path. Decode with flate2::MultiGzDecoder (already a dependency),
    // assert the warning line present among members AND the stream decodes
    // to end (no trailing-garbage corruption — the #639 regression shape).
    // (flate2 is already used by commands.rs; no new dependency.)
}
#[test]
fn loopback_warn_table_matches_portage_text() {
    // Pure unit test: table[EPERM-1] starts with
    // "Unable to configure loopback interface: " and ends with b'\n'.
    // Assert only prefix/newline (strerror body is locale-dependent).
    // Assert table length covers 1..=135 and the fallback exists.
}
```

- [x] **Step 2: Run them to see red**

Run: `cargo test --release -p portuale loopback_` (compile fail: `loopback_warn_table` + `fail_at`-less call mismatch) and the root-gated tests against the pre-fix binary (no warning at head; .gz test would show raw tail or missing line — record which)
Expected: red for the stated reasons.

- [x] **Step 3: Implement**

Per Interfaces above. Keep the abort-rest early returns, the `fail_at` hook, and the `has_ipv6` plumbing byte-identical in behavior; only the reporting leg changes (table + single `write(2)`; delete pipe/read/append/eprint). Update the stale `// Portage _exec2: loopback …` comment block and the pre-exec no-printing comment to name the raw-write exception and its S0 grounding. Keep demotion order, umask, pid dance untouched.

- [x] **Step 4: Re-run root-gated plus the full crate suite**

Run: `sudo <test-binary> demoted_loopback --test-threads=1` (serial: process-global env hook) then `cargo test --release -p portuale`
Expected: PASS (warning first line; .gz decodes with warning inside; fib_trie absence still holds on injected failure; control clean; SKIP only when not root; suite green modulo known host-state reds).

- [x] **Step 5: Commit**

```bash
git add crates/portuale/src/ebuild_phases/commands.rs crates/portuale/src/ebuild_phases/tests/sandbox.rs
git commit -m "phases: report loopback failure from pre_exec via write(2) (#639, #640)"
```

Body: cite the S0 result (fd 2 == phase stderr in pre_exec), `process.py:867-871` (text), what was deleted (pipe/read/append/eprint) and why each destination still works (log file direct, gzip pump member, tee pipe, terminal inherit), and the graceful-degradation note for toolchain reorder.

---

### Task 2: Regression gate (no new oracle on this host)

**Files:**
- Test: none expected (amend only if the gate exposes a gap)
- Evidence: `LLM/evidence/2026-11-01-639-loopback-report/` (S0 files already there untracked; add acceptance log)

**Interfaces:**
- Consumes: Task 1 product + tests.

- [x] **Step 1: Re-run the Task 1 root tests on the final binary (rebuild first — stale-binary trap)**

Run: `cargo build --release -p portuale && cargo test --release --no-run -p portuale && sudo <fresh-binary> demoted_loopback --test-threads=1`
Expected: all green.

- [x] **Step 2: Host acceptance — hyperfine once, as regression gate**

```bash
sudo ./target/release/portuale emerge -1 app-benchmarks/hyperfine 2>&1 | tee LLM/evidence/2026-11-01-639-loopback-report/hyperfine-acceptance.log; echo "rc=${PIPESTATUS[0]}"
```

Expected: rc=0, `completed emerge (1 of 1) app-benchmarks/hyperfine-1.21.0`. Rationale (say so in the report): this run cannot trigger the failure path; it proves the rewritten reporting leg didn't break spawns — the spawn tests are the pin, this is the gate.

- [x] **Step 3: Full verification pass**

```bash
cargo fmt --check
cargo clippy --release --all-targets   # zero warnings
cargo test --release
cd "$PMTEST_DIR" && scripts/pytest-parallel.sh
```

Expected: green modulo the known host-state reds (1 cargo `embedded_runtime` + 7 pytest `mrg_remote`, root-only `gnupg.getuto.tmp` tar exit 2; #632/#633 unexplained-once; #634 basetemp workaround) — disclose with evidence, do not fix here. Zero corpus drift — any drift is a red flag, never a bless.

- [x] **Step 4: Commit (evidence only if tests untouched)**

```bash
git add LLM/evidence/2026-11-01-639-loopback-report
git commit -m "phases: pin child-side loopback warning end to end (#639, #640)"
```

Body: test names, acceptance rc + version, pmtest untouched (no pretend change). If Task 2 had to amend a test, include the file and say why.

---

### Task 3: Close-out

**Files:**
- Modify: `LLM/backlog-tasks.md` (#639 + #640 → DONE; file any new residue with a fresh number)
- Modify: `LLM/what-this-proves.md` (append one paragraph, never rewrite)
- Modify: this plan's `Status:` header → `done <date>`
- Modify: `LLM/code-notes/portuale/ebuild_phases/{commands,sandbox}.md` (terse entries: child-side write, deleted pipe, table builder; run `code_notes_index.py` + `fact_coverage.py` over comment edits)

**Interfaces:**
- Consumes: Task 1 commit + Task 2 evidence and verification.

- [x] **Step 1: File residues (check numbers free first)**

```bash
grep -hoE '^[0-9]+\. \*\*' LLM/backlog-tasks.md LLM/backlog-tasks-*.md | grep -oE '^[0-9]+' | sort -n | tail -1
```

File only what the evidence supports (candidate: toolchain-reorder graceful degradation is a comment, not a residue — do not file it).

- [x] **Step 2: Append the `what-this-proves.md` paragraph**

Cause (one line + #639/#640 pointers), fix shape (S0 fd-2 finding → child-side `write(2)`, pipe deleted, table-driven Portage-identical text), runnable proofs (`sudo <test-binary> demoted_loopback`, hyperfine acceptance rc, `.gz` decode test), honest limits (C-locale strerror caveat in unit test; residue numbers).

- [x] **Step 3: Re-run the close-out verification**

Same gate as Task 2 Step 3 (docs-only delta expected — product identical; record counts, disclose reds, zero drift).

- [x] **Step 4: Flip the entries + this plan's Status, commit the close-out**

```bash
git add LLM/backlog-tasks.md LLM/what-this-proves.md LLM/superpowers/plans/2026-11-01-639-loopback-warn.md LLM/code-notes/
git commit -m "docs: close #639, #640"
```

Do **not** push unless asked (separate request). pmtest stays commit-free.
