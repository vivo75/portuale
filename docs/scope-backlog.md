# Scope backlog

**Not** a Python-vs-Rust parity backlog. Every slice ships on both sides in
one commit, verified byte-identical via the shared contract suite before it
counts as done (`agent-context.md`'s "portability of change, not of
source"). ~1160 cases pass across all suites (869 in the `emerge`-pretend
contract file); an inventory scan (CLI flag tables, function-level
architecture, `--json` fields, git history) still finds zero
Rust-vs-Python behavioural gaps.

This file inventories real portage behaviour **not yet ported to either
side** — deliberate, documented scope cuts and `agent-context.md`
architecture boundaries. It deliberately carries **no shipped-item
narrative**: **`what-this-proves.md` is the authoritative record of what
has shipped**, `git log` is the slice-by-slice history. Re-verify any
entry here against both before trusting it.

> **Compaction passes:** 2026-08-31 (moved the per-slice shipped
> narrative to `what-this-proves.md`), 2026-09-03 (purged the narrative
> that had re-accreted — the pre-purge snapshot is
> [`history/scope-backlog-2026-09-03.md`](history/scope-backlog-2026-09-03.md)),
> 2026-09-05 (purged again after the A. Resolver / F. Whole `emerge`
> actions / `os.lchown` passes — pre-purge snapshot is
> [`history/scope-backlog-2026-09-05.md`](history/scope-backlog-2026-09-05.md),
> which also carries the fuller investigation trail for each remaining
> cut below; `git log` has the same detail per-commit).
> The structure: a compact "already done" summary (Part 1), the
> genuinely-remaining work (Part 2), the standing non-goals (Part 3), and
> an honest distance-to-parity assessment (Part 4).

---

## Part 1 — already shipped (one-paragraph summary)

