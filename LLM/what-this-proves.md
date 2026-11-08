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
