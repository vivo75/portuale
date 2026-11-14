# What this proves

> Append-only record of what each slice shipped, with a runnable example.
> New slices append at the end; an entry is never rewritten, and a claim that
> later went stale is corrected by a later entry.
>
> Everything up to 2026-11-07 (21 068 lines, written while Portage was still
> treated as the oracle) is archived, not deleted: see
> [`ARCHIVE.md`](ARCHIVE.md), `what-this-proves.until-2026-11-07.md`. Read
> `docs/charter.md` first: where the archived text says "mimic Portage" or
> "drop-in", the charter wins. `git log` is the same history per commit.

---

Backlog #680 (2026-11-07): a build-time dependency is now merged *usable*
(charter property 2, PMS 8.1). An installed package whose recorded
`:SLOT/SUB=` binding the plan breaks no longer satisfies a dependency in the
merge-order graph, and every build waits for the run-time dependencies of its
build-time dependencies that no installed package satisfies. With the
hand-built `p680*` shape (installed `p680perl-1.0:0/1.0`, XS module `p680xs`
bound to it, `p680doc` DEPENDing on the module, `p680build` BDEPENDing on the
installed tool `p680tool` whose RDEPEND is the module),
`emerge -pvuD app-misc/p680meta` now merges `p680perl`, `p680cycb`,
`p680cyca`, `p680gettext`, `p680xs`, `p680doc`, `p680build`, `p680meta`
(before: `p680build` and `p680doc` at #2-#3, `p680xs` last). Live check, from
the pmtest root: `python3 -m pytest pytests-contract-suite -q -k test_680`.
On the bed, `l0-health-20261107T210124Z` against `l0-health-20261107T195447Z`:
the six plans with the `Locale-gettext` findings (`-puvDN @world`, its
`--exclude sys-devel/gcc` form, `-pve @world`, `-pve @system`,
`-pve app-misc/tmux`, `MULTI_emptytree-system`) are clean, the other 179 have
the same findings. L0 `l0-20261107T205456Z`: 91 clean, 44 unexplained (was 43):
`MULTI_deep-update-world` and `MULTI_emptytree-system` still diverge from
Portage at the same position (#21, #144) inside the perl-virtual cluster, but
portuale now puts `virtual/perl-CPAN` there, so the first no longer matches the
`perl-cluster-hash-order-601` allowlist entry.

**#669: a downgrade picked through an argument's own dependency is `UD`, not `N`.**
When a slot conflict is settled by keeping a version that will be merged, the
slot may already hold a different version. That keeper was always recorded as
`New`, so the row read `N` and `--json` said `new` for an occupied slot. It is
now compared with what the slot holds (Portage `_get_installed_best`): older is
`Downgrade` (`UD`), otherwise `Upgrade`, and `New` only for an empty slot.
Runnable (grounded in Portage 3.0.83's own capture, not copied from its code):
with `w603lib-3` and `w603cons-1` (`RDEPEND="<dev-libs/w603lib-2"`) installed,
`emerge -p dev-libs/w603lib app-misc/w603cons` prints
`[ebuild     UD ] dev-libs/w603lib-1 [3]`, then `R w603cons-1` and the skip
block. Pin: `python3 -m pytest pytests-contract-suite -q -k test_669` in pmtest
(fails with `[ebuild  N     ]` before the change).

**#682: the merge-order candidate sort has a total order, so `emerge` no longer panics or varies between runs.**
Among the mergeable nodes of one `cp`, an entry with no visible candidate
(version `""`) compared equal to every real version, so the sort comparator was
not a total order and the standard sort could panic ("user-provided comparison
function does not correctly implement a total order") depending on the hash
order of its input. A missing or unparseable version now sorts first, the
graph index breaks remaining ties, and the same helper replaces the two other
copies of the pattern. Runnable: `for i in $(seq 30); do portuale emerge -pvuDN
--getbinpkg=n media-video/ffmpeg 2>&1 | md5sum; done | sort | uniq -c` prints
one line with count 30 (before the change 7 runs in 10 aborted with rc 134).
Pins: `version_order_is_total_with_missing_and_unparseable_versions` and
`find_smallest_cycle_ignores_input_order_with_a_versionless_entry` in
`crates/portage-repo/src/merge_order/tests/cycles.rs`. Bed L0
`l0-20261108T110937Z`: 91 clean, identical to the run before the change. The
command still exits 1 on a real resolver result, filed as #685.

**#683: a `||` choice under `--update` now prefers the alternative that holds the upgrade instead of always keeping the installed slot (Portage `dep_check.py:771-775`).** `promote_tied_alternative` (`crates/portage-repo/src/resolve/or_preference.rs`) ports the installed-slots promotion guard with a lazily computed per-choice `want_update`, so `dev-lang/python-3.15.2_p1` and `dev-lang/rust-bin-1.99.0` are added beside (not instead of) the still-needed installed slot. Runnable (live-verified 2026-11-09 on branch `backlog/683-or-upgrade-preference`): `cargo test --release -p portage-repo promote_tied_alternative` prints `19 passed; 0 failed`. Pins: 7 gate legs in `tests_162.rs`. Tester fresh pre-merge: full workspace 3360 passed / 0 failed, fmt clean, clippy 0 warnings. Not verified: the host `-uDNt --getbinpkg=n -p @world` diff and L0 were NOT RUN (owner step).

**#685 (2026-11-08): an installed consumer's pin constrains only its own slot.** `portuale emerge -pvuDN --getbinpkg=n media-video/ffmpeg` used to abort (rc 1, `no ebuilds to satisfy "~llvm-core/llvm-22.1.8[llvm_targets_XCore=]"`) because the pin `~llvm-core/llvm-23.1.4` of the installed lld/lldb/clang-23.1.4 was filed against every slot of llvm. Now it exits 0 with the same plan lines on every run: `for i in 1 2 3; do portuale emerge -pvuDN --getbinpkg=n media-video/ffmpeg | grep -E '^\[|^Total' | md5sum; done` prints one hash three times. Unit tests: `slot_scoped_positive_constraint_spares_other_slots`, `enforced_pin_is_filed_under_the_broken_slot`.

Backlog #687 (2026-11-08): the system a real merge leaves is now checked
against charter properties 1 and 4. `python3
../pmtest/pytests-contract-suite/health_checker.py --installed /` reads the
VDB and the files under a root as a plan that merges nothing and reports an
unmatched runtime or post-merge dependency, a violated blocker or
`REQUIRED_USE`, two packages in one slot, and an installed ELF file whose needed
library the dynamic linker does not find. On this host's real root (2376
packages) it prints no failure in about 30 s and 24 `private-library` notes
(libraries a JDK, systemd or a vendored application keeps outside the search
path). With `--baseline BEFORE.jsonl`, the output of the same run before a
merge, only what the merge introduced fails. The L1 bed runs it in each
consumer container and `compare/health-state-report.py` adds the verdict to
`l1-report.txt`: the glibc + bash gate (`l1-merge-gate.txt`, build, then
`L1_SKIP_BUILD=1 L1_CONSUME_REINSTALL=1`) gave rc 0 twice, merged 2 of 2, 0 hard
findings and `health of the installed state: 0 failure(s) introduced by the
merge`. Its mutation test, `python3
../pmtest/differential-test-bed/compare/health-state-mutants.py /`, breaks a copy
of the VDB five ways and one harmless way: every mutant is reported and the
control is not. Order (2) and newest version (3) cannot be read from a final
state; property 3 for binary packages is #688 and running the check after the
L3 and L32 cells is #689.

Backlog #688 (2026-11-08): charter property 3 (newest visible version) now
covers binary packages. `emerge --pretend -v --json` ends with an `options`
object (`usepkg`, `usepkgonly`, `getbinpkg`, `getbinpkgonly`, as given on the
command line); runnable: `emerge --pretend -v --json --usepkgonly dev-libs/
binaryonlypkg | grep -o '"options":{[^}]*}'` prints `"options":{"usepkg":false,
"usepkgonly":true,"getbinpkg":false,"getbinpkgonly":false}`, and the same with
`--usepkg` shows `"usepkg":true`. `health_checker.py` reads the local binary
index (`PKGDIR/Packages`) next to the repositories' ebuilds: a higher visible
binary than a merged binary is a failure in every mode, a higher ebuild is one
with `--usepkg` and not with `--usepkgonly`, a higher binary than a merged
ebuild is one only with `--usepkg`, and a binary whose baked USE differs from
the configured one is not a candidate. Without `options` (an older plan) a
higher ebuild beside a merged binary is a note. Pins: `python3 -m pytest
pytests-contract-suite/test_health_checker.py -q` in pmtest (119 tests; the
binary rules, the `options` key for both flags, and the mutation operator
`older_visible_binary`); Rust `candidate_options_json_names_the_four_binary_options`.
A remote binhost and a flat PKGDIR are not read: #690.

Backlog #686, #689, #690 (2026-11-08): three loose ends of the health work.
#686: `emerge -pvuDN --getbinpkg=n --backtrack=0 media-video/ffmpeg` used to
print a different `~llvm-core/llvm-23.1.4[llvm_targets_X=]` row per consumer on
each run (a hash-set walk of the consumer's atoms); `for i in 1 2 3 4 5 6; do
emerge -pvuDN --getbinpkg=n --backtrack=0 media-video/ffmpeg 2>&1 | md5sum;
done` now prints one md5 six times. #689: the L3 and L32 beds run
`health_checker.py --installed /` before and after each build or cell and fail
on what portuale introduced (Portage's run is the control); the wiring was
checked with `bash -n` and `compare/test-l3-concurrent-mock.sh` (51 passed), a
real L3/L32 run is still to be done. #690: property 3 reads the cached remote
binhost indexes and the PKGDIR files outside the index; `python3 -m pytest
pytests-contract-suite/test_health_checker.py -q` in pmtest (122 tests) pins
a cached `binrepos.conf` index, `PORTAGE_BINHOST`, and a `.tbz2` plus a
`.gpkg.tar` read for their slot.

#491 (second step, 2026-11-09): `emerge -uDpvN =kde-frameworks/kdbusaddons-6.31.0`
on the current host tree ends rc 0 with `backtrack: 5/20` and 30 packages
(Portage 3.0.83: rc 0, `2/20`, 29), where it was rc 1 with `20/20` and 71
packages with `openssl` 3.6.5 and 4.1.0 in one slot. The installed parents a new
child version must satisfy now include the world members the request does not
reach (their pending upgrade no longer excuses them), the dynamic-deps view of
every installed parent (`opencv`'s live `<media-video/ffmpeg-9:=`), and, on a
plain pass, the blocker-retry closure; an unsatisfiable built dep of a
merge-bound binary masks that binary instead of seeding its installed twin.
Run `target/release/portuale emerge -uDpvN =kde-frameworks/kdbusaddons-6.31.0;
echo rc=$?` and compare with `emerge` on the same request; the captures are in
`LLM/evidence/2026-11-09-491/` (taken before the fix). Unit tests: the three in
`tests/built_parent_update_probe.rs` and the `binary-twin` case of
`tests/unsat_probe.rs` (both fail on the old code). Left over: #695, #696.

#695 (2026-11-09): an installed consumer the request does not reach now keeps
its `[use]` dependency. `emerge -uDpvN =kde-frameworks/kdbusaddons-6.31.0` on the
host tree no longer plans `[binary Rg] app-misc/mime-types-2.1.54-1
USE="-nginx*"` (the installed `www-servers/nginx` needs `mime-types[nginx]`);
rc 0, like Portage, which removes that binary in its slot-conflict handler.
Two causes: `-D` ran the complete-mode re-walk with the USE-break table off,
and a break from a reinstall nobody requested by name was only reported, never
held back. A package requested by name keeps the conflict block and rc 1.
Run, in pmtest: `python3 -m pytest pytests-contract-suite -q -k f695`
(`dev-lang/f484-py` with `www-servers/f484-ng` in @world: no merge row);
unit test `reverse_dependency_constraints_enforces_a_use_break_when_only_a_dependency_pulls_the_reinstall`.
Left over: #696.

#696 (2026-11-10): a binary mask hides one build, not every build of that
version. `emerge -uDpvN =kde-frameworks/kdbusaddons-6.31.0` on the host tree now
plans `[binary gU] media-video/pipewire-1.6.11-3` (29 packages, 21 binaries, rc 0,
the same totals as Portage) instead of compiling `pipewire-1.6.11` from source:
when the newest build (4) carries a built `:S/SS=` dep that cannot be met, only
that build is masked, and build 3 stays a legal candidate. The mask is now
`!=cat/pkg-ver[binary:BUILD_ID]` and the filter runs before the per-version
dedup. Run `target/release/portuale emerge -uDpvN =kde-frameworks/kdbusaddons-6.31.0;
echo rc=$?` and compare with `emerge`; in pmtest, `python3 -m pytest
pytests-contract-suite -q -k test_696` (two builds of `app-misc/u257par-1`, the
newer one stale: `[binary N] u257par-1-3`, not `[ebuild N]`). Unit test
`binary_mask_hides_reads_only_marked_negatives_and_honours_the_build_id`. The
`@world` merge rows were not re-compared byte for byte against the previous
binary. Left over: #697.

#431 S2 (2026-11-10): `--solver=pubgrub` now offers the same keyword, licence and
`package.mask` suggestions as the default solver. When the strict solve has no
plan, the bridge re-solves over a pool relaxed one autounmask level at a time
(licence, then `~arch`, then masks, as the walk's `autounmask_levels`), and the
first plan that stands carries the change blocks and the `~`/`#` row markers.
Run `target/release/portuale emerge --solver=pubgrub --pretend
--autounmask-keep-keywords=n dev-libs/akk0a` in `pmtest/fixtures` (environment as in
`fixtures/` tests): it prints `[ebuild  N    ~] dev-libs/akk0b-1`, the
`akk0a-2` row and `=dev-libs/akk0b-1 ~amd64` under "keyword changes are
necessary", exit 1 — before this it printed `!!! All ebuilds that could satisfy
"dev-libs/akk0b" have been masked`. Same for `dev-libs/hardmaskedpkg
--autounmask` (mask) and `dev-libs/crossrepolicensepkg --autounmask
--autounmask-license=y` (licence). In pmtest,
`python3 -m pytest pytests-contract-suite -q -k suggests_keyword_license_mask`
compares stdout, stderr and exit code with the default solver on those four
cells; unit test `autounmask_levels_order_license_keywords_masks_least_invasive_first`.
The expected text is the default solver's, not a fresh Portage capture, and the L0
real-tree bed with `--solver=pubgrub` was not run. S1 (the informational
"updates/rebuilds have been skipped" warning on a solvable plan, `btparent`) stays a
documented cut: it needs backtracking state the engines lack.

#430 S1 (2026-11-10): `--solver=pubgrub` now plans binaries. With `--usepkg` the
bridge's pool takes the visible binaries the default walk would accept (same
respect-USE, user-patches, changed-deps and ebuild-visibility filters) and a
binary replaces the ebuild of its version, keeping the USE it was built with.
Run in `pmtest/fixtures` (environment as in the fixture tests):
`target/release/portuale emerge --solver=pubgrub -pv --getbinpkg=y --usepkg=y
dev-libs/remotebinpkg` prints `[binary  N g   ] dev-libs/remotebinpkg-1.0-1::gentoo
... USE="-rbfoo" 560 KiB`, rc 0, the same as the default solver; before this it
printed no plan for that atom, and `dev-libs/binebuildtie` showed an `[ebuild]`
row instead of `[binary  N g   ]`. In pmtest,
`python3 -m pytest pytests-contract-suite -q -k plans_binaries_like` compares
stdout, stderr and exit code on three cells; unit test
`load_versions_offers_a_binary_that_replaces_its_ebuild_under_usepkg`. Not yet
done: a built package's DEPEND/BDEPEND are still traversed (`dev-libs/binbdepconsumer`
still lists `binbdeponly`, #430 S2), `--usepkgonly` still offers ebuilds (S3), and the
L0 bed with `--usepkg` (S4) was not run.

#430 S2 (2026-11-10): the binaries `--solver=pubgrub` plans now behave like the
default solver's on build-time deps and on a stale ABI. A binary's recorded
DEPEND/BDEPEND are not traversed unless bdeps is on (`--usepkg` turns the `auto`
default off): in `pmtest/fixtures`, `portuale emerge --solver=pubgrub -pv
--getbinpkg=y --usepkg=y dev-libs/binbdepconsumer` now prints the single
`[binary  N g   ]` row without `binbdeponly`, as the default solver does (before:
two rows); with `--with-bdeps=y` both solvers still agree. A binary whose built
`mbabilib:0/1=` no longer matches the planned `0/2` provider is masked and its
ebuild planned: `emerge --solver=pubgrub --pretend --verbose --update --deep
--usepkg dev-libs/mbabicons` prints `[ebuild  N     ] dev-libs/mbabicons-1.0`
and no `[binary` row (S1 alone planned the stale binary, found by sweeping the
existing `--usepkg` cases). In pmtest: `python3 -m pytest pytests-contract-suite
-q -k "drops_a_built or went_stale"`; unit test
`a_binary_record_drops_build_time_deps_unless_bdeps_is_on`. The stale-binary check
itself has only the pmtest pin, no Rust unit test. Differences that remain on the
`--usepkg` cases (the `mbabilib` `r` marker and old sub-slot, `f484`'s extra `R` row,
the `=n` flag cases) predate binaries; `--usepkgonly` still keeps the ebuild pool (S3).

#430 S3 (2026-11-10): `--usepkgonly` now leaves `--solver=pubgrub` with binaries
and installed packages only, as the default solver does (and the explicit
`--usepkgonly=n`, which Portage treats as dropping the ebuild db). In
`pmtest/fixtures`, `portuale emerge --solver=pubgrub -pv --usepkgonly
dev-libs/newpkg` fails like `emerge -pv --usepkgonly dev-libs/newpkg` (an ebuild
exists, no binary does), and `-pv --usepkgonly=n dev-libs/pg367-top` and
`-pvuD --usepkgonly=n dev-libs/pg34-blib` give the default solver's output; before,
pubgrub planned source builds for all three. In pmtest, `python3 -m pytest
pytests-contract-suite -q -k usepkgonly_drops` compares stdout, stderr and exit code
on six cells; unit test `usepkgonly_offers_binaries_and_no_ebuilds`. Re-running the
existing `--usepkg` cells under both solvers leaves four differences, none about
binaries (`pg34-gone` with `--usepkg=n` twice, the `mbabicons` `r` marker and old
sub-slot, `f484`'s extra `R` row). The L0 bed with `--usepkg` (S4) was not run.

#430 S4 (2026-11-10): the L0 real-tree bed was run with `L0_PORTUALE_OPTS=--solver=pubgrub`
in both `L0_EMERGE_OPTS=-pv` and `-pv --usepkg` (`differential-test-bed/run/l0-resolver.sh`,
no bed change needed; 120 probes each), once on the tree with #430/#431 and once on a frozen
copy of the unmodified binary (`PMTEST_PM=portuale-bed PMTEST_NO_BUILD=1`, binary in
`/var/tmp/bedbin`). The portuale output is byte-identical on all 120 probes in both modes, so
the changes move nothing on the real tree. That is also the limit of the evidence: the container
has no usable binpkgs (no `[binary` row in any output), so the binary pool is covered by the
fixture pins, not by this bed. The unexplained graded findings both runs share (25 in `-pv`, 34 in
`-pv --usepkg`) are filed as #698.

#596 (2026-11-10): the `_unsatisfied_system_deps` gate is now run through the real dispatch loop,
not only as two pure functions. `cargo test --release -p portuale system_gate` builds S (`@system`,
`PDEPEND` on the slow X = `schedwait`, `sleep 8`) and U (`schedmark`, whose `src_compile` touches a
marker), runs `--jobs 2`, and compares U's start marker with X's vdb entry. Under
`--merge-wait-scope=deep` and `system` U starts only after X landed; under `none` and `toolchain`
(`schedsys` is no core-toolchain package) U starts beside X. Disabling `system_deps_hold_new_builds`
or the `wait_set.contains` arm in `note_landed` turns the first test red (both checked). The `/`
condition is simulated by the test seam `MergeOptions.system_gate_forced`, so this is portuale's
loop against its own rule, not a Portage capture: a bed cell with Portage's `emerge.log` order
(step 2 of #596) stays open only if doubt remains. The expectation rests on the charter's
healthy-installation property (no build against a half-merged system); Portage 3.0.83
`_system_merge_started` is the hint.

#676 (2026-11-10): an installed package reached only through a package the run merges is now a
consumer. On the Q19 ROOT (`p590vpar`, `virtual/p590vva`, `virtual/p590vvb`, `p590vprov-5.42`
installed, world = `dev-libs/p590vprov`), `emerge -p dev-libs/p590vpull` used to print
`U p590vprov-5.44` + `N p590vpull-2`, which leaves `virtual/p590vvb-1` (`5.42` or `5.38` only) without
an accepted provider and says nothing; it now prints `N p590vpull-1` and the skip block naming both
virtuals' `=5.42*`, the same plan host Portage 3.0.83 gives. Live check:
`pytests-contract-suite/test_emerge_pretend_contract.py -k test_676`; reverting
`live_reachable_consumers` turns it red (checked). The closure follows only the `||` arms the walk
took: taking all of them broke the #643/#647 pins. The expectation rests on the charter's
dependency-satisfied property; Portage's `_complete_graph` is the hint. With an empty world and no
`@system` the scope is still off (#700).

#697 (2026-11-10): a binary package whose own dependency cannot be met now yields to the ebuild
instead of failing the run. With one binary build of `app-misc/u257par-1` recording
`RDEPEND: app-misc/u701gone` (a package no repository has) and a fine ebuild of the same version,
`emerge --pretend --usepkg --binpkg-changed-deps=n app-misc/u257par` used to print
`emerge: there are no ebuilds to satisfy "app-misc/u701gone"` and exit 1; it now plans
`[ebuild  N     ] app-misc/u257prov-2` and `[ebuild  N     ] app-misc/u257par-1`, rc 0, the plan host
Portage 3.0.83 gives (`backtrack: 1/20`). Live check:
`pytests-contract-suite/test_emerge_pretend_contract.py -k test_697`; reverting the binary-marked mask in
`unresolved_dep.rs` turns it and the Rust test `missing_dep_trigger_masks_a_binary_parent_by_build` red
(checked). The expectation rests on the charter's dependency-satisfied property; Portage is the hint.

#698 (2026-11-11): a provider upgrade is no longer withheld when every installed package that pins the
old version can follow it. On a scratch ROOT where `dev-libs/p569prov` is at 5.42 and three installed
@world holders recorded the old pins (`p698rebuild` built `:0/5.42=` only, `p698reinst` pinned
`=p569prov-5.42*` with a tree ebuild that already accepts 5.44, `p698upg` pinned `=5.42*` and has a `-2`
that wants `=5.44*`), `emerge --pretend dev-libs/p569prov` plans the provider 5.44 together with an `R`
for `p698rebuild`, an `R` for `p698reinst` and a `U` for `p698upg`, and prints no skipped-update block,
with `--solver=portage` (the default) and `--solver=pubgrub` alike. Portage 3.0.83 keeps 5.42 with an
`R` and the block (Q31 in `LLM/portage-quirks.md`); on the real tree that is perl 5.42.2 held by 69
installed `virtual/perl-*` and `dev-perl/*` pins, each of which has a route to 5.44, so portuale plans
perl 5.44.0 and the 48 holders it reaches. With a holder that has no route (`p573virt`, whose tree ebuild
still pins `=5.42*`) the provider stays at 5.42, as before. Live check:
`pytests-contract-suite/test_emerge_pretend_contract.py -k test_698`; making `holder_has_route` return
`None` for every holder (`co_upgrade.rs`) turns the first test red (checked). The expectation rests on
the charter's dependency-satisfied property and "as up to date as it can be"; Portage is the hint only.

#699 (2026-11-11): `emerge -pu` now says when it holds an update back for an installed package's `[use]`
dependency. On a scratch ROOT with `dev-libs/w605x-1` installed with `w605flag`, a world consumer
`app-misc/w605conc-1` (`RDEPEND="dev-libs/w605x[w605flag]"`) and `w605x-2` visible with the flag off,
`emerge --pretend --update --deep @world` still plans no `w605x` row and exits 0, and now also prints
`NOTE: One or more updates were not merged because an installed package needs a USE flag:` with
`not merged: 2 (app-misc/w605conc-1 needs dev-libs/w605x[w605flag])` and
`Change USE: dev-libs/w605x +w605flag`. With the `||` consumer `w605cona` (`|| ( w605x[w605flag] w605y )`,
`w605y` installed) nothing is printed. Portage 3.0.83 fails the first root with the same USE hint; here the
plan is healthy so the exit code stays 0. Live check:
`pytests-contract-suite/test_emerge_pretend_contract.py -k test_699`; making `held_back_use_updates` return
an empty list turns the first cell and the Rust test `update_held_back_by_an_installed_consumers_use_dep_is_reported`
red (checked). The expectation rests on the charter's property 5 (say what the user can do); Portage is the hint.

#704/#705 (2026-11-11): measurements and triage only, no product code. The co-upgrade's second walk costs
about one more resolve on a probe where an upgrade is withheld and the holders can follow: in the bed
container `emerge -pv dev-lang/perl` takes 0.23 s with the pre-#698 binary and 0.53 s on `bc31ebc9`,
`kde-apps/kdecore-meta` 1.84 s and 3.68 s, while five probes with no perl in them stay within 1 %; in seven
of the ten slower cells the plan is unchanged, so the walk is dropped after it ran. The bed findings #698
left behind are sorted by cause: the default-solver run has 0 unexplained findings and 0 failing plans in
`l0-health.sh`, but 136 invariant violations (#713); the pubgrub run keeps 224 unexplained, of which two
are bridge defects (#714 misleading rows, #715 rebuilds merged before the perl upgrade) and 22 are legal
order differences; the `--usepkg` cell keeps 4 legal order differences that Portage itself introduces.
Four corpus records that had drifted since 2026-11-07 are blessed after review; the fifth is #711. Live
check: `python3 differential-test-bed/compare/resolve-compare.py differential-test-bed/logs/l0-20261111T110117Z
differential-test-bed/compare/known-divergences.yaml` prints `UNEXPLAINED : 224`. The verdicts rest on the
charter's properties; Portage's output is the comparison.

#701 (2026-11-11): the `@world` analysis in `crates/portage-world` (the engine behind `mrg --check-world`
and `--plan-world`) now has a test for each rule its entry in #684 lists. Run
`cargo test --release -p portage-world`: 46 tests pass, among them one per rule: newest version first and
repository priority as the tie-break, `=`, `<` and `~` narrowing, `||` taking the installed branch, the four
relative USE dependency forms, an unneeded slot against an invalid one under `unique`, the five rebuild sets,
and a result that is the same at 1, 2 and 8 threads. Each test was seen to fail when its rule was broken
(for instance reversing the version comparison, or making `parallel_map` return results in completion
order). The expected values come from PMS `dependencies.tex` and `names.tex` and from the charter, not from
Portage. The unique, thread-count and unneeded-slot rules are reachable only through the library (#706), and
the set tests use their own `sets.conf`, so they show the set engine works, not that Portage's shipped
definitions are right.

#704 (2026-11-11, decision): the co-upgrade's second walk stays as it is. Live check, in the bed
container: `emerge -pv dev-lang/perl` takes 0.23 s before #698 and 0.53 s after, `emerge -pv app-text/texlive`
0.92 s and 1.89 s with the same 146 rows, a full `@world` update 0.85 s and 0.87 s, and five probes without
perl 0.99 to 1.01 times. The cost is one extra resolve, paid only when an upgrade is withheld and every holder
can follow it; the 18 holders of perl pins that the world does not reach on the measuring host are left
unprotected, as before #698. The numbers and the rows they come from are in the `#704` entry of
`LLM/backlog-tasks.md`.

#715 (2026-11-11): `--solver=pubgrub` now merges a provider before the packages that are rebuilt for it. On
the shape `dev-libs/p715prov` (version 2 builds with an uninstalled `p715lib`) with two installed holders of
`p715prov:=` in the world, `emerge --pretend --solver=pubgrub dev-libs/p715prov` printed `p715lib`, the two
holders as `rR`, then `p715prov-2`, which builds the holders against the old provider again; it now prints
`p715lib`, `p715prov-2`, then the holders, as the default solver does. Live check:
`pytests-contract-suite/test_emerge_pretend_contract.py -k test_715` and `cargo test --release -p portage-repo p715`;
restoring `deps: Vec::new()` in `slot_operator_rebuild_entries` turns the pubgrub variants red (checked). On the
bed, `L0_PORTUALE_OPTS=--solver=pubgrub l0-resolver.sh` has no `rR` row before the perl upgrade in the ten
cells that had 18 to 31 of them. The expectation rests on charter property 2 and PMS `dependencies.tex`,
"Slot dependencies"; the default solver already ordered it this way (Portage not probed on this shape).

#713 (2026-11-11): the plan now says why a package is in it when the only reason is an upgrade. On the scratch
ROOT of the #698 pin, `emerge --pretend --json dev-libs/p569prov` lists the three holders (`p698rebuild`,
`p698reinst`, `p698upg`) with `requested: false` and `required_by: [dev-libs/p569prov]`; before, `required_by` was
empty and the bed's output invariants reported 6 violations on that document, none now. On the L0 bed the
default solver has 0 invariant violations (136 before) and exits 0. Live check:
`pytests-contract-suite/test_emerge_pretend_contract.py -k test_713` and `cargo test --release -p portage-repo p713`;
removing the line that sets the parent in `resolve/driver.rs` or in `resolve/assemble.rs` turns the matching Rust
test red (checked). The expectation rests on charter property 5 and the project's own output invariants; Portage is
not consulted. Pubgrub holders and `--tree` are #718.

#703 (2026-11-11): a holder that cannot follow an upgrade is now named where nothing else names it. On the scratch
ROOT of the #698 pin (installed `p573virt-1` pins `=p569prov-5.42*`, no visible version accepts 5.44),
`emerge --pretend --solver=pubgrub dev-libs/p569prov` keeps the provider at 5.42 and prints `NOTE: One or more updates
were not merged because an installed package cannot follow them:` with `installed: 5.42`, `not merged: 5.44
(dev-libs/p573virt-1 needs =dev-libs/p569prov-5.42*)` and `No visible version of dev-libs/p573virt accepts
dev-libs/p569prov-5.44.`; before, pubgrub printed nothing. The default solver already prints the skipped-update
block naming the holder, so it prints no second note. Live check:
`pytests-contract-suite/test_emerge_pretend_contract.py -k "test_698 or test_703"` and
`cargo test --release -p portage-repo p703`; forcing `holder_has_route` to `Some` turns both solvers' pins red
(checked). The expectation rests on charter property 5; Portage prints the block only. On the L0 bed the default
solver has 0 invariant violations; pubgrub keeps the 8 `plain != --json` ones it had before.

#702 (2026-11-11): `--solver=resolvo` and the perl co-upgrade, recorded as a cut (CUT-12). On the scratch ROOT of the
#698 pins (`p569prov-5.42` installed, 5.44 in the tree, three holders in the world), `emerge --pretend
--solver=resolvo dev-libs/p569prov` prints no merge row and exits 0, and so do `--update --deep dev-libs/p569prov` and
`--update --deep dev-libs/p698upg`; `dev-libs/p590vpull`, which is not installed, still merges. Resolvo keeps
installed packages as they are: no holder is left without its dependency, and there is no upgrade for the holders to
follow, so nothing was wired. Live check: `pytests-contract-suite/test_emerge_pretend_contract.py -k test_702`; the
pin goes red once resolvo plans an upgrade of an installed package. Source: the capture itself, Portage not
consulted. Resolvo not upgrading installed packages is the engine-side gap the #211 pin docstring already mentions.

#700 (2026-11-11): an installed consumer is protected also when the world is empty. On the scratch ROOT of the #676 pin with
`world = []`, `emerge --pretend dev-libs/p590vpull` used to plan `U dev-libs/p590vprov-5.44` and `N p590vpull-2`, leaving the
installed `virtual/p590vvb-1` (`=p590vprov-5.42*` or `=5.38*`) with nothing that satisfies it and no warning; it now plans
`N p590vpull-1` only and prints the skip block naming `virtual/p590vvb-1`. `--solver=pubgrub` holds 5.44 back too and names the
holder in the #703 NOTE; it also prints an `R p590vprov-5.42` row (#729). Live check:
`pytests-contract-suite/test_emerge_pretend_contract.py -k test_700` and `cargo test --release -p portage-repo p700`; both
solvers' pins and the Rust test fail on the previous commit (checked). The expectation rests on charter property 1; Portage was
not consulted for this shape.

**2026-11-11 — #729: a provider the pubgrub bridge holds back prints no `R` row.** The holder pass pins the installed version with an extra `=cat/pkg-ver` atom; the non-selective rule "an installed requested package is rebuilt" turned it into `[ebuild   R    ]`. The atom is now dropped from the rebuild set unless the user asked for it. Check: `emerge --pretend --solver=pubgrub dev-libs/p590vpull` on the #676 ROOT with an empty world (`test_700_empty_world_still_protects_an_installed_consumer[pubgrub]`) merges only `N p590vpull-1`, with the #703 NOTE naming `virtual/p590vvb`. Oracle: charter property 5 (messages help).

**2026-11-11 — #706: `mrg --check-world` rules reachable by a user.** `mrg --check-world --future-options=check-world-unique,check-world-unneeded` on a world holding `dev-libs/w701dup` and `dev-libs/w701two` exits 1 with `INVALID dev-libs/w701dup:0` and lists `UNNEEDED dev-libs/w701two:1`; without the option the same world exits 0 and prints neither. `check-world-threads=N` changes the time, not the report. The option, its stages and its precedence (default, `make.conf`, `PORTUALE_FUTURE_OPTIONS`, command line) are in `docs/future-options.md`. Oracle: the charter (properties 1, 3, 5).

**2026-11-12 - #717: a long ROOT path in the `--keep-going` notice wraps where Python's `textwrap` wraps.** A word longer than the room left on the line is cut after its last hyphen that fits (`TextWrapper._handle_long_word`), not at the column. Check: `python3 -m pytest pytests-contract-suite/test_portuale.py -k test_emerge_keep_going_endgame_matches_portage --basetemp=/var/tmp/p715-backlog-700-705-2026-11-11-evidence-x` (the path that failed on 2026-11-11) now passes, and `cargo test --release -p portuale wrap_words` pins the same lines. The preserved-symlink and stale-env notices share the one wrapper. Oracle: Python `textwrap` as used by Portage's `Scheduler.py`, as a hint; charter property 5 (output does not depend on where the user's ROOT is).

**2026-11-12 - #727: a plain request leaves a perl-style provider where it is.** `emerge -pv dev-lang/ruby` on the bed tree plans 62 new packages and keeps perl 5.42.2, with both solvers, as Portage does (111 packages with `perl U` before). The #698 co-upgrade still applies when the request names the provider (`emerge -pv dev-lang/perl`: 49 rows) or is an `--update --deep`. Check: `python3 -m pytest pytests-contract-suite/test_emerge_pretend_contract.py -k test_727` and `cargo test --release -p portage-repo p727` (both solvers; the Rust test fails on the previous commit). Oracle: the owner decision of 2026-11-12 (smallest plan), Portage 3.0.83 `_want_update_pkg` as a hint, charter property 1 (the provider stays, so its holders keep their pins).

**2026-11-12 - #728: a package rebuilt for a new sub-slot takes its best visible version.** `emerge -pv --usepkg dev-lang/perl` on the bed tree plans `U perl-core/File-Temp-0.231.200 [0.231.100-r1]` and `U virtual/perl-File-Temp-0.231.200 [0.231.100-r1]` with both solvers (it planned `rR` of the installed versions), 8 rows as Portage. Check: `python3 -m pytest pytests-contract-suite/test_emerge_pretend_contract.py -k test_728` and `cargo test --release -p portage-repo p728` (both fail on the previous commit). Oracle: charter property 3, Portage 3.0.83 `emerge -pv --usepkg dev-lang/perl` as a hint (`r U` rows; `_want_update_pkg` counts the auto-rebuild set as an argument).

**2026-11-12 - #718 (a): the pubgrub holders name the provider as their parent.** `emerge --pretend --solver=pubgrub --json dev-libs/p569prov` on the #698 root now lists `p698rebuild`, `p698reinst` and `p698upg` with `requested: false` and `required_by` = `dev-libs/p569prov` (it was empty), as the default solver does since #713; `--tree` is unchanged by decision. Check: `python3 -m pytest pytests-contract-suite/test_emerge_pretend_contract.py -k test_718` and `cargo test --release -p portage-repo p718` (the Rust test and the pin fail on the previous commit, checked with the 2026-11-12 binary). Oracle: the project's own output invariants (`output_invariants.check_json`) and charter property 5.


**2026-11-12 - #731 and #726: the bed runs the pubgrub modes with pubgrub and knows which solver a run used.** `L0_PORTUALE_OPTS=--solver=pubgrub PMTEST_PM=portuale-bed PMTEST_NO_BUILD=1 differential-test-bed/run/l0-resolver.sh` (in pmtest) now passes the option to the `--tree`, `--quiet` and `--json` runs too, so the output invariants read the pubgrub plan: the seven `plain merge list != --json entries` lines are gone and 71 other lines show (`--tree` nests a row under a parent that `required_by` does not list, #732, and a root at depth 1 without a parent row, #733). The run directory now holds `run-options.json` (the effective `L0_*` options), `pm.json` carries `binary_sha256`, and `l0-report.txt` prints a `## run options` block; allowlist entries can carry `solver:` and `usepkg:` and `layer: l0-pubgrub` explains pubgrub runs only. Result on the same binary: default bed 91 clean / 0 unexplained / 0 violations (unchanged), pubgrub bed 63 clean / 151 unexplained (all `use`, #714), down from 207. Check: `python3 differential-test-bed/compare/test-count-categories.py` in pmtest prints `ALL OK`; `python3 differential-test-bed/compare/resolve-compare.py differential-test-bed/logs/<pubgrub run>` regrades a stored run. Oracle: charter properties 2 and 5, the project's own output invariants.


**2026-11-12 - #730: `--solver=pubgrub` no longer plans a virtual whose only matching `||` arm names a package with no ebuild.** The bridge now drops, before the entries reach the `portage-atom-pubgrub` crate, every `||` arm that names a cp with no ebuild in any repo and no installed instance (a group left with no arm stays whole, so the miss is still reported). With perl held at 5.42 by a plain request, `emerge -pv --solver=pubgrub dev-libs/p727top2` (pmtest `ROOT` of the #698 fixtures) plans `dev-libs/p727virt-1`, the virtual that accepts the kept provider, like the default solver; before, it planned `p727virt-2` (`|| ( =p569prov-5.44* ~p727core-2 )`, `p727core` does not exist) beside the old provider. Check: `python3 -m pytest pytests-contract-suite/test_emerge_pretend_contract.py -k test_727_a_kept_provider` in pmtest (both solver cases pass, the xfail is removed) and `cargo test --release -p portage-repo p730` here. Oracle: charter property 1.


**2026-11-12 - #714: `--solver=pubgrub` rows are compared with the installed package.** An upgrade, downgrade or new-slot row of `--solver=pubgrub` now carries the same markers as the default solver's: `emerge -pv --solver=pubgrub --update dev-libs/upgradeusepkg` (pmtest fixture tree) prints `USE="added%* keep -change* (-drop%)"` (it printed every flag plain and no `(-drop%)`), `-p` shows only the changed flags, an ebuild row prints the bytes still to fetch (`dev-libs/fetchrestrictmissingpkg`: `13 KiB`, it printed `0 KiB`) and the `[oldver]` bracket of an upgrade shows the installed sub-slot (`[1.0:0/1::testrepo]`, not the new `0/2`). The bridge reuses the default solver's choice of the version a row is diffed against (`display_versions`), so there is one rule and one place. Check: `python3 -m pytest pytests-contract-suite/test_emerge_pretend_contract.py -k test_714` in pmtest (red on the previous binary for the pubgrub cases, green now) and `cargo test --release -p portage-repo p714`. Oracle: the default solver's rows for the same atoms, charter property 5.


**2026-11-12 - #733: `--solver=pubgrub --tree` no longer indents a requested row under a hidden parent.** A kept installed package that is not expanded (no `--deep`) is a leaf in Portage and in the default walk: its dependencies are not in the plan. The bridge gave such entries their md5-cache dependency edges, so for `dev-build/ninja` (`PDEPEND=app-alternatives/ninja`, whose `reference? ( >=dev-build/ninja )` points back) the tree nested the requested reinstall under the hidden installed `app-alternatives/ninja` and printed it one level deep with no parent row. Now the leaf entry has no `deps`. Check: `python3 -m pytest pytests-contract-suite/test_emerge_pretend_contract.py -k test_733` in pmtest (fixtures `dev-libs/p733top` and `p733dep`; the pubgrub case printed `R    ]  dev-libs/p733top-1` with two spaces on the previous binary) and `cargo test --release -p portage-repo p733`. Oracle: the project's output invariant (a depth-1 row has a parent row), charter property 5.


**2026-11-12 - #716: the pubgrub plans pass the health checker's dependency checks.** `--solver=pubgrub --json -v` entries carried no `dependencies`, `iuse`, `use`, `required_use` or `sub_slot` (the default solver's do), so `health_checker.py` evaluated a planned package with no USE and reported 122 `runtime-deps` and 4 `blocker` findings in 22 plans, for example `sys-apps/systemd` PDEPEND `>=sys-apps/dbus-1.9.8[systemd]` against the planned `dbus`. The plans were right; the bridge now fills the fields from the record it solved. Check: `L0_PORTUALE_OPTS=--solver=pubgrub PMTEST_PM=portuale-bed differential-test-bed/run/l0-health.sh` in pmtest (see the backlog entry for the numbers) and `python3 -m pytest pytests-contract-suite/test_emerge_pretend_contract.py -k test_716`. Oracle: the checker's own contract (charter property 1) and the default solver's JSON for the same atom.


**2026-11-12 - #732: the pubgrub `--tree` and `required_by` agree.** The 67 `--tree nests X under Y, which is not in either's required_by` lines of the pubgrub bed (41 in the `dev-lang/perl` cell) had three causes: 26 non-holder rows were invented edges of installed leaves (fixed by #733), 41 were co-upgrade holders whose `required_by` names the moved provider while `--tree` follows the real dependency edge (owner decision: the invariant accepts a package whose own dependencies name the holder; `required_by` stays), and one row had a real second dependent that `required_by` lacked (the bridge now fills `required_by` from the same edges the tree uses, `complete_required_by`). Check: `python3 differential-test-bed/compare/check-invariants.py differential-test-bed/logs/<pubgrub run>` in pmtest (2 lines left, #736) and `python3 -m pytest pytests-contract-suite/test_output_invariants.py -k "holder or nesting"`. Oracle: the project's output invariants, charter property 5.


**2026-11-12 - #735: pubgrub tells the user which file to fetch by hand.** `emerge -pv --solver=pubgrub dev-libs/fetchrestrictmissingpkg` printed `[ebuild  N     ]` and no `Fetch Restriction:` line, so a `RESTRICT=fetch` package gave no warning that its distfile must be put in DISTDIR first. The bridge now evaluates `RESTRICT` against the pick's USE and checks the Manifest files in DISTDIR, as the default solver does: `[ebuild  N F   ]` and `Fetch Restriction: 1 package (1 unsatisfied)`, or `f` and no suffix when the file is present. Check: `python3 -m pytest pytests-contract-suite/test_emerge_pretend_contract.py -k test_735` in pmtest and `cargo test --release -p portage-repo s735`. Oracle: the default solver's output and the existing `Fetch Restriction` pin (Portage output.py, getfetchsizes), charter property 5.


**2026-11-12 - #736: a withheld provider that nothing needs is not a plan entry.** `emerge -pv --json --solver=pubgrub virtual/perl-ExtUtils-MakeMaker` (installed; its newer version needs the new perl, which the installed holders keep back) listed `dev-lang/perl` as an `already_installed` entry that nobody requested and nothing required, so the invariant checker reported an unexplained row. The pin that withholds the provider has done its work once the plan has no dependent for it, so the bridge drops the entry; when a dependent exists (the default solver reinstalls the virtual) the entry stays with its parent. Check: `python3 -m pytest pytests-contract-suite/test_emerge_pretend_contract.py -k test_736` in pmtest (fixture shape: installed `dev-libs/p727virt-1` on the #698 root) and `python3 differential-test-bed/compare/check-invariants.py differential-test-bed/logs/<pubgrub run>` (0 violations). Oracle: the output invariants, charter property 5.


**2026-11-12 - #711: the stale-binary case spends one backtrack again.** `emerge -pv --usepkg dev-libs/somaskparent` printed `backtrack: 2/20` since #696. The product was right and the fixture was not: the `Packages` index stanza for the stale `somaskchild` gpkg named no `PATH`, so the pool held the same binary twice (the file without a build id, the stanza as an orphan with `BUILD_ID: 1`), and a mask that names a build hides only one of them. The stanza now names its file. Check: `python3 -m pytest pytests-contract-suite/test_emerge_pretend_contract.py -k test_711` in pmtest (`backtrack.restarts == 1`; it reads `2` with the old stanza). Oracle: Portage keys the mask by package object (depgraph.py, `_slot_change_backtrack`), so one binary is one mask.


**2026-11-12 - #734: the perl allowlist entries name their rows.** `l0-perl-co-upgrade-698` matched any finding with "perl" in it on ten cells, for both solvers, and `-consequences` also hid every `order` difference there; it had already hidden #727. The entries now list the exact rows of the current runs (48 extra packages and one missing row in the `dev-lang/perl` cell, the two skipped-update rows, the two totals rows, two pubgrub-only rows of the `virtual/perl-ExtUtils-MakeMaker` cell), and the comparator pairs the slot-less rows of one package by version, which retires the two interim `multislot-row-collapse` entries. Check: `python3 differential-test-bed/compare/test-count-categories.py` in pmtest, and replay a stored run without a bed: copy `differential-test-bed/logs/<run>` to a scratch directory, delete its `l0-report.*`, then `python3 differential-test-bed/compare/resolve-compare.py <copy> differential-test-bed/compare/known-divergences.yaml` (0 unexplained on the default and the pubgrub run of 2026-11-12). Oracle: the charter (an allowlist entry hides only what it names) and the health run.

**2026-11-12 - `experiment-tree-diagram`: the dependency tree of packages or sets as a diagram.** A new experiment crate (not a default workspace member, like `experiment-pre-sort-atoms`; build it with `cargo build --release -p experiment-tree-diagram`) prints Graphviz DOT on stdout, so `experiment-tree-diagram --depth 2 app-editors/nano dev-vcs/git | dot -Tsvg > tree.svg` gives a vector diagram. Only the two text/vector forms the owner allowed exist; Mermaid and d2 were weighed and dropped because neither can be told which packages share a row (DOT's `rank=same` can), and `dot` is already on the hosts. Targets are atoms or `@sets` (`@world`, `@system`, `@profile`, user sets); the walk is `portage-world`'s first pass, extended in the same slice with `World::edge_kinds` (which `*DEPEND` variables stand behind each edge, as bits of `DEP_KEYS`), `Options::targets` (start from these instead of `@world`) and `seed_sets` (`@system`, `@profile` and `@selected` each on their own). What the picture says: dependents are above their dependencies, one row per build level (`levels.rs`: a package sits one row above the highest package it waits for, so a row can be built in parallel; `DEPEND`, `BDEPEND`, `RDEPEND` and `IDEPEND` make a package wait, `PDEPEND` never does and is drawn `constraint=false`; a cycle is cut at a run-time edge first). Every kind has its own arrow: `DEPEND` blue, `RDEPEND` green with an open head, `BDEPEND` red dashed, `PDEPEND` purple dotted, `IDEPEND` orange dashed with a circle head. `DEPEND` and `RDEPEND` on the same pair merge into one arrow of two parallel lines (blue and green); any other combination, `DEPEND` with `BDEPEND` included, stays as separate arrows in different colours. A package takes the look of the most important set it belongs to, in the owner's order `@system` (red, double border), `@profile` (orange, rounded), `@world` (green, thick border), everything else (white); a set claims a package by name and slot, the atom's version is not compared. A legend is part of the diagram. Build-time dependencies of installed packages are followed by default (a package already installed would otherwise show no `BDEPEND`); `--without-bdeps` restores Portage's rule. `--kinds all|KIND[,KIND]...` (default `all`, also `tutte`; names in any case, e.g. `--kinds depend,bdepend`) draws only the chosen kinds: the rows still come from every dependency, so a row still means "buildable at the same time", and a package left with no drawn arrow is dropped unless it was asked for (`app-editors/nano dev-vcs/git --depth 2` is 68 packages with `all`, 54 with `--kinds BDEPEND`). `--bundle` joins the arrows of one kind that reach the same package from two or more packages into a small dot (`shape=point`) just above it and goes on as one arrow, so a package everybody needs has one long arrow instead of a fan (it adds one edge per join, so it tidies the picture but does not remove arrows or crossings). `--reduce` hides every ordering arrow a longer path already implies (`a -> b -> c` makes `a -> c` redundant), whatever its kinds, computed on the kinds drawn so no arrow hides behind one that is not shown, and `PDEPEND`/cycle-cut arrows neither count as a path nor are hidden; on `app-editors/nano dev-vcs/git --depth 2` it takes the arrows from 236 to 139 (96 with `--kinds depend,rdepend`), and unlike `--bundle` it is what makes the picture readable; the price is that a direct dependency a path implies is no longer drawn. `--jobs N` (also `--jobs=N`) numbers each package with the build step it falls in when N builds run at once (label `[n] cat/pkg-ver`): a package starts in the first step after everything it waits for finished, the ready ones beyond N are picked by longest chain of dependents above, then by name; with `--jobs 1` every number differs, with more two packages can share a number and are built together. Every package counts as one step (a round, not a duration), a package installed at the chosen version is labelled `[x]` and one with no version to choose `[!]`; neither is numbered or makes others wait, and the numbers come from every dependency whatever `--kinds`/`--reduce` draw. Live: `app-editors/nano dev-vcs/git --depth 2` is 68 packages in 68 steps with `--jobs 1`, 34 with 2, 18 with 4 (the number of rows); a script over the DOT found no ordering arrow whose dependency has the higher number and never more than N packages in one step. `--depth N` keeps only the packages within N dependencies of a target, because `dot` needs about 25 s (with the `nslimit=4` the output sets) on the 535 packages `nano` reaches and the picture is unreadable anyway. Known cuts: a repository without `metadata/md5-cache` adds no dependencies (no aux-metadata provider is registered); a set atom's version is not compared. Check: `cargo test --release -p experiment-tree-diagram` (levels and DOT text) and `cargo test --release -p portage-world a_target_is_followed_and_each_edge_knows_its_dependency_kind` (fixture `dev-libs/odw0a`, one package per `*DEPEND` variable); live, `target/release/experiment-tree-diagram --depth 1 app-editors/nano | dot -Tsvg -o /dev/null` renders. Oracle: the PMS dependency-variable meanings (`3rdparty/gentoo/proj/pms/`), the charter's "healthy installation"; Portage's output is not compared.

**`mrg --check-world` says what it could not read, and prints findings a program reads (#692, plan `02.692` slices A1 and A2, 2026-11-12).** An ebuild that is on the path of `@world` and whose metadata cannot be read (no cache entry and it cannot be sourced) is now a row `UNREADABLE <cpv>  (<reason>)`, the summary gains `, N not read` when N is not zero, and the exit code is 1: a check that silently dropped what it could not see would call a tree clean that it never looked at (the health skill's rule that a blind spot is never "healthy"). `--json` prints one object per finding, in the shape `health_checker.py` uses (`property` one of `world-invalid`, `world-no-match`, `world-unneeded`, `checker-limitation`; `package`, `atom`, `reason`), then the summary `{"atoms","slots","unsatisfiable","not_read"}`; the text rows and the JSON lines come from one list. Live on this host: `mrg --check-world` exits 1 with `NO MATCH app-misc/mime-types[nginx]  <- www-servers/nginx-1.31.7-r1` and `check-world: 4535 atoms, 2297 package slots, 1 unsatisfiable` in about half a second; `--json` ends with `{"atoms":4535,"slots":2297,"unsatisfiable":1,"not_read":0}`. Check: `cargo test --release -p portuale check_world` (among them `an_unreadable_ebuild_fails_the_check`, `json_of_an_unreadable_ebuild_is_a_checker_limitation`, `the_thread_count_does_not_change_the_answer`). Oracle: charter property 1; PMS `metadata-cache.tex` for what a cache entry is; Portage's output is not compared.

**The world check has mutants (#692, slice A3, 2026-11-12).** `../pmtest/differential-test-bed/compare/check-world-mutants.py` copies a tree, breaks it once per operator and requires `mrg --check-world --json` to exit 1 with a finding that names the broken package: `mask_best`, `drop_keyword`, `raise_bound`, `remove_package` (expected `world-invalid` or `world-no-match`) and `unreadable` (expected `checker-limitation`); the control `harmless` masks a package nothing reaches and must change nothing. A mutation that changes no file is an error, never a kill, and the unmutated copy must first give the result of the original tree (the copy keeps symlinks; #449 is why). Live: `PORTUALE_BIN=target/release/portuale python3 ../pmtest/differential-test-bed/compare/check-world-mutants.py` prints `6/6 good`, each mutant with the files it changed. Check: `python3 -m pytest pytests-contract-suite/test_check_world_mutants.py` in pmtest (four tests, one of them a mutant the check must not report, which has to survive). Oracle: charter property 1, by construction of each mutant; Portage is not involved.

**Three views of one `@world` side by side (#692, slice A4, 2026-11-12).** `../pmtest/differential-test-bed/compare/world-crosscheck.py RESOLVER.json FAST.json [--check CHECK.jsonl]` compares the resolver's plan, the fast plan and the check, and labels each difference: `[judge]` for one slot with two "best" versions, for a world atom the check finds no version for while a plan holds one, and for a clean check next to a resolver that gave up; `[note]` for a slot only one plan lists. It exits 0: a difference is something to judge against the charter, not a failure. The L0 health bed (`run/l0-health.sh`) now captures `WORLD_resolver-nobin`, `FAST_world.json` and `world/check-world.jsonl` and prints a "world cross-check" section; the fast plan is shown and does not gate. Live on this host against the resolver's `-pvuDN --getbinpkg=n @world` capture: no `world-version-differs`; two `world-invalid-but-planned` lines for `app-misc/mime-types[nginx]` (both plans keep 2.1.54 installed, and the line says USE dependencies are not judged, so the check and the plans do not contradict each other); 5 slots only the resolver lists (#739, #740) and 102 reinstalls only it lists. In the bed container (`logs/l0-health-20261112T182116Z`, before the rebuild rows): 584 atoms, 409 slots, 0 unsatisfiable; 9 resolver-only slots and 56 resolver-only reinstalls without binaries. Check: `python3 -m pytest pytests-contract-suite/test_world_crosscheck.py` in pmtest. Oracle: charter properties 1 and 3; the script shares no code with `portage-world`.

**`mrg --plan-world --sort depend` follows the PMS on post dependencies, and the plan is a document the health checker judges (#693, slices B1 and B2, 2026-11-12).** `World::edge_kinds` records which of `DEPEND`, `RDEPEND`, `PDEPEND`, `BDEPEND`, `IDEPEND` names each edge, and `World::ordering_edges()` leaves out an edge only a `PDEPEND` names, because a post dependency is merged "at some point before the package manager finishes the batch" (PMS `dependencies.tex`) and so orders nothing. `--json` prints one line `{"entries":[...],"kept":[...],"request":["@world"],"aborted":null}` in the shape of `emerge -pv --json`; `request` is what lets the checker judge property 3 (newest visible version). Live: `mrg --plan-world --json --sort depend > plan.json && python3 ../pmtest/pytests-contract-suite/health_checker.py --vdb /var/db/pkg plan.json` gives `"findings": []` on this host (it gave 15 failures before the rebuild rows of the next paragraph). Check: `cargo test --release -p portage-world a_post_dependency_does_not_order` and `-p portuale json_rows_equal_text_rows_in_order`. Oracle: PMS `dependencies.tex`; charter properties 2 and 3 through `health_checker.py`.

**The fast plan rebuilds the consumers of a slot operator (#693, slice B3, 2026-11-12).** A package kept at its installed version gets a row `[ebuild   R    ]` when a `:=` dependency recorded in its VDB entry names a sub-slot that the plan changes (PMS `dependencies.tex`, slot operators: `:=` binds to the slot and sub-slot of the best installed match, `:*` does not), placed after the provider; the walk is repeated until the set stops growing. With the alpha future option `plan-world-newuse` a kept package whose installed `USE`, cut to the `IUSE` of that version, differs from the configured one is rebuilt too (owner decision Q4). The rows are computed only for the plan (`Options.rebuilds`), so `--check-world` does not pay for them. Live: `mrg --plan-world --sort depend` prints 246 rows here (234 `U`, 2 `N`, 10 `R`, the consumers of `dev-lang/perl:0/5.42=` among them) in under a second and the checker's 15 failures are gone; with `--future-options=plan-world-newuse` one more row. A package whose binding was already stale before the plan is not listed (#741). Check: `cargo test --release -p portuale a_slot_operator_consumer_is_rebuilt_after_its_provider a_changed_use_rebuilds_only_with_the_future_option` (cargo takes one filter: run them one at a time). Oracle: PMS `dependencies.tex`; charter properties 1 and 2 through `health_checker.py`.

**`mrg --plan-world=emptytree`, and both plans say what they did not check (#693, slices B4 and B5, 2026-11-12).** `--plan-world` takes an optional mode: `update` (the default) or `emptytree`, any other word a usage error (exit 2) that names the two. `emptytree` lists every slot `@world` reaches: `N`, `U`, `D`, or `R` for an installed version that stays; build dependencies are followed for every package, because everything is built. Standard error carries two lines, the count and order, then `not checked: blockers, REQUIRED_USE, slot conflicts, USE rebuilds; run 'mrg -puDN @world' for the full plan` (no `USE rebuilds` with `plan-world-newuse` on or in `emptytree`, whose full plan is `mrg -pe @world`); standard output stays rows only. Live: `mrg --plan-world=emptytree` prints 2177 rows here (9 `N`, 241 `U`, 1 `D`, 1926 `R`) in about a second, and the checker on its `--json` finds one failure, `www-servers/nginx` asking `app-misc/mime-types[nginx]`, the atom `--check-world` reports, plus three `checker-limitation` notes. `-up @world` (no `--deep`) is not built, by owner decision Q5: without the deep walk the fast model is only a filter on rows and can hide a needed upgrade. Human documentation: `docs/world-check-and-plan.md`. Check: `cargo test --release -p portuale plan_world` and `python3 -m pytest pytests-contract-suite/test_plan_world.py` in pmtest. Oracle: charter property 5 for the second line (pinned text only; property 5 still has no checking program); the PMS dependency classes for which edges an everything-is-built plan follows.

**Close-out of plan `02.692` (#692, #693, 2026-11-12).** The L0 health bed after all slices (`../pmtest/differential-test-bed/logs/l0-health-20261112T224433Z`, exit 0): 122 plans, 113 checked, 0 with a failure portuale introduced; `check-world: 584 atoms, 409 slots, 0 unsatisfiable`; against the resolver without binaries the fast plan lacks 2 slots (`sys-devel/gcc:16`, `sys-devel/binutils:2.47`, #739) and 22 reinstalls, down from 9 and 56 before the rebuild rows. The fast plan itself is shown apart and does not gate: the checker finds 10 `build-order` failures in it there (#743), none on this host. Code size, tests included: `check_world.rs` 282 -> 601 lines (88 -> 261 without tests), `plan_world.rs` 241 -> 623 (159 -> 305 without tests), against base `d99dd587`; the plan's hope that the shared helper would keep the two files near their old size did not hold, the new features (JSON, unreadable rows, modes, rebuild rows, the second summary line) are most of the growth. Left open with numbers: #739 (new slot of an atom without a slot), #740 (openssl 3 -> 4 and the reinstalls only the resolver lists), #741 (a `:=` binding stale before the plan), #742 (`emptytree` and a second installed slot), #743. Check: `sh LLM/tools/gate.sh` (3487 Rust tests, 4933 pmtest tests, none failed, at `561ce17b` / pmtest `0149dbc`). Oracle: the charter's properties through `health_checker.py`; Portage's plan is a comparison, not the expected value.

**2026-11-12 - #737: a kept consumer of a moved sub-slot rebuilds, and a weak blocker the plan itself removes is a note.** On the host `-pvuDN @world` plan the checker reported 922 `build-order` findings around the `openssl` 3.6.5 -> 4.1.0 move: `cargo-c-0.10.25` keeps `openssl:0/3=` but has no merging entry, so the same-slot arm (complete-mode reachability only) never saw it. A kept orphan — installed, no merging entry, outside the pass scope and unwalked — now enters the same-slot scope when its own `:=` sub-slot pin names the moved sub-slot (the `SubslotChangedSet` shape); it is a fallback per provider, so an orphan-only edge on a provider an in-scope consumer already pins stale never fires (the first, wider shape regressed 8 tests and was narrowed the same day). The checker's half: a weak (`!`) blocker matching a package the plan replaces later is now a `cycle-noted` note instead of a failure — only a strong (`!!`) blocker fails (PMS 8.3) — and a need met only by a later merge is documented as a checker limitation, allowlisted for the host `@world` run. Check: `cargo test --release -p portage-repo tests::rebuild_scan` (21 passed, 0 failed) and, in pmtest, `python3 -m pytest pytests-contract-suite/test_health_checker.py -q` (129 passed; fixtures `dev-libs/w737compat`, `dev-libs/w737old`). Oracle: the checker written from the PMS (Portage is not run); the one genuine plan defect in the capture, the `runtime-deps` row for `cargo-c`, is closed by the planner fix scheduling the missing rebuild.

**2026-11-12 - #737 review correction.** The #737 paragraph above claims a kept-orphan planner arm that rebuilds the `cargo-c` consumer and that the `runtime-deps` row is closed. It is not: on the host `-pvuDN --getbinpkg=n @world` the arm never fired (its `covered` fallback keeps it off for `openssl`; 2080 entries before and after), and the arm was withdrawn in the review; the row stays open as #744. What stays true is the checker half (weak-blocker note, 129 pytest passes) and the order finding: Portage's own plan on this host puts `openssl` at row 35 and `net-nds/openldap` and `sys-fs/cryptsetup` at rows 270 and 271 of 348. Check: `emerge -pvuDN --getbinpkg=n --color=n @world | grep -n 'openssl-4\|openldap\|cryptsetup'`.

**2026-11-12 - #684: `experiment-pre-sort-atoms` is retired; the experiment worked.** The question it was built for (can the packages `@world` needs, and the best version of each slot, be listed without running the resolver) was answered yes, and its useful half now lives in the product: `portage-world` behind `mrg --check-world` and `mrg --plan-world` (#691-#693). The crate, its binary, its test and the hidden `portuale __aux-metadata` entry (its only caller) are deleted; the libraries it caused (`portage_repo::depcache_aux_metadata`, `installed_deps_changed`, `needed_elf`, `preserved_libs`) stay because the product reads them. The experiment's own findings stay recorded in the #684 entry and in `LLM/evidence/merge-gate-backlog-684-experiment-pre-sort-atoms.md` (history, not repointed). Check: `grep -rn 'pre-sort-atoms' Cargo.toml crates` finds nothing; `cargo test --release --workspace` (3506 passed, 0 failed, after `cargo build --release`, which several `portuale` tests need). Oracle: the charter; no Portage comparison, nothing user-visible changes (`mrg --plan-world` messages now say `portage-world:` instead of `experiment-pre-sort-atoms:` in the rare set-not-found line).

**2026-11-13 - #739, #742: the fast plan names the newest slot of a slotless atom; #743: a build dependency on an `||` group is ordered after the changed branch.** Plan modes (`mrg --plan-world`, including `=emptytree`) narrow an atom without a slot to its best visible slot even when an older slot is installed (check mode keeps the installed-first narrowing, so `--check-world` output is byte-identical); the same rule settles #742, where `emptytree` listed both installed slots while the resolver builds one. Live on this host: `world-crosscheck.py` before had `resolver-only app-emulation/wine-staging:11.21` (installed `:11.20`), after it is gone; `fast-only` gains `dev-lang/rust-bin:1.99.0` and `media-libs/gexiv2:0.16`, both of which the resolver merges cleanly when asked directly, so no constraint is broken and property 3 supports listing them. For #743 the `||` resolves to the changed branch (installed `cat/pkg` beats available, ties go to the first), so the `virtual -> provider` edge exists and the old order came from cutting the dependency cycle at the back edge; `depend_order` now cuts a cycle at its first runtime-only edge, keeping every build-time edge. Live: the host `--plan-world --sort depend` plan has the same 249 rows with 0 checker findings before and after (25 runtime cuts through kept `systemd`/`perl`/`python`, 21 head rows re-serialized provider-first). Check: `cargo test --release -p portage-world narrow` (`plan_mode_takes_the_best_slot_only_even_when_an_older_one_is_installed`), `cargo test --release -p portuale plan_world` (`a_bare_atom_with_an_installed_older_slot_lists_the_newest_slot_as_new`, `emptytree_lists_one_slot_of_a_bare_atom`), `cargo test --release -p portage-world order` (`a_cycle_is_cut_on_a_runtime_edge_first`, red before the cut logic, green after), and in pmtest `python3 -m pytest pytests-contract-suite/test_plan_world.py -q` (the 4 fixture rows do not move). Oracle: charter properties 3 (#739/#742) and 2 (#743) through `health_checker.py`; Portage's output is a comparison, not the expected value. Residue filed as #745 (the checker still fails a genuinely unserializable runtime cycle that no static order satisfies).

**2026-11-13 - #741, #744: a kept package whose `:=` binding is stale is rebuilt, in the plan and in the resolver.** Charter property 1 (a `:=` binding naming a gone sub-slot leaves the dependency unmatched; PMS `dependencies.tex`, `sec:slot-dep`) plus property 5 (each unrepaired binding named once with its atom and reason). The fast plan judges staleness against the *final* provider version -- the chosen one when the plan changes the provider's slot, else the installed VDB version -- so a binding stale before the plan still repairs; the resolver gains the same rule as a third scan arm (`stale_pin_for_consumer`). Live on this host (`emerge -pvuDN --getbinpkg=n @world`, a source build): `mrg --plan-world | grep -c '   R    ]'` goes 10 -> 30 (every added row a kept go consumer: `app-admin/helm-4.2.4`, `app-admin/mongo-tools-100.9.4` and 18 more; `terraform`, `runc`, `kubelogin` were ordinary upgrades already), kept 1925 -> 1905, `mrg --check-world` byte-identical. The resolver plans the same 20 (19 new `reinstall` rows plus `gocryptfs`, already one) and newly upgrades `dev-util/cargo-c-0.10.25 -> 0.10.26` at merge order 2070 (not a reinstall row: the forced reinstall resolves to the best visible version; the binding is repaired either way). The `health_checker.py` `runtime-deps` row for `cargo-c` is gone (findings 925 -> 925, plus one new `build-order` row for `skopeo` (`DEPEND >=app-crypt/gpgme-1.5.5:= is not usable yet: RDEPEND of net-libs/ngtcp2-1.25.0 is met only by net-libs/ngtcp2-1.26.0, merged later (#1048 > #897)): a late-rebuild ordering the checker reports by design (the documented #737 limitation, health_checker.py:698-706 -- a need met only by a later merge is a checker limitation, not a plan defect). The unmet atom is a *runtime* need of the intermediate ngtcp2, not a build-time need of skopeo: skopeo's build-time inputs are all present via the installed ngtcp2-1.25.0 instance, and the final merged set holds 1.26.0, so properties 1 and 2 both hold). Refusals print as `plan-world: N stale slot-operator binding(s) not rebuilt: cat/pkg (<atom> not satisfied by <final version>)` and ride `--json` as `stale-bindings`, omitted when empty (none on this host). The arm reads runtime keys always and build-time keys for a walked consumer when builds can happen (`walked && (with_bdeps || !usepkg)`); a same-version reinstall moves nothing (the unsatisfied-oneshot oracle pins the bare row); under `--emptytree` it stays out (Portage files no slot-operator rebuild there). Only a genuine version change earns the `causing rebuilds` pair, and only when the consumer's tree still names the provider -- a rebuild re-records the live deps, so a dropped dep has nothing stale to repair. Check: `cargo test --release -p portage-world rebuild` (new `a_kept_package_with_a_stale_binding_is_rebuilt`, `a_rebuild_that_would_hold_back_a_changed_provider_is_refused`, `a_consumer_with_no_visible_ebuild_is_not_rebuilt`, updated `a_sub_slot_change_rebuilds_the_slot_operator_consumers_only`), `cargo test --release -p portuale plan_world` (`a_stale_binding_the_plan_cannot_repair_is_named`), `cargo test --release -p portage-repo tests::rebuild_scan` (new stale-pin tests incl. the reinstall-is-no-move and emptytree-out shapes, the walked-but-unscoped masso carve-out) and the four p698 tests unchanged; in pmtest the with_bdeps matrix, the unsatisfied-oneshot, the missed-update and the #254 oracles (the last keeps its rebuild but prints no block), plus the `revdepslotconsumer` pin whose tree names no dep (upgrade only). Suite: `scripts/pytest-parallel.sh` 4937 passed, 2 skipped, 2 xfailed, 0 failed (plus the self-test drift line the corpus mechanism always prints). Beds: `l0-health.sh` exit 0 with findings identical to the same-day baseline (122 plans, 113 checked, 0 with a failure portuale introduced; 268 findings before and after, zero added, zero removed); `l0-resolver.sh` exit 0, UNEXPLAINED 0 over 120 probes, with the `l0-737-openssl-world-late-reinstall-order` allowlist entry now matching nothing (filed as #747). Oracle: the charter (Portage is compared, never followed); the #737 review-correction stands (the kept-orphan arm stays withdrawn). Follow-up review fixes, same branch: the fast plan only rebuilds when the chosen tree ebuild still names the provider (a rebuild re-records the live deps); the resolver considers every stale provider of a consumer instead of stopping at the first; `world-crosscheck.py` reports unrepaired bindings as `[note] fast-stale-binding-unrepaired` (with a unit test).

**2026-11-14 - #740: the fast plan takes the `||` arm that admits the newest version (plan `02.740`).** Charter property 3 (each package ends at the best visible version every constraint allows): `branch_preference` in plan mode (`Options::rebuilds`, the same flag as #739's `narrow_slotless`) ranks a branch every non-blocker atom of which the best visible candidate of its slot matches above a merely installed one — the package's best visible when the atom names no slot, the best visible in that named main slot when it does — with the installed bit as the tie-break (new `AltPreference::BestVisibleAvailable` / `BestVisibleInstalled`, top of the derived `Ord`, selectable in the first pass; check mode keeps today's order, so `mrg --check-world --json` is byte-identical). Live on this host: `mrg --plan-world` upgrades `dev-libs/openssl` 3.6.5 -> 4.1.0 and lists `dev-libs/openssl-compat:3` as new (before: kept 3.6.5, no compat row); the `world-crosscheck.py` resolver-only rows for both are gone and `resolver-only-reinstall` drops 101 -> 26 (the 71 openssl-naming slot-operator rows plus 4 `llvm_slot_18` USE rows become fast reinstalls; the 25 USE and 5 unexplained slot-operator rows stay, the 5 filed as #748). The fast plan also upgrades `app-crypt/sbsigntools` and `sys-devel/bpf-toolchain` (both were resolver-only upgrades): rebuilt `efitools`/`systemd` now follow their build-time edges, so the rows are the documented cascade of a rebuilt package, not arm churn. New `fast-only` rows, each property-supported: `dev-vcs/pkgcruft-git` reinstall (its recorded build-time `openssl:0/3=` binding repaired, the #741 rule for a walked consumer on a source build; the resolver leaves it kept) and `fast-stale-binding-unrepaired dev-util/cargo-c (dev-libs/openssl:0/3=)`: cargo-c records a sub-slot-3 pin with no visible ebuild to rebuild to (its tree ebuild now says bare `:=`), so the plan keeps 0.10.25, names the binding once, and the resolver's `cargo-c-0.10.26` upgrade row stays resolver-only (explained, no number: the package is unreachable from `@world` and its tree ebuild postdates the 11-13 capture). Unchanged from before: the two `fast-only` new-slot rows (`rust-bin:1.99.0`, `gexiv2:0.16`, property 3, both merge cleanly when asked) and the single `world-no-match` (`mime-types[nginx]`). Check: `cargo test --release -p portage-use-reduce best_visible` (`a_selectable_best_visible_alternative_beats_an_installed_one`, red as a missing variant), `cargo test --release -p portage-world branch` plus `… -p portage-world stale_installed_arm` and `… -p portage-world parallel_slot_arm` (the five new tests: `a_plan_prefers_the_branch_that_admits_the_best_version`, `the_same_branch_choice_in_check_mode_keeps_the_installed_one`, `a_best_visible_tie_prefers_the_installed_branch`, `a_stale_installed_arm_loses_to_a_best_visible_uninstalled_one`, `a_parallel_slot_arm_is_judged_against_its_own_slot`), `cargo test --release -p portuale plan_world` (`the_fast_plan_takes_the_newest_arm_of_a_disjunctive_dependency`), and in pmtest `test_740_the_plan_takes_the_newest_arm` on the committed `w740*` fixtures plus `scripts/pytest-parallel.sh` 4939 passed, 2 skipped, 2 xfailed. Beds: `l0-health.sh` 2026-11-14 vs the 2026-11-13 baseline finds the identical finding set (669 finding instances and 268 unique each side, zero added, zero removed; 122 plans, 113 checked, 0 with a failure portuale introduced), and default `l0-resolver.sh` reports UNEXPLAINED 0 over 120 probes with the identical explained-finding set (every portuale-side cell capture byte-identical; the Portage-side captures differ only in timing lines, the perl-cell extras only in display order). No churn beyond the rows above. Evidence: `docs/evidence/02.740/` (before/after captures, the S0 requester dump, bed deltas). Oracle: charter property 3; the four `openssl-compat` consumers' ebuild `||` lines and kept `sudo`'s `openssl:=` as the mechanism's ground, never Portage's output.

**2026-11-14 - #745, #746: the fast plan cuts a cycle where it can still be built, and breaks an uncuttable one by building twice (charter 2/2a, plan `02.745-746-property-2a`).** A runtime edge out of a package the plan builds with still orders (PMS 8.1): `depend_order` computes the build-time-used set (targets of a build-kind edge, never propagated through `RDEPEND`; `virtual/*` targets excluded as a heuristic that keeps the perl/libcrypt rebuild cluster ordered, not as PMS soundness) and cuts the first runtime-only edge that is not build-time in fact, so the repaired provider comes first — on the bed's FAST plan `Locale-gettext` moved before `automake` and the automake `build-order` finding is gone, with `perl-5.44` still early and the plan fully checker-clean. A cycle with no cuttable edge duplicates its back-edge consumer (at most twice) right after the cycle's last member; `--json` marks only the second entry `"built_twice": 2`, the stderr summary counts unique slots, and `--sort alpha`/`resolver` still list each slot once. The checker excuses the first build's need when the identical cpv is merged again later with the need matched (`cycle-noted` naming the second order) and collapses an identical cpv before the one-slot check. Runnable: `cargo test --release -p portage-world order` (`a_runtime_edge_that_carries_a_build_time_need_is_not_cut`, `a_build_time_cycle_is_broken_by_rebuilding_the_consumer_once`), `cargo test --release -p portuale plan_world` (`a_cycle_through_a_kept_package_orders_the_repaired_provider_first`, `an_all_build_time_cycle_gets_a_second_row`), and in pmtest `python3 -m pytest pytests-contract-suite/test_health_checker.py -q` (the `test_745_*` quintet, plus the `drop_second_build` mutant in `health-mutants.py`). Residues: #749 for the resolver (`emerge`) half of 2a, #751 for a kept virtual whose installed provider is broken (ordered early by the alias rule instead of waiting for its rebuild).

**2026-11-14 - #745: the fast plan verifies its own cut with a usability closure and rebuilds the first package an unmet need chain leaves behind (phase 2 of plan `02.745-746-property-2a`).** Charter 2a, planner side: after the cut order exists, `close_usability_gaps` walks it in merge order and duplicates the first package whose build-time need chain is unmet, at the first later position where the closure holds (mirroring the checker's `_Moment.gap`: build needs must be usable via their `RDEPEND` closure; a cycle back counts as usable; a kept provider's unmet need is pre-existing, not the plan's). The #745 shape now orders `[prov, virt, user, user]`, the checker notes the first `user` build once and reports nothing else — the duplicate-free plan still fails, so the fix is provably planner-side. Determinism: first in merge order, name-ordered edges, at most twice per slot. Host `@world`: 8 second builds, all §2.2 breakers (§8 adds none there — verified by running the host plan with the closure stubbed to identity: identical 8 slots; the checker stays at 60 `build-order` + 1 pre-existing failure). Check: `cargo test --release -p portage-world usability` (3), `cargo test --release -p portuale plan_world` (`a_usability_closure_left_open_by_the_cut_gets_a_second_build`), pmtest `test_745_duplicate_free_mixed_cycle_still_fails`. Residues #749/#751 unchanged; no new backlog numbers filed.

**2026-11-14 - #738: the `@world` binary failure closed as a #741/#744 consequence, and the conflict block names its parents and calls a binary a binary.** Charter properties 1 (the stale `:=` binding repairs instead of conflicting) and 5 (no bare `pulled in by`, no `ebuild` label on a `[binary ...]` row). The S0 re-baseline on the `backlog/738-binpkg-slot-conflict` branch exits 0 both ways (`-pvuDN --color=n --json @world`: 1992 entries, no `slot_conflicts`, openssl 3.6.5 -> 4.1.0 plus new `openssl-compat:3`, ffmpeg 8.1.3 kept with no second slot-0 entry, `cargo-c` 0.10.25 -> 0.10.26; `--getbinpkg=n`: 2081 entries, exit 0); the 15 duplicate `(cat,pkg,slot)` keys are all `already_installed` at different versions with `slot: null`. The §2/§5 binpkg rule from the plan was deliberately not built (STOP rule). The plan-level pin is `stale_pin_stale_bound_consumer_rebuilds_with_one_entry_per_slot` (`crates/portage-repo/src/tests/rebuild_scan.rs`, pmtest fixtures `dev-libs/w738prov`, `dev-libs/w738cons`): provider `Upgrade 3 -> 4`, consumer `Reinstall` with `slot_operator_rebuild: true` and the `abi_rebuilds` pair, one merge-bound entry per slot, no conflict. The rebuild re-resolves through the pool (from the binary here -- `--binpkg-changed-deps` strips `:slot/sub=` on both sides exactly like Portage's `strip_slots` in `_changed_deps`, so a sub-slot-only drift never forces the ebuild in either implementation). Both message defects reproduced cheaply on the current tree and are fixed in `show_conflict_instance`: two versioned atoms used to render a bare `pulled in by` (Portage shows `{atom} (Argument)` whenever the argument carries a version/slot/USE reason), and a binary instance used to read `ebuild scheduled for merge` (now read from its own merge row). Runnable: the S0 pair above, plus `=dev-libs/slotconflicttarget-1.0 =dev-libs/slotconflicttarget-2.0` (both `(Argument)` lines, exit 1) and `--usepkg =dev-libs/binpkgrmpkg-1.0 =dev-libs/binpkgrmpkg-2.0` (`binary scheduled for merge` on the 2.0 instance, exit 1). Check: the Rust test, and in pmtest the two new slot-conflict tests plus their CASES rows (`test_slot_conflict_two_versioned_atoms_name_their_argument_parents`, `test_slot_conflict_binary_instance_reads_binary_scheduled_for_merge`). Evidence: `docs/evidence/2026-11-14-738/`. Residue: binary conflict *parent* labels still read `ebuild` (#752).