The core `emerge` / `ebuild` loop is **real and live** — it resolves,
builds, merges, and unmerges real Gentoo packages (verified end to end
against an actual tree inside `TEST/`'s container). Shipped, at a
capability-area level (see `what-this-proves.md` for the cited-source
detail of each):

- **`--pretend` dependency resolution** — the full atom / slot / sub-slot
  / USE-dep grammar; `||` groups; every `*DEPEND` key; the `--update` /
  `--deep` / `--newuse` / `--changed-*` / `--with-*` / `--exclude` /
  `--newrepo` / … selection family; every `package.*` file, repo-scoped
  across main **and** overlays; the whole `env.d → repo → features →
  pkginternal → defaults → conf → pkg → env` `USE_ORDER` chain;
  `USE_EXPAND`; `REQUIRED_USE`; keyword / license / PROPERTIES / RESTRICT
  masking; slot‑operator rebuild edges; blocker + slot‑conflict
  detection; a `'backtrack` retry loop that reconciles solvable slot
  conflicts, masks unsolvable ones (`runtime_pkg_mask`), and reports the
  rest with real portage's own notice; the full `resolver/output.py`
  bracket layout + ANSI colour + counters + `--tree` / `--columns`; the
  whole `--autounmask*` read-only family; bare command-line names
  (`emerge eix` → `app-portage/eix`); `--json` provenance trace.
- **Real ebuild phase execution** — the full `pkg_pretend → … → install`
  chain via an embedded `brush` driving unmodified `bin/*.sh`; real
  eclass `inherit()`; real `SRC_URI` fetch (Manifest digests, `mirror://`
  + custom/third-party mirrors, `RESTRICT=mirror`/`fetch`, resume).
- **Real filesystem mutation** — `ebuild <file>` merge / unmerge / qmerge
  / package / config / info / prerm / postrm; real `CONFIG_PROTECT`,
  `collision-protect` / `protect-owned`, preserve-libs (full `LinkageMap`
  computation, wired into merge **and** unmerge), `env_update()` /
  `ldconfig`, fifo/device `CONTENTS` nodes, `os.lchown`/`os.chown`
  ownership preservation.
- **`emerge` itself, non-`--pretend`** — `emerge <atom>` source
  build+merge (New / Upgrade / Downgrade / Reinstall, in-place same-slot
  replace); `--getbinpkg` / `--getbinpkgonly` remote download+merge;
  `--buildpkgonly`; `FEATURES=buildpkg` / `--buildpkg`; `--keep-going`;
  `emerge -jN` parallel build scheduler + `--load-average` +
  build-log capture + `--quiet-build`; `--resume` / `--skipfirst`
  (mtimedb) incl. `--resume --pretend`; `--ask` / `CLEAN_DELAY`;
  world / world_sets recording (real `create_world_atom`); `@world` /
  `@system` / `@selected` / `@installed` / `@<custom>` sets; `--oneshot`;
  `emerge -C` / `--unmerge` / `--depclean` / `--prune` / `--config` /
  `--deselect` real removal; `elog` (`echo` / `save` / `save_summary`
  modules, merge **and** removal paths); `PORTAGE_NICENESS` /
  `PORTAGE_IONICE_COMMAND` / `PORTAGE_SCHEDULING_POLICY`.
- **Standalone actions** — `--search` / `-s` / `-S` (fuzzy + regex),
  `--list-sets`, `--check-news`, `--info` (incl. `--info <atom>`),
  `--clean`, `--rage-clean`, `--regen`; every recognized-but-unimplemented
  modifier flag from the 2026-09-02 batches.
- **Binary packages** — xpak + gpkg readers/writers, `$PKGDIR` scan,
  `--usepkg` family, all six compressors, `build-info`-into-vdb metadata
  + `:=` binding, gpkg internal `Manifest` digest check.
- **Sandbox / build isolation** — `sandbox` / `usersandbox` +
  `network` / `ipc` / `mount` / `pid`-sandbox all wrap the `src_*`
  phases; the build-phase env carries the resolved `USE` + compiler/make
  flags + `package.env`'s non-USE vars.
- **Misc** — `profiles/updates/` package moves, `color.map` /
  `PORTAGE_COLORMAP`, `--quiet` verbosity level 1, `emerge --help` /
  `portuale` applet listing, `emerge --shell bash|brush` (merge, removal
  hooks, and `--config`).

---

## Part 2 — genuinely still open

### A. Resolver

Every *forward-pass* resolver feature is shipped, and the `'backtrack`
retry loop (real `_emerge/resolver/backtracking.py` shape) reconciles a
**solvable** slot conflict, masks a version to resolve an **unsolvable**
one via real `runtime_pkg_mask`, drives the full `--autounmask*` in-loop
family, the slot-operator-rebuild sub-slot cascade, and both real
`||`-preference feedback paths — see Part 1 and `what-this-proves.md`
for the cited detail. What remains is architectural — a single-pass BFS
can't grow into these incrementally:

- **"Backtracking exhausted" diagnostics** — *narrower than it looks:*
  the `--backtrack=30` advisory-hint gating already ships; real's
  remaining signal is the `Dependency resolution took X s (backtrack:
  N/M).` report line, whose timing is non-deterministic (a deliberate
  cut — portuale is a deterministic tool).
- **Slot-collision notice's remaining cuts** — `pkg_use_display` for a
  package with non-default USE **shipped 2026-09-05**: every instance
  header and every shown parent line now carries that package's own
  `pkg_use_display(pkg, modified_use=…)` (`USE="…"` + `USE_EXPAND`
  groups, every IUSE flag, enabled-first, `( )`-wrapped for force/mask),
  via a new per-instance/per-parent `use_display` on `SlotConflict`
  (`what-this-proves.md`'s "slot-collision notice `pkg_use_display`"
  entry). Still cut, each needing new plumbing for a purely
  informational payoff: the `use`/`soname` reason keys
  (atom-vs-package USE-conditional-violation matching + soname-aware
  collision detection), operator/USE-token colorization (which faithfully
  reproduces a genuine upstream `highlight_violations` marker-drift bug —
  see `history/scope-backlog-2026-09-05.md`), and the `need_rebuild`
  "cannot be rebuilt" trailer (`_equiv_ebuild_visible`/`useoldpkg_atoms`/
  `excluded_pkgs` threading).
- **Circular-dep's remaining cuts** — full elementary-cycle enumeration /
  `large_cycle_count` (real's own richer multi-priority digraph, a
  different graph representation than portuale keeps) and the cycle-only
  `--tree` re-display (needs that same digraph fed through the entire
  `--tree` renderer); the *conditional* `followup_change` grandparent
  variant has no fixture (the *hard*-clash grandparent case does,
  2026-09-05).
- **Merge-list order, remaining sub-algorithms** — `real_discovery_order`/
  `merge_order_bias`/batched leaf selection are shipped and match real
  exactly over long contiguous runs; still open: the full-transitive-tree
  discovery-order walk (real ranks discovery across its *entire*
  already-installed transitive graph, portuale only tracks what it
  needs — the confirmed remaining cause of the one known live gap,
  `docs/what-this-proves.md`'s "Merge-list order" entries have the full
  empirical trail), real's fuller `DepPriorityNormalRange` ladder (cited
  behavior for genuinely circular graphs, confirmed not the cause of any
  observed gap), `asap_nodes`/libc-first, the `_FrontierDigraph` perf
  layer, blocker/uninstall interleaving, a full `gather_deps` port
  (researched — only reachable via a genuine unresolved runtime cycle,
  which no current fixture has).
