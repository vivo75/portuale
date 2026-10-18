# S8 report (347-ERR, 347-MRG) — 2026-10-18, no commit, no push, no bless

Branch `backlog/346-rest` (both repos). Oracle: Portage 3.0.82.2 S0 C cells
in `LLM/evidence/2026-10-15-346-s0/c/` (seed runs; every `emerge` cell rc 2
except c02). Say "Portage". Container bed not run (not required for this
slice). Corpus NOT blessed (coordinator blesses in the pmtest commit).

## What changed

portuale (product code):
- `crates/portuale/src/pretend/mod.rs`: new `parser_error_text` /
  `parser_error` (one `emerge: error:` line, message taken exactly as
  Portage passes it, trailing `\n` included — rule D4); `report_option`'s
  unknown-token arm now prints `emerge: error: unrecognized arguments:
  <token>` (was `emerge: unrecognized option "<token>"`).
- `crates/portuale/src/pretend/args.rs`: 10 error sites moved to the
  C-cell texts via the helper — `Invalid --jobs/--load-average/--deep/
  --backtrack parameter: '<v>'` (+ blank line) and `argument
  --color/--with-bdeps/--with-bdeps-auto/--onlydeps-with-rdeps/
  --onlydeps-with-ideps/--autounmask-backtrack: invalid choice: '<v>'
  (choose from …)` (single `\n`, single-quoted value and choices).
- `crates/portuale/src/pretend/cli_atoms.rs`: both exclude-family
  messages go through the helper with Portage's trailing `\n` (texts
  already matched; only the D4 blank line is new). Doc comment: the
  `usage:` block stays a documented cut (347-USG), reworded per plan.
- `crates/portuale/src/mrg.rs`: 347-MRG — new `Append` options
  `--rebuild-exclude`, `--rebuild-ignore`, `--usepkg-exclude`
  (`action: "append"` in Portage `_emerge/main.py:565-594`) + the three
  `emerge_handles` entries. The emerge codepath already parsed and
  validated all three; only the `mrg` table lacked them.
- Tests: new `pretend/tests/error_lines.rs` (+ `tests/mod.rs` entry).

portuale (docs, append-only / notes): `LLM/what-this-proves.md` paragraph;
`LLM/code-notes/portuale/{mrg,pretend/cli_atoms,pretend/mod}.md`
(+ `code_notes_index.py` re-run; INDEX.md unchanged).

pmtest: `test_emerge_pretend_contract.py` (moved pins below, 8 new CASES
rows, new `test_s8_argparse_error_lines_match_portage` with 13 C-cell
params); `test_portuale.py` (new
`test_mrg_forwards_rebuild_and_usepkg_exclude`).

## Tests added

- Rust (failing first — both failed before the fix, pass after):
  `pretend::tests::error_lines::parser_error_keeps_portages_trailing_newline_rule`
  (D4, grounded in c01/c06/c12),
  `mrg::tests::rebuild_and_usepkg_exclude_forward_like_exclude`.
