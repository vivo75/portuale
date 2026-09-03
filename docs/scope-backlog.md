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
> [`history/scope-backlog-2026-09-03.md`](history/scope-backlog-2026-09-03.md)).
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
  `ldconfig`, fifo/device `CONTENTS` nodes.
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

Every *forward-pass* resolver feature is shipped. What remains is
architectural — a single-pass BFS can't grow into these incrementally:

- **Backtracking.** `resolve_pretend_graph` is a `'backtrack` retry loop
  (real `_emerge/resolver/backtracking.py` shape): it reconciles a
  **solvable** slot conflict, masks a version to resolve an **unsolvable**
  one via real `runtime_pkg_mask` (trial-and-revert), exposes
  `--backtrack=COUNT`, and renders the real slot-collision (now with
  `collision_reasons` grouping, one-representative-per-reason selection,
  `--verbose-conflicts`, the `^` marker line, and the `(and N more …)` /
  `NOTE:` trailer) and circular-dependency notices. **Still open** (the
  deferred half):
  - "backtracking exhausted" diagnostics — *narrower than it looks:* the
    `--backtrack=30` advisory-hint gating already ships; real portage's
    remaining signal is the `Dependency resolution took X s (backtrack:
    N/M).` report line, whose timing is non-deterministic (a deliberate
    cut — portuale is a deterministic tool);
  - autounmask USE/keyword levels tried *in sequence inside* the loop;
  - autounmask parent-flip re-resolve feeding `extra_constraints`;
  - `||`-preference / slot-operator-rebuild feedback driving a retry;
  - the slot-collision notice's remaining cuts: `pkg_use_display` for a
    package with non-default USE (the ` USE=""` slot renders, non-empty
    flag lists don't), the `use`/`soname` reason keys, operator/USE-token
    colorization, the `need_rebuild` "cannot be rebuilt" trailer;
  - the circular-dep cuts: the reduced cycle-only `--tree` re-display,
    `_find_suggestions`' ~180-line USE-flag heuristic, full
    elementary-cycle enumeration / `large_cycle_count`.

  *(The resolver-extraction item shipped 2026-09-03: the ~1700-line
  graph walk + backtracking loop is now `backtracking_resolve(req:
  &ResolveRequest)` behind a `trait Resolver` / `BacktrackingResolver` /
  `active_resolver()`; `resolve_pretend_graph` is a thin 44-arg
  marshaller. Self-contained and runtime-swappable — a different
  resolver architecture is one `impl Resolver` + an `active_resolver`
  branch away, no call-site changes.)*

  `agent-context.md` lists a real backtracking resolver as out of scope
  for v1; the shipped loop nonetheless takes it from "detects and reports
  conflicts" to "reconciles solvable conflicts, masks unsolvable ones,
  and reports the rest with real portage's own notice". The items above
  are the substantive remainder.

- **`--root-deps` / multi-root, remaining edges.** *Mostly a non-gap for
  this fork* — the ebuilds are all EAPI 7+, where `--root-deps=rdeps` is
  a complete no-op and `BDEPEND`/`IDEPEND` always resolve against the
  running root (which portuale does, `--root-deps` or not). The full
  multi-root graph (a `root` per dependency edge) stays a deliberate
  edge-by-edge approximation; a running-root entry's `PDEPEND` stays a
  target-`ROOT` concern (a permanent non-gap).

- **Slot-operator rebuild v1 cuts** — single-pass (no backtracking for a
  rebuild that itself shifts another sub-slot); the rebuilt consumer's
  own `:=` deps not re-bound in the pretend graph; no `--changed-slot`
  interaction; `IUSE_EFFECTIVE` in the built-dep domain.

- **`package.provided` depclean corner** — a provided CPV as a depclean
  root, and the `-pc` advisory's "will be removed by depclean even if in
  world" wording.

- **Bare command-line names, remaining shapes** — a versioned or slotted
  bare name (`emerge eix-1.2`, `emerge eix:0`) is not category-qualified
  (real `dep_expand`'s `null/`-insertion path handles those); real's
  non-`--quiet` `ambiguous_package_name` runs a full `search` before the
  `!!!` lines (portuale emits only the deterministic list).

### B. Scheduler / build orchestration

**Substantially complete.** Remaining:

- the merge step's `pkg_*` hooks still run uncaptured through brush
  (residual stderr noise under `-jN` / `--quiet-build`);
- one tokio runtime per `run_commands`; killing in-flight builds on a
  hard failure; `PORTAGE_LOGDIR` / `split-log`;
- `mtimedb["resume"]`: `resume_backup` rotation; the build-time-flag half
  of `myopts`; binary-entry replay;
- `--ask`: TTY gating, prompt colour, re-prompt on a bad answer;
- `elog`: `mail` / `mail_summary` (out of scope — a real SMTP client +
  MIME assembly), `syslog` / `custom` (unported);
- `PORTAGE_SCHEDULING_POLICY` reaches only this process (real
  `apply_priorities` also does the `multiprocessing` forkserver pid).

### C. Config resolution depth — **complete (2026-09-03)**

The whole `env.d → repo → features → pkginternal → defaults → conf → pkg
→ env` `USE_ORDER` chain is modelled, per-profile-level `defaults`
interleaving included; the build-phase env carries the resolved `USE` +
compiler/make flags + `package.env`'s non-USE vars.

Remaining are documented simplifications only, none observed to matter:
env-layer `USE_EXPAND` values are last-wins into `scalars`, not
genuinely incremental; no per-file `${VAR}` expand map for
`package.env` / `env.d` (real portage seeds one from the global config);
portuale's `FEATURES` is a last-wins scalar (modelled via
`feature_enabled`), not real incremental stacking; `env.d` is read
relative to `config_root`, not a distinct `eroot` (they coincide in
every tested and typical configuration).

### D. Sandbox / build isolation — **substantially complete**

The whole `FEATURES` isolation set wraps the six real `src_*` phases
(`unshare` + `sandbox`). Remaining (deliberate cuts):

- `RESTRICT=network-sandbox` / `PROPERTIES=live` / `test_network`
  exemptions (no USE-reduced `RESTRICT`/`PROPERTIES` in the phase env);
- the `AI_ADDRCONFIG` loopback addresses; SELinux sandbox;
- `userpriv` / `fakeroot` (single-user dev/test context — see the
  `os.lchown` note below);
- various **non-isolation** `FEATURES`, forced to `""` in the phase env:
  `ccache`, `distcc`, `splitdebug`, `installsources`, `nostrip`/`strip`,
  `compressdebug`, `test` gating beyond `src_test` running, the
  `preserve-libs` live-`scanelf` orphan branch. A scoped real-`FEATURES`
  passthrough is a separate slice;
- build flags / resolved `USE` are still `""`/absent for a standalone
  `ebuild <file> <phase>` (no graph) and for `emerge --resume`
  (`resume_entry` carries none); the `Packages` *index* `USE` field for
  an `emerge -b` binpkg isn't back-filled from build-info.

### E. Binary packages / fetch

- **Remote binhost** — live `layout.conf` negotiation,
  `RESTRICT=primaryuri` interleave, `Packages.bz2` / `.lz4`, binpkg
  `SHA1` (no sha1 crate).
- **gpkg** — `.sig` verification + signing (`FEATURES=binpkg-signing` —
  cut, portuale has no crypto), bare `.xpak` multi-instance,
  `binpkg-multi-instance`, mtime-staleness index revalidation, `BUILD_ID`
  in the basename.
- **`BUILD_ID` / `splitdebug` / `packdebug` / RPM**, PKGDIR-index
  locking, `FEATURES=buildpkg-live`, real `EbuildBinpkg` failure
  semantics under `--keep-going`.
- **Fetch** — real candidate ordering / shuffling.

### F. Whole `emerge` actions

The action and modifier-flag surface is broadly complete. Remaining:

- `--info`: the host-state half (version header, uname/mem, tool-version
  probes, `info_pkgs`, timestamps) is a documented cut. For `--info
  <atom>`: the installed block reads the individual vdb `build-info`
  files, not `environment.bz2`, and skips the `( )` force/mask wrapping;
  the `(non-installed binary)` case, the `pkg_info()` phase run itself,
  and ANSI colour on the `USE=` line are all cut;
- `--regen`: sequential (no `--jobs` threading), no stale-entry pruning,
  `_eclasses_` from this repo only (no masters chain);
- `--check-news`: no `.unread` / `.skip` *write-back*,
  `Display-If-Installed` only;
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

### H. Misc / cosmetic

- `os.lchown` / privilege-preserving ownership is not reproduced (needs
  root, which portuale's single-user context never has — it would only
  no-op); directory merge traversal is sorted for test determinism, not
  real `os.listdir()` order (`CONTENTS` line order carries no semantics
  portage relies on). Both are deliberate — see `ebuild_merge.rs`'s
  module doc comment.

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

1. **The backtracking resolver (Part 2.A).** The shipped `'backtrack`
   loop reconciles solvable slot conflicts, masks unsolvable ones, and
   renders the real notices — but it does **not** try USE/keyword
   autounmask levels *inside* the loop, or let `||`-preference /
   slot-operator-rebuild feedback drive a retry. An upgrade that needs
   portage to juggle all of those together still exceeds portuale. This
   is the one piece of architectural work left between "installs and
   uninstalls packages" and "is portage".

2. **The rest of Part 2** — the scheduler tail (2.B), the deliberate
   sandbox and `FEATURES` cuts (2.D), the remote-binhost / gpkg-signing
   gaps (2.E), the `--info` host-state half and `--regen` threading
   (2.F), the brush `declare -f` upstream fix (2.G). Each is one focused
   slice, the rhythm portuale already runs at.

Config-resolution depth (2.C) is complete; the action/flag surface (2.F)
and sandbox isolation (2.D) are substantially complete.
