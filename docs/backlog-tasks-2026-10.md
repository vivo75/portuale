# Backlog tasks — 2026-10

One line per open task. Each has a code/doc pointer so a fresh model can
start without a full context load. This file continues
[`backlog-tasks.md`](backlog-tasks.md) (last entry there: #298); numbering
carries on from it. Source of truth for detail:
[`scope-backlog.md`](scope-backlog.md), `what-this-proves.md` (what already
shipped), `git log`, and the memory notes under
`.claude/projects/-home-vivo-repo-PORTUALE-portuale/memory/`.

Rules that apply to every task: take expected output from real Portage
(container bed, host `emerge`, or an upstream resolver test), pin it in
`pytests-contract-suite/test_emerge_pretend_contract.py` /
`test_portuale.py`, keep `test_output_invariants.py` green, and run the
full verification pass (`AGENTS.md` step 8). The suite and the container
bed live in the sibling `pmtest` repo (`agent-context.md`, "Where the tests
live"). The Python mirror was removed 2026-09-15 —
[`history/second_python_copy_removal.md`](history/second_python_copy_removal.md).

---

## Tier 1 — focused slices (scoped, ~one sitting each)

> **Filed 2026-10-04 from a live `emerge -puDN @world` comparison on the
> author's host** (portuale vs real Portage 3.0.x, `PKGDIR=/.gentoo/cache/binpkgs`,
> binhost `https://gpkg.f1r.eu/seed-desk/`, `binpkg-multi-instance` on).
> After a real `portuale emerge -uDvN @world` upgrade the system was left
> half-done: 104 upgrades applied, the 36 sub-slot reinstalls not, and a
> repeat `-p` run planned **4 downgrades + a slot conflict** (Portage: 36
> clean binary reinstalls). Minimal repro:
> `portuale emerge -pv app-text/libspectre` plans `ghostscript-gpl`
> 10.08→10.06 and `libspectre-0.2.12-6`; Portage plans only
> `libspectre-0.2.12-7`. Root cause: portuale identifies a binpkg by
> `cat/pkg-version` and ignores `BUILD_ID`, so it cannot tell the builds of
> one version apart. #299–#301 are the three places this shows up; they
> compound (#301 downloads the wrong build, #300 makes it shadow the right
> one forever, #299 reads the wrong build's deps). **Recommended order:
> #299 → #300 → #301 (all three done); then #303**, then re-run the host comparison
> (`emerge -pvuDN @world` on both) — expect byte-identical plans modulo the
> `hplip` conflict both report.
>
> **Reproducers (were red; now green):** three unit tests in
> `rust/portage-repo/src/lib.rs` (`mod tests`, next to
> `dedup_binary_instances_keeps_newest_build_time_per_group`), shared
> fixture `multi_instance_entry_1010` / `in_memory_binhost_1010`. Run:
> `cargo test -p portage-repo --lib multi_instance`. All three fail now for
> the reasons below; each task turns its own test green. When a fix
> changes a function signature (adds `build_id`), update the test call and
> keep the "no `BUILD_ID` → newest build" fallback assertion.

299. **DONE 2026-10-04 (branch `backlog/299-301-multi-instance-binpkg`) — a chosen binpkg instance walks the first same-`CPV` instance's dependencies, not its own.** `portage-repo/src/lib.rs::read_binary_metadata` (`:3127`) returns the first `Packages` entry whose `CPV` equals `cat/pkg-version` and ignores `BUILD_ID`; its caller in the graph walk (`read_binary_metadata_any`, `:3146`, called from the node-expansion path at ~`:31267`, where `GraphEntry::build_id` is already known) hands that to the dependency reader. `dedup_binary_instances` (`:2634`) correctly keeps the newest build (-7), but the walk then follows -6's `RDEPEND` — in the field `>=app-text/ghostscript-gpl-9.53.0:0/10.06=` instead of `:0/10.08=` — so `--debug` shows `Parent: libspectre-0.2.12-7` / `Depstring: …:0/10.06=` and the solver downgrades the installed ghostscript 10.08 and drags libcdio/libcbor/simdutf and a slot conflict with it. **Fix sketch:** give `read_binary_metadata`/`_any` a `build_id: Option<&str>` and match `CPV` **and** `BUILD_ID` (entry without `BUILD_ID` keeps today's first-match, so non-multi-instance indexes are unchanged); pass `resolved.build_id` from the walk; audit the other callers (`:12908` `resolve_info_binary_candidate`, `ebuild_package.rs` tests) and `binary_deps_changed` (`:9529`), which already reads the candidate's own `binary_deps` and needs nothing. **Reproducer:** `multi_instance_walk_reads_the_selected_instances_own_deps` — local index with builds 1 (RDEPEND `olddep`, older `BUILD_TIME`) and 2 (RDEPEND `newdep`, newer), assert the entry is build 2 and the walk contains `newdep` but not `olddep` (today: `["spectre", "olddep"]`). **Verification:** `portuale emerge -pv app-text/libspectre` on the host with an empty `PKGDIR` plans only `libspectre-0.2.12-7` and no ghostscript change; add a pmtest `CASES` entry with a two-build fixture whose builds differ in a `:=` sub-slot (expected value from real Portage in the container bed). [A]
300. **DONE 2026-10-04 (same branch) — a version already in `$PKGDIR` hides every remote build of that version, including newer `BUILD_ID`s.** `list_remote_binary_candidates` (`:2709`) seeds `shadowed` from `local_versions` (the local index's `version` strings) and skips any remote candidate whose `version` is in it, then grows the set a whole binrepo at a time. That ports `bintree.isremote` per *version*; real's check is per *instance* (`cpv` + `build_id` under `binpkg-multi-instance`). On the host `/.gentoo/cache/binpkgs` holds `libspectre-0.2.12-6.gpkg.tar`, so the remote -7 (the only build matching the installed ghostscript 10.08) is dropped and -6 wins; with `PKGDIR` emptied the plan flips to -7. **Fix sketch:** key the shadow set on `(version, build_id)` (treat a missing `BUILD_ID` as its own key so non-multi-instance behaviour is unchanged), keeping the "earlier binrepo shadows a later binrepo's identical instance" rule; the fixture-based test `list_remote_binary_candidates_reads_each_binrepos_own_packages_index` must stay green (it passes the binhost as its own local, so identical `(version, build_id)` pairs still shadow). Also check `has_local_binary_candidate` (`:12852`) and the `remote` flag consumers that assume "local wins". **Reproducer:** `multi_instance_local_build_does_not_shadow_a_newer_remote_build` — local build 6, remote builds 6 and 7: expect remote 7 present and remote 6 shadowed (today: `[]`). **Verification:** with `/.gentoo/cache/binpkgs` as-is, `portuale emerge -pv app-text/libspectre` plans -7. [A]
301. **DONE 2026-10-04 (same branch) — the download lookup fetches the first same-`CPV` build, not the planned one.** `find_remote_binpkg` (`:2756`) returns the first binrepo `Packages` entry whose `CPV` matches, ignoring `BUILD_ID`; both consumers (`portuale/src/remote.rs:1512`, `portuale/src/emerge_build.rs:1707`) then download that record's `PATH`. The plan shows `-7` but the oldest listed build (`-6`) is fetched — very likely how `libspectre-0.2.12-6.gpkg.tar` got into `$PKGDIR` during the 2026-10-04 upgrade, which is what triggers #300. (Inferred from timestamps and the shared lookup; not reproduced end-to-end.) **Fix sketch:** add `build_id: Option<&str>` to `find_remote_binpkg` and match `CPV` + `BUILD_ID`; when `None`, return the **newest** matching build (highest `BUILD_TIME`, then `BUILD_ID` — real `dbapi._cmp_cpv` order) instead of the first; thread `GraphEntry::build_id` through the two callers; make sure the merge/verify path uses the same record's `SIZE`/digests. **Reproducer:** `multi_instance_download_lookup_returns_the_newest_build` — builds 6 (`BUILD_TIME` 1000) and 7 (2000) listed oldest first: expect `BUILD_ID` 7 (today: 6, `PATH` `…-6.gpkg.tar`). **Verification:** a `--getbinpkgonly` pretend+fetch of `app-text/libspectre` against the seed-desk index downloads `libspectre-0.2.12-7.gpkg.tar`. [A]
302. **DONE 2026-10-04 (resolved by #299–#301, no separate fix) — Python ebuild reinstalls and `flit-core` upgrade planned by portuale but not Portage.** Confirmed fallout of the multi-instance bugs: after the fix the host `portuale emerge -p -uDvN @world` plans **35 binaries, 0 ebuild rows** (was 44 packages, 32 ebuilds); the `dev-python/*`, `python-exec*`, `flit-core`, `hatchling` rows are gone. [A]
303. **OPEN (filed 2026-10-04) — an installed consumer's built `:=` pin holds its dependency back even though the consumer is itself being reinstalled.** Residue of the host `@world` comparison after #299–#301: portuale still plans `libcbor` 0.14→0.13 and `libcdio` 2.4→2.2 (2 downgrades) and omits `libcdio-paranoia`, `libfido2`, `mplayer` reinstalls that Portage plans (35 vs 36 packages). `--debug` shows `Parent Dep: dev-libs/libcbor:0/0.13= required by (dev-libs/libfido2-1.17.0:0/1::gentoo, installed)` selecting `libcbor-0.13.0-1`: the *installed* `libfido2` (built against sub-slot 0.13) pins the child while the plan separately rebuilds `libfido2` as build 2 (`libcbor:0/0.14=`). `portuale emerge -pv dev-libs/libfido2` alone is correct, so it only bites in the full world walk. Same family as the shipped #210 ("reinstall-with-slot-change", `reverse_dependency_constraints` at `portage-repo/src/lib.rs`, real `_complete_graph` `:8619-8648` and the `_slot_operator_update_probe`, #24b/#211): the constraint of an installed parent must be dropped when that parent is scheduled for replacement, and the replacement's own recorded deps used instead. **Next step:** reduce with a two-package fixture (child `0.13`→`0.14` sub-slot bump, parent installed with `child:0/0.13=` and a binary rebuild available built against `0.14`) in the container bed, expected value from real Portage; then find why #210's `Reinstall`/`upgrading` map misses a parent whose replacement is a *binary instance* (check that the replacement is recorded under its `BUILD_ID`/`binary_deps`). Expect byte-identical host plans (modulo `hplip`) when closed. [A]

---

## Deliberate cuts — do NOT pick these

- Fixing this by hiding old builds (filtering the binhost index to the newest `BUILD_ID` per `cpv` at load time) — it would make `--binpkg-respect-use` / `binpkg-changed-deps` fallbacks to an older, matching build impossible, which real Portage supports. The fix is to carry `BUILD_ID` through selection, download and metadata reads (#299–#301), not to drop instances. [A]