- pmtest CASES rows (all rc 2, Portage's code): `--backtrack=x`,
  `--color=maybe`, `--with-bdeps=x`, `--with-bdeps-auto=x`,
  `--onlydeps-with-rdeps=x`, `--onlydeps-with-ideps=x`,
  `--autounmask-backtrack=x`, `--frobnicate`.
- pmtest `test_s8_argparse_error_lines_match_portage` (13 params,
  c01+c03–c14; c02 omitted — already at parity, see below): exact stderr
  bytes incl. the D4 blank line; docstring names every capture file and
  the full Portage argv.

## Gates (in order, never concurrently)

- `cargo fmt --check`: clean. `cargo clippy --release --all-targets`:
  0 warnings. `cargo build --release`: ok.
- `cargo test --release` (workspace): exit 0 — 2797 passed, 0 failed.
- pmtest `python3 -m pytest pytests-contract-suite -q
  --basetemp=/var/tmp/pt346c -p no:cacheprovider`: **4028 passed,
  37 skipped, 2 xfailed, 0 failed**, 3 corpus-drift warnings (reviewed
  below, not blessed). Basetemp removed afterwards (needed `sudo -n rm`
  for root-owned merge-test leftovers). One mid-slice full run failed
  exactly one test (`s8[…c12-frobnicate]`: `report_option` used
  `eprintln!` over the helper text, doubling the newline); fixed,
  re-gated clean. A re-run against a non-removed basetemp errored 4063
  setup errors (stale root-owned dirs); removed, re-ran green — infra,
  not product.

## Moved pins (every one)

`test_emerge_pretend_contract.py`:
- `test_short_flag_bundle…` (`-pz`): `…unrecognized option "-z"` →
  `emerge: error: unrecognized arguments: -z`.
- `test_deep_rejects_a_negative_inline_value`: → `emerge: error:
  Invalid --deep parameter: '-1'`.
- `test_jobs_and_load_average…` table: `--jobs=x`, `--load-average=0`,
  `--load-average=x` → `emerge: error: Invalid …` texts.
- `test_genuinely_unrecognized_option…`: → `emerge: error:
  unrecognized arguments: --totally-fake-option`.
- Six S7 exact-stderr asserts (exclude ×2, usepkg-exclude,
  buildpkg-exclude ×2, strict-families): appended the D4 `\n`
  (asserts are `==`, so the blank line is pinned).
- One docstring quote fix (`"unrecognized arguments"`).
- `test_portuale.py`: no text pin moved (it holds no `emerge:` error
  texts; the plan's "4" does not exist as text pins — verified by grep).

Deliberately NOT moved (behaviour unchanged, no C cell; see notes):
`-jx` pin (`invalid -j parameter`), bare `--load-average` pin
(`requires a positive number`), all rc-only CASES rows.

## Corpus drift (old -> new, NOT blessed)

Runtime-flagged (3, all reviewed against the C cells):
- `…scheduling_only_under_pretend#8` (`--jobs=x`):
  `emerge: invalid --jobs parameter: "x"\n` →
  `emerge: error: Invalid --jobs parameter: 'x'\n\n` (= c01).
- `…#11` (`--load-average=0`): `…invalid --load-average parameter:
  "0"\n` → `emerge: error: Invalid --load-average parameter:
  '0'\n\n` (same Portage f-string as c03, `main.py:1066-1069`).
- `…#12` (`--load-average=x`): → c03 text + blank.
- Bless note: `test_pretend_case_exit_code` rows never flag (matcher is
  nodeid-first and every CASES nodeid on this branch shifted vs harvest
  — e.g. `--jobs=x` is `args67` today vs `args45` in the corpus — so
  they resolve to None, "stale, not drifted"). The coordinator's bless
  will therefore update only the 3 above; the 8 below stay silent
  orphans and need a re-harvest decision, not a plain bless:
- Silent-but-invalidated (deduced by args+env; new text = what the
  moved pins assert, same code paths as the exact-pinned rows):
  CASES `--deep=-1`, `--jobs=x`, `--load-average=0`,
  `--autounmask-backtrack maybe`, `--with-bdeps auto`,
  `--with-bdeps-auto maybe`, `--totally-fake-option`, `-pz`
  (old texts as harvested; new texts per the Moved-pins list).
- Still valid (code untouched): `-jx` ×2, `--quickpkg-direct`,
  `invalid atom` ×3, bare `--load-average` (`requires a positive
  number` has no corpus blob).

## Left as-is (adjacent shapes, no C cell)

- c02 (`-j x` → atom, rc 1) already matches (minus the known `for
  <root>.` suffix convention); no change.
- `-j<attached>` invalid (e.g. `-jx`): Portage recycles the rest into
  argv (`main.py:263-288`, so `-jx` dies as `unrecognized arguments:
  -x`) — behaviour work beyond S8's text scope; pin and text kept.
- Bare `--load-average` and the `requires an argument` family keep
  portuale-authored texts (Portage inserts defaults / argparse-native
  `expected one argument` there — same class of behaviour work).
- `ebuild:`-applet messages untouched (S8 is emerge-only).
- Untracked `pmtest/fixtures/*pg359*` files left alone.