- **`--root-deps` / multi-root, remaining edges.** *Mostly a non-gap for
  this fork* — the ebuilds are all EAPI 7+, where `--root-deps=rdeps` is
  a complete no-op and `BDEPEND`/`IDEPEND` always resolve against the
  running root (which portuale does, `--root-deps` or not). The full
  multi-root graph (a `root` per dependency edge) stays a deliberate
  edge-by-edge approximation; a running-root entry's `PDEPEND` stays a
  target-`ROOT` concern (a permanent non-gap).
- **Slot-operator rebuild v1 cuts** — single-pass (no backtracking for a
  rebuild that itself shifts another sub-slot), the rebuilt consumer's
  own `:=` deps not re-bound in the pretend graph, no `--changed-slot`
  interaction, `IUSE_EFFECTIVE` in the built-dep domain. Investigated
  2026-09-05: real's slot-operator machinery is a *reconciliation* with
  an undo path (`_slot_operator_update_probe`/`_backtrack`/etc.,
  `depgraph.py:2400-3200`); portuale's `slot_operator_rebuild_entries`
  fixpoint has no undo path at all, so "single-pass" and "no
  `--changed-slot` interaction" are the same missing piece, not two —
  see `history/scope-backlog-2026-09-05.md` for the full citations.
  `--changed-slot` itself already ships standalone (`slot_changed`).

### B. Scheduler / build orchestration — **complete (2026-09-04)**

Merge-hook log capture, shared tokio runtime + kill-in-flight builds +
`PORTAGE_LOGDIR`, `mtimedb["resume"]` rotation + binary-entry replay,
`--ask` TTY/colour/re-prompt, `elog` `syslog`/`custom`,
`PORTAGE_SCHEDULING_POLICY` confirmed a non-issue (no forkserver
equivalent in an OS-thread scheduler) — see `what-this-proves.md`'s
"Scheduler / build orchestration" entry for the cited detail. Only
documented simplifications remain (`FEATURES=compress-build-logs`,
`mail`/`mail_summary` elog modules, a resumed binary entry always
resolving from the local `$PKGDIR`), none observed to matter.

### C. Config resolution depth — **complete (2026-09-03)**

