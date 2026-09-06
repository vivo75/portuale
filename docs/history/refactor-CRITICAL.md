# refactor-01: CRITICAL-impact rust-skills audit of the `rust/` workspace

A source-grounded audit of the whole `rust/` workspace — every crate
(`portage-versions`, `portage-dep`, `portage-use-reduce`,
`portage-profile`, `portage-repo`, `portage-required-use`,
`portage-fetch`, `portuale`, plus the `*-harness` mains) — against the
**CRITICAL**-impact rule categories of the `rust-skills` skill: `own-`
(12), `err-` (12), `mem-` (17), `unsafe-` (7). The aim is an honest,
verifiable list of deviations, sized into actionable slices, each
respecting the project's hard constraints. Open judgment calls are flagged
explicitly rather than silently decided — see "[Closed/open judgment
calls](#closed-open-judgment-calls)".

## Method

- Rule sources: `3rdparty/rust-skills/rules/{own,err,mem,unsafe}-*.md` (all
  48 CRITICAL files) and `rust-skills/SKILL.md` (CRITICAL > HIGH > MEDIUM
  > LOW priority).
- Audit was mostly mechanical greps over `rust/`:
  `rg`/`grep` for `unsafe`, `panic!`, `unreachable!`, `unwrap()`,
  `format!`, `with_capacity`, ownership-smart-pointer types, dependency
  lists, and workspace lint config, plus per-site reads of every hit in
  the "action" column.
- Classification heuristic for `unwrap()`/`panic!` was "production vs
  test": `#[cfg(test)]`/`#[test]`/`mod tests`-tracked scan; treat the
  per-crate production counts as **approximate** — the exact per-site
  verdicts in the tables below are the ground truth, not the totals.
- Nothing was changed; this is an audit document only.

## Hard constraints that shape every fix (do not break these)

1. **CLI/parity output is pinned byte-for-byte** to the real
   Portage/Python output by the shared contract suite
   (`tests/test_emerge_pretend_contract.py`, 1282 passing cases). Any
   error string, delimiter, or exit path that appears in pinned output is
   frozen even if a rule (e.g. `err-lowercase-msg`) would prefer
   otherwise.
2. **Rust and Python sides must stay behaviorally identical**, verified
   empirically (both run against `fixtures/`), not just via pytest.
3. **musl static binary, zero dynamic runtime deps** (hard goal): any
   fix must stay pure-Rust with no c-linkage. `panic = "abort"` is set
   in the workspace profile — panics abort, they don't unwind.
4. **Near-zero-dependency ethos** in the library crates (std + `regex` +
   `tokio` + `brush` + `libc` + `filetime` is the whole universe). New
   deps (e.g. `thiserror`) are a judgment call per crate, not a given.
5. **Verification gates**: `cargo fmt --check`, `cargo clippy --release
   --all-targets` (zero warnings), `cargo test --release` (whole
   workspace, 792 tests), `python3 -m pytest tests -q` (whole contract
   suite), and the musl static smoke test in CI. No slice closes without
   all four green.
6. **No behavior change outside the mechanics being fixed.** Refactors
   must be byte-identical on the harnesses/CLI.

## Audit summary

| Category | Verdict | Action required |
|---|---|---|
| `unsafe-` (7) | 3 clean, 4 N/A, 1 partial | Real work: 10 missing `// SAFETY:` markers in `portuale` |
| `err-` (12) | mixed | 2 data-dependent panic! sites + ~2 parse unwraps; error-string casing deliberately column-pinned |
| `own-` (12) | strong | 1 `&String` closure param; rest idiomatic already |
| `mem-` (17) | mostly N/A | `format!`/`with_capacity` notes; nothing urgent |

---

## 1. `unsafe-` rules (CRITICAL)

All `unsafe` in the workspace is confined to `portuale/src/`
(`ebuild_phases.rs`, `elog.rs`, `portage_lock.rs`, `pretend.rs`,
`ebuild_merge.rs`); none of the library crates contain `unsafe`. The
library crates are therefore unaffected by the whole `unsafe-` category.

### 1.1 `unsafe-safety-comment` — **deviating (the main unsafe fix)**

Rule: a `// SAFETY:` comment directly above every `unsafe` block. The
crates do **not** enable `clippy::undocumented_unsafe_blocks` (no
`[lints]` section anywhere), so today this is a convention, not a CI
gate — but it is the kind of low-risk, straightforward fix that should be
landed before anything deeper. Current `// SAFETY:` markers: 5 total,
all in `pretend.rs` (3) and `elog.rs` (2). **Missing markers**:

| Site | Call | Where |
|---|---|---|
| `portage_lock.rs:43` | `libc::flock` | only unsafe in crate |
| `ebuild_phases.rs:1953` | `libc::kill` | only unsafe in crate |
| `ebuild_merge.rs:1562,1569,1584,1618,1620` | `mkfifo`/`mknod`/`chmod`/`lchown`/`chown` | production `lchown_or_chown`/`merge_one_node` |
| `ebuild_merge.rs:3293,3295,3319,5217` | `lchown`/`chown`/`geteuid`/`mkfifo` | `#[cfg(test)]` blocks |
| `elog.rs:933,941` | `std::env::set_var`/`remove_var` | test helper — comment present but not the `// SAFETY:` form |

All other unsafe blocks are already covered (`pretend.rs:3462-3496`
scheduler/cgroup syscalls share one marker; `elog.rs:363-384` has two).

**Fix slice (S1)**: add a terse, verifiable `// SAFETY:` above each of
the ~10 sites above. For the `ebuild_merge.rs` production block the
existing doc comment already states the invariant ("dest_c outlives the
call"; the error-path returns without touching the pointer) — mirror it
in the marker.

### 1.2 `unsafe-minimize-scope` — **compliant**

Every unsafe block is a single syscall expression with the `CString`
built outside the block. No enlarging needed.

### 1.3 `unsafe-miri-ci` — **N/A beyond env-var tests**

`cargo miri test` across the whole crate is impractical here: portuale's
unsafe is almost entirely libc syscall FFI (`kill`, `openlog`, `syslog`,
`flock`, `mkfifo`, `mknod`, `chmod`, `lchown`, `chown`), which Miri
cannot execute (unknown/emulated syscall → Miri gives up or panics on
those paths). The one Miri-meaningful spot is the `elog.rs` env-var test
helper (`set_var`/`remove_var`, which Miri instruments for data-race /
leak detection). Action: **document the cut** (like `scope-backlog.md`
does); optionally add a small Miri-targeted unit test for the
env-var helper only. Do not advertise `unsafe-miri-ci` as satisfied.

### 1.4 `unsafe-extern-block` — **N/A**

No `extern` blocks are declared in the workspace at all (all FFI goes
through the `libc` crate).

### 1.5 `unsafe-no-mangle-unsafe` — **N/A**

No `#[no_mangle]`/`export_name`/`link_section` anywhere in the
workspace (grep confirmed).

### 1.6 `unsafe-send-sync-manual` — **N/A**

No `unsafe impl Send`/`Sync` in the workspace.

### 1.7 `unsafe-maybeuninit` — **N/A**

No `MaybeUninit`, `mem::uninitialized`, or `mem::zeroed` usage.

---

## 2. `err-` rules (CRITICAL)

### 2.1 Error-style baseline (relevant to nearly every `err-` rule)

The codebase uses **`Result<_, String>` everywhere** — no `thiserror`,
no `anyhow`, no custom `Error` types, no `error!` macro. Messages are
**capitalized** and often carry a `{context}: {cause}` shape (e.g.
`"SRC_URI: expected \"(\"...\" (parse error in atom) at position N"`,
`"USE flag '...' is not in IUSE"`). This is a deliberate, consistent
design, and on every CLI-visible path the strings are pinned by the
contract suite. Consequences:

- `err-thiserror-lib` / `err-custom-type` / `err-anyhow-app` —
  **deviating by design, and that deviation is now the shipped model**
  (S4, option B, 2026-09-06 — reopened and resolved without thiserror):
  every library crate has its own hand-rolled `pub enum Error` with
  byte-identical `Display` and `From<Error> for String` for crossing
  sites; `portuale` carries the bounded internal
  `Error { kind, detail: Vec<String> }` seam (see below) on its
  CLI-visible boundary, while its `Result<_, String>` internals are
  deliberately left alone ("not now"). No new dependencies; the contract
  suite stays byte-identical.
- `err-from-impl` / `err-source-chain` / `err-question-mark` — **late**
  for the `?`-only parts (plain `?` is used), but no `From`/`#[source]`
  chain machinery exists; not applicable to the String-error design.

### 2.2 `err-result-over-panic` — **deviating at isolated, data-dependent sites**

Production `panic!`/`unreachable!`/`unimplemented!` sites:

| Crate:line | Kind | Verdict |
|---|---|---|
| `portage-versions/src/lib.rs:52` | `.unwrap_or_else(\|_\| panic!("version component too wide for i128: {s:?}"))` | **DATA-DEPENDENT.** A >~38-digit numeric component (or offset accumulator) hits this. Today it aborts. This is the one real resilience hole. |
| `portage-versions/src/lib.rs:114` | `panic!("suffix chunk … should already be validated by ver_regexp")` | invariant (regex pre-validated). Acceptable under `err-expect-bugs-only`; fine to leave. |
| `portage-versions/src/lib.rs:46` | `unreachable!("invalid version suffix …")` | invariant (same regex). Fine. |
| `portage-dep/src/lib.rs:182,363,387,444,609` | `unreachable!()` | operator-regex invariants after `Operator::from` parsing. Fine. |
| `portage-required-use/src/lib.rs:112` | `unreachable!()` | parser invariant. Fine. |
| `portage-use-reduce/src/lib.rs:424` | `unreachable!()` | parser invariant. Fine. |

**The one real fix** is `portage-versions/src/lib.rs:52`:
`version component too wide for i128`. `portage-versions` is a library
returning `Option<Ordering>`/`bool`; its caller-side convention is
`None` for incomparable. The panic is reachable from *hostile* version
strings. **Resolved in S2** (see "Closed / open judgment calls", item
(a)): because the Python reference's `int()` is arbitrary-precision and
*never* fails on these strings, returning `None` would break the parity
contract. Instead `parse_component` became a `Part` enum whose `BigNum`
variant compares the overflowed digit string by length-then-digits —
exactly Python's bignum ordering, with no panic and no behavior change
on any in-range input.

### 2.3 `err-no-unwrap-prod` (`anti-unwrap-abuse`) — **mostly invariant, 2 data-dependent**

Production `unwrap()` count is ~84 across the workspace (heavy in
`portage-versions`, `portage-use-reduce`, `portage-dep`, `portage-repo`).
Nearly all are **regex/parser-stack invariants** (e.g.
`portage-versions/src/lib.rs:21,27,115,174-183`;
`portage-use-reduce/src/lib.rs:139-159,177-272,423,480`) — the
"known-capture-group after a full-match" pattern where `?` cannot be
used. Acceptable under `err-expect-bugs-only`; **do not churn these**
(the ergonomic replacement is `if let Some(c) = caps.get(1)` which many
sites already use — convert lazily, only when touched).

**Data-dependent production unwraps to act on:**

| Site | Why it can panic | Fix |
|---|---|---|
| `portage-versions/src/lib.rs:216,221` | `rev1.parse().unwrap()` (i64) — the `-r<digits>` regex (`(\d+)` unbounded) allows giant revision numbers → `parse` fails on overflow | fixed in S2: bignum string compare (same `Part` mechanism) |
| `portage-versions/src/lib.rs:52` (see 2.2) | i128 overflow on huge components | fixed in S2: bignum string compare |

Everything else in the production counts is guarded or invariant (e.g.
`portage-repo/src/lib.rs:5545,5604` `versions.pop().unwrap()` are
reachable only after a `len() >= 2` guard; `pretend.rs` bare-name/match
`pop`/`get_mut`/`next` unwraps are guarded by the same match arm they
resolve). Review-not-fix.

### 2.4 `err-thiserror-lib` / `err-custom-type` / `err-anyhow-app`

**Resolved in S4 (option B, 2026-09-06)**: hand-rolled `Error` enums in
`portage-use-reduce`, `portage-fetch`, `portage-required-use`,
`portage-repo`, `portage-profile` — each `Display` byte-reproduces the
prior `format!` messages and `From<Error> for String` lets crossing
sites compile unchanged. `portuale` itself wraps only its CLI boundary
in the §2.1 `{ kind, detail: Vec<String> }` seam (`pretend.rs` resolve
handle, kind `"resolve"`); internals and harnesses stay `String`.
`anyhow` remains banned: dynamic error, dead weight next to the
CLI's column-pinned formatting.

### 2.5 `err-lowercase-msg` — **column-pinned; deviate only on un-pinned paths**

New/internal (non-contract-pinned) messages should be lowercase,
unpunctuated per the rule. All pinned CLI strings, and anything the
parity suite compares, must NOT be re-cased. This is the "correct
**new** code" reading: fix forward, don't mass-edit.

### 2.6 `err-context-chain`, `err-doc-errors`

- `err-context-chain`: partially satisfied by the
  `"<context>: {cause}"` message shape; `.context()` (anyhow) is
  inapplicable. Fine as-is.
- `err-doc-errors`: doc comments on fallible `pub fn`s do not
  systematically carry `# Errors` sections. Low priority; add when
  touching a function, or skip.

---

## 3. `own-` rules (CRITICAL)

Overall: **the cleanest category.** Idioms in use are already the right
ones (`Cow` in 1 file, `RefCell`/`Arc` appropriately, `RwLock` in
`portage-repo` for read-heavy tables, `Mutex` for the 2 registries,
`std::sync` 10 places, no `Rc` anywhere, no `Copy` abuse).

| Rule | Verdict | Note |
|---|---|---|
| `own-borrow-over-clone` | deviating, isolated | one `.cloned().collect()` of a `&String` list in `pretend.rs` (bare-name candidate collection) — correct-API, fine to leave. Review only. |
| `own-slice-over-vec` | deviating, isolated | `pretend.rs:3126` closure was `\|a: &String, b: &String\|` — the one `&String`-typed param in the workspace. ✅ fixed in S3 (`&str` + deref-coercing `sort_by` wrappers). All function signatures use `&str`/`&[T]`/`&Path`. |
| `own-cow-conditional` | compliant | `Cow` used where conditional ownership applies. |
| `own-arc-shared` | compliant | `Arc` for the shared scheduler state. |
| `own-rc-single-thread` | N/A | no `Rc` — everything shared is cross-thread. |
| `own-refcell-interior` / `own-mutex-interior` / `own-rwlock-readers` | compliant | read-heavy `RwLock` choice is right for `portage-repo`. |
| `own-copy-small` / `own-clone-explicit` / `own-move-large` / `own-lifetime-elision` | compliant | no gratuitous `Copy`, clones are intentional, large structs moved, lifetimes elided. |

Action: ✅ done in S3 (`pretend.rs:3126`); otherwise no work.

---

## 4. `mem-` rules (CRITICAL)

Mostly N/A — this is a CLI tool where the hot paths are string
rendering, not allocation-heavy numerics.

| Rule | Verdict | Note |
|---|---|---|
| `mem-with-capacity` | partial | 14 `with_capacity` sites. A handful of obvious-known-size `Vec` allocations in the pretend/`-pv` rendering and `portage-repo` `by_cp`/`by_cat` maps could pre-size. Low urgency — profile first (benchmarks already exist). |
| `mem-avoid-format` | compliant by luck | the only literal-only `format!` is `regen.rs:227` (`format!(".{pf}.regen")` — dynamic, correct) and error-string builders — all necessary. No `format!("constant")` waste found. |
| `mem-write-over-format` | N/A / opportunistic | would matter only for hot `.push_str(&format!(...))` loops in the renderers; perf-driven, not now. |
| `mem-smallvec`/`mem-arrayvec`/`mem-thinvec`/`mem-boxed-slice`/`mem-arena-allocator`/`mem-zero-copy`/`mem-compact-string`/`mem-smaller-integers`/`mem-assert-type-size`/`mem-clone-from`/`mem-reuse-collections`/`mem-take-replace`/`mem-drop-order`/`mem-box-large-variant` | N/A | none of these patterns exist; adding any would be speculative. `mem-reuse-collections` could apply in the backtracking resolver's re-walk, but only with a measured win. |

No required mem work today.

---

## Prioritized fix plan

| Slice | Scope | Behavior delta | Verification |
|---|---|---|---|
| **S1** ✅ shipped | Add the ~10 `// SAFETY:` markers (1.1). No code change. | none | fmt + clippy clean, `cargo test` 792/792, pytest 1282 passed / 5 pre-existing non-TTY |
| **S2** ✅ shipped | `portage-versions` overflow panics (2.2, 2.3) removed: `parse_component`, suffix numerals, and `rev` now fall back to an arbitrary-length `BigNum` decimal-string compare when the value outgrows `i128` — mirroring the Python reference's unbounded `int` (judgment call (a): `None` was rejected because Python compares, it doesn't fail). Also fixed a previously-silent parity bug: oversized *suffix* numerals were coerced to `0`; they now compare as bignums. | oversized components/revisions/suffix-numerals stop aborting (and match Python on inputs that previously miscompared); all in-range behavior byte-identical | fmt clean, `cargo test` 797/797 (5 new `portage-versions` unit tests), pytest 1292 passed / 5 pre-existing non-TTY. (3 test-module clippy warnings surfaced under `--all-targets`; cleaned up in S3.) |
| **S3** ✅ shipped | `pretend.rs:3126` `&String`→`&str` `vercmp_key` closure params (the workspace's only `&String`-typed param), via deref-coercing `sort_by` wrappers at its 3 call sites. Also fixed the 3 `portage-versions` test-module clippy warnings found while re-running the full gate. | none | fmt clean, clippy `--all-targets` zero-warn (re-verified), `cargo test` 797/797, pytest 1292 passed / 5 pre-existing non-TTY |
| **S4** ✅ shipped | `err-*` error-model migration (**option B**: hand-rolled `Error` enums, no thiserror) — typed errors in `portage-use-reduce`/`portage-fetch`/`portage-required-use`/`portage-repo`/`portage-profile` with byte-identical `Display` + `From<Error> for String`; `portuale::Error { kind, detail: Vec<String> }` seam on the CLI boundary only (§2.1's "not now" keeps portuale's 115 `Result<_, String>` sites). | none in behavior; every pinned CLI string byte-identical | fmt clean, clippy `--all-targets` zero-warn, `cargo test` 799/799 (+2 new `error.rs` unit tests), pytest 1292 passed / 5 pre-existing non-TTY |

Info-only (record, don't act): the `err-` counts table in
`docs/refactor-01.md` doubles as the canonical production-panic/unwrap
map for future greps; the lib-crates-are-unsafe-free fact means all
future `unsafe-` work is `portuale`-only.

## Verification pass for each slice

1. `cargo fmt --check`
2. `cargo clippy --release --all-targets` — zero warnings (S1/S3 must not
   add any; S2 must not)
3. `cargo test --release` — whole workspace (S2 especially; the
   `portage-versions` unit tests plus the contract pairs pin the new
   oversized-input behavior)
4. `python3 -m pytest tests -q` — whole contract suite (S2 must stay
   byte-identical on all pinned inputs)
5. musl static smoke in CI

## Closed / open judgment calls

- **Closed by this audit:** lib-crate no-unsafe fact;
  `unsafe-miri-ci` = documented cut (Miri can't run the syscall FFI);
  invariant unreachable/unwrap sites are acceptable and must not be
  churned; pinned error strings keep their casing.
- **Closed by S4 (was open (b)):** the `err-` error-model re-open —
  shipped as option B (hand-rolled per-crate `Error` enums +
  boundary-only `portuale::Error`), **not** thiserror (zero new
  dependencies, constraint #3 preserved) and **not** anyhow.
- **Closed by S2 (was open (a)):** oversized-input semantics — the Python
  reference (`lib/portage/versions.py` `vercmp`) uses arbitrary-precision
  `int`, so a huge component *does not* fail there; it compares. `None`
  would break parity, so S2 mirrors the bignum with a decimal-string
  fallback (`Part::BigNum`), verified against the reference on 39/40-digit,
  huge-revision, and huge-suffix-numeral pairs in the contract suite.