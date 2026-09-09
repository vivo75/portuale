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
  + `:=` binding, gpkg internal `Manifest` digest check, gpkg `.sig`
  signing (`FEATURES=binpkg-signing`) + merge-time GPG verification.
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
  entry).   The `use` reason keys have since **shipped observably**
  (`dev-libs/slotusegroup` fixture: unconditional `[y]` before violated
  `[x]`, `^` spans, no color; see `what-this-proves.md`): classification,
  display selection, unconditional-first ordering, and USE-token markers
  all work dual-language, byte-identical. The `need_rebuild` trailer code
  is also landed but still dormant (no fixture can trigger it yet). Still
  cut or blocked:
  - operator/USE-token colorization (which faithfully reproduces a
    genuine upstream `highlight_violations` marker-drift bug — see
    `history/scope-backlog-2026-09-05.md`; portuale marks the uncolored
    string, so markers stay aligned -- observable only under
    `--color y`);
  - the `soname` reason key -- **unreachable, not merely unimplemented**:
    `portage-dep` cannot parse soname atoms and dep flattening drops
    unparseable tokens, so no soname parent atom can ever reach the
    collision renderer (a deliberate non-gap, not a cut);
  - the `need_rebuild` fixture (blocked on the vdb walk below);
  - **resolver substrate, shipped or scoped** (all verified live against
    real portage): slot-reuse USE re-verification **shipped** (a
    `>=T-1.0[x]` parent no longer silently reuses a resolved x-off
    instance -- it pulls a second instance and reports the conflict;
    the backtrack solvability pre-check is USE-aware too); conflict
    records merge per (slot, existing, current) triple (real keeps one
    handler per slot); puller filing is USE-aware and subslot-carrying
    (built-`:=` parents no longer vanish);
    `--dynamic-deps=n` not switching the AlreadyInstalled walk source
    via the CLI (both languages walk ebuild deps; the
    `dynamic_deps_picks_ebuild_vs_vdb_deps` unit test disagrees --
    unexplained, needs owner eyes);
    literal bound `:=` atoms accepted in ebuilds (real masks them
    "improper context for slot-operator built atom syntax" -- an atom
    validation gap; it also rules out ebuild-carried `:=` fixtures).
  - instance display order (resolved-first vs real's arbitrary
    set-iteration order -- pre-existing, no semantic content).
- **Circular-dep's remaining cuts** — full elementary-cycle enumeration /
  `large_cycle_count` (real's own richer multi-priority digraph, a
  different graph representation than portuale keeps) and the cycle-only
  `--tree` re-display (needs that same digraph fed through the entire
  `--tree` renderer). The *conditional* `followup_change` grandparent
  variant has a fixture now (`dev-libs/fucyclea`/`fucycleb`/`fucyclec`,
  see `what-this-proves.md`); the *hard*-clash case has had one since
  2026-09-05.
- **`emerge --pretend --debug`: real's resolver trace — shipped
  2026-09-07.** All six stages plus the header/cycle dumps, dual-language
  (`portage-repo/src/resolver_trace.rs` + the `emerge_pretend_reference.py`
  mirror), on real's own stdout/stderr split: `Arg:`/`Atom:`, the
  per-atom `ebuild:`/`installed:` candidate list, the per-package
  `Parent:`/`Depstring:`/`Priority:`/`Candidates:` / `Child:`/`Parent
  Dep:` / `Virtual Parent:` / `Exiting...` narration, the `forced
  reinstall atoms:` / `slot operator dependencies:` / `forced rebuilds:`
  summaries, and the `\ndigraph:\n\n` + `debug_print()` merge-digraph
  dump + `runtime cycle digraph` dumps. Deliberate divergences (goal is
  duplicated info, not a byte-copy of real; contract pins Rust==Python):
  plain-text node labels, portuale's post-prune closure + pseudo-arg
  nodes as the node set, BFS-ordered narration (successful pass only),
  ebuild+installed candidates only, `abi_rebuilds`-only slot-op dump.
  See [`emerge-pretend-debug.md`](emerge-pretend-debug.md) and
  `what-this-proves.md`.
