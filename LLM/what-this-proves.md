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