The whole `env.d → repo → features → pkginternal → defaults → conf → pkg
→ env` `USE_ORDER` chain is modelled, per-profile-level `defaults`
interleaving included; the build-phase env carries the resolved `USE` +
compiler/make flags + `package.env`'s non-USE vars. Remaining are
documented simplifications only, none observed to matter: env-layer
`USE_EXPAND` values are last-wins into `scalars`, not genuinely
incremental; no per-file `${VAR}` expand map for `package.env` /
`env.d`; `FEATURES` is a last-wins scalar, not real incremental
stacking; `env.d` is read relative to `config_root`, not a distinct
`eroot` (they coincide in every tested and typical configuration).

### D. Sandbox / build isolation — **complete (2026-09-04)**

The whole `FEATURES` isolation set wraps the six real `src_*` phases
(`unshare` + `sandbox`): `RESTRICT=network-sandbox`/`PROPERTIES=live`/
`test_network` exemptions, `AI_ADDRCONFIG` loopback addresses, real
`FEATURES` passthrough to the phase env (`bin/estrip`/`__dyn_test`/etc.
now actually gate correctly), `Packages`-index `USE` back-fill for
`emerge -b` — see `what-this-proves.md`'s "Sandbox / build isolation"
entry for the cited detail. SELinux sandbox and `userpriv`/`fakeroot`
are confirmed non-goals (Part 3). Build flags / resolved USE stay
`""`/absent for a standalone `ebuild <file> <phase>` (no graph) and for
`emerge --resume` (`resume_entry` carries none) — both need a resolved
graph this deep, a documented gap.

### E. Binary packages / fetch — **substantially complete (2026-09-04)**

