# T5+T2-SLICED ∷ BabelTele mirror — LLM→LLM consult-only

> **Historical planning/investigation doc, retired 2026-09-15.** Its outcome is `docs/backlog-tasks.md`'s status line for this item; the extracted ground truth, gotchas and dead ends are in [`../recap-of-backlog-ops-2026-09-15.md`](../recap-of-backlog-ops-2026-09-15.md). Kept verbatim below for citation/provenance only.

SRC=docs/backlog_tier_5_and_2_sliced.opus.md @ main:69f5877 2026-09-14; AUTH=SRC (divergence→SRC wins, never cite THIS as evidence)
PROJ: readability-relaxed semantic projection (BabelTele, arXiv:2606.19857); glyph-light ⇒ savings from dropped prose; ALL ids/paths/line-refs byte-exact; line-refs drift→relocate by symbol
LEGEND (PY/CT-byte-identical refs below = pre-2026-09-15 rules, superseded by PYREF-REMOVED): `∷`is `;`fence `|`field `→`leads/causes `⇒`decision `←`from `∈`in `∉`not-in `¬`not `∧`and `∨`or `:=`def `!`must-not `?`unverified `+`add `-`remove `@`anchor file:sym:line `F/M/S`=tiers(F Opus5/Fable5.1, M Sonnet5, S Haiku4.5; Frev=F reads full diff pre-user-commit) `U`=user-owned `D#`=owner decision §3 `PR`=rust/portage-repo/src/lib.rs `MO`=rust/portage-repo/src/merge_order.rs `PT`=rust/portuale/src/pretend.rs `EP`=rust/portuale/src/ebuild_phases.rs `EM`=rust/portuale/src/ebuild_merge.rs `MD`=rust/mrg-director/src/lib.rs `PY`=python/emerge_pretend_reference.py `CT`=tests/test_emerge_pretend_contract.py `F25`=docs/025-tier2-closeout.deepseek.md §11 `L3F`=TEST/findings/l3.md `L2F`=TEST/findings/l2.md

