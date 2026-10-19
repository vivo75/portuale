# #420 design: masked-binary disclosure beyond the top-level respect-use case

Scope: three cells where Portage prints a masked block but portuale prints
a plain miss. Root recipe: `/var/tmp/w0/s10/setup.sh` (throwaway
`root420`, follows `LLM/evidence/2026-10-19-b415/setup.sh` + b424:
`EMERGE_LOG_DIR` set, Portage run via `root420/bin/emerge` symlink;
deviation: no `PORTAGE_TRUST_HELPER`/signature flags — cells use synthetic
`.tbz2` `Packages` rows, pretend-only, so no signature path runs).

## 1. Cells (smallest existing fixtures)

- (A) dependency respect-use: `repo-341` `app-misc/t341-built`
  (`RDEPEND="dev-libs/t341-dep:0/1="`) + the #402 two-build synthetic
  `PKGDIR` (`_t341_bid_cell_configroot`: builds 1/2, `IUSE: x`) with user
  `package.use` mismatching **both** builds. Throwaway mirror: `t420dep`
  (IUSE=x, baked USE x/empty) + `t420parent`; `package.use`:
  `=dev-libs/t420dep-1-2 x`. No new fixture needed.
- (B) top-level changed-deps: existing `dev-libs/bcdeppkg` (tree
  `RDEPEND bcdepnew`, binhost binary `RDEPEND bcdepold`) with
  `--usepkgonly=y --binpkg-changed-deps=y`. Mirror: `t420cdep`
  (`RDEPEND t420new`, binary `RDEPEND t420old`). No new fixture needed.
- (C) dependency changed-deps: parent of the (B) binary. No ebuild in any
  fixture repo depends on `bcdeppkg` (grep: zero hits) — specify new
  fixture `dev-libs/bcdepparent-1.0.ebuild` (`RDEPEND="dev-libs/bcdeppkg"`,
  SLOT 0, KEYWORDS amd64) + `metadata/md5-cache` entry; the binary side
  reuses the existing binhost `bcdeppkg` row (parent binary via a synthetic
  `Packages` row in the pin, `_t341_bid_cell_configroot` precedent — no
  archive needed for pretend). Mirror: `t420cparent`.

## 2. Portage 3.0.82.2 captures (see `portage-{A,B,C}.txt`, full argv line 1)

- A: `emerge -pv --package-moves=n --usepkgonly=y --binpkg-respect-use=y
  dev-libs/t420parent` → rc 1, `All ebuilds that could satisfy
  "dev-libs/t420dep" … have been masked` + one cpv-deduped row
  `- dev-libs/t420dep-1::t420repo (masked by: use flag configuration
  mismatch)` + chain `(dependency required by "…t420parent-1::t420repo"
  [binary])` / `(… "dev-libs/t420parent" [argument])`.
- B: same flags with `--binpkg-changed-deps=y dev-libs/t420cdep` → rc 1,
  same block shape, reason `(masked by: changed deps)`, no chain lines.
- C: `--binpkg-changed-deps=y dev-libs/t420cparent` → rc 1, the (B) block
  plus the `[binary]`/`[argument]` chain. All three cells re-ran
  identically live (modulo the timing line).

## 3. Portuale diffs (see `portuale-{A,B,C}.txt`; streams combined)

- A: `emerge: there are no binary packages to satisfy "dev-libs/t420dep"
  …` + the same two chain lines. Diff: masked block ↔ plain miss;
  chains already match.
- B: bare `emerge: there are no binary packages to satisfy
  "dev-libs/t420cdep".` Diff: whole masked block missing.
- C: the plain miss + the same two chain lines. Diff: masked block
  missing; chains already match.

## 4. Code to change

- `resolve/pretend.rs:1249-1278`: both `binary_candidates.retain(...)`
  (respect-use gate, changed-deps gate) silently drop rejects before any
  report sees them. Record per-atom `(candidate, reason)` for
  version-matching rejects instead of dropping the evidence.
- `resolve/pass/top_level_unsatisfied.rs:178-264`: the top-level report
  re-derives rejects from re-listed pools and only knows the respect-use
  gate (`all_binary_masked_report_with_respect_use`, `masked_deps.rs:682`).
  Add the changed-deps twin (reason `changed deps`); better, consume the
  recorded rejects from selection so report and selection cannot drift.
- `resolve/pass/unresolved_dep.rs:510-631` (`record_miss_disclosure`):
  consults only ebuild pools (`masked_candidates_for_atom`); binary-pool
  rejects are invisible → dependency plain miss. Add the recorded
  binary rejects here, rendered with the existing chain walk (Portage:
  `_show_unsatisfied_dep` over the bintree, `depgraph.py:~6658-6663`,
  reasons from `ignored_binaries`).
- Risk: the masked helpers currently re-list pools; carrying rejection
  reasons through selection touches the hot path and must keep the
  cpv-dedup Portage shows (cell A: one row for two builds) and the
  `by_str` ebuild/binary key collapse. Failing-first pins: (B) top-level
  today, (A)/(C) dependency disclosure today.
- Size: one slice if top-level changed-deps rides with the dependency
  disclosure (shared reject-carrying); two if split (top-level first).

Recommendation: ship as one slice — carry binary reject reasons out of
selection and disclose them top-level and on the dependency edge.