Remote-binhost MD5 indexing, gpkg mtime-staleness revalidation +
`BUILD_ID` basename, binpkg-multi-instance selection (`--binpkg-
respect-use`/atom-`[use]` filtering/`dedup_binary_instances`),
`--binpkg-changed-deps`/`--rebuilt-binaries`/`_equiv_ebuild_visible`,
PKGDIR-index locking, `FEATURES=buildpkg-live`, real `EbuildBinpkg`
failure semantics (2026-09-04), and the `BUILD_ID` env-var export that
unblocked both the archive's own embedded `build-info/BUILD_ID` file
and `FEATURES=packdebug` (2026-09-05, real `EbuildBinpkg._start`'s own
`if "binpkg-multi-instance" in features` condition, exactly), and
`FEATURES=binpkg-multi-instance` writing/scanning for **both** formats
(2026-09-05 — real `bintree._allocate_filename_multi`'s `<cat>/<pn>/
<pf>-<build_id>.<suffix>` subdir layout, `.xpak` extension for xpak: an
xpak multi-instance file turned out byte-format-identical to a `.tbz2`,
not the "bare metadata segment" the earlier deferral assumed; the fix
also corrected gpkg multi-instance, which portuale had been writing one
directory level too shallow) all
shipped — see `what-this-proves.md`'s "Binary packages / fetch" entry
for the cited detail.

`identical_binary` (bug #354441, real `depgraph.py:8001-8014`) was
**investigated 2026-09-05 and found not to be a portuale bug**: real's
`identical_binary` guards against real rejecting an *installed built
instance* for ebuild-invisibility and then merging the available binary
in its place. Portuale's resolver has no rejectable "installed package"
candidate — `_equiv_ebuild_visible` only ever filters *binary*
candidates, and "already installed" is a pure vdb-membership check
(`candidate_is_installed`) — so a binary at the installed version is
classified `AlreadyInstalled` directly, for a matching *or* differing
`BUILD_TIME`. Verified empirically (ebuild removed / keyword-dropped /
package.mask'd, `--selective` vs bare top-level) — pinned by
`test_usepkg_binary_of_a_since_removed_ebuild_is_not_reinstalled`. The
one narrow residual real-divergence: ebuild gone **and** a
differing-`BUILD_TIME` binary at the installed version — real reinstalls
it, portuale keeps installed (which is exactly what `--rebuilt-binaries`
opts into); left as a deliberate cut.

Still open: `.sig` verification/signing
(`FEATURES=binpkg-signing` — no crypto crate, a real cut); a
`BUILD_TIME`-vs-installed reinstall
trigger outside `--rebuilt-binaries` (the residual divergence above),
`useoldpkg` multi-instance, and
the explicit `--binpkg-changed-deps=y|n`/`--use-ebuild-visibility`
overrides (a ~30-to-90-call-site plumbing job for a rarely-used
explicit override of an already-automatic default — deferred). Binpkg
`SHA1` (no sha1 crate) and fetch candidate ordering/`RESTRICT=
primaryuri` (determinism > a non-observable mirror-selection detail)
are deliberate, pre-existing cuts documented in their own module doc
comments.

### F. Whole `emerge` actions

The action and modifier-flag surface is broadly complete — `--regen`
stale-entry pruning + eclass masters-chain lookup, `--check-news` real
`.unread`/`.skip` write-back, `--info <atom>`'s `( )` force/mask wrap +
ANSI USE colour all shipped 2026-09-05, see `what-this-proves.md`'s
"Whole emerge actions backlog" entry for the cited detail. Remaining:

- `--info`: the host-state half (version header, uname/mem, tool-version
  probes, `info_pkgs`, timestamps) is a documented cut; the installed
  block still reads the individual vdb `build-info` files, not
  `environment.bz2`. The `(non-installed binary)` candidate path and the
  `pkg_info()` phase run itself both shipped 2026-09-05: `--usepkg
  --info` now selects the highest local `$PKGDIR` binary that defines
  `pkg_info()` and renders its `(non-installed binary) was built with the
  following:` block, and for every selected ebuild/binary/installed
  package that defines `pkg_info()` portuale prints
  `>>> Attempting to run pkg_info() for '<cpv>'` and actually runs the
  phase. The deterministic message is dual-language contract-tested; the
  phase's own output is Rust-only (`test_portuale.py`), the same
  test-architecture split `--config`/`--regen` use. v1 cut: an installed
  match with an entirely empty vdb `DEFINED_PHASES` is not attempted
  (real's falsy-check quirk still would);
- `--regen`: `--jobs` threading stays unimplemented on purpose — real's
  scheduler parallelism only changes wall-clock time, not the cache
  content written, so there's no correctness gap to close;
- `--check-news`: versioned/slotted `Display-If-Installed` atoms are
  matched in full now (2026-09-05, real `DisplayInstalledRestriction.
  checkRestriction` → `vardb.match`); remaining v1 cuts are narrow — a
  `[use]`-dep in the atom isn't post-filtered (the same `match_from_list`
  scope every portuale caller has), a malformed atom is an unsatisfied
  restriction rather than an invalid item, and the `News-Item-Format`
  1.x/2.x EAPI atom-validity gate isn't applied (`portage_dep` has no
  EAPI parametrization, Part 3);
- `--metadata` is an architectural no-op (portuale reads
  `metadata/md5-cache` directly, models no `depcachedir`);
- `--sync` is a permanent non-goal (points at `emaint sync`); GLSA /
  `@security` is not in scope.

### G. Shell backend

- minimize + report the brush `declare -f` heredoc bug upstream (it
  corrupts a function with a redirected here-doc, which is why the
  default backend is `bash`, not the embedded `brush`);
- periodic re-pin to keep up with upstream `reubeno/brush` `main` (see
  `brush-pin.md`'s checklist).

---

## Part 3 — explicit non-goals / architecture boundaries

Standing decisions, not oversights.

- **`--autounmask-write`** and any config-*writing* autounmask mode
  (conflicts with "never writes config" — the read-only suggest/resolve
  half is shipped). Note `emerge --deselect` / `-C` / `--depclean` *do*
  write `world` / `world_sets` / the vdb now — those are user state the
  package manager owns, not `/etc/portage` config.
- **A real backtracking resolver** (see Part 2.A — listed there because it
  is the substantive gap, here because `agent-context.md` scoped it out for v1).
- **PyO3 / in-process FFI embedding** — would foreclose the
  two-sibling-implementations end state.
- **EAPI 0/1/2/3/4/6** — dead in this repo; the `portage-*` crates have no
  EAPI parametrization at all within the 5+ floor.
- **`bsd_chflags`** — `None` on non-BSD; portuale is Linux-only/musl-static.
- **RPM binary packages, repo syncing (`emerge --sync`), news items,
  GLSA/`@security`, GPG signing/verification, Prefix/cross-`ROOT` beyond
  the `ESYSROOT` distinction** — not in scope.
- **`equery` / `portageq` / `etc-update` / `dispatch-conf`** — separate
  tools, separate binaries.
- **Directory merge traversal order** — sorted by filename for test
  determinism, not real `os.listdir()`'s arbitrary/OS-dependent order.
  `CONTENTS` line order carries no semantics portage itself relies on
  (unmerge re-sorts, `qmerge`/`qlist` sort on read), and determinism is
  worth more here than bug-compatible arbitrariness — see
  `ebuild_merge.rs`'s module doc comment.
- **Switching CLI option parsing to `clap`** — evaluated 2026-09-02,
  rejected. The parser (`pretend.rs`'s parse loop + `emerge_options.rs`
  tables) faithfully reproduces `emerge`'s `argparse` quirks that `clap`
  has no idiom for: optional values consumed only when they look like an
  integer (`--deep[=N]`, `--jobs[=N]`, `--backtrack[=N]`), `true_y_or_n`
  (bare / `=y` / `=n` / space `y`/`n`) vs `y_or_n` (required),
  `action:"append"` atom lists where each occurrence is itself
  space-split, `-pX requires an argument and can't be bundled`, and the
  exact real error strings. It also carries the
  recognized-but-unimplemented machinery (a real emerge option reports
  "not yet implemented in portuale", not "unknown") and is kept
  structurally parallel to the Python reference so the two parsers can't
  drift. `clap` would fight every one of these; ~1500 lines across two
  languages under ~1100 contract tests, near-zero payoff.

---

## Part 4 — how far is this from a "perfect clone that installs and uninstalls"?

**Short answer: portuale already installs and uninstalls packages for
real** — `emerge <atom>` (source and binary), `emerge -C`, `--depclean`,
`--prune`, `--config`, `--deselect` all perform real filesystem mutation,
with real ebuild-phase execution, real vdb bookkeeping, real
`CONFIG_PROTECT` / `collision-protect` / preserve-libs / `env_update`, and
it has built + merged + unmerged real Gentoo packages end to end. For the
**happy path of operating on one package (or a small dependency closure)
at a time**, it is close.

The distance to a drop-in replacement is now dominated by **one** large
item, with a short incremental tail:

1. **The backtracking resolver (Part 2.A) — the architectural core is
   now in place.** The shipped `'backtrack` loop reconciles solvable
   slot conflicts, masks unsolvable ones, renders the real notices,
   tries USE/keyword autounmask levels *inside* the loop (2026-09-03),
   drives the slot-operator-rebuild sub-slot cascade to a fixpoint
   (2026-09-03, container-verified), and drives **both** real
   `runtime_pkg_mask` feedback paths — `_feedback_slot_conflict` and
   `_feedback_missing_dep` — into `||` alternative re-selection
   (2026-09-03, container-verified —
   `docs/history/or-preference-backtrack-plan.md`). What is left here is
   depth/fidelity work on the pieces already built (richer
   `_slot_conflict_backtrack` mask-target analysis, `dep_zapdeps`'
   full preference bins, deeper multi-constraint interplay), not a
   missing mechanism.

2. **The rest of Part 2** — the remaining gpkg-signing/xpak-multi-instance
   gaps (2.E), the `--info` host-state half (2.F, a fixture-driven test
   can't verify real host state anyway), the brush `declare -f` upstream
   fix (2.G). Each is one focused slice, the rhythm portuale already
   runs at.

Config-resolution depth (2.C), sandbox isolation (2.D), and scheduler /
build orchestration (2.B) are complete; the action/flag surface (2.F)
is substantially complete.