PYREF-REMOVED 2026-09-15 (branch backlog/python-copy-removal; docs/second_python_copy_removal.md) ⇒ R3b..R5 ¬PY ¬Rust==PY: pin Rust vs real oracle ∧ tests/test_output_invariants.py green ∧ reviewed corpus drift blessed (PORTUALE_CORPUS_BLESS=1) same commit ∷ #26 F-A1 w/ PORTUALE_DYNAMIC_DEPS_APPEND=1 ⇒ only test_oracle_slotop_undo_cascade fails (PY half gone) ∷ found+fixed C2 race dc8022b (concurrent cache-miss shared depend metadata file)
STATUS: proposed 2026-09-14 ∷ D1-D7 ANSWERED ∷ R0 DONE (#21 cut) ∷ W1 DONE ∷ W2 DONE (R2 rescoped per (a): #36 not-reproducible-as-framed, genuine upstream oracle captured) ∷ W3 DONE: P2b (env rows 0) ∥ C2 (depend-phase fallback) ∥ R3a (025b design, owner read pending before R3b) ∥ H3 (6/8 slots) ∷ W4 DONE (P3 C3 R3b) ∷ W5 H4 DONE ∷ W6 R3c STOPPED→REVALIDATED 2026-09-15 ⇒ R3c∧R3d WITHDRAWN ∷ D7 ANS opt1 ⇒ W6 R3e′ DONE (#25 DONE-PARTIAL) ∷ W7 DONE (R4 #35 ∧ H5 #28) ∷ W8 DONE (R5 #17 CLOSED deliberate cut) ⇒ ALL TRACKS DONE/CLOSED
SCOPE: docs/backlog-tasks.md OPEN only: T5{41,44,45} T2{17,20,21,25,28,35,36} ∷ DONE/DONE-PARTIAL ∉scope
RF: AGENTS.md(step8 verify); docs/agent-context.md; docs/scope-backlog.md §A §H §I §K; per-track plan/finding below

§1 TRACKS+ORDER
  P:={#44,#45} real-exec container ¬PY ∷ unblocks l3-core/@system (#30 S4 stop rule) ⇒ highest leverage ⇒ FIRST
  C:={#41} repo reader+depend phase ∷ ¬PY (D2) ∷ parallel w/ P ∷ BEFORE H4
  R:={#20,#36,#25,#35,#17} resolver (dual-lang until 2026-09-15; R3b+ Rust-only) (#21 cut D1) ∷ order: R0 DONE ∷ R1 #20 → R2 #36 → R3 #25 → R4 #35 → R5 #17
    why #36<#25: local selection change, default-budget fixpoint proven identical ⇒ ¬L0 move ⇒ stable base
    why #35>#25: real graph_db.match_pkgs sees installed as nodes = what #25 adds
    why #17 last: F-B3 ⇒ iteration-1 batch order ← _create_graph insertion order (stable-sort bias) = #25 R3b ⇒ timing on final graph only
  H:={#28} Rust refactor behaviour-neutral ¬PY ∷ any time 1 slot/commit ∷ H4 after C
  DAG: P0→{P1,P2a}; P2a→P2b; {P1,P2b}→P3 ∷ C0→C1→C2→C3→H4 ∷ R2→R3a→R3b→R3c→R3d→R3e→R4→R5 ∷ H1→H2→H3→H4→H5
  FILE-CONFLICT: C×R ∈PR (diff regions; worktrees; rebase C first) ∷ P×C ∈EP (land P2b first ∨ C2 call-site only) ∷ H4×C by design
    P: EM EP rust/portuale/src/emerge_getbinpkg.rs TEST/layers/l3/* ∷ C: PR EP:run_depend_phase ¬PY ∷ R: PR MO PT CT tests/corpus/ ∷ H: MD + call sites portuale/src/{fetch,pretend,emerge_build,emerge_getbinpkg}.rs

§2 INV (every slice)
  R ⇒ Rust pinned vs real oracle ∈CT + test_output_invariants green + blessed corpus drift (was: Rust+PY one commit, superseded 2026-09-15) ∷ P,H ⇒ real-exec only ¬CASES
  expected output ← real Portage oracle (container TEST/ ∨ host emerge) ¬reading source
  ! weaken L0/L1/L2 ∷ R slices: L0 (TEST/layers/l0/in-container.sh) clean/parity/order before+after; any probe regression ⇒ STOP report
  `git add` fixture BEFORE `git clean -fdq fixtures/` ∷ compare failing test NAMES vs clean-main baseline ¬counts
  pub sig change ∈portage-* ⇒ cargo build/test/clippy --release @workspace root
  closing commit updates backlog-tasks.md + findings + scope-backlog.md ∷ ¬push ∷ branch backlog/<track>

§3 DECISIONS ANSWERED 2026-09-14 (U; binding)
  D1 #21→Part 3 deliberate cut ⇒ ANS yes ∷ APPLIED backlog-tasks.md "Deliberate cuts" + scope-backlog.md Part 3 ⇒ R0 DONE
  D2 #41 PY mirror ⇒ ANS no; Rust-only test ∈tests/test_portuale.py ¬CT ∷ C2
  D3 #41 cache ⇒ ANS write depcachedir(/var/cache/edb/dep) if writable else in-memory (=real porttree.py depcachedir_w_ok split) ∷ C3
  D4 #25 ⇒ ANS yes: per-slice L0 order may move ±; net after R3e ¬worse; log every flipped probe ∷ R3b+
  D5 #17 ⇒ ANS time-box R5 = 600 s; limit hit ⇒ STOP + residue→deliberate cut w/ lever table so far ∷ R5
  D6 #28 ⇒ ANS slots only (¬Director as production entry) ∷ H5
  D7 #25 post-revalidation ⇒ ANS 2026-09-15 opt1: close #25 on this evidence (new slice R3e′) ∷ +gedit/nautilus entry ∈known-divergences.yaml like gnome-shell ∷ correct F-B4 ∧ #25 DONE-PARTIAL w/ 2 small residues ∷ verify 2-probe L0 run ⇒ unexplained order 17→15 ∷ R3e′

§4 TRACK-P L3 producer (#44 #45)
  GOAL: L3 smoke (TEST/atomlists/l3-smoke.txt) OWNER+VDB environment 0 unexplained ⇒ l3-core(344) startable
  EVIDENCE: L3F "FILED (backlog #44)" "FILED (backlog #45)" ∷ plan docs/030_L3-source-build-parity.deepseek.md S4 ∷ repro `L3_CONTROL=1 TEST/run/l3-source-parity.sh TEST/atomlists/l3-smoke.txt` → `diff.py --layer l3`
  P0(S,1h,¬code) oracle bed FEATURES=keepwork both containers, sys-libs/ncurses:
    stat -c '%u:%g %n' ${D}/usr/include/curses.h + 1 terminfo PRE-merge both ∷ same paths live root pre∧post merge
    ${T}/environment after EVERY phase both sides (real: /etc/portage/bashrc pre_/post_ hook; portuale: equivalent hook ∨ debug env?)
    ACCEPT: artefacts TEST/logs/… quoted ∈L3F
  P1(M,Frev,2-4h) #44 `l3-merge-owner-1-1` real keeps 1:1 portuale 0:0 (2121 rows, sha256 identical)
    hypotheses ← P0: H1 ${D} already 1:1 on real (install phase loses it) ⇒ fix ∈EP ∷ H2 real merge preserves existing dest FILE ownership (portuale has dir-only exception EM:lchown_or_chown:1889) ⇒ mirror vartree.py _merge_contents/movefile incl when NOT preserved ∷ H3 bed artefact (uid map/normalize) ⇒ harness fix + doc ¬product
    test: install over pre-existing 1:1 file as root (sudo passwordless) fails@main passes@fix
    ACCEPT: smoke OWNER ncurses=0 ∧ L1 porttest 0 findings
  P2a(F,2-3h) #45 `l3-merge-vdb-env-accumulates` bisect ← P0 per-phase env: FIRST phase each symptom appears portuale-only
    symptoms: A real 1× vs distfile 3× + patch list 2× ∷ RESTRICT "test" vs "test test" ∷ stray `declare -- f` `declare -- x=""`
    suspects first: brush ___save_and_filter_ebuild_env wrapper ∷ EP:run_one_phase_bash:2743 env assembly ∷ top-level helper loop leaking f/x (¬local)
    counterpart real: bin/ebuild.sh, bin/phase-functions.sh (next phase re-sources ⇒ re-derives)
    ACCEPT: root cause per symptom ∈L3F w/ file/fn + real counterpart ∷ 3 independent causes ⇒ P2b = 3 commits
  P2b(F,3-6h) fix AT CAUSE ! post-hoc dedup of saved file
    test fixture: ebuild appends A ∧ RESTRICT accumulating under wrong re-source + helper w/ non-local var ⇒ vdb environment.bz2 (bzip2 -dc) each value once ∧ ¬stray locals ∷ both --shell bash ∧ brush
    ACCEPT: smoke VDB env rows 0 (¬LANG/LC_* already normalized) ∧ L1 porttest vdb environment byte-match real ∧ cargo test --release ∧ pytest
  P3(S/M,1h+container) re-run smoke candidate+control ⇒ 0 unexplained ⇒ L3F #44/#45 FIXED w/ log dirs ∷ backlog-tasks #44 #45 DONE + #30 note ∷ lift S4 stop rule ∈030 plan ∷ start l3-core = U

§5 TRACK-C #41 `l2-no-md5-cache-ebuild-fallback`
  GOAL: `emerge -p porttest/docs` on repo ¬metadata/md5-cache = real
  EVIDENCE: L2F entry ∷ workaround committed TEST/images/overlay/porttest/metadata/md5-cache/ ∷ real 3rdparty/portage/lib/portage/dbapi/porttree.py (depcachedir, auxdb, doebuild depend)
  SHAPE: PR:read_md5_cache:1315 ∷ 35 read sites (18 ∈PR) ∷ list_candidates (both langs) ALREADY walks <cat>/<pkg>/*.ebuild =real cp_list BUT cache miss ⇒ silent skip (PY `except OSError: continue`; Rust same shape) ⇒ gap=metadata ¬listing ∷ cache-dir listers (MD:Md5Cache::category) list from cache dir ⇒ also fix
  C0(S,1h) oracle container: overlay copy w/o md5-cache → real `emerge -p porttest/docs` output + ls /var/cache/edb/dep/<repo path>/ ∷ non-root unwritable depcachedir branch ∷ stale cache entry + newer ebuild ⇒ real validates _md5_/_mtime_ & regenerates? ⇒ if yes FILE follow-up ¬grow C
    ACCEPT: quoted ∈L2F
  C1(M,2-3h) single aux-metadata entry point per repo; all 35 sites route through (mechanical, neutral) + per-repo has-usable-cache flag; miss ⇒ ask fallback (still Err this slice); cache-dir listers → *.ebuild when flag false ∷ ! change cached-repo behaviour
    ACCEPT: grep read_md5_cache only ∈entry point ∧ CT + L0 unchanged
  C2(Frev,3-5h,D2) fallback := EP:run_depend_phase:2954 (already used by --regen) → parse → same HashMap shape
    layering: portage-repo ¬call portuale ⇒ PR exposes metadata-provider hook (trait obj ∨ fn ptr registered at startup); portuale registers depend runner; none registered ⇒ today's behaviour
    ACCEPT: cache-less copy output == C0 real byte-for-byte ∧ Rust-only test ∈tests/test_portuale.py
  C3(M,2h,D3) write-back depcachedir real flat layout if writable else in-memory (VolatileCache semantics); 2nd run ¬depend phases ∷ +L2 variant deleting committed cache ∷ TEST/images/overlay/porttest/README.md: committed cache = optimisation ¬workaround (keep)
    ACCEPT: 2nd run ¬depend ∧ L2 porttest `strict hard=0 soft=0` w/ ∧ w/o committed cache ∧ L2F→FIXED

§6 TRACK-R resolver
  R0 DONE 2026-09-14 (D1) #21 → backlog-tasks "Deliberate cuts" + scope-backlog Part 3 1-sentence rationale ∷ owner re-opens ⇒ end of R
  R1(M,Frev,2-4h) #20 [use]-dep unsat block
    TARGET: `emerge: there are no ebuilds built with USE flags to satisfy "<atom>".` + `!!! One of the following packages is required to complete your request:` + `- <cpv>::<repo> (<reason>)` rows ∷ NOW bare `!!! no visible ebuild for dependency` @PT:1216 PY:22591 helper doc PR:8474 ∷ exit1 ∧ Rust==PY already ok
    1 oracle: container fixture ← CT:3789 test_or_group_use_unsat_alternative_reports_the_dependency_it_enqueued_without_autounmask; capture full real incl (dependency required by …) chain + reason wording (change USE / missing IUSE) ∷ + real-tree gnome-shell samba[client] (F25 F-B5)
    2 port depgraph._show_unsatisfied_dep USE branch (candidate set, order, reason text) ∷ REUSE #19 masked-block dependency-chain renderer ! second renderer
    3 re-pin CT:3789 + 1 case per reason kind
    ACCEPT: fixture == real ∧ CT green (names) ∧ L0 clean ≥ before
  R2(F,4-6h) #36 mask-aware selection fallback
    real _select_pkg_highest_available: versions highest-first + dep_check masks applied ⇒ masked-dep version skipped IN selection ∷ portuale: highest visible → masked dep NVC → lower only via next missing-dep mask step
    oracle exists docs/023-oracle.md mg3 ∷ fixtures btparent mgf mgfa ∷ diff visible only tight budget: `mgfa --backtrack=1` real merges, portuale reports
    1 failing pin first ∷ 2 probe MASKED case only ! generalise to any-unsat (graph-consulting dep_check = R4) ∷ 3 default-budget btparent/mgf/mgfa unchanged + --debug backtrack counts where CT pins
    ACCEPT: pin green both langs ∧ L0 unchanged (any move ⇒ STOP report)
  R3 #25 _complete_graph installed nomerge nodes — OWNS: F-B4 (initially-satisfied-by-installed ⇒ no edge; same-slot merge supersedes installed ⇒ drop its in-edges; nghttp2→systemd; gedit #5 nautilus #8; ¬static build_digraph rule since MULTI_deep-update-world needs opposite edge ⇒ walk order is the info) ∷ F-B3 (-pe @system tie-break = real _create_graph LIFO insertion vs MO:build_digraph:1314 DFS from top atoms; _system #11 _world #14 MULTI_emptytree-system #13) ∷ orig (real keeps every @world/@system-reachable installed pkg as node; portuale reverse-dep atoms only MO:add_installed_dependency_closure:1044)
    R3a(F,4h,docs,Frev, U reads) docs/025b-complete-graph-nodes.md: real _create_graph/_add_pkg/_complete_graph node+edge recording (insertion order, DepPriority.satisfied, superseded in-edge removal) vs portuale resolver→GraphEntry→build_digraph ∷ data to hand over: (1) per-node insertion seq (2) per-edge satisfied-by-installed-at-add flag (3) installed nodes first-class ∷ acceptance probes via TEST/scripts/mo-trace/: gtk:4, gedit, -pe @system, MULTI_deep-update-world
    R3b(F,4-6h) insertion seq → GraphEntry → pre-bias order ∈build_digraph (replace DFS) ¬edge change ∷ ACCEPT: MO_ORDER -pe @system = real (368==368 same seq) + D4 flip log
    R3c WITHDRAWN (premise false, see REVAL) was: (F,4-6h) record satisfied-by-installed at edge add ⇒ drop like real priority ∷ ACCEPT: gedit nghttp2 only virtual/pkgconfig edge ∧ MULTI_deep-update-world portage/gentoolkit order kept (B2 guard)
    R3d WITHDRAWN (node sets already = real 16/18) was: (F,4-6h) reachable installed ⇒ nodes; same-slot supersede ⇒ move/drop in-edges; retire reverse-dep atom approximation at parity ∷ ACCEPT: gtk:4/gedit/nautilus node+edge sets = real --debug digraph dump ∧ MO_NODES equal all 4 probes
    R3e′(M,2h+L0 subset,D7) known-divergences.yaml `gedit-nautilus-cluster-a-rewalk-abort` [order] gedit+nautilus ∷ 025 §11 F-B4 correct ∷ backlog-tasks #25 DONE-PARTIAL residue {virtual/man ||-bundle pop timing, docbook-xml-dtd hash swap} ∷ ACCEPT subset: 2 slugs explained ∧ nothing else moves ∷ L0 order 17→15
    R3e(superseded by R3e′) full L0; TEST/findings/l0.md "## I" + F25 F-B3/F-B4 resolved∨residue explained + backlog-tasks ∷ net L0 order count ¬rise (D4)
  R4(F,4-6h) #35 downgrade_probe + live graph_db
    real dep_zapdeps conflict_downgrade/installed_downgrade 3rdparty/portage/lib/portage/dep/dep_check.py soft 476-521 bug 531656 ∷ seam PR:9069 ∷ plan docs/023-backtracking_resolve.md B2
    ¬wait R3d (withdrawn): live graph_db = resolver in-progress entries + best_installed_for_atom ∷ was: after R3: live graph_db = read resolver current node set + slot index ¬new parallel structure
    oracle fixture (build if plan only describes): || group, 1st alt conflicts w/ installed higher version same slot ∷ implement downgrade_probe (config/CLI accept downgrade?) + 2 guards; pin vs oracle (¬PY)
    ACCEPT: fixture == real ∧ L0 ≥ before
  R5(F,time-box 600s,D5) #17 F-B1 drain timing
    state: gtk:4 398==398 nodes; iterations 578 vs real 290; first div = iteration-1 greedy batch order (sys-libs/zlib real pos 9 vs portuale 45) ∷ F25 + TEST/findings/l0.md "## I"
    1 re-measure post-R3: batch order fixed by R3b? ⇒ record L0 + CLOSE #17
    2 else mo-trace replay real _serialize_tasks frontier vs --debug dump; 1 lever at a time ∈14-probe installed-chain family; table lever→probes flipped
    3 600s hit ⇒ STOP ⇒ residue→deliberate cut w/ lever table so far (D5)
    ACCEPT: #17 DONE w/ numbers ∨ deliberate cut w/ lever table

§7 TRACK-H #28 mrg-director
  STATE@69f5877: production {SchedulerPolicy (emerge_build.rs UnlimitedPolicy), MergeEngine (merge_engines.rs, emerge_getbinpkg.rs), NewsSelector (PT FilesystemNews)} ∷ ¬production (MD+tests only) {Fetcher/WgetFetcher, PackagesDb/VdbReader, BinpkgIndex/PkgdirBinIndex+RemoteBinhostIndex, RepoCache/Md5Cache+VolatileCache} ∷ MD:Director:1120 test-only
  INV: behaviour-neutral (trait call over same impl) ∧ CT ∧ L1 porttest ∧ cargo test --release unchanged ∷ 1 slot/commit
  H1(M,2-3h) Fetcher: FIRST fix stale trait doc MD:192-234 ("fsmirror … out of scope"; fsmirrors shipped 5ca67f9) ∷ route portuale::fetch::fetch_src_uri per-candidate download via &dyn Fetcher ∷ sig (entry+distdir ¬Manifest) insufficient ⇒ change trait ! fetch semantics
  H2(M,2-3h) PackagesDb: narrowest production consumer first (depclean reverse-dependents ∨ CONTENTS reads) ⇒ PR:installed_contents_files:5165 ∧ installed_reverse_dependents get production caller via VdbReader
  H3(M,2h) BinpkgIndex: emerge_getbinpkg.rs local PKGDIR index (+remote Packages if same site) via PkgdirBinIndex/RemoteBinhostIndex
  H4(M,2-3h,after C) RepoCache: C1 entry point BECOMES RepoCache: Md5Cache if cache else depend-backed impl write-through VolatileCache/depcachedir ⇒ #41 fallback implemented once
  H5(F,2h,docs,D6) proposal Director as production action_build entry w/ post-H1..H4 call graph ⇒ close #28 per D6

§8 WAVES (parallel ∥; gate)
  W1 P0 ∥ C0 ∥ R1 ∥ H1 (R0 done) ⇒ gate: oracles captured (D1-D6 answered)
  W2 P1 ∥ P2a ∥ C1 ∥ R2 ∥ H2 ⇒ gate: P2a root cause reviewed
  W3 P2b ∥ C2 ∥ R3a ∥ H3 ⇒ gate: R3a approved ∧ D4
  W4 P3 ∥ C3 ∥ R3b ⇒ gate: L3 smoke clean ∧ L2F #41 FIXED
  W5 H4 (R3c withdrawn)
  W6 R3e′ (D7 ANS opt1) ⇒ gate: 2-slug L0 subset
  W7 R4 ∥ H5
  W8 R5 600s ⇒ D5
  sizes = agent-hours excl container runs (L0 longest)

§9 DONE
  T5: #41 #44 #45 DONE ∧ L3 smoke 0 unexplained ∧ l3-core unblocked (run=U)
  T2: #20 #36 #25 #35 DONE ∧ #21 deliberate cut ∧ #17 DONE∨cut w/ evidence ∧ #28 DONE per D6
  ∀commit: ¬L0/L1/L2 regression ∧ CT green ∧ backlog-tasks/scope-backlog/findings updated in closing commits

REVAL 2026-09-15 (W6; SRC §9 "Wave 6 revalidated"; TEST/findings/l0.md "R3c revalidation"; artefacts TEST/logs/r3c-revalidate-20260915/)
  oracle: real --debug stdout+stderr merged unbuffered ⇒ walk+solver+digraph one stream
  mech: nghttp2 >=systemd-209 → installed systemd @add (=portuale) ∷ polkit systemd[policykit] → merge same slot ∷ _solve_non_slot_operator_slot_conflicts depgraph.py:1774 ⇒ _remove_pkg(installed) drops ALL in-edges ⇒ broken parents → _dep_stack → _create_graph re-walk ⇒ edges redirected → merge
  gedit∧nautilus: re-walk returns 0 @gnome-keyring >=gcr-3.27.90:0=[gtk] unsat (rc1 autounmask) ⇒ return ignored ⇒ unwalked parents {nghttp2 pam shadow pambase service-manager} lose edge ∷ walked {gvfs gcr p11-kit dbus util-linux procps} keep → merge ∷ split := CPython set order PYTHONHASHSEED=0 ⇒ ¬portable ∷ real nondeterministic w/ random seed
  complete re-walk (networkmanager kdecore-meta vlc wireshark libreoffice) ⇒ in-edges redirected = portuale today ⇒ "satisfied-at-add ⇒ no edge" FALSE (why variant1 broke _system/_world)
  18 [order] probes: node sets (incl n:) identical 16/18 (exc MULTI_emptytree-system portage-version row ∧ gnome-shell cluster-A nasm) ∷ re-walk abort only gedit nautilus gnome-shell
  ⇒ F-B4 := cluster-A abort residue (F-B5 family) ∷ ¬code change
  ALT (D7 ANS opt1 2026-09-15): 1 REC+CHOSEN R3e′ close #25 on evidence ∷ 2 faithful port (solver re-walk + SipHash13/tuple-hash/set-probe emulation) = deliberate cut ∷ 3 abort-state heuristic drop REJECTED (drops gvfs/gcr/dbus→systemd real keeps)
  knock-on: R4 ¬gated on R3d ∧ W7 startable (D7 answered) ∷ R5 owns 15 non-abort order rows (equal node sets ⇒ frontier/edge timing; families pyproject-metadata {gtk:4 gtk+:3 networkmanager wireshark kdecore-meta} ∧ freetype {firefox thunderbird gimp i3}) ∷ R5 step1 (re-measure post-R3) DONE

W6 CLOSED 2026-09-15 R3e′ DONE: known-divergences.yaml gedit-nautilus-cluster-a-rewalk-abort [order] ∷ L0 subset l0-20260915T063724Z both explained ∧ portuale out byte-identical ∧ advice/error unchanged ⇒ order unexplained 17→15 ∷ F-B4 corrected ∈025 §11 ∷ #25 DONE-PARTIAL residue {virtual/man bundle timing, docbook-xml-dtd hash swap, 023 btnr (installed instance ∉resolved_slots ⇒ ¬slot-conflict party; resolver-level, ¬touched by R3)}
  ⇒ R4 graph_db: installed matches via best_installed_for_atom ! assume resolved_slots covers installed ∷ NEXT W7 R4 ∥ H5

W7 DONE 2026-09-15:
  R4 #35 DONE: alternative_downgrade_demoted + downgrade_probe + visible_tree_matches ∈PR ← disjunction_preference (both || sites) ∷ live graph_db := resolver in-progress entries (¬new structure) ∷ oracle test_or_choices.py::testConflictMissedUpdate via ResolverPlayground + guards-off control (merges nothing = old portuale) TEST/logs/r4-20260915/ ∷ fixtures mlocaml/mllablgl/mllabltk ∷ pin test_or_choice_avoids_downgrade_into_the_graphed_update (default ∧ --backtrack=0 partial)
    +fix build_residual_slot_conflicts: dropped pins accumulate across passes ⇒ filter consumer merge-bound ∈final entries (=reverse_dependency_constraints skip)
    narrowing: highest_in_slot = graph entries only (¬bare installed w/o --update)
    verify: fmt clippy ok ∷ cargo test 1074 pass ∷ pytest 1595 pass ∷ L0 l0-20260915T070018Z clean 100 parity 0.833 UNEXPL 34 order 15 ∧ portuale out byte-identical 120/120 vs l0-20260915T005709Z ⇒ L0-neutral
  H5 #28 DONE: docs/028-director-proposal.md call graph (all slots incl solver production) ∷ REC Director stays test-only (D6) + revisit triggers
  housekeeping: backlog-tasks #35 #36 + "Tier 3" header dropped by e5ffd0f (H3) ⇒ restored
  NEXT W8 R5 #17 600s (D5) ∷ 15 order rows, equal node sets

W8 DONE 2026-09-15 R5 #17 CLOSED (deliberate cut, D5 600s):
  step1 re-measure: R3b seed-sort only covers pretend.rs expand_top_level_atoms (explicit @system/@world arg) ∷ gtk:4 mo-trace re-run post-R3b (TEST/logs/r3c-revalidate-20260915/) ⇒ UNCHANGED 578 vs real 290 iter, nodes 398==398, iter-1 batch div @zlib
  step2 1 lever: same unsorted seed ∈add_installed_dependency_closure Seed-1b (complete-mode @system, fires EVERY complete probe ¬just explicit arg) ⇒ ported identical sort fix
  result: 0 effect ∷ 14-probe container rerun + full L0 (l0-20260915T073208Z) byte-identical 120/120 vs pre-fix ⇒ rules out seed/discovery order ⇒ wall = installed-nomerge-node DRAIN TIMING ∈_serialize_tasks (confirms 2026-09-09 "Deeper dig") ⇒ needs per-node trace across ~400-node graph = multi-session, ¬600s
  DISPOSITION: #17 CLOSED deliberate cut ∷ lever table ∈TEST/findings/l0.md "R5 — #17 closed" ∷ seed-sort fix KEPT (harmless L0-neutral, closer to real) ¬reopen
  ⇒ ALL 7 TRACKS (P C R H) DONE/CLOSED: T5 #41/#44/#45 DONE ∷ T2 #17 cut #20/#25/#28/#35/#36 DONE(-PARTIAL) #21 cut

