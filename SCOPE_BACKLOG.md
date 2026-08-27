# Scope backlog

This is **not** a Python-vs-Rust parity backlog. Every slice in this pilot is
implemented on both sides in the same commit and verified byte-for-byte
identical via the shared contract suite before being considered done
(`PORTING/PROMPT.md`'s own "portability of change, not of source" hard goal).
733 contract tests pass as of this writing; an inventory scan (CLI flag
tables, function-level architecture, `--json` fields, git history) still
finds zero Rust-vs-Python gaps.

What this file inventories is real portage behavior this pilot hasn't ported
to **either** side yet — deliberate, documented scope cuts or explicit
`PROMPT.md` architecture boundaries.

> **Re-derived 2026-08-27** against current source (`portage-repo`,
> `portage-profile`, `portage-use-reduce`, `portuale/src/fetch.rs`,
> `git log`), not carried forward from the previous version. The original
> file was written 2026-08-17 (commit `578246278`) and never updated across
> the ~90 `PORTING/` commits since; **almost every item it listed has since
> shipped**. Part 1 records what closed; Part 2 is the genuinely-remaining
> work; Part 3 is the explicit non-goals. Re-verify against
> `README.md`/`git log`/the actual source before trusting even this version.

---

## Part 1 — shipped since 2026-08-17 (the original 21 items)

| # | Original item | Status | Landed by |
|---|---|---|---|
| 1 | Sub-slot modeling (`SLOT="0/5"`) | **shipped** | `9c926033f` (`Candidate::sub_slot`, real `_match_slot` sub-slot check; fixed a silent dependency-match bug) |
| 2 | Structured (non-flat) `use_reduce` | **shipped for the depgraph** | `59237ccbb` (`||` groups: resolve only the first satisfiable alternative) + `3ca7a66b4` (`subset=` for `--with-test-deps`). `DepNode`/`build_dep_tree`/`use_reduce_flat_disjunctive` wired into both real dep-walk sites (`lib.rs:5266`/`5569`). *Residual:* `--changed-deps` still compares flat atom sets, not structured trees — see Part 2. `flat=False`/`opconvert` genuinely never needed. |
| 3 | `repos.conf` `masters` (repo inheritance) | **shipped** | `04601e1a9` (implicit main-repo default) + explicit `masters =` chain resolution (`RepoConfig::masters`, `find_repos`) + `f7057b159`/`5a7bbeff7` (eclass `inherit()` across the masters chain). *Residual:* `layout.conf`'s own `masters =` key and `profile-formats` gating — see Part 2. |
| 4 | Per-level/per-source config precedence (real `USE_ORDER`) | **partly shipped** | `6fa34677f`/`992a82117`/`5f7c6f059` (real `USE_ORDER` precedence for global force/mask, implicit IUSE, `+/-` defaults). `package.mask`/`.unmask`/`.accept_keywords` now stack per-source (`stack_mask_lines`, `[repo, profile-chain, user]`). *Residual:* `package.use`'s full `configdict["repo"]`/`["defaults"]` per-level interleaving with each level's `make.defaults`, and the `env`/`pkginternal`/`features`/`env.d` `USE_ORDER` layers — see Part 2. |
| 5 | `--changed-slot` | **shipped** | `97a27a317` (`slot_changed`, real `_changed_slot`, as an independent `Reinstall` trigger) |
| 6 | `--with-test-deps` | **shipped** | `3ca7a66b4` (real `use_reduce` `subset={"test"}` via `use_reduce_flat_subset`) |
| 7 | Overlay repos' own `package.mask`/`.unmask`/`profiles`/`license_groups` | **shipped** | `b6c386ef6` + `6e368f28f` (overlay `package.mask`/`.unmask`, `::repo`-auto-scoped) + `9a03b8734` (overlay `package.use`/`.mask`/`.force`/`.stable.*`). Overlay `license_groups` reaches in via cross-repo profile parents (#10) — the same sourcing real `LicenseManager` uses (`profile_locations`), so it is *not* a gap. |
| 8 | `package.use`'s own full `USE_ORDER` precedence | **partly shipped** — same residual as #4 | `9a03b8734` (all three sources stacked, flat model) |
| 9 | `--deselect` world_sets/custom-set integration | **shipped** | `2ba3c8a5f` (`emerge --deselect @set` against the combined `world_set`) |
| 9b | Real `Atom.intersects()` algebra for `--deselect` | **shipped** | `7406bae50` (dropped the narrower category/package check + a bogus installed-check) |
| 10 | Cross-repo profile parents (`reponame:path` / bare `:path`) | **shipped** | `afd1a210c` (`expand_parent_colon`/`repo_containing`, real `LocationsManager._expand_parent_colon`) |
| 11 | `USE_EXPAND` corners | **partly shipped** | `66a8a7703` (`USE_EXPAND_UNPREFIXED` — real, load-bearing: it is how `amd64`/`x86`/`arm64` exist as USE flags at all). *Residual:* `USE_EXPAND_HIDDEN`/`_IMPLICIT` and IUSE-aware `_*` wildcard expansion (`linguas_*`) — see Part 2. |
| 12 | `accept_keywords_defaults` bare-atom substitution | **shipped** | `743cd9b4a` (bare `package.accept_keywords` atom → implicit `~arch` at both profile and user level) |
| 13 | `strip_libc_deps` in `--changed-deps` | **shipped** | `b29600063` |
| 14 | `--changed-deps-report` | **shipped** | `69ca60846` (real cosmetic "you might want `--changed-deps`" notice, its own `--json` `changed_deps_report` array) |
| 15 | `--with-bdeps-auto` | **shipped** | `c505df6eb` |
| 16 | Real atom-grammar wildcards/build-ids | **descoped** (not a gap) | Decision recorded: the bounded `*/*`/`category/*`/`*/package` matcher is sufficient for `package.mask`-style matching; full wildcard/glob/build-id atoms never reach `DEPEND`/`RDEPEND` parsing. Not on the backlog anymore. |
| 17 | `--autounmask*` family | **read-only suggestion mode shipped** | `2003e020d` (`--autounmask` keyword suggestion) + `927402f3f` (extended to a dependency's own `NoVisibleCandidate`) + `--autounmask-use`/`--autounmask-keep-keywords`/`--autounmask-write` recognition. *Residual:* `--autounmask-write` itself (writes files) — a `PROMPT.md` "never writes" boundary, see Part 3. |
| 18 | `--root-deps`/cross-`ROOT` dependency resolution | **substantially shipped** | Real `ESYSROOT`-vs-`ROOT` distinction, `running_root_satisfies_atom`, `||` branch selection fed by running-root satisfiability, `93327d274`, `356088e6c` (recursive build-entry, first increment), `678a8875d` (output marking). *Residual:* the recursion follow-up — see Part 2. |
| 19 | Binary package support | **local `PKGDIR` shipped** | `3099d9adf` (`--usepkg`/`--usepkgonly`/`--binpkg-respect-use`) + `96d8fbccb` (`--usepkg-exclude`/`-include`) + `0ae1f8be6` (`--rebuilt-binaries`) + `0b18b2140` (downgrade detection) + `7e5a380d7` (real `ebuild … package` builds an xpak binpkg) + real `PORTAGE_COMPRESSION_COMMAND`. *Residual:* `--getbinpkg`/`--getbinpkgonly` (remote), `gpkg` format, `BUILD_ID`/splitdebug/packdebug/RPM, PKGDIR-index locking — see Part 2. |
| 20 | Real ebuild phase execution | **shipped** | `eeecd96cd` (the `actionmap_deps` phase chain via embedded `brush`) + `2f5a3ddad`/`39907fee6` |
| 21 | Real merge/install/filesystem mutation | **shipped** | `2f5a3ddad` (`merge`) + `2a52f7d88` (`unmerge`) + `qmerge`/`config`/`info`/`prerm`/`postrm` + real `CONFIG_PROTECT`/`collision-protect`/`preserve-libs`/`env_update` |

---

## Part 2 — genuinely still open

Ranked roughly by how self-contained each is.

### A. Small, self-contained dry-run/config slices

1. **`layout.conf`'s own `masters =` key** (and `profile-formats` gating).
   `repos.conf`'s `masters =` is fully resolved; `layout.conf`'s equivalent
   isn't read at all (`RepoConfig::masters` doc comment, `lib.rs:108-110`).
   Cross-repo profile parents (#10) are also allowed unconditionally here
   rather than gated on the current node's repo declaring
   `profile-formats = portage-2` in `layout.conf` (`portage-profile`
   module doc, "Cross-repo profile parent references … gated in real
   portage on … `layout.conf`").

2. **`USE_EXPAND_HIDDEN` / `USE_EXPAND_IMPLICIT`.** Real `emerge --info`
   display-only concerns (`elibc_*`/`kernel_*` implicit-flag regex modeling).
   Named as out of scope in `portage-profile`'s module doc (line ~199).

3. **IUSE-aware `_*` wildcard expansion** (e.g. `linguas_*` in `package.use`
   or `USE`). Needs a specific package's own `IUSE`, which global config
   resolution has no access to — would have to move into `portage-repo`'s
   per-candidate `effective_use_flags` layer, the same way slotted
   `package.use` matching already did.

4. **`--changed-deps` structured (non-flat) tree comparison.** Currently a
   deliberate flat-atom-set difference (`deps_changed`, `lib.rs:2518-2543`)
   — a `||`-group reordering that real portage's structured
   `_changed_deps` would flag as changed is not caught here. A documented,
   narrow approximation; closing it means giving `deps_changed` the same
   `DepNode` tree machinery `use_reduce_flat_subset`/`_disjunctive`
   already built.

### B. `--root-deps` recursion follow-up

5. ~~**Walk the running-root build entry's own further dependencies.**~~
   **Shipped 2026-08-27** (`resolve_root_deps_build_entries`): a
   running-root build entry's own `DEPEND` + `BDEPEND` + `RDEPEND` are
   walked against the running root recursively, cycle-guarded by the
   existing `root_deps_build_seen` set; an unbuildable, not-installed
   build dep is now surfaced as its own `NoVisibleCandidate` entry
   rather than swallowed. **Residual:** `IDEPEND` of a running-root
   build entry (real portage resolves it against the running root too),
   and the full multi-root graph architecture (a `root` carried per
   dependency edge) this pilot still approximates edge by edge.

### C. Binary packages / fetch

6. **Remote binpkg fetching** — `--getbinpkg`/`--getbinpkgonly` and the
   remote `PKGDIR`-index/`Packages` negotiation they need. Recognized but
   unimplemented; a real debug trace is vendored at
   `PORTING/helpers/emerge_-1v_--debug_--getbinpkgonly__sys-fs--fuse.log`
   for reference.

7. **`gpkg` binary package format** (`bin/gpkg-helper.py`,
   `lib/portage/gpkg.py`). `BINPKG_FORMAT` is hardcoded `xpak`
   (`ebuild_package.rs:327`). `FEATURES=verify-sig` (GPG) lives here too —
   it is a `gpkg`/repo-sync concept, **not** `SRC_URI` fetch (the earlier
   backlog mis-scoped it).

8. **`BUILD_ID` / `splitdebug` / `packdebug` / RPM**, and PKGDIR-index
   locking. All named as cuts in `ebuild_package.rs`.

9. **Fetch: resume support** (`RESUMECOMMAND`'s retry-with-`-c`), **live
   per-mirror `layout.conf` negotiation**, **`RESTRICT=mirror` /
   `RESTRICT=primaryuri`**, real candidate ordering/shuffling. All named
   as cuts in `fetch.rs:28-40`.

### D. Config-resolution `USE_ORDER` depth

10. **`package.use` full per-level `USE_ORDER`.** Real repo-level
    `package.use` belongs in `configdict["repo"]` and profile-level in
    `configdict["defaults"]` (merged per-level with that level's own
    `make.defaults` USE); this pilot flattens all three sources into one
    incremental list. Also missing: the `env`, `pkginternal`, `features`,
    and `env.d` `USE_ORDER` layers entirely (`portage-profile` module
    doc, "Only the `defaults` and `conf` layers … are implemented").
    A genuinely bigger undertaking — the flat `Config` model has no
    per-layer structure at all.

### E. brush / shell backend

11. **brush strategy #2** — rewrite this repo's own `bin/*.sh` to avoid
    brush-hostile constructs. Low-risk, immediately effective for this
    tree, doesn't preempt real-world ebuilds.

12. **brush strategy #3** — a fork-tracking doc for `vivo75/brush`.
    PR [reubeno/brush#1274](https://github.com/reubeno/brush/pull/1274)
    (brace-less function bodies) **merged upstream 2026-08-20** (`18851e7`);
    the fork now carries only the pipeline-function-stage-deadlock fix
    (`c78ea429`, branch `fix/pipeline-function-stage-deadlock`), which has
    no upstream PR yet. `portuale/Cargo.toml` pins the fork by exact rev
    with no record of upstream-vs-fork-only status and no periodic rebase.

### F. preserve-libs

13. **The one live-`scanelf` branch inside real `LinkageMapELF.rebuild()`**
    (`LinkageMapELF.py:233-324`) — orphaned preserved libs with no
    `NEEDED.ELF.2` entry. Everything else in `rebuild()`/`findConsumers()`/
    `_find_libs_to_preserve()` is ported (`needed_elf.rs`). Deliberately
    excluded, **confirmed with the user each time it comes up**: it is the
    one real spot a raw ELF-header read (not `scanelf` output) would
    matter.

---

## Part 3 — explicit non-goals / architecture boundaries (`PROMPT.md`)

Not oversights — standing decisions, listed for completeness.

- **`--autounmask-write`** (and any file-*writing* autounmask mode).
  Conflicts with the pilot's "never writes config" invariant. The
  read-only "suggest changes" half is shipped (#17).
- **Virtuals as dedicated code / backtracking.** Virtuals are ordinary
  packages with an any-of `RDEPEND`, already handled; a real backtracking
  resolver is out of scope.
- **PyO3 / in-process FFI embedding.** Would foreclose the
  two-sibling-implementations end state (`PROMPT.md` "Open / deliberately
  undecided").
- **EAPI 0/1/2/3/4/6.** Dead in this repo — every profile is EAPI 5+, and
  the `portage-*` crates go further with no EAPI parametrization at all
  within the 5+ floor.
- **`bsd_chflags`.** `lib/portage/__init__.py:311` sets it to `None`
  unconditionally on non-BSD; the pilot is Linux-only/musl-static.
- **RPM binary packages, repo syncing (`emerge --sync`), news items.**
  Not in scope.
