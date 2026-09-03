# agent-context.md "Open backlog" — pre-2026-09-03 snapshot

> **HISTORIC.** This is the `### Open backlog` section of
> [`../agent-context.md`](../agent-context.md) (lines 331–1039) as it
> stood on 2026-09-03, immediately before it was replaced by a short
> pointer. The section had grown to ~700 lines of per-slice
> "recently closed" / `~~struck-through~~ — **shipped DATE**` narrative
> — the same drift that `scope-backlog.md` fought, and the same fix.
>
> Nothing here is lost: every shipped item is recorded, with its
> cited-source grounding, in [`../what-this-proves.md`](../what-this-proves.md)
> and in `git log`; the genuinely-remaining work is in
> [`../scope-backlog.md`](../scope-backlog.md) Part 2. This snapshot is
> kept only so the exact wording of each slice's scope cuts stays
> recoverable.

---

### Open backlog (re-derived 2026-08-25, preserve-libs control-flow wiring, CONFIG_PROTECT "confmem rejected", preserve-libs NEEDED.ELF.2 pruning, fifo/device node CONTENTS, and --root-deps recursive-build-entry (first increment) items updated 2026-08-26 — treat any earlier version of this list as void)

Every item this list previously carried (`--usepkg-exclude`/
`--rebuilt-binaries`, `opt=`/`opt?` USE-deps, real `--tree`, the `--json`
state-change trace, explicit `repos.conf` `masters =`, `--root-deps`
satisfiability/disjunctive-branch-selection, preserve-libs control-flow
wiring, CONFIG_PROTECT's "confmem rejected this update" skip-copy,
preserve-libs `NEEDED.ELF.2` pruning on `remove_from_contents`, fifo/
device node `CONTENTS` support) has since shipped, and `--root-deps`'s
own recursive-build-against-the-running-root gap has its first real
increment shipped too (still not fully recursive — see the backlog
entry below for exactly what's left). Re-verify against
`what-this-proves.md`/`git log` before trusting even *this* version — a "scope the
next slice" round should always re-ground candidates in current code,
not just read this list. What's actually left, grouped by area:

> See also `scope-backlog.md` — a wider inventory of real portage
> behavior not yet ported to *either* side (config-resolution `USE_ORDER`
> `env`/`features`/`env.d` layers, `RESTRICT=primaryuri`, brush strategy
> #2 (rewrite brush-hostile `bin/*.sh`), …).
> Re-derived 2026-08-27; keep it in sync alongside this file when a
> slice closes one of its entries. Recently closed from it:
> **Standalone `emerge` actions buildout (Part 2.F, in progress
> 2026-09-01)**: user asked to "implement all missing flags (--info,
> --search, --regen, --metadata, --check-news, --clean ...)"; scoped via
> AskUserQuestion to "the standalone-actions batch + --regen/--metadata"
> (`--sync` stays a non-goal, modifier flags after). **Shipped so far:
> `--list-sets`** (`run_list_sets` / `_run_list_sets` — parse
> `cnf/sets/portage.conf` `[section]` headers minus the `multiset`
> generator + `/etc/portage/sets/` files) and **`--search` / `-s` /
> `--searchdesc` / `-S`** (`run_search` / `_run_search` — substring
> match over `portage_repo::all_cp` + set names, real `search.output()`
> shape, `-v` block; v1 cuts: index, `--usepkg`, full mask filter —
> **`--fuzzy-search` / `--regex-search-auto` / `--search-similarity`
> shipped 2026-09-02**, `--search` is fuzzy + regex-auto by default now,
> new `portuale/src/difflib.rs` `SequenceMatcher.ratio()` port);
> **`--check-news`** (`run_check_news` / `_run_check_news` —
> count valid+relevant GLEP 42 items per repo, minus `.read`/`.skip`
> (`.skip` read added 2026-09-02); v1 cuts: no `.unread`/`.skip`
> write-back, `Display-If-Installed` only; fixture news items in
> `fixtures/repo/metadata/news/`); **`--clean`**
> (`run_clean_pretend` -> `run_prune_nodeps_or_clean` +
> `portage_repo::clean_selection` — keep newest per slot, no portage
> self-skip) and **`--rage-clean`** (fast `--unmerge`;
> `run_unmerge_pretend` gained an `action` label); **`--info`**
> (`run_info` / `_run_info` — deterministic `Repositories:` + binrepos +
> `Installed sets:` + sorted `VAR="value"` dump; `Config` gained
> `other_vars` = the make.conf/profile scalar map; big cut: the
> host-state half — version header, uname, tool-version probes,
> `info_pkgs`, timestamps). Next: `--regen`/`--metadata` (source each
> ebuild's `depend` phase, write `metadata/md5-cache`). New
> `portage_repo::all_cp` / `clean_selection`, `Config::other_vars`.
> `--read-news` stays recognized-unimplemented.
> `test_real_action_not_implemented_message_says_action_not_option` now
> uses `--moo` as its example (was `--search`, then `--sync`); `--sync`
> now prints its own "Functionality has moved to `emaint sync`." message.
> **Sandbox / build isolation — Part 2.D substantially complete
> (2026-09-01)**: for the six real `src_*` phases
> (`SANDBOXED_SRC_PHASES`), `run_one_phase` builds a wrapped bash
> subprocess (`Isolation` / `phase_isolation` / `sandbox_wrapped_command`),
> forcing the `Bash` backend:
> `unshare <flags> --map-root-user -- sh -c '<config>; exec "$@"' _
> [sandbox] bash bin/ebuild.sh <phase>`. `FEATURES=sandbox`/`usersandbox`
> → `sandbox` binary (real `spawn_sandbox`; `SANDBOX_LOG=${T}/sandbox.log`
> + `SANDBOX_DISABLED=0` so `bin/ebuild.sh` does its own `SANDBOX_ON=1`/
> `addwrite`; non-zero-exits on a write outside the tree — and the
> `misc-functions.sh` calls are wrapped too, `sandbox-misc.log`).
> `network-sandbox` → `unshare --net` + `ip link set lo up`;
> `ipc-sandbox` → `--ipc`; `mount-sandbox` → `--mount` + `mount
> --make-rslave /`; `pid-sandbox` → `--pid --fork --mount-proc`. All
> compose; `unshare` combo validated once + cached; one-shot-warning
> degrade. New fixtures `dev-libs/netsandboxpkg` (records
> `/proc/self/ns/{net,ipc,mnt,pid}` + proc count) / `dev-libs/fssandboxpkg`.
> `Environment` gained `portage_tmpdir`. Cuts: `RESTRICT`/`PROPERTIES`
> exemptions; `AI_ADDRCONFIG` loopback addresses; SELinux; `userpriv` /
> `fakeroot`. Non-isolation `FEATURES` (`ccache`/`distcc`/`splitdebug`/
> `nostrip`/…) still unmodelled — a scoped `FEATURES` passthrough is a
> separate slice.
> **`package.use` per-level `USE_ORDER` layering (Config depth, Part
> 2.C, 2026-09-01)**: the three `package.use` sources split into
> `Config::{package_use_repo, package_use, package_use_user}` at their
> own real positions, `use_tokens` split into profile `make.defaults` +
> `conf_use_tokens`; `effective_use_flags` does the real reversed-
> `USE_ORDER` walk `repo → pkginternal → defaults → conf → pkg`. Repo
> `package.use` was wrongly the *strongest* layer before; now it's the
> weakest. New fixtures `dev-libs/repouseweakpkg` / `profileuseweakpkg`.
> Follow-ups shipped 2026-09-02: the process-`env` layer
> (`apply_env_layer` over an allowlist), `package.env`'s `USE=` half
> (`Config::package_env{,_use}`, a `pkg`-layer contribution before user
> `package.use`) **and its non-`USE` build-var half**
> (`Config::package_env_vars` → `MergeOptions::package_env_vars` →
> `emerge_build::entry_package_env_vars`, a per-package override of
> `build_config_env`'s run-wide `CFLAGS`/`MAKEOPTS`/… — build-phase only,
> no Python mirror), the main repo's `profiles/make.defaults` USE
> (`Config::repo_make_defaults_use`, head of the `repo` layer), and the
> per-profile-level `defaults`-tier walk (`Config::profile_use_layers` /
> `ProfileUseLayer`), the `features` tier (`Config::features_use` —
> `FEATURES=test` → `test`, applied between `repo` and `pkginternal`;
> new `dev-libs/featuretestpkg`, Python-mirrored), and **`$USE` at its
> real `env` position** (`Config::env_use_tokens`, the highest tier —
> `effective_use_flags` replays it after the user `package.use`, so
> `USE="-X" emerge foo` beats a `package.use` flag; was folded into
> `conf` before), and **per-overlay `profiles/make.defaults` USE**
> (`Config::repo_make_defaults_use` is now `(repo_name, tokens)` pairs;
> empty name = main = applies to all, real name = only a candidate from
> that overlay via `candidate_str`'s `::<repo>`; new
> `dev-libs/overlaymakedefaultpkg`, Python-mirrored). The **`env.d`**
> tier (`/etc/profile.env` `USE=`, the lowest `USE_ORDER` layer) closed
> 2026-09-03 — **Part 2.C is now complete**.
> Also recently closed:
> the **remote binpkg download + merge** (2026-08-31): `emerge
> --getbinpkgonly <atom>` non-`--pretend` — live `Packages` refresh
> (`bintree._populate_remote`, `wget`), binary-only resolve, download
> into `$PKGDIR` + `SIZE`-check, then merge (`binpkg::extract_binpkg`,
> `ebuild_merge::merge_binpkg` reusing `merge_tree`/`env_update`,
> `emerge_getbinpkg.rs`; `write_vdb_entry_from_dir` refactor).
> **Replacing an installed version shipped 2026-08-31**: `merge_binpkg`
> does the real merge-then-unmerge (new binpkg + vdb entry first, then
> every same-slot installed version's files unmerged with the new `PF`
> folded into `others_in_slot`, then the old vdb dir deleted);
> `run_unmerge` split into `pub(crate)` `unmerge_pkgfiles` +
> `delete_vdb_dir` so the replace reuses them phase-free;
> `run_getbinpkgonly` takes `Upgrade`/`Downgrade`/`Reinstall` too.
> **All four binpkg install/remove `pkg_*` hooks shipped 2026-08-31**:
> `extract_binpkg` keeps `environment.bz2` + `<pf>.ebuild` verbatim;
> `ebuild_phases::run_phase_from_saved_env` = `run_single_phase` + real
> `BinpkgEnvExtractor` (`bunzip2 environment.bz2 > ${T}/environment` +
> `.raw` marker, `EMERGE_FROM=binary`); `merge_binpkg` peeks metadata
> first, runs `setup`→`preinst` before the copy, then per replaced
> same-slot version `prerm`→remove→`postrm` (from that version's own vdb
> `environment.bz2`), then `postinst`, all gated on `DEFINED_PHASES`;
> new `binpkgphasepkg-1.0` + `binpkgrmpkg-{1.0,2.0}` fixtures. v1 cuts:
> no preserve-libs on replace, `SIZE`-only digest, no
> `layout.conf`/`Packages.gz`/resume. Rust-unit-tested end to end over
> loopback HTTP;
> the `[ebuild …]` **bracket + merge-order** two-part real-tree finding
> (2026-08-30, container run against a real Gentoo tree). **Increment 1
> — mask column**: the pilot gated the 7th `PkgAttrDisplay` column on
> `-v`, but real `include_mask_str()` = `verbosity > 1` and real default
> `emerge -p` verbosity is 2, so it shows at plain `-p` too, absent only
> under `--quiet` (not modelled). `attr_display_field` /
> `format_blocker_lines` render it unconditionally now; keyword/hard-mask
> markers (`~`/`*`/`#`) now visible without `-v`. **Increment 2 — merge
> order** (Model A, confirmed via AskUserQuestion): the pilot's BFS
> built `entries` parent-first; real portage's `mylist` is a topological
> merge schedule. `resolve_pretend_graph` (+ Python mirror) now re-sorts
> `entries` dependency-first as its last step
> (`topological_merge_order`, stable, cycles kept in discovery order).
> `entries` canonically merge-ordered for the flat list, `emerge
> --buildpkgonly`, and `--json` (which also stamps an explicit
> `"merge_order"` int); `--tree` re-derives from `required_by`,
> unaffected. Both increments: ~240 pinned assertions each, both
> implementations byte-identical throughout. **Follow-on slice —
> `USE="…"` at plain `-p`, DONE 2026-08-30** (real-tree finding): real
> `print_use_string = verbosity != 1` (not `-v`-gated); `-v` changes
> `all_flags` = *which* flags render. Inc 1: a `New` entry's USE line
> shows at `-p` (identical to `-pv` bar `::repo` + counters). Inc 2:
> `build_use_expand_display`/`_build_use_expand_display` grew
> `all_flags: bool`, `render_flag` returns `Option` and omits unchanged
> flags (+ the `(-flag%)` removed list) when off — so a
> `Reinstall`/`Upgrade`/`Downgrade` shows only its changed flags at
> `-p`. New `GraphEntry::use_expand_display_p` (Python re-renders at
> display time). `-pv` unchanged. ~30 pinned `-p` assertions total.
> Inc 3 (2026-08-31): `reinst_flags` — `build_use_expand_display` gained
> a `reinst_flags` set (`Reinstall::changed_flags` / real
> `_reinstall_for_flags`); a trigger flag is force-shown even when
> unchanged. Only visible at `-p`: a flag dropped from IUSE that still
> triggered a `--newuse`/`--changed-use` reinstall now shows in the
> `(-flag%)` removed list (`dev-libs/reinstdropiusepkg` fixture).
> `_create_use_string` now fully modelled bar ANSI colour);
> `license_groups` read location (**real-tree finding**, 2026-08-30 —
> the first time portuale was run against a real Gentoo tree, inside the
> new `localhost/test-portuale` container, `emerge --pretend <anything>`
> failed for every package: `resolve_config` read `license_groups`
> per-profile-chain-level, but real `LicenseManager._read_license_groups`
> (`LicenseManager.py:47`) iterates `LocationsManager.profile_locations`
> (`LocationsManager.py:432` = `[<main_repo>/profiles] + [<overlay>/
> profiles …]`) — the repo `profiles/` *bases*. `@FREE` expanded to
> nothing → every ebuild failed the license check. `resolve_config` now
> reads `<repo>/profiles/license_groups` for the main repo + every
> overlay; 2 fixture files moved out of their chain-level subdirs; after
> the fix portuale's `--pretend` package selection matches real `emerge
> --pretend` on a real tree); `RESTRICT=fetch` in the fetch path (real `fetch.py:1167` — a plain
> `SRC_URI` URI + the public mirrors are barred from the wget candidate
> list; a fetch-restricted package fetches only from an already-verified
> `DISTDIR` copy; `fetch+`/`mirror+` re-permits it; `pkg_nofetch` phase
> = a documented cut; new `dev-libs/fetchrestrictpkg` fixture,
> 2026-08-30); `emerge -pc
> <atoms> --deselect=n` (real `action_depclean`'s `deselect` — keeps
> `world` as a protection root in args mode so a named world member is
> kept; `depclean_cleanlist` `deselect` param; fixed `--deselect`
> wrongly triggering the standalone action alongside `--depclean`/
> `--prune`/`--unmerge`, 2026-08-30); the `-pC`/
> `-pP` higher-slot set-protection refinement (real `unmerge.py:421-441`
> `higher_slot` — the "still listed in package sets" warning is
> suppressed when a newer, different-slot install also matches the set
> atom; shared `still_listed_parents`, new `dev-libs/dualslotpkg`
> fixture, 2026-08-30); real
> `--autounmask` **keyword + USE** resolution (2026-08-30, two
> increments — the pilot now applies the implicit `=cpv ~arch` /
> `package.use` flip, resolves the graph, and prints real
> `_display_autounmask`'s `The following <keyword|USE> changes are
> necessary to proceed:` block, exit 0; `-pv`'s `USE=` line reflects the
> USE flip; `resolve_pretend` `autounmask_keywords`/`autounmask_use`
> params, `GraphResult::autounmask_keyword_changes`/`autounmask_use_
> changes`, `--json` arrays; `--autounmask-use=n` restores strict
> matching. **Increment 3 — the `opt=` parent flip — done 2026-08-30**:
> when a dep's `opt?`/`opt=` use-dep is unsatisfiable AND the child's
> flag is `use.mask`'d (no child flip), `resolve_pretend_graph` flips
> the *requesting parent's* flag, re-resolves the freed dep, records
> `>=<parent-cpv> -flag` (new `dev-libs/parentflip{childpkg,eqpkg}`
> fixtures). Cuts: re-resolves only the freed dep, one-level parent dep
> chain, non-`New` parent re-rendered as `New`. `--autounmask-license`
> still open);
> `mirror+`/
> `fetch+` SRC_URI prefixes (`portage_fetch` strips + records
> `override_mirror`/`override_fetch`; `mirror+` re-permits the
> `GENTOO_MIRRORS` fallback under `RESTRICT=mirror`; `override_fetch`
> inert until `RESTRICT=fetch` is modelled, 2026-08-30); `ebuild …
> package` / `emerge --buildpkgonly` emitting `gpkg` (`BINPKG_FORMAT=
> gpkg` → real, unmodified `bin/gpkg-helper.py compress`; round-trips
> through this pilot's own `read_gpkg_metadata`; signing cut, 2026-08-30);
> the `-pv`
> `USE=` flag list's natural (`_alnum_sort_key`) sort (2026-08-30);
> `alias:path` profile parents (real `get_location_for_name`, 2026-08-30 — note the
> `::alias` *atom* case was never a gap: real `match_from_list` does a
> straight name comparison, the pilot matched it already); the
> `$PKGDIR` directory-scan fallback (real `bintree._populate_local` —
> `binpkg.rs` gpkg+xpak readers + `scan_pkgdir` + `BinaryIndex`
> refactor, 3 increments, done 2026-08-30) and the `build-info`
> dependency-metadata generation it surfaced (real
> `_post_src_install_write_metadata` + whole-`build-info`-into-vdb,
> 2026-08-30); real
> `RESTRICT=mirror` (the public `GENTOO_MIRRORS` flat-layout fallback is
> skipped — `FetchOptions::restrict_mirror` — 2026-08-29); the blocker
> line's own real `output.py::_blockers` layout + `PKG_BLOCKER` colour +
> deferred ordering (2026-08-29); an installed dependency's USE-dep
> checked against its *built* `USE` (real `_iuse_implicit_built`, bug
> 640318 — 2026-08-29); the `--pretend` half of remote binpkgs
> (`--getbinpkg`/`--getbinpkgonly` — `binrepos.conf`/`PORTAGE_BINHOST`,
> cached-`Packages`-index candidates, the `g` bracket column; the actual
> download is still open); `USE_EXPAND`
> corners in full (including `emerge -pv`'s own `USE_EXPAND` grouping +
> `USE_EXPAND_HIDDEN` + installed-vs-new `*`/`%` markers + the `( … )`
> forced/masked wrap + the `[ebuild N ~]` bracket-mask column), the
> `--root-deps` recursion + `IDEPEND`
> follow-ups (including a top-level package's own
> `IDEPEND`-vs-running-root), `--changed-deps` in full (per-key +
> `strip_slots` + the structured `flat=False` comparison via new
> `portage_use_reduce::use_reduce_structured`), and
> `metadata/layout.conf` in full
> (`masters =` middle tier, `repo-name`, `profile-formats` gate) +
> `profiles/repo_name` canonical name + `aliases` + the section-name
> mismatch drop.

**Merge/unmerge subsystem** (`ebuild_merge.rs`/`ebuild_unmerge.rs`):
- **preserve-libs *registration* side** — the full real *computation*
  (`_find_libs_to_preserve`/`LinkageMap.rebuild`/`findConsumers`, ELF
  `NEEDED`/soname introspection via `NEEDED.ELF.2` plus a graph-based
  reachability algorithm) shipped across five narrow, individually-
  confirmed slices (2026-08-25/26) in new `needed_elf.rs` — see
  `what-this-proves.md`'s own "`preserve-libs` registration: the full `LinkageMap`/
  `findConsumers`/decision computation" for the full grounding of all
  five. In order: (1) real `NEEDED.ELF.2` generation via real,
  unmodified `bin/misc-functions.sh install_qa_check` (real `scanelf`,
  no ELF-parsing code at all) copied into the real vdb entry on merge —
  this also surfaced and fixed a real, broader gap (`_post_phase_cmds
  ["install"]` wasn't running at all before, affecting every install,
  not just preserve-libs) and a real fixture bug (EAPI 8's own
  `strict-keepdir` unconditionally strips a genuinely empty `dodir`'d
  directory; `envupdatepkg`/`collisionpkg-a` fixed to use `keepdir`);
  (2) `NeededEntry`, the data model for one parsed line; (3)
  `read_all_needed_entries`, real `rebuild()`'s own initial per-package
  vdb-read loop; (4) `rebuild` itself, the real soname providers/
  consumers map (multilib categorization, `$ORIGIN` runpath expansion,
  implicit-runpath inference for bundled libraries, real inode-based
  `ObjKey` dedup); (5) `getlibpaths`/`find_consumers`/`find_libs_to_
  preserve`, real `findConsumers()`'s own consumer-satisfaction logic
  and `_find_libs_to_preserve()`'s own graph-reachability decision (a
  minimal `LibGraph`, narrowed to exactly the three real `digraph`
  operations needed).
  ~~the real control-flow integration that actually wires `find_libs_to_
  preserve`'s own output into a real merge/unmerge~~ — **shipped
  2026-08-26**: `preserve_libs_on_unmerge` (new, `ebuild_merge.rs`) ports
  real `dblink._prune_plib_registry(unmerge=True, ...)`
  (`vartree.py:2228-2314`, called right before `_unmerge_pkgfiles()` at
  `vartree.py:2493`/`2529`), narrowed to the one real shape this pilot's
  own always-separate `merge`/`unmerge` CLI invocations reach
  (`unmerge_with_replacement` always `False`). `register_preserved_libs`
  ports real `PreservedLibsRegistry.register`/`.unregister` as one
  function (real `unregister` **is** `register(..., [])`).
  `ebuild_unmerge::run_unmerge` calls it right before `remove_contents`,
  threading the preserved-path set into a new `remove_contents` parameter
  that filters them out of `CONTENTS` before the per-file removal loop —
  see `what-this-proves.md`'s own "`preserve-libs` registration: wired into a real
  unmerge's control flow" for the full grounding, including the real,
  `gcc`-compiled dual-package (`libpreservetest`/`consumepreservetest`)
  end-to-end fixture proof and live-CLI verification. `needed_elf.rs`
  now has a real caller.
  ~~real `NEEDED`/`LinkageMap` bookkeeping in `unregister_preserved_libs`
  (real `removeFromContents` also strips matching `NEEDED` lines so
  stale linkage data doesn't corrupt a future preserve-libs decision) —
  moot without the registration side ever writing `NEEDED` data~~ —
  **shipped 2026-08-26, no longer moot**: `remove_from_contents`
  (`vartree.py:1244-1310`) now prunes a package's own `NEEDED.ELF.2` too
  whenever a `CONTENTS` line was actually removed, dropping any entry
  whose `filename` no longer appears among the surviving paths — see
  `what-this-proves.md`'s own "preserve-libs: real `NEEDED.ELF.2` pruning on
  `remove_from_contents`". New `NeededEntry::to_needed_line`
  (`needed_elf.rs`) ports real `NeededEntry.__str__`, the rewrite-side
  sibling of the existing `parse`/`parse_file` read side. This was the
  last documented preserve-libs gap in this immediate area.
  Deliberately still missing, confirmed with the user each time: the one
  non-`NEEDED.ELF.2`-driven branch inside real `rebuild()` itself (live
  `scanelf` for orphaned preserved libs, `LinkageMapELF.py:233-324` —
  the one real spot a raw ELF header read would matter).
- CONFIG_PROTECT/`_protect()`: no remaining gaps in this immediate area.
  Symlink protection, `--noconfmem`, `new_protect_filename`'s last-file
  reuse, the type-independent `dest_md5`/`dest_link` comparison, and now
  `_installed_instance`/`FEATURES=config-protect-if-modified` have all
  shipped — see `what-this-proves.md`'s own "CONFIG_PROTECT for symlinks,
  `--noconfmem`, and `new_protect_filename`'s own file-reuse logic",
  "CONFIG_PROTECT: a type-changing update is real-protected too", and
  "CONFIG_PROTECT: `_installed_instance`/`FEATURES=config-protect-if-
  modified`" (the latter also surfacing a real, previously-undiscovered
  default-`FEATURES` mismatch: `config-protect-if-modified` *is* one of
  `make.globals`'s own default tokens, `cnf/make.globals:79`, the same
  category the `protect-owned`/`unmerge-orphans` fix found earlier).
  `installed_instance_pf`/`owned_node_value_pf`, new, reuse the real
  per-package `COUNTER` file this pilot already writes on every merge
  rather than needing new persistence.
  ~~One remaining, deliberate v1 simplification: an already-offered,
  unmodified-since update is applied directly instead of real portage's
  own "leave the destination untouched, still record CONTENTS"~~ —
  **shipped 2026-08-26**: `protect_decision` now returns `(write_dest,
  moveme)` instead of just a path, and `merge_tree`'s own `obj`/`sym`
  branches only perform the actual copy/symlink-write `if moveme` —
  matching real `mergeme()`'s own `if moveme:` gate around `movefile()`
  (`vartree.py:5547`/`5749`), traced against the real `move_me = protected
  = bool(cfgfiledict["IGNORE"])` gate in `_protect()`
  (`vartree.py:5831-5901`) line by line. See `what-this-proves.md`'s own "CONFIG_
  PROTECT: \"confmem rejected this update\"" for the full grounding and a
  live-verified example. This closes the last documented CONFIG_PROTECT
  gap in this area.
- `unmerge` remaining cuts: none left in this immediate area.
  ~~`bsd_chflags` handling~~ — **removed 2026-08-25, was never a real
  gap**: `lib/portage/__init__.py:311` sets `bsd_chflags = None`
  unconditionally on non-BSD, and this pilot's own hard goal is
  Linux-only/musl-static — same category of mis-scoping as the earlier
  `FEATURES=verify-sig` removal below.
  ~~`INFOPATH`/`INFODIR` env-var-driven inode-match half of real
  `INFOPATH` cleanup~~ — **shipped 2026-08-25**: `env_update::
  info_dirs_inodes` collates real `INFOPATH`/`INFODIR` values from
  `/etc/env.d/*` the same way `env_update::run_env_update` collates
  every other real `COLON_SEPARATED` key, and `run_unmerge` threads the
  resulting inode set down through `remove_contents`/`remove_dirs` into
  `cleanup_info_dir` — see `what-this-proves.md`'s own "`unmerge`: real `INFOPATH`
  cleanup". (Both `FEATURES=unmerge-orphans` and the "symlink orphan"
  refinement, bug #326685 + `_unmerge_dirs()`'s own bug #640058
  recursive-parent-revisit, have also shipped — see `what-this-proves.md`'s own
  "`unmerge`'s own \"symlink orphan\" refinement (bug #326685)"` and
  "`unmerge`: real `FEATURES=unmerge-orphans`", the latter including a
  real, confirmed finding that real `_unmerge_protected_symlinks()`'s
  own delete-or-warn logic is unreachable dead code in current portage.
  `run_unmerge` also gained an `UnmergeOptions` struct in the process,
  mirroring `MergeOptions`'s own shape now that it has five fields
  instead of two loose parameters.)
  ~~per-file failure tolerance~~ — **removed 2026-08-25, was never a real
  gap**: re-reading real `_unmerge_pkgfiles()` directly
  (`vartree.py:3033-3059`) shows it `raise`s on an unexpected `errno`,
  exactly like this pilot's own current hard-fail behavior; the
  "failure counter" this item used to cite belongs to a different
  function (`dblink.unmerge()`'s own prerm/postrm phase-failure count,
  not per-file removal).
- No remaining standalone-command gaps in this area (`qmerge`,
  standalone `config`/`info`, and standalone `prerm`/`postrm` all
  shipped — see `what-this-proves.md`'s own "Real `ebuild <file> qmerge`",
  "Standalone `ebuild <file> config`/`info`", and "Standalone `ebuild
  <file> prerm`/`postrm`"). `preinst`/`postinst` deliberately stay
  internal-only (real ordering constraint tying them to `merge`'s own
  file-copy step, `dblink.treewalk()` invokes them directly around it,
  a constraint `prerm`/`postrm` don't share with `unmerge`).
- Minor: no `chown`/permission-preserving `chmod` reproduction (this
  pilot's own single-user dev/test context has no privilege-dropping
  concept anywhere else either); directory merge order is sorted for
  determinism, not real `os.listdir()`'s own arbitrary order (cosmetic,
  `CONTENTS` line order has no real semantic meaning).
  ~~fifo/device nodes: real `mergeme()` handles these too, but no
  fixture this pilot has needs them~~ — **shipped 2026-08-26**: real
  `mergeme()`'s own fifo/device `else:` branch (`vartree.py:5787-5811`)
  is real now — new `create_special_node` recreates a fresh node at the
  destination via real `mkfifo(3)`/`mknod(3)` (matching the source's own
  type/permissions/major-minor), only when nothing's there yet, with the
  real `fif`/`dev` `CONTENTS` line always written regardless. The
  unmerge side needed no functional change — real `_unmerge_pkgfiles()`
  never unlinks either node type in the first place, and this pilot's
  own catch-all already happened to match by coincidence, doc comment
  now corrected to cite the real reason. See `what-this-proves.md`'s own
  "`ebuild_merge.rs`/`ebuild_unmerge.rs`: real FIFO/device node
  `CONTENTS` support (`fif`/`dev`)". Real device-node creation is only
  unit-tested (genuinely requires root/`CAP_MKNOD`, not reproducible
  live in this dev/test environment); the real FIFO case is proven live
  end to end with a new `dev-libs/fifopkg` fixture.

**Binary packages / fetch**:
- `--getbinpkg`/`--getbinpkgonly`: the **`--pretend` half shipped
  2026-08-29** (`binrepos.conf`/`PORTAGE_BINHOST` parsing, remote binhost
  candidates from each binhost's *cached* `Packages` index, the `g`
  bracket column, download `SIZE` -> `Size of downloads:`). The **actual
  remote download + merge shipped 2026-08-31**: `emerge --getbinpkgonly
  <atom>` (non-`--pretend`) refreshes each `http(s)` binhost's live
  `Packages` (real `bintree._populate_remote`, `wget`), resolves
  binary-only, downloads each remote binpkg into `$PKGDIR`, size-checks
  it, and merges it (new `binpkg::extract_binpkg`, `ebuild_merge::merge_
  binpkg`, `emerge_getbinpkg.rs`; `write_vdb_entry_from_dir` refactor).
  **Replacing a same-slot installed version shipped 2026-08-31**:
  `merge_binpkg` does the real merge-then-unmerge (new binpkg + vdb
  entry written first, then every same-slot installed version's files
  unmerged — the new `PF` folded into `others_in_slot` so a shared path
  survives with the new content — then the old vdb dir deleted, then
  `env_update`); `run_unmerge` split into `pub(crate)` `unmerge_pkgfiles`
  (env-free file-removal core) + `delete_vdb_dir` for phase-free reuse;
  `run_getbinpkgonly` accepts `Upgrade`/`Downgrade`/`Reinstall` outcomes.
  **All four binpkg install/remove `pkg_*` hooks shipped 2026-08-31**:
  `extract_binpkg` keeps `environment.bz2` + `<pf>.ebuild` verbatim;
  `ebuild_phases::run_phase_from_saved_env` = `run_single_phase` + real
  `BinpkgEnvExtractor` (`bunzip2 environment.bz2 > ${T}/environment` +
  `.raw` marker + `EMERGE_FROM=binary` → `bin/ebuild.sh` sources the
  saved env, skips re-sourcing the ebuild, for `pkg_setup` too);
  `merge_binpkg` peeks metadata first (image → real
  `${PORTAGE_BUILDDIR}/image`), runs `setup`→`preinst` before the copy,
  then per replaced same-slot version `prerm`→remove→`postrm` (from that
  version's own vdb `environment.bz2`), then `postinst`, all gated on
  `DEFINED_PHASES`; new `binpkgphasepkg-1.0` + `binpkgrmpkg-{1.0,2.0}`
  fixtures.
  **`--getbinpkg` mixed source+binary merge shipped 2026-08-31**:
  `run_getbinpkgonly` → `run_merge_plan`, dispatching per resolved entry
  on `entry.source` (`Binary` → `merge_one_binary_entry`, else →
  `emerge_build::merge_one_source_entry`). "Prefer binary, fall back to
  source" stays the resolver's job.
  **`merge_binpkg` collision-protect / blocker exclusion / preserve-libs
  parity shipped 2026-08-31**: the same `find_collisions` +
  `collision-protect`/`protect-owned` abort + `unregister_preserved_libs`
  as `merge_after_install`; `blockers_from_flat_deps` (factored out)
  reads the binpkg's already-USE-reduced `*DEPEND` build-info for the
  `mypkglist` blocker term.
  Still open: live `layout.conf` negotiation, `Packages.gz`,
  resume, real `SHA*` digest verification.
- **`$PKGDIR` directory-scan fallback** (real `bintree._populate_local`
  — scan `$PKGDIR` for binpkg *files* when there's no trusted index; a
  `gpkg` already resolves fine when it's *in* an index, which is
  format-agnostic). **DONE — 3 increments, 2026-08-29/30**:
  `portuale/src/binpkg.rs` `read_gpkg_metadata` (real `gpkg.get_metadata`,
  `tar` + seven decompressors) + `read_xpak_metadata` (real
  `xpak.tbz2.scan`, pure Rust) + `scan_pkgdir` wired into `pretend.rs`
  (`Config::scanned_binpkgs`, only when `Packages` absent, never written
  back). `portage_repo::BinaryIndex` refactor
  (`from_pkgdir`/`from_entries` through every binary-candidate fn). Both
  sides; contract-tested. **gpkg internal `Manifest` digest verification
  shipped 2026-09-01**: `binpkg::verify_gpkg_manifest` (real
  `gpkg._verify_binpkg`'s checksum layer — size + BLAKE2B/SHA512 per
  container member via `portage_fetch::verify_digests`, member↔record
  set match) runs first in `extract_binpkg` for any `.gpkg.tar` (the
  merge path only; the pool-populate reader still trusts the container).
  `gpkgreadpkg-1.0.gpkg.tar` rebuilt with a real Manifest; `blake2`/
  `sha2` added as dev-deps. Remaining: `.sig` / inline-PGP verification
  (cut — no crypto), bare `.xpak` multi-instance, mtime-staleness index
  revalidation. See [[porting_pkgdir_scan_gpkg_buildout]].
- **`build-info` metadata generation** (found during the binpkg-scan
  buildout) **shipped 2026-08-30**: `ebuild_phases::write_post_install_
  metadata` (real `doebuild.py::_post_src_install_write_metadata`) writes
  the `DEPEND`/`RDEPEND`/`LICENSE`/… build-info files
  `bin/phase-functions.sh` doesn't, and `write_vdb_entry` now copies the
  whole `build-info` dir into the vdb (real `treewalk()`) — so a
  pilot-merged/built package carries its real dependency metadata.
  **`:=` slot-operator binding shipped 2026-08-31**: `ebuild_phases::
  bind_slot_operator` (real `_slot_operator._eval_deps`) rewrites each
  `*DEPEND` `:=` atom to `:<slot>/<sub-slot>=` from the highest installed
  match in `<root>/var/db/pkg`, bare if unresolvable — so a pilot-merged
  package's stored deps carry the sub-slot info a rebuild check needs.
  **Slot-operator REBUILD edges shipped 2026-08-31 (v1)**:
  `resolve_pretend_graph`'s post-pass `slot_operator_rebuild_entries`
  (real `_slot_operator_trigger_reinstalls` / the
  `@__auto_slot_operator_replace_installed__` set) — an installed
  consumer whose built `cat/pkg:S/SS=` dep no longer matches how the run
  leaves `cat/pkg` in that slot becomes a
  `Reinstall { slot_operator_rebuild: true }` (`[ebuild R]`, new sixth
  Reinstall trigger field, `--json` bool), ordered after the dep.
  **Increment 2 (2026-08-31)**: `_show_abi_rebuild_info`'s "The following
  packages are causing rebuilds:" block (`GraphResult::abi_rebuilds`;
  `--verbose-slot-rebuilds[=y|n]` wired, default on, NOT `--verbose`;
  `--json` `abi_rebuilds` array).
  **Increment 3 (2026-08-31)**: `--ignore-built-slot-operator-deps`
  (real `main.py:470`, `y_or_n`, debug-only) — `resolve_pretend_graph`
  gained an `ignore_built_slot_operator_deps` param that skips the
  `slot_operator_rebuild_entries` post-pass entirely (real portage
  strips the built `:=` parts via `FakeVartree`; same net effect).
  v1 cuts: single-pass (no backtracking), consumer's own `:=` not
  re-bound, no `--changed-slot` interaction. New
  `dev-libs/slotbind{target,consumer,fresh}` fixtures.
  Cuts still: `IUSE_EFFECTIVE`; reading the binpkg `Packages` *index*
  entry from build-info instead of md5-cache (cosmetic — `--pretend`
  reads the index); real `_eval_deps`'s RDEPEND/PDEPEND-vs-DEPEND/BDEPEND
  target-vs-running-vdb split (pilot binds all against target `ROOT`).
- `BINPKG_FORMAT=gpkg` on the *write* side **shipped 2026-08-30**
  (`PackageOptions::binpkg_format` → real, unmodified
  `bin/gpkg-helper.py compress` via `__dyn_package`'s own gpkg branch; a
  `<cat>/<pf>.gpkg.tar` this pilot's own `read_gpkg_metadata`
  round-trips; `Packages` gets a `PATH` field). Cuts: gpkg signing
  (`FEATURES=binpkg-signing` — no crypto), `BUILD_ID` in the basename.
  Still open: `BUILD_ID`/packdebug/splitdebug/RPM;
  `PKGDIR`-index locking. (Real `PORTAGE_COMPRESSION_
  COMMAND` resolution shipped — see `what-this-proves.md`'s own "Real
  `PORTAGE_COMPRESSION_COMMAND` resolution": all six real compressors,
  `BINPKG_COMPRESS` now defaults to real `"zstd"` — this pilot's own
  previous hardcoded `"bzip2 -c"` predated noticing real portage's own
  default had changed from `bzip2`.)
- Fetch: resume support, live `layout.conf` negotiation, `RESTRICT=
  primaryuri` (doesn't port cleanly — candidate ordering already
  deviates), and running the ebuild's own `pkg_nofetch` phase for a
  missing `RESTRICT=fetch` distfile (real `spawn_nofetch`). (`mirror+`/
  `fetch+` SRC_URI prefixes + `RESTRICT=fetch` shipped 2026-08-30 —
  `portage_fetch` strips the prefixes + records `override_mirror`/
  `override_fetch`; `FetchOptions::restrict_fetch` bars a plain
  `SRC_URI` URI + the public mirrors so a fetch-restricted package
  fetches only from an already-verified `DISTDIR` copy; `fetch+`/`mirror+`
  re-permits the URI; new `dev-libs/fetchrestrictpkg` fixture.) (Real
  `RESTRICT=mirror`
  shipped 2026-08-29 — `FetchOptions::restrict_mirror` gates the
  `GENTOO_MIRRORS` flat-layout fallback, new `dev-libs/restrictmirrorpkg`
  fixture. Real `custommirrors` shipped — see
  `what-this-proves.md`'s own "Real `custommirrors`: an admin-configured
  `/etc/portage/mirrors` file"; `FetchOptions` gained a `config_root`
  field mirroring `ebuild_merge::MergeOptions`'s own. Real
  `FEATURES=distlocks` shipped too — see `what-this-proves.md`'s own
  "`FEATURES=distlocks`, and a real default-`FEATURES` correction to
  `protect-owned`/`unmerge-orphans`", which also fixed a real,
  previously-undiscovered mistake: `protect-owned` and `unmerge-orphans`
  had shipped defaulting to `false`, but real `cnf/make.globals` (lines
  77-84) actually defaults both of them **and** `distlocks` to `true` —
  both now correctly default `true`, confirmed live. ~~`FEATURES=
  verify-sig` (GPG)~~ — **removed 2026-08-25, was mis-scoped from the
  start**: real signature verification is a `gpkg`/repo-sync concept,
  not `SRC_URI`/distfile-fetch at all — zero hits grepping `fetch.py`
  directly for either term.)
- `--keep-going` is real for all three non-`--pretend` merge paths:
  `--buildpkgonly` (see `what-this-proves.md`'s own "`emerge --buildpkgonly
  --keep-going`" — its depgraph gate guarantees no entry depends on
  another) and, as of 2026-08-31, `emerge <atom>` / `emerge --getbinpkg`
  (`emerge_build::run_merge_loop` — the general version: BFS-drop the
  failed entry's transitive dependents via `GraphEntry.required_by`, real
  `Scheduler._calc_resume_list`).
- **Scheduler Part 2.B shipped 2026-09-01**: `emerge -jN` parallel build
  scheduler + `--load-average` + build-log capture + `>>> Jobs:` line +
  `--ask`/`-a` (prompt before a real merge / `-C`/`--depclean`/`--prune`
  removal / `--deselect` write / `--config` run — `ask_confirm`, plus
  `ask_select` for `--config`'s package menu; exit 130 on No) +
  `CLEAN_DELAY` countdown
  (`clean_delay_countdown`, tests pin it to 0 via an autouse conftest
  fixture). `PORTAGE_NICENESS`/`PORTAGE_IONICE_COMMAND` also shipped
  (`apply_portage_scheduling_policy`, real `actions.py::apply_priorities`
  -- renice/ionice this process once at startup). The elog modules
  shipped in `elog.rs`: `echo` (real `mod_echo`, default-on, `* Messages
  for package <cpv>:`), `save` + `save_summary` (2026-09-02, real
  `_combine_logentries` files), and **unmerge `prerm`/`postrm` elog**
  (2026-09-02 -- real `dblink.unmerge`'s `_elog_process(phasefilter=...)`;
  `execute_unmerge` + the post-merge loop now share
  `elog::process_batch`). `mail`/`mail_summary` stay a one-line
  "unsupported" notice; `create_directories` makes `${T}/logging`.
  `--resume`/`--skipfirst` also shipped (`mtimedb.rs` -- a failed source
  `emerge` writes `mtimedb["resume"]`, `emerge --resume [--skipfirst]`
  replays it; 2026-09-02 the `myopts` half -- `--oneshot`/`--onlydeps`
  are recorded in `ResumeOpts` and honoured on replay so `--resume`
  doesn't world-record a `--oneshot` run's recovered packages).
  **Part 2.B is now substantially complete.** Remaining odds and ends:
  `resume_backup` rotation, the rest of `myopts` (build-time flags,
  bundled with binary-entry replay), `mail*`/`syslog`/`custom` elog, the
  in-place-replace path's superseded-version prerm/postrm elog,
  `PORTAGE_SCHEDULING_POLICY`, killing in-flight builds on a hard
  fail.

**Depgraph / dry-run**:
- **`--root-deps` fuller fidelity, remaining half.**
  ~~doesn't feed running-root satisfiability into disjunctive (`||`)
  branch selection~~ — **shipped 2026-08-25**: both real dep-walk sites
  (the main New/Upgrade/Reinstall flatten and `enqueue_dependencies`'s
  own `--deep`/`AlreadyInstalled` recursion) now accept a `||` branch
  that's running-root-satisfied even when it's invisible in the tree —
  see `what-this-proves.md`'s own "`emerge --pretend --root-deps`" section, mirrored
  in `emerge_pretend_reference.py` with a new pytest contract test
  (`test_root_deps_disjunctive_branch_selection_matches_between_
  implementations`).
  ~~Still open: real portage can recursively pull in and build a *new*
  package against the running root when it's not already there, never
  attempted here~~ — **first increment shipped 2026-08-26, confirmed
  with the user to build toward the full real multi-root shape one
  increment at a time** (matching the five-part preserve-libs
  registration precedent) rather than the full architectural rewrite in
  one slice: new `unsatisfied_root_deps_atoms`/`resolve_root_deps_
  build_entry` resolve an unsatisfied `DEPEND`/`BDEPEND` atom against the
  running root directly (reusing `resolve_pretend` wholesale) and add it
  as its own real `GraphEntry` (`targets_running_root: true`, new field).
  Also fixed in the same slice: a real, previously-invisible bug where
  such an atom used to *also* fall through and resolve a second time
  against `ROOT` via the ordinary `flat_deps` queue — invisible until the
  new `dev-libs/rootdepsbuildpkg` fixture (deliberately tree-visible,
  unlike `rootdepsprovider`/`rootdepsnonexistent`) exposed it. See
  `what-this-proves.md`'s own "`emerge --pretend --root-deps`: recursively
  building a new package against the running root" for the full
  grounding, the real `BDEPEND`-always-targets-running-root/`DEPEND`-
  EAPI-conditional finding that motivated confirming scope with the user
  before writing any code, and why this increment is deliberately **not**
  recursive (a real cycle-safety hazard — two packages `BDEPEND`ing each
  other, neither satisfied by the running root, would recurse with no
  cross-call cycle memory).
  ~~deciding whether/how `--pretend` output should visually distinguish a
  `targets_running_root` entry from an ordinary one, left as its own
  separately-scoped follow-up~~ — **shipped 2026-08-27**: new
  `root_suffix` (`pretend.rs`, mirrored in `emerge_pretend_reference.py`)
  ports real `output.py:841-862`'s own `darkgreen("to " + pkg.root)`
  suffix, narrowed to annotate *only* the running-root build entries
  (not every non-`/` `ROOT` entry — that would break contract-suite
  determinism). `--json` grew a `"builds_against_running_root"` field;
  `--tree` carries the marker through the indent. Also fixed a real,
  pre-existing Rust/Python `required_by` divergence the new `--json`/
  `--tree` parity assertions exposed (Python's post-pass unconditionally
  overwrote `required_by`, wiping the build entry's own `[owner]`; Rust
  already guarded it). See `what-this-proves.md`'s own "`emerge --pretend
  --root-deps`: the running-root build entry is marked in `--pretend`/
  `--json`/`--tree` output".
  ~~walking the new entry's *own* further dependencies (the recursion,
  with cross-call cycle memory)~~ — **shipped 2026-08-27**:
  `resolve_root_deps_build_entries` (replacing the non-recursive
  `resolve_root_deps_build_entry`) walks a running-root build entry's
  own `DEPEND` + `BDEPEND` + **`RDEPEND`** against the running root
  recursively (RDEPEND is the deliberately-broader half, confirmed with
  the user — real `_add_pkg_deps` resolves all three against `pkg.root`
  when `pkg.root` is the running root). Cycle safety via the existing
  `root_deps_build_seen` set (insert-before-recurse); `required_by` now
  names the *immediate* requester so `--tree` nests correctly. Also
  confirmed with the user: an unbuildable, not-installed build dep is
  now surfaced as its own `NoVisibleCandidate` entry (the renderer's
  `!!! no visible ebuild` note) instead of being silently swallowed.
  New `dev-libs/rdr*` fixtures. `unsatisfied_root_deps_atoms` grew a
  `dep_keys` param; `root_deps_unsatisfied` is now a `Vec` not a
  `HashSet` for deterministic entry order. See `what-this-proves.md`'s own
  "`emerge --pretend --root-deps`: the running-root build walk is
  recursive".
  ~~`IDEPEND` of a running-root build entry~~ — **shipped 2026-08-27**:
  the recursive dep-key set is `["DEPEND", "BDEPEND", "RDEPEND",
  "IDEPEND"]` now (real `depgraph.py:4247-4252` — `IDEPEND` *always*
  targets `_running_root.root`). New `dev-libs/rdri*` fixtures, +1 Rust
  unit test, +1 contract test. `PDEPEND` deliberately still out (real
  portage keeps it a target-`ROOT` concern).
  ~~a *top-level* package's own `IDEPEND` still resolves against `ROOT`
  under `--root-deps`~~ — **shipped 2026-08-27**: `root_deps_satisfied_
  atoms` gained the same `dep_keys` param its complement already had, and
  both ordinary dep-walk sites now pass `["DEPEND", "BDEPEND", "IDEPEND"]`
  to *both* the satisfied and unsatisfied helpers (they must agree, or an
  atom in neither falls through to `ROOT`). New `dev-libs/topidepapp`
  fixture, +1 Rust unit test, +1 contract test. See `what-this-proves.md`'s own
  "`emerge --pretend --root-deps`: a top-level package's own `IDEPEND`
  vs the running root".
  **Scope note (2026-09-01, confirmed with the user)**: this fork's
  ebuilds are all EAPI 7+ (profiles stay EAPI 5, but `--root-deps` is
  never a profile concern). At EAPI 7+ (`eapi_attrs.bdepend`),
  `depgraph.py:4218-4238` puts the `--root-deps == "rdeps"`
  `ignore_depend_deps` branch inside the `else` of `if
  eapi_attrs.bdepend`, so **`--root-deps=rdeps` is a complete no-op for
  this fork**. What *does* apply at EAPI 7+: `BDEPEND` and `IDEPEND`
  always resolve against the running root (`_running_root.root`,
  genuinely `/`), `DEPEND` against `ESYSROOT` (≈ target `ROOT` when not
  cross-compiling), and bare `--root-deps`/`=True` folds
  `DEPEND`/`BDEPEND`/`IDEPEND` into `RDEPEND` (a debugging flag).
  ~~**Remaining gap**: real portage routes `BDEPEND`/`IDEPEND` to `/`
  *unconditionally*, the pilot only under `--root-deps`~~ — **closed
  2026-09-02**: `pretend.rs::resolve_root_deps_running_root` returns
  `Some(running_root)` whenever `--root-deps` is set **or**
  `running_root != target ROOT` (a cross-root/stage build), matching
  real portage's unconditional behavior; a strict no-op when the roots
  coincide (every `ROOT=/` run). Determinism is kept at the test
  boundary — `fixture_env` pins `PORTAGE_RUNNING_ROOT` to the fixture
  `ROOT`. See `what-this-proves.md`, "`BDEPEND`/`IDEPEND` route to the
  running root unconditionally". The full multi-root graph architecture
  (a `root` per dependency edge) stays a deliberate edge-by-edge
  approximation.

**Eclass/shell backend**:
- ~~`eclass_locations_value` is single-repo only — no masters-chain
  eclass resolution~~ — **shipped 2026-08-25**: `eclass_locations_value`
  now resolves the real masters chain (`RepoConfig::masters`, already
  resolved elsewhere for profile/USE config stacking) via `portage_repo::
  find_repos(config_root)`, exported in real declared order (own repo
  first, masters after) — see `what-this-proves.md`'s own "Real eclass
  `inherit()` support: `PORTAGE_ECLASS_LOCATIONS`". Required threading
  `config_root: &Path` through the whole phase-execution call chain
  (`run_commands`/`run_single_phase` down to `phase_env_vars`) and a new
  `config_root` field on `UnmergeOptions`/`PackageOptions` (mirroring
  `MergeOptions::config_root`'s own established shape) — `ebuild_merge.rs`
  already had one. Proven with a real, end-to-end fixture: an overlay
  ebuild inheriting an eclass that only exists in its own master repo,
  never redeclared locally — the exact real case the single-repo v1
  couldn't reach.
- brush strategy #3 (formalize the forked `brush` dependency, track
  fork-vs-upstream-merged fixes) — **done 2026-08-27**: see
  `brush-pin.md` (#1274 merged upstream, #1276 open and the sole
  fork-only fix, ancestry of the pin, re-pin checklist). brush strategy
  #2 (rewrite this repo's own `bin/*.sh` to avoid brush-hostile
  constructs) still open.
