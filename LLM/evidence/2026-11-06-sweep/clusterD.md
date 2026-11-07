# Cluster D (#625, #656, #658, #596) - Portage 3.0.83 oracle report, 2026-11-06

Portage: /usr/lib/python3.14/site-packages (3.0.83, = bed image localhost/test-portuale:latest, tree e07309ba).
Scratch captures (this dir): 625b-real-debug.txt, 625b-ptl-debug.txt, 625b-ptl.json, probe625a2.py, mk625a.py.

## #625(b) automake/autoconf [NS] vs [R] in MULTI_emptytree-system  -- ROOT CAUSE FOUND (not _serialize_tasks)
argv (both, bed image, PYTHONHASHSEED=0, ROOT=/, seeded world as layers/l0/in-container.sh):
  /usr/sbin/emerge -pe @system --color=n --debug          -> R autoconf-2.72-r7 @214, NS 2.73 @215 (reproduces bed)
  /usr/local/bin/portuale emerge -pe @system --color=n --debug (target/release @ b24891d1) -> NS 2.73 @213, R 2.72 @268, rsync @262 BEFORE 2.72
Walk is identical: same 22 parents pick `>=dev-build/autoconf-2.72-r1:2.72` from `|| ( >=autoconf-2.73:2.73 >=autoconf-2.72-r1:2.72 )`
(both traces: "Parent Dep: >=dev-build/autoconf-2.72-r1:2.72 required by (net-misc/rsync-3.5.1 ...)").
Final digraph differs: Portage rsync -> autoconf-2.72-r7 (buildtime), 22 edges into 2.72, 21 into automake-1.18;
portuale rsync -> autoconf-2.73-r2, ZERO edges into 2.72 (57 into 2.73). 2.72/1.18 float as parentless roots.
Portage: edges are added only for the atoms the walk selected - depgraph.py:4508-4515 (_select_atoms), :4536 loop over
selected_atoms -> _add_dep :3364. portuale: merge_order/alt_edges.rs:12 suppressed_alt_edges re-picks the `||` branch
AFTER the walk ("first branch all of whose atoms match a merge-bound node", :105-117), ignoring the walk's choice.
Shared by build.rs:49, kept_alt_branches (blocker wait), resolved_dep_targets (--tree display).
Fix: record the walk's chosen branch per (entry, dep key, group) where it is decided (disjunction_stack.rs:667
enqueue_chosen_atoms + the expansion-time group resolvers) and let suppressed_alt_edges use it; keep today's ranking only as
fallback for entries with no record (installed-closure entries). Test: Rust unit on suppressed_alt_edges with two merge-bound
arms and a recorded second-arm choice; CASES pin on a hermetic `-e` twin (expected from ResolverPlayground); then full L0.

## #625(a) installed-without-ebuild libc edge  -- portuale ALREADY MATCHES; only the pin is missing
Portage: FakeVartree.py:132-159 (_aux_get_wrapper: no ebuild -> live_metadata None -> vdb deps kept, :161 _apply_dynamic_deps);
libc asap only for a NEW libc version (depgraph.py:9639-9668, `not vardb.cpv_exists`), so an R libc can merge late.
Oracle (ResolverPlayground, 3rdparty/portage @ae324a809 = 3.0.83, probe625a2.py), request [dev-libs/x dev-libs/y sys-libs/glibc], no options;
glibc-2.0 tree+installed (BDEPEND y), virtual/libc-1 -> glibc, x-0.5 installed / x-1 tree RDEPEND dev-libs/orphan, orphan-1 installed only:
  with_ebuild=False vdb RDEPEND '>=sys-libs/glibc-2.0' -> ['dev-libs/y-1', 'sys-libs/glibc-2.0', 'dev-libs/x-1']
  with_ebuild=False vdb RDEPEND ''                    -> ['dev-libs/y-1', 'dev-libs/x-1', 'sys-libs/glibc-2.0']
  with_ebuild=True  vdb RDEPEND '>=sys-libs/glibc-2.0' -> ['dev-libs/y-1', 'dev-libs/x-1', 'sys-libs/glibc-2.0']