- **Merge-list order, remaining cuts.** The `_serialize_tasks` port
  itself shipped 2026-09-06 (`portage-repo/src/merge_order.rs`): a typed
  `DepPriority` digraph, the `DepPriorityNormalRange`/
  `DepPrioritySatisfiedRange` `ignore_priority` ladder, `_merge_order_bias`
  + `_find_deep_system_runtime_deps`, `find_smallest_cycle`/`gather_deps`,
  `asap_nodes` (`PDEPEND` promotion), and real's own
  `_dep_disjunctive_stack` deferral of `||`/`virtual` deps. Verified
  live against real portage: `net-libs/rest` 15/15, `sys-devel/gcc`
  14/14, `app-crypt/gnupg` 14/14 exact-position. The `asap_nodes`
  *libc-first* seeding (real merges the `virtual/libc` / `virtual/os-headers`
  provider asap, bug #303567 / #328317) **shipped 2026-09-07**
  (`merge_order::seed_toolchain_asap`: the graphed `virtual/libc` /
  `virtual/os-headers` entry's `RDEPEND` providers seed `asap_nodes`
  before the selection loop, os-headers first). `--implicit-system-deps=n`
  **shipped** (the `_merge_order_bias` early return, threaded
  CLI → `ResolveRequest` → `serialize_merge_order` on both sides; see
  `what-this-proves.md`). Still open: the `_FrontierDigraph` perf layer
  and blocker/uninstall interleaving (a `--pretend` merge graph has no
  uninstall nodes to interleave).
- **`_complete_graph` as graph *nodes*.** Its reverse-dependency
  **atoms** shipped 2026-09-07 (`reverse_dependency_constraints` — a vdb
  reverse scan fed into the `'backtrack` loop's `slot_constraints`,
  closing the `media-libs/libdisplay-info` membership divergence; see
  `what-this-proves.md`). What is still not ported is real's actual
  re-walk, which carries every installed package `@world`/`@system`
  reaches as a nomerge graph *node* — 1854 for a case whose own closure
  is 461. The `_serialize_tasks` validation showed those nodes are not
  needed for ordering, and nothing observed now needs them for membership
  either; they would matter for a divergence that depends on an installed
  package's *position* in the graph rather than on its recorded atoms.
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
(`candidate_is_installed`) — so a binary at the installed version was
classified `AlreadyInstalled` directly, for a matching *or* differing
`BUILD_TIME`. Verified empirically (ebuild removed / keyword-dropped /
package.mask'd, `--selective` vs bare top-level). The one narrow
residual real-divergence — ebuild gone **and** a differing-`BUILD_TIME`
binary at the installed version → real reinstalls it — **shipped
2026-09-08** (see the 2.E entry above): the matching-`BUILD_TIME` half
stays `AlreadyInstalled` (`identical_binary`), the differing half now
reinstalls under `--update`, pinned by
`test_usepkg_binary_of_a_since_removed_ebuild_is_reinstalled_only_when_it_differs`.

`--useoldpkg-atoms` + `binpkg-multi-instance` was likewise
**investigated 2026-09-05 and found already correct**: `dedup_binary_
instances` collapses each `cpv:slot::repo` group to its highest-
`(satisfies-atom-use, BUILD_TIME, BUILD_ID)` instance *before* the
`--useoldpkg-atoms` filter runs on the matched set, so the newest
multi-instance old binary is the one preferred over the newer ebuild —
matching real's `_iter_match_pkgs` newest-first + `break`. Pinned by
`test_useoldpkg_atoms_picks_the_newest_multi_instance_old_binary`
(rust ≡ python). The one narrow spot: `useoldpkg_atom_matches` matches
by `cat/pkg-version` only (a `:slot`/`[use]` in a `--useoldpkg-atoms`
atom isn't post-filtered), the same `match_from_list` scope every other
portuale caller has.

The explicit `--binpkg-changed-deps=y|n` and `--use-ebuild-visibility`
overrides **shipped 2026-09-05** — the "~30-to-90-call-site plumbing
job" the earlier deferral feared evaporated once done with the same
env-free process-global pattern `--useoldpkg-atoms` / `--package-moves`
use (two `RwLock`/`AtomicBool` statics + setters in `portage-repo`, one
`true_y_or_n` parse block in `pretend.rs` mirroring the `--rebuild-if-*`
one, and the two filter conditions in `resolve_pretend` /
`resolve_pretend_graph` gaining a `binpkg_changed_deps_active(usepkgonly)`
/ `use_ebuild_visibility()` term). `--binpkg-changed-deps=n` keeps a
stale binary `--getbinpkg` would reject; `=y` forces the check under
`--usepkgonly`. `--use-ebuild-visibility` enforces `_equiv_ebuild_visible`
on a built candidate even under `--usepkgonly` / a `--useoldpkg-atoms`
match (real `depgraph.py:8027`'s `not use_ebuild_visibility and
(usepkgonly or useoldpkg)` guard). Dual-language; 2 dedicated contract
tests + 6 `CASES`.

**quickpkg multi-instance shipped 2026-09-06**: the
`FEATURES=unmerge-backup` `quickpkg_from_vdb` path now honours
`FEATURES=binpkg-multi-instance` too — real `bin/quickpkg` -> `bintree.
inject` -> `getname(..., allocate_new=True)` -> `_allocate_filename_multi`
gives it the `<pkgdir>/<cat>/<pn>/<pf>-<build_id>.<suffix>` subdir path
(reusing `allocate_binpkg_build_id` from the earlier multi-instance
work), and the `BUILD_ID` env export + `Packages` field flow through.
The idempotency check also moved from bare-filename existence to real
`_quickpkg_dblink`'s own "any existing binpkg at this cpv+`BUILD_TIME`"
(`Packages`-index scan). Rust-only (execution), `test_portuale.py`.

**`.sig` signing/verification shipped 2026-09-08**
(`FEATURES=binpkg-signing` via the system `gpg` subprocess — no crypto
crate needed, the musl-static story untouched; see `what-this-proves.md`):
the real helper signs at package time (detached `.sig` sidecars +
clear-signed `Manifest`, real `BINPKG_GPG_SIGNING_*` passthrough, real
`!!! {var} is not set` pre-check), and every merge verifies
(`GOODSIG` + ultimate/full trust required, `request`/`ignore`-signature
`FEATURES` honored). Deliberate residual cuts: no dropped-privilege
`gpg` spawn when root, no `shlex`/`varexpand` for the command template,
no per-binrepo `verify-signature = false` at merge time, no GPG on the
pool-populate read (merge-time enforcement only).
**`BUILD_TIME`-vs-installed reinstall shipped 2026-09-08** (the 2.E
residual, closing the `identical_binary` del: 2560): with the ebuild
gone from the tree, `_equiv_ebuild_visible` (`depgraph.py:7399`) +
the `identical_binary` gate (`depgraph.py:7999-8030`) make real reject
the installed built instance and merge a binary at the installed
version over it when that binary's `BUILD_TIME` differs -- gated by
`--update` (real's `avoid_update = "--update" not in myopts`;
`depgraph.py:7826`; without `--update`, real's final
`if avoid_update:` keeps the installed package). Ported as
`binary_reinstall_warranted` (Rust) / `_binary_reinstall_warranted`
(Python), wired into the main `--update` already-installed path only,
with `ebuild_visible_at` querying the *tree* (not the pool, so it stays
correct under `--usepkgonly`). See `what-this-proves.md`.
Binpkg
`SHA1` **shipped 2026-09-09** (the `sha1` crate exists -- RustCrypto,
pure Rust, zero C linkage, musl-static story untouched; see
`what-this-proves.md`): both `Packages` writers now record `MD5` **and**
`SHA1` (real `_pkgindex_hashes = ["MD5", "SHA1"]`), and
`download_and_verify` verifies each digest field present (real
`_get_digests` + `digestCheck`), removing the file and failing on a
`SHA1` mismatch exactly like `MD5`. Fetch candidate
ordering/`RESTRICT=
primaryuri` (determinism > a non-observable mirror-selection detail)
is the remaining deliberate, pre-existing cut, documented in its own
module doc comment.

### F. Whole `emerge` actions

The action and modifier-flag surface is broadly complete — `--regen`
stale-entry pruning + eclass masters-chain lookup, `--check-news` real
`.unread`/`.skip` write-back, `--info <atom>`'s `( )` force/mask wrap +
ANSI USE colour all shipped 2026-09-05, see `what-this-proves.md`'s
"Whole emerge actions backlog" entry for the cited detail. Remaining:

- `--info` **config-layer completeness shipped 2026-09-07**: portuale now
  reads `cnf/make.globals` (the base db, via a new multi-line-quote /
  apostrophe-comment-safe `logical_lines`), `/etc/profile.env` (the
  `env.d` db — `CONFIG_PROTECT*` fragments + scalars like `LANG`/`LEX`),
  and `<PORTDIR>/profiles/info_vars` (the extra `myvars` names). The
  `const.INCREMENTALS` displays (`FEATURES`, `CONFIG_PROTECT`,
  `CONFIG_PROTECT_MASK`, `ENV_UNSET`) are `-*`/`-tok`-resolved then
  sorted, exactly as real `config.regenerate()` stores them
  (`Config::resolved_incremental`); `USE_EXPAND` variable display values
  are USE-consistent-resolved (`-* intel …` → `intel …`,
  `GRUB_PLATFORMS`); `CBUILD` defaults to `CHOST`, `PORTAGE_CONFIGROOT`
  is stamped; env-only `info_vars` (`SHELL`) fall through to the process
  env. `Binary Repositories:` now shows `location` + `verify-signature`
  in real `BinRepoConfig.info_string()` field order. Verified
  byte-exact against a live `emerge --info` for **every** `VAR="…"` line
  + both repo blocks. Dual-language, contract-pinned
  (`test_info_stacks_make_globals_profile_env_and_info_vars`).
  **Follow-on shipped same day**: (b) and (c) below are now closed —
  `UseManager.extract_global_USE_changes` (the `*/*` user-`package.use`
  fold onto global USE, USE_EXPAND shorthand included) + global
  `use.force`/`use.mask` applied to the `--info` USE line and USE_EXPAND
  values (`resolved_global_use`, real `regenerate()`'s trailing
  `myflags.update(useforce); difference_update(usemask)`); and
  `RepoConfig` gained `sync_type`/`sync_uri`/`volatile`/
  `module_specific_options` with the global `/usr/share/portage/config/
  repos.conf` merged under the user's, so the `Repositories:` block
  prints real `info_string()`'s fields. The `USE=` line and both repo
  blocks now `diff`-clean against a live run.
  **Host-state header shipped 2026-09-07** (the last `--info` piece):
  the `Portage <ver> (python…, <profile>, <gcc>, <libc>, <kernel>)` line
  (portage/glibc from vdb, gcc from `gcc -dumpversion`, profile a
  faithful `get_profile_version` port, kernel from `uname`), the
  65-char rule (+ centred `System Settings` title under `--info <atom>`),
  `System uname:` (real `platform.platform(aliased=1)` rebuilt from
  `uname` + `/proc/cpuinfo` + glibc), `KiB Mem:`/`KiB Swap:`
  (`/proc/meminfo`), per-repo `Timestamp of repository`
  (`metadata/timestamp.chk`) + `Head commit of repository` (`git
  rev-parse HEAD` for a git repo), the `sh:`/`coreutils:`/`ld:` probes,
  and the `info_pkgs` version table (six hardcoded atoms +
  `profiles/info_pkgs`, one-level `expand_new_virt` for
  `virtual/os-headers`, `<ver>::<repo>` rows). `diff <(emerge --info)
  <(portuale emerge --info)` is now a **single line** — the `KiB Mem`
  free value, which changes between the two process spawns. The
  contract's `--info` `rust==python` checks run through a
  `_normalize_info` regex filter that blanks the host-state values to
  `XXX` first. The 1-byte trailing-newline mismatch is fixed. `--info`
  is now byte-for-byte parity with real modulo genuinely-live memory.
- `--info`: the
  `(non-installed binary)` candidate path and the `pkg_info()` phase run
  itself both shipped 2026-09-05: `--usepkg --info` now selects the
  highest local `$PKGDIR` binary that defines `pkg_info()` and renders
  its `(non-installed binary) was built with the following:` block, and
  for every selected ebuild/binary/installed package that defines
  `pkg_info()` portuale prints `>>> Attempting to run pkg_info() for
  '<cpv>'` and actually runs the phase. The deterministic message is
  dual-language contract-tested; the phase's own output is Rust-only
  (`test_portuale.py`), the same test-architecture split
  `--config`/`--regen` use. The empty-`DEFINED_PHASES` falsy-check quirk
  (real `actions.py:2350`: an installed match with no `DEFINED_PHASES`
  file at all still gets `pkg_info()` attempted, while `"-"` does not) is
  matched now too (2026-09-06). The installed block's `CHOST`/
  `CFLAGS`/… **shipped 2026-09-08**: read from the vdb
  `environment.bz2` via real `_aux_env_search` (a pure-Rust `bzip2`
  backend in `portage-repo` -- default `bzip2` 0.6 over
  trifectatechfoundation's libbz2-rs-sys, zero C linkage, so the
  musl-static story and the subprocess-free boundary both hold; the
  C `bzip2-sys` backend stays off), including the `var_assign_re` /
  multi-line-continuation parser, the missing-file-means-all-`Unset:`
  rule, and the present-but-empty-matches-empty-prints-nowhere rule
  (see `what-this-proves.md`);
- `--regen` — **complete (2026-09-09)**: `--jobs`/`--load-average`
  threading (`thread::scope` dispatch with real `AsyncScheduler` /
  `PollScheduler._can_add_job` semantics + the per-builddir key
  serialization matching real `doebuild()`'s `EbuildBuildDir` lock;
  byte-identical cache *and* stdout vs serial; `--jobs=0` = CPU count
  per real `main.py:1023-1041`); the `_pull_valid_cache` shortcut (skip
  the `depend` phase when the on-disk entry is already valid --
  performance only, content-identical; the file is left untouched,
  mtime preserved); and `metadata_regen_retry`'s `cp_retry` (re-run a
  whole cp whose phase failed with an *unexpected* returncode --
  exit-code semantics: the phase's own exit verbatim, setup failures
  code 1, up to 3 passes, first-seen cp order for determinism; a
  finally-failed cpv is dropped from the valid set so no stale entry
  survives). One deliberate divergence, documented in `regen.rs`:
  real `emerge --regen`'s own `action_regen` never retries -- the loop
  is `egencache`'s path, folded into portuale's single regen tool.
  See `what-this-proves.md` for the cited detail.
 - `--check-news`: versioned/slotted `Display-If-Installed` atoms
   (2026-09-05), a `[use]`-dep in the atom (2026-09-07, checked against
   the matched version's vdb `IUSE`/`USE` via `use_deps_satisfied` —
   `portage_repo::installed_pkg_iuse_and_use`), a malformed atom
   making the whole item invalid (2026-09-07, moved into
   `news_item_valid`), and the `News-Item-Format` 1.x/2.x EAPI
   atom-validity gate (2026-09-08, real `isValid`'s `eapi="0"`/`"5"`
   split) are all handled now — the gate is implemented as a narrow
   field check (1.x rejects `:slot`/`:sub`/`:=`/`:*`/`[use]`, 2.x keeps
   parse-at-all), *not* a `portage_dep` EAPI parametrization, so no
   Part 3 non-goal is crossed (the backlog's earlier "needs EAPI
   parametrization" premise turned out stale);
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

### H. The `mrg` applet

`mrg` (2026-09-06: a clap front end over portuale's own emerge codepath;
started as a parse-only first slice) is the deliberate counter-example
applet — its requirements are NOT emerge/ebuild's: it is allowed to lean
on major mainstream crates (see Part 3's `clap` bullet, which applies
only to the hand-rolled `emerge` parser — `mrg` is where clap lives on
purpose). Current state and what's inside each of the following bullets:
- the clap-backed parser covers the full real `lib/_emerge/main.py`
  option surface (actions incl. shorts, `options` booleans, the real
  `longopt_aliases`, required-value choice options, `append`
  repeatables), with real `insert_optional_args` semantics for
  `--deep`/`--jobs`/`--load-average` (`require_equals` +
  `join_optional_values`) and real-emerge-style exit codes (0 success/
  help, 2 usage error); it is Rust-only (no Python reference — the
  black-box surface tests live in `tests/test_portuale.py` + Rust
  unit tests in `mrg.rs`);
- on a successful parse `to_emerge_argv` translates the match into
  canonical long-form argv and hands it to `pretend::run` (the exact
  function the `emerge` applet runs): resolution output and exit codes
  are literally emerge's, byte-identical for the same invocation.
  `Flag`s forward BARE; implemented `Value`/`Append` options forward
  `--long=<value>` (per occurrence); options the codepath does not
  implement yet forward BARE so `report_option` reports them by their
  real spelling ("a real emerge option, but is not yet implemented",
  exit 2); the optional-value bare forms (`"True"`) and `-j y`/`-j n`
  forward BARE too (that is their real unlimited meaning);
- documented cuts (module doc comment + `what-this-proves.md`): the
  y/n optional-value *family* (`--ask`/`--verbose`/`--quiet`/…,
  `--buildpkg`, `--usepkg`, …) is modelled as plain flags so
  `-av pkg`/`-pv pkg` never swallow the atom — the explicit
  `=y`/`=n` spellings are not parsed yet; `--jobs`'s separate-value
  y/n forms (`-j y`, `--jobs y` → value `y`) join like real's
  `valid_integers_or_y_or_n` and forward BARE;
- open: nothing — `mrg` already runs the emerge codepath's real
  resolution. Everything globally still open for that codepath
  (see the other Part 2 sections) is open for `mrg` too, by
  definition.

**Director contracts (2026-09-06, eight slots 2026-09-08)**: `mrg` is
more than a front end —
it is the **director**, orchestrating interchangeable components, one
per part of portage. The `rust/mrg-director` crate is the contract
layer (no runtime behaviour): `Resolver` (re-exported, never
re-defined — a `portage_solver`/`pubgrub`/`resolvo` backend is one
`impl` plus an `active_resolver` branch), `PackagesDb` (installed-db /
vdb read side), `RepoCache` (`cache/template.py::database` read side;
`sqlite`/`anydbm`/`volatile` are future backends), `Fetcher`
(per-file `SRC_URI` materialization), `MergeEngine`
(`MergeListItem`-dispatch-shaped `execute(unit, ctx) -> outcome`),
plus the `Director` wiring struct (`plan()` = solver delegation).
Each slot names its single current implementation as the marker to
replace. **The three previously-named-only future slots shipped
2026-09-08**: `BinpkgIndex` (real `bintree.py`'s two real backends,
both now behind the trait — local `$PKGDIR` [`PkgdirBinIndex`] and
remote `PORTAGE_BINHOST` [`RemoteBinhostIndex`], a genuine second
implementation per slot; both delegate to the `portage_repo::BinaryIndex`
reads the `g` bracket column already runs), `NewsSet`
(`portage/news.py::Item.isRelevant`/`isValid` — `MetadataNews` marker;
the real evaluation stays in `pretend.rs::run_check_news`; a GLSA
`@security` selector would satisfy the same seam but remains gated on
`@security` entering scope, Part 3), and `SchedulerPolicy`
(`_emerge/Scheduler.py::Scheduler._run`'s jobs + `--load-average` gate —
`LoadAwarePolicy` marker implementing the serial/gated default inline).
The `Director<S, D, C, F, M>` struct is now `Director<S, D, C, F, M, B,
N, P>` carrying all eight slots; contract tests grew from seven shape
pins to eleven. **RepoCache second backend shipped (H.14, first slot
after `BinpkgIndex`)**: `VolatileCache` (real `cache/volatile.py`'s
dict backend, `deepcopy` in/out) next to `Md5Cache` (real
`flat_hash.py`) behind the same three reads — see `what-this-proves.md`.
What remains is landing further *second* implementations per slot —
PackagesDb, Fetcher, MergeEngine, NewsSet, SchedulerPolicy, one slice
each — new-algorithm work, not new-seam work
(see `what-this-proves.md`'s "`mrg` director contracts" entry).

**`--solver=` alternate backends**: the solver slot's
second and third implementations have landed — `active_resolver_for`
selects `BacktrackingResolver` (portage, default) or lu-zero's PubGrub /
resolvo bridges (`portage-repo/src/solver_bridge.rs`) at runtime via the
portuale-only `--solver=<portage|pubgrub|resolvo>` on `emerge`/`mrg`
(parsing mirrored in `emerge_pretend_reference.py`; non-portage values
are Rust-only there). **Visibility filtering shipped 2026-09-08**: the
bridges now resolve from the same `is_visible`-filtered pool as the walk
(`LazyRepo::load_versions` drops keyword/license/mask/PROPERTIES/RESTRICT-
invisible candidates before the md5 read), so an invisible version is
never offered to either engine. Remaining V1 depth cuts, each a future
slice: slot-conflict/autounmask/`:=`-rebuild/blocker/circular notices,
engine-native failure text, merge-order fidelity beyond plan install
order.

**Hard invariant: `mrg` is a portuale-only applet. There will never be
a portage counterpart or Python reference implementation.** Only its
own Rust code and CLI surface matter.

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
  GLSA/`@security`, GPG for sync/webrsync (`sync-*-verify-signature`)
  and for xpak (which has no signature mechanism at all),
  Prefix/cross-`ROOT` beyond
  the `ESYSROOT` distinction** — not in scope. (gpkg binpkg
  `.sig` signing/verification itself is shipped — see Part 2.E.)
- **xpak (`.tbz2`) binary packages for `mrg`** — out of scope. `mrg`
  targets the modern gpkg format, not full binary-format compatibility;
  xpak is the old format. The shared server-side binpkg reader happens to
  open `.tbz2`, so it may work incidentally on `mrg`, but it is never a
  tested or guaranteed `mrg` path (`emerge`/`ebuild` keep their own local
  xpak support, which is fully in scope and tested). See
  `docs/remote-merge.md` §1 / §14.
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
  languages under ~1100 contract tests, near-zero payoff. This applies
  to the **`emerge`/`ebuild` parsers only** — the new `mrg` applet
  (Part 2.H) is the deliberate counter-example and *does* use clap.

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
   `docs/history/or-preference-backtrack-plan.md`), and (2026-09-07)
   prefers an already-installed `||` alternative (real `dep_zapdeps`'s
   `preferred_installed` choice bin — fixes the live `emerge -puD @world`
   abort on `virtual/wine` → `wine-vanilla`). What is left here is
   depth/fidelity work on the pieces already built (richer
   `_slot_conflict_backtrack` mask-target analysis, the *finer*
   `dep_zapdeps` bins — `in_graph`/`any_slot`/`unsat_use_*`/`other_*` —
   and its full `all_use_satisfied` computation, deeper multi-constraint
   interplay), not a missing mechanism.

2. **The rest of Part 2** — the remaining 2.E tail (fetch
    candidate ordering), the `--info` host-state half (2.F, a
    fixture-driven test can't verify real host state anyway), the brush
    `declare -f` upstream fix (2.G). Each is one focused slice, the
    rhythm portuale already runs at.

Config-resolution depth (2.C), sandbox isolation (2.D), and scheduler /
build orchestration (2.B) are complete; the action/flag surface (2.F)
is substantially complete.