portuale (same shape as a scratch config root, mk625a.py; `emerge -p --ignore-default-opts dev-libs/x dev-libs/y sys-libs/glibc`):
  noebuild-libc: y, R glibc-2.0, U x-1 ; noebuild-plain: y, U x, R glibc ; ebuild-libc: y, U x, R glibc  == oracle.
Fix: pytest with a hermetic tmp_path config root (pattern _slotop_pin_tree, test_emerge_pretend_contract.py:12778) - three params,
no shared fixtures/var/db/pkg change, no corpus impact. Rust unit already exists (installed_closure.rs test :249).

## #656 ruling: CLOSE as DONE (superseded by #657), residue lives in #658
The divergence #656 describes is gone in code and on the bed: replacing posture = rev_dep_or.rs:1360 replacing_posture_holds
(Portage dep_check.py:461-465 `replacing`, :480/:531 `if not replacing` guards), enforcement-open fa85b574, hold 9a2e7701.
L0 l0-20261105T215133Z: libreoffice/gimp only `[order]` rows (set equal), gnome-shell clean, dev-lang_perl R + skip block.
Remaining delta = skip-block parent list (10 vs 69) and Try-Tiny-family skip block -> already #658. Do NOT re-pin oracle_210/C1
(nothing on the bed contradicts them). Keeping #656 open would double-track #658.

## #658 skipped-updates block of dev-lang/perl:0
Portage: an installed child is never ignored (depgraph.py:4466-4478 `not dep.child.installed`), so the installed virtual is
added and marked traversed (:4629-4633); its inline-selected atoms are added whatever the depth (:4642-4700); the inner `||`
is resolved inside the puller's dep_check (dep_check.py:220-246 _expand_new_virtuals -> dep_check) with "prefer upgrades"
(:796-797) -> perl-5.44 scheduled -> slot conflict -> backtrack -> skip block. portuale: pass/unresolved_dep.rs:402/:420
walks installed children only when deep.recurses_at(depth), and resolves their `||` at pop time on the disjunctive stack.
Fix: decide an installed new-style virtual's inner `||` at enqueue time of the puller (pre-demoter snapshot), mirroring
_expand_new_virtuals, and feed the resulting atoms as ordinary deps; reconcile with rev_dep_or.rs seeds. RED exists:
tests_p647.rs:161-162 (ignored). Risk high (two attempts broke 11-13 oracle pins).

## #596 bed capture: FEASIBLE, not with l3-source-parity.sh as is
Gate: Scheduler.py:1686-1730 (_system_merge_started, ROOT=/ only :1699-1702), :1833-1835 (system pkg -> merge-wait queue +
start listener), :2469-2476 (no new job while jobs and set non-empty), :1870-1872 drain, :1988 clear.
portuale: crates/portuale/src/emerge_build/scheduler.rs:906 set, :639-677 fill/drain, :1003 system_deps_hold_new_builds.
Blockers: l3 build-and-merge.sh reads tree atoms only; portuale writes NO emerge.log (no "completed emerge" writer in crates/).
Plan: new pmtest runner (e.g. run/l3-overlay-cell.sh) reusing podman_run_pm: bind an overlay from differential-test-bed/repos/
(new `sysgate` repo: sys-libs/s PDEPEND x, dev-libs/x RDEPEND s, dev-libs/u), add repos.conf + /etc/portage/profile/packages
`*sys-libs/s` in the guest, run `emerge -v --jobs 2 sys-libs/s dev-libs/x dev-libs/u` per PM in fresh containers. Evidence must be
PM-neutral: ebuild phase hooks append `$(date +%s.%N) <phase> <pkg>` to an addwrite'd log (S pkg_preinst = merge start,
X pkg_postinst = land, U pkg_setup = start), plus sleeps in S's pkg_preinst/X's src_compile to widen the window. Compare the
event order, not timestamps; run N=5 for flakiness.
