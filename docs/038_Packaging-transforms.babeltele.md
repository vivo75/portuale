# 038-PT ∷ BabelTele mirror — LLM→LLM consult-only
SRC=docs/038_Packaging-transforms.plan.md @ main:ec13936 2026-09-13; AUTH=SRC (divergence→SRC wins, never cite THIS as evidence)
PROJ: readability-relaxed semantic projection (BabelTele, arXiv:2606.19857); glyph-light ⇒ savings from dropped prose; ALL ids/paths/line-refs byte-exact; claims ±provisional until container confirms
LEGEND: `∷`is `;`fence `|`field `→`leads/causes `⇒`merged-decision `←`from `∈`in `∉`not-in `¬`not `∧`and `∨`or `:=`def `!`must-not `?`unknown `+`add/need `-`remove `{a,b}`set `@`anchors file:sym:lines `IQA`=install_qa_check `TS`=transform sites `RF`=read-first `O`=oracle `ELF`=NEEDED.ELF.2 `PM`=package-mgr `F/M/S`=tiers(F front Opus5/Fable5.1, M Sonnet5, S Haiku4.5; Frev=F reads full diff pre-user-commit)

STATUS: IN-PROGRESS S0 done 2026-09-13(§11; TEST/findings/l2.md `#38 recon`); #37 LANDED (resolved FEATURES+PORTAGE_COMPRESS*+USE+SLOT in phase env)
GENESIS: merge of 3 drafts {deepseek,musespark,claude}; §0=disagreements+checkout adjudication; THIS file?? no — SRC `.plan.md` = scope authority for agents; drafts=history
REFS: backlog docs/backlog-tasks.md:74; scope-backlog.md §K:669-672; findings TEST/findings/l2.md l2-gpkg-docompress:121-132 + l2-gpkg-dostrip-splitdebug:255-260; L3 contract docs/030_L3-source-build-parity.deepseek.md §S3+G0.5

RF: AGENTS.md steps 1,4(real-exec carve-out),5,7,8; docs/agent-context.md; TEST/findings/l2.md; 037 plan §2+G1-G3-as-landed; code→
  ebuild_phases.rs(run_commands_async post-install misc-fn calls :2996-3029; run_misc_function :2889; phase_features_value :1581)
  ebuild_merge.rs(run_merge→run_commands(["install"]) :2719 = treewalk mirror)
  ebuild_package.rs(run_package :448; package_after_install :487; invoke_dyn_package :773)
  ebuild.rs:38(instprep stub note); TEST/run/l2-portuale-builder.sh(KNOWN_FINDINGS :57-71); TEST/compare/known-divergences.yaml:98-134
REAL-AUTH vendored, unmodified, already executed by portuale:
  misc-functions.sh(IQA :77; ecompress gate :147-152; scanelf/NEEDED :154-236; estrip gate :239-250; __dyn_instprep :265-308; __dyn_package :540)
  bin/estrip(has_feature/has_restriction :404-430; tool names :462-470; save_elf_debug :98-210; debugedit warn :176-181)
  bin/ecompress(die :228-229; defaults :232-247; disable note :254); bin/phase-helpers.sh:21-28(array defaults)+:164-220(docompress/dostrip helpers)
  bin/save-ebuild-env.sh:20-29(arrays dropped only on FINAL save); vartree.py:4440-4450(instprep←treewalk); cnf/make.globals:77-84,107-111
  ALL lines @ec13936; relocate-by-symbol before edit

§0 ADJUDICATIONS (topic|deepseek|musespark|claude|⇒merged|evidence)
  TS-location | IQA :147-152,:239-250 (portuale already runs post-install) | __dyn_package via invoke_dyn_package | IQA, already run every src path incl merge | ⇒ds/cl. ms factually wrong: gates ∈ IQA() :77-263 ¬ __dyn_package() :540; run_commands_async runs IQA unconditionally after `install` on emerge<atom>-merges, scheduler builds, --buildpkgonly, ebuild<file> | grep -n binpkg-do misc-functions.sh→:149,:243,:282,:290; ebuild_phases.rs:2996-3029; ebuild_merge.rs:2719; ebuild_package.rs:455
  merge-path-transforms-today? | yes once env right | neither path | yes(default-on) once env right | ⇒YES default-on post-#37; NO for -binpkg-* complement(__dyn_instprep) which real runs from treewalk on EVERY merge incl binpkg | vartree.py:4440-4450
  why-inert-today | FEATURES raw env + PORTAGE_COMPRESS unset | same + no instprep caller | same | ⇒#37-reasons-only for default path: harness exports FEATURES="buildpkg binpkg-multi-instance splitdebug xattr …" w/ neither binpkg-*; real gets both from make.globals(incremental) | TEST/layers/l2/build-portuale.sh:28; make.globals:77-84; ecompress:228
  work-shape | recon→fix env gaps→pin→delete allowlist | 3 transforms=3 slices, instprep=merge mechanism | verify default fires→pin vs oracle→decide instprep | ⇒cl shape + ms per-transform slicing: S1 docompress; S2 dostrip+splitdebug(ONE script ONE fixture, ¬2 slices); S3 RESTRICT/nostrip fixtures; S4 instprep decision | —
  instprep | file #38b unless cheap(G2.2) | implement via run_misc_function before merge_tree(G0.2) | file #38b unless harness needs -binpkg-*; user decides | ⇒explicit user decision S4, default file #38b: implementing adds phase to EVERY merge incl binpkg(L1 blast-radius); inherit real's complement gating; NEVER wire both sites unconditionally | misc-functions.sh:265-308 idempotent via .instprepped
  reimpl-in-Rust? | rejected | rejected | rejected | ⇒REJECTED: env+invocation of vendored scripts only; NEVER edit bin/* to make test pass | —
  determinism | byte-det strip/objcopy, build 2× | payload bytes change BY DESIGN; grade vs real ¬vs old portuale | over-scoped for #38 | ⇒ms/cl: #38 grades portuale-vs-real SAME fixture SAME image; determinism claims ∈L3 | —
  RESTRICT/FEATURES-matrix | 1 dostrip -x case if oracle proves | full matrix(nostrip/strip/binchecks/splitdebug×tokens) | only oracle-gradable cells: default, splitdebug, RESTRICT=strip, FEATURES=nostrip | ⇒cl: cell needs real archive built same setting; _l1-pkgcache=defaults only→2 tiny new fixtures both PMs build; rest filed | TEST/logs/_l1-pkgcache/porttest/
  env-gaps-found-here | fix via #37 builder | file into 037/S4 | fix AND test in #37 builder ¬#38 diff | ⇒cl/ms: missing whitelisted var=#37 bug; lands as #37 follow-up commit | —
  tool-absence | record presence; classify | real's warn/skip = spec; no Rust pre-gating | same | ⇒kept: debugedit absent→eqawarn once, continue(estrip:176-181) | image has dev-util/debugedit-5.3, app-misc/pax-utils-1.3.10
  PORTAGE_COMPRESS_FLAGS | export if config carries | — | export only if SET(script uses -v test) | ⇒cl: exported-empty would suppress per-compressor defaults | ecompress:232
  tiering | F recon+instprep, M rest | M throughout, F review (b)/(c) | F recon+instprep-decision, M rest | ⇒F:S0,S4; M(Frev):S2; M:S1,S3,S5 | —

§1.1 VERDICT: mostly #37 tail; do NOT write a stripper. 3 facts:
  F1 portuale ALREADY runs real code containing transforms: IQA(misc-functions.sh:77) holds ecompress gate(:149)+estrip gate(:243); run_commands_async runs `IQA install_symlink_html_docs install_hooks` after every successful `install`(ebuild_phases.rs:2996-3029), same extra_env, every src path
  F2 gates false today ONLY due #37: FEATURES in phase env = raw process env(no binpkg-dostrip/docompress; real gets both from make.globals, FEATURES incremental); PORTAGE_COMPRESS="bzip2"(make.globals:108) never exported(whitelisted ebuild_phases.rs:1469, never set)→ecompress die(:228-229). #37 builder delivers both(FEATURES via resolved_incremental; PORTAGE_COMPRESS* via other_vars ∩ real environ_whitelist)
  F3 -binpkg-* complement = separate merge-time phase: real runs instprep from dblink.treewalk()(vartree.py:4440-4450) on every merge src|bin; __dyn_instprep(:265-308) applies transforms iff token ABSENT, idempotent(.instprepped), else near-noop(chflags); portuale treewalk mirror never runs it(ebuild.rs:38)
  ⇒#38 = verify default-on fires post-#37 → pin vs real oracle archives w/ existing fixtures → +2 tiny oracle-graded fixtures RESTRICT=strip|nostrip → decide instprep explicitly → delete temp allowlists. Rust delta default-path ≈0(±env pair proven missing=#37 fix). Risk ¬implementation; risk=(a) misclassify integration failure(tool missing, vendored bin/ vs image portage 3.0.82.2 skew) as transform bug (b) double-transform if instprep wired without real complement (c) declare victory on allowlist match vs oracle
§1.2 SLICES/TIERS: S0 recon under #37 env{build fixtures, read logs, tool table, classify every non-firing branch, real instprep call order}|F|unknown-unknowns; classification decides all-after
  S1 docompress close l2-gpkg-docompress vs O|M|1 script 1 fixture; bytes match or not
  S2 dostrip+splitdebug close l2-gpkg-dostrip-splitdebug*|M(Frev)|estrip dense; tool-absence semantics; .build-id layout; CONTENTS interaction
  S3 RESTRICT=strip/FEATURES=nostrip fixtures|M|2 tiny fixtures both PMs build; oracle-graded
  S4 instprep decision(file #38b|implement)|F|new merge-path phase incl binpkg merges; user call
  S5 closeout{allowlist deletion, real-set rerun, L1 rerun, docs}|M/S|mechanical+container runs
  cheap-model-only: S1,S2,S3,S5 after F-written S0 table w/ expected path sets frozen; S0+S4 stay F
§1.3 DIFFICULTY: Rust 1-2(≈0 default; +1 misc-fn call if S4 impl); Bash 3(estrip ~700L, ecompress ~260; read to CLASSIFY ¬port); Integration/triage 4(tool presence, version skew, env provenance, QA_PRESTRIPPED noise); Test-design 3(oracle-driven; new fixtures need both PMs); OVERALL 3(low volume high observation cost; hinges entirely #37). Effort ~14-26 agent-h, 3-4 sittings post-#37S2; +4-8h if S4 implements
§1.4 ORDER: #37→#38. S0 may start once #37S2 on branch(resolved FEATURES visible in metadata/FEATURES); S1-S3 wait land. inside: S0→S1→S2→S3→S4→S5(ascending tool-dependence; later slice expectations assume earlier transform bytes)

§2 GROUND-TRUTH(@ec13936)
  GATES misc-functions.sh: :149 `[[ ${PORTAGE_COMPRESS} ]] && contains_word binpkg-docompress "${FEATURES}"`→`ecompress --queue "${PORTAGE_DOCOMPRESS[@]}" --ignore "${PORTAGE_DOCOMPRESS_SKIP[@]}" --dequeue`; :243 contains_word binpkg-dostrip→`estrip --queue "${PORTAGE_DOSTRIP[@]}" --ignore --dequeue`(___eapi_has_dostrip, EAPI7+; else --prepallstrip). complements :282/:290 ∈__dyn_instprep. default FEATURES(make.globals:77-84) has BOTH binpkg-* on; splitdebug,compressdebug,installsources OFF(L2 harness+L3 block turn splitdebug ON)
  ARRAYS: PORTAGE_DOCOMPRESS=(/usr/share/{doc,info,man}); _SKIP=(/usr/share/doc/${PF}/html); PORTAGE_DOCOMPRESS_SIZE_LIMIT=128; PORTAGE_DOSTRIP=(/) for EAPI7+(phase-helpers.sh:21-28); helpers docompress[-x]/dostrip[-x] extend(:164-220); survive src_install→IQA via $T/environment; only FINAL --exclude-init-phases save drops them(save-ebuild-env.sh:20-29). NOTHING for Rust
  ESTRIP :404-430: has_feature[compressdebug dedupdebug installsources nostrip splitdebug xattr]←FEATURES; has_restriction[binchecks dedupdebug installsources splitdebug strip]←PORTAGE_RESTRICT; RESTRICT=strip ∨ FEATURES=nostrip→banner-off, skip(unless installsources). tools debugedit,dwz,${CHOST}-{objcopy,ranlib,readelf,strip}(:462-470),scanelf(:393). debugedit absent→eqawarn once, continue w/o build-ids(:176-181). reads PORTAGE_STRIP_FLAGS(opt), STRIP_MASK(exported misc-functions.sh:244), KERNEL, CHOST, ED/D/T, SLOT
  ECOMPRESS: die w/o PORTAGE_COMPRESS(:228-229); per-compressor default flags iff _FLAGS UNSET(-v test :232-247); skips pre-compressed suffixes(:87); needs find0/___parallel(isolated-functions.sh)+compressor∈PATH. PORTAGE_COMPRESS=""=documented disable(:254)
  ORDER∈IQA: scanelf NEEDED-writer(:154-236) BEFORE estrip(:239)—bug 749624; strip ¬change DT_NEEDED. portuale own NEEDED.ELF.2 writer(needed_elf.rs)=#39
  PORTUALE-ENV-TODAY: PORTAGE_RESTRICT USE-reduced w/ EMPTY USE for non-depend phases(ebuild_phases.rs:2079-2086); real RESTRICT="!x? ( strip )" would diverge—post-#37 USE use effective set(file #37 if seen; NEVER patch estrip)
  IMAGE-TOOLS(TEST/logs/l1-20260913T013910Z/portage.installed-before.txt): debugedit-5.3, pax-utils-1.3.10(scanelf), binutils, bzip2. REAL-O: TEST/logs/_l1-pkgcache/porttest/{docs,splitdebug,setuid}/*.gpkg.tar
  FIXTURES: TEST/images/overlay/porttest/porttest/{docs,splitdebug,setuid}; README.md:32-36 expected split(dodoc -r→compressed, newdoc, doman compressed, doinfo NOT, docinto html NOT; splitdebug→.debug+.build-id for binary AND soname lib). size-witness setuid 15424(portuale) vs 14384(real)=STALE post-#37S2(S0 both 14384); README `doman compressed` ¬true this fixture(O pt.1 33B plain <SIZE_LIMIT). KNOWN_FINDINGS regexes TEST/run/l2-portuale-builder.sh:65-71; yaml known-divergences.yaml:98-134(l2-gpkg-dostrip-splitdebug{,-contents,-libptsd,-dirs})
  VERSION-SKEW: portuale runs THIS repo's bin/estrip|ecompress|misc-functions.sh; reference archives from image portage 3.0.82.2

§3 SCOPE
  IN: default-on binpkg-dostrip+binpkg-docompress firing on all src paths via existing IQA call; FEATURES=splitdebug parity(/usr/lib/debug/**, .build-id/**); RESTRICT=strip+FEATURES=nostrip pinned by 2 tiny oracle-graded fixtures; CONTENTS recording post-transform paths/md5s on src merge; brush-backend smoke; explicit instprep decision; delete temp allowlist entries
  OUT(file,¬absorb): packdebug(__generate_packdebug); installsources/dedupdebug/compressdebug beyond non-regression; xattr/selinux/chflags; NEEDED.ELF.2 field-count+gpkg metadata members(#39); reimpl estrip/ecompress∈Rust; edit vendored bin/*; deterministic-compression(L3); resolver/merge/scheduler/compare changes; Python mirror/contract CASES(real-exec-only)

§4 GATES
  G1 trigger=resolved config ONLY: no Rust-side switch, no hardcoded PORTAGE_COMPRESS=bzip2, no cfg(test) shortcut. branch ¬fire⇒bug∈#37 builder—fix+test there. owner agent
  G2 instprep: default FILE #38b∈S4 w/ container repro + scope-backlog §K entry, unless S0 shows L2/L3 harness configures -binpkg-*(¬today). if user implements: 1 `run_misc_function(..., "__dyn_instprep", ...)` at real treewalk position(after install-chain, before copy-loop, ALSO binpkg merges), same phase env, rely on real complement; L1+L2 rerun mandatory. owner USER; S0 presents call-order table
  G3 shell: prove bash(default); run porttest/splitdebug once under --shell brush; external estrip/ecompress misbehave under brush parent⇒FILE(brush-pin workflow), ¬drop brush. owner agent
  G4 expected outputs from real O(tar tf + gpkg-diff.sh --mode strict), NEVER fixture README(claim≠proof). owner agent
  G5 new fixtures built by BOTH PMs on L2 bed, isolate 1 cell each, trivial src_install(README rule), name-collision check fixtures/repo+overlay. owner agent
  G6 env gaps discovered here=#37 bugs: land as #37 follow-up commits(builder+unit test), #38 waits. owner agent
  EVIDENCE-BAR: gpkg-diff.sh --mode strict shows NO BIG.txt|usr/lib/debug|libptsd|usr/bin/pt- rows for docs|splitdebug|setuid WITH corresponding KNOWN_FINDINGS/yaml deleted FIRST; src-merge CONTENTS matches O-VDB. green run still matching allowlist row ≠ acceptance

§5 SLICES
S0(F,3-5h,no product code) pre: #37S2 branch|landed; metadata/FEATURES of portuale-built porttest archive ∋ binpkg-docompress binpkg-dostrip
  1 build porttest/{docs,splitdebug,setuid} --buildpkgonly + -b∈L2 container(TEST/run/l2-portuale-builder.sh TEST/atomlists/l1-porttest.txt); keep logs
  2 per fixture: did IQA reach ecompress|estrip(grep compress banner|strip: lines|eqawarn)? which tools did estrip resolve(name_of)? diff archive image path-set+sizes vs O(tar tf, gpkg-diff.sh)
  3 write table TEST/findings/l2.md(new `## #38 recon`; split l2-transforms.md if grows): fixture×branch→fired?|output-equal?|cause∈{portuale-env,container-tool,version-skew,real-divergence,fixed}; tool-present table; real instprep call-order w/ §2 citations for G2; expected path-sets frozen from O
  4 any portuale-env row⇒file vs #37(G6)+STOP #38 until lands; >3 systemic non-env blockers⇒stop,file,resequence w/ user
  ACCEPT: every non-firing branch=named cause+repro cmd; expected trees frozen; G2 data; no code changed
S1(M,2-4h) 1 confirm PORTAGE_COMPRESS,_EXCLUDE_SUFFIXES(and _FLAGS only if set∈config) reach misc-fn env(S0 table); confirm BIG.txt.bz2, small.txt UNcompressed(<128B), html/ skipped, doman compressed, doinfo ¬—exactly as O(G4)
  2 src merge(emerge porttest/docs): ${D} no .ecompress residue; CONTENTS md5s=compressed files; symlinks into compressed docs repaired(ecompress relink)
  3 delete KNOWN_FINDINGS l2-gpkg-docompress|BIG\.txt + yaml entry; rerun porttest track
  4 Rust e2e(tests/test_portuale.py pattern): doc-bearing fixtures ebuild w/ seeded make.conf(PORTAGE_COMPRESS=bzip2, FEATURES=binpkg-docompress), assert .bz2∈${D}; skip w/ explicit reason if bzip2 absent host
  ACCEPT: l2-gpkg-docompress closed w/ before/after evidence∈l2.md; full suite green
S2(M,Frev,4-8h) 1 estrip fires+resolves strip|objcopy|readelf|debugedit|scanelf; setuid sizes=O(15424→14384); /usr/lib/debug/usr/bin/pt-*.debug, /usr/lib/debug/usr/lib64/libptsd.so.0.0.0.debug, /usr/lib/debug/.build-id/xx/yyyy.debug+symlinks—same set as O(tar tf both, diff)
  2 src merge: CONTENTS stripped md5+debug objects; NEEDED.ELF.2 unchanged(field-count=#39—¬touch allowlist rows)
  3 brush once(G3); file if broken
  4 rescope/delete l2-gpkg-dostrip-splitdebug* yaml+KNOWN_FINDINGS rows; porttest track 0-unexplained
  5 STOP: toolchain-class failure(strip flags, QA_PRESTRIPPED handling, musl quirks)⇒file w/ repro, escalate; NEVER patch vendored script
  ACCEPT: l2-gpkg-dostrip-splitdebug closed w/ evidence; Frev=classification ¬just diff
S3(M,2-4h) 1 add porttest/restrict-strip(compiled binary, RESTRICT="strip"); both PMs build L2 bed; assert unstripped-size parity+no /usr/lib/debug. nostrip variant only if bed passes per-fixture FEATURES cheaply; else file
  2 TRAP: -binpkg-dostrip ≠ no-strip—ebuild-called dostrip|prepstrip still run(misc-functions.sh:241 note); ¬write fixture asserting no-strip for that case
  3 update overlay README table; name-collisions(G5)
  ACCEPT: 2 oracle-graded cells pinned; no other matrix claims
S4(F,1h-file|4-8h-impl) 1 container repro: FEATURES="-binpkg-dostrip -binpkg-docompress", emerge porttest/docs porttest/setuid under BOTH PMs; real strips|compresses at merge(vartree.py:4440), portuale ¬. same w/ binpkg merge of unstripped archive
  2 decide w/ user(G2). FILE: #38b∈backlog-tasks.md+scope-backlog §K w/ repro+note ebuild.rs:38 stub comment. IMPLEMENT: 1 run_misc_function at real treewalk position EVERY merge(src+bin), env=phase env; rerun L1(w/ portage upgrade)+L2; fixture=repro above green
  EXIT: implemented+fixtured OR #38b filed w/ repro. NO 3rd state
S5(M/S,2-3h+container) 1 porttest track clean fresh run; real set(L2_REBUILD=1 L2_MODE=payload-tolerant L2_BUILD_MODE=deep … atomlists/l1-merge.txt) as far as goes; record stop-point; classify new findings(env→#37, metadata→#39, else new)
  2 L1 porttest rerun(archive consumer unaffected); if S4 implemented instprep L1 mandatory + any diff=finding
  3 docs: what-this-proves.md 1 appended para(live cmd: build porttest/docs, tar tf→BIG.txt.bz2; porttest/splitdebug→.debug tree); scope-backlog §K bullet closed; backlog-tasks.md:74 DONE(+#38b if filed); fixture README rows; docs/030 G0.5 pointer
  4 full verification(cargo fmt --check, clippy 0-warn, cargo test --release, python3 -m pytest tests -q) then L2 porttest, L1

§6 FIXTURES|ORACLES|TESTS
  porttest/docs @ TEST/images/overlay/porttest/porttest/docs/ ∷ BIG.txt(>128B) vs small.txt, html/, man, info
  porttest/splitdebug @ same ∷ binary+soname lib→.debug+.build-id
  porttest/setuid @ same ∷ stripped-size witness
  porttest/restrict-strip(new,S3) @ same ∷ RESTRICT=strip cell
  REAL-O @ TEST/logs/_l1-pkgcache/porttest/{docs,splitdebug,setuid}/*.gpkg.tar ∷ THE expected path-set|sizes
  COMPARE @ TEST/compare/{gpkg-diff.sh --mode strict, gpkg-structure.sh, diff.py --layer l2} ∷ archive-vs-archive, root|VDB
  RUNNER @ TEST/run/l2-portuale-builder.sh+KNOWN_FINDINGS ∷ delete rows as they pass
  RUST-UNIT/E2E @ rust/portuale/src/{ebuild_phases,ebuild_merge}.rs, tests/test_portuale.py ∷ env wiring, .bz2∈${D}; NO Python mirror
  PY-CONTRACT @ tests/ ∷ untouched, green
  VERIFY-STEP8: cargo fmt --check, cargo clippy --release --all-targets, cargo test --release, python3 -m pytest tests -q, then L2 porttest; ALSO L1(TEST/run/l1-merge-from-binpkg.sh w/ portage upgrade) because packaging|merge path touched

§7 RISKS/TRAPS/STOP
  1 VERSION-SKEW vendored bin/ vs image portage 3.0.82.2—classify every diff vs both before calling bug
  2 DOUBLE-TRANSFORM never wire instprep without real's `!contains_word binpkg-*` complement; diff adding both sites unconditionally=wrong on sight
  3 ALLOWLIST-THEATRE delete KNOWN_FINDINGS/yaml rows FIRST then run; grade portuale-vs-real, NEVER portuale-vs-old-portuale
  4 TOOL-ABSENCE=BEHAVIOUR real warns+continues w/o debugedit; no Rust pre-check, no test-skip hiding it
  5 PORTAGE_COMPRESS="" disables—must flow as empty; _FLAGS must NOT export when unset
  6 QA_PRESTRIPPED/eqawarn = log-noise unless payload differs; compare payload first
  7 PAYLOAD-BYTES CHANGE BY DESIGN—expect KNOWN_FINDINGS/yaml churn same commit; reviewers check vs real
  8 SCOPE-PULL NEEDED.ELF.2 fields, SIZE/IUSE members, packdebug, installsources→#39|file
  9 #37-SLIPPAGE S0 may run on branch; S1-S3 NEVER stub env "to make progress"
  10 STOP: >3 systemic non-env blockers∈S0⇒stop,file,re-sequence; toolchain-class estrip failure∈S2⇒file,escalate

§8 REVIEW-CHECKLIST(attach each slice): 1 transform-family per slice?(no S1+S2 blob) | no transform switch outside resolved config? no hardcoded PORTAGE_COMPRESS? | complement gating mirrored exactly if instprep wired? | expectations from O-archive, cited by path? | allowlist rows deleted BEFORE green run? | tool-absence path=script, no Rust pre-gating? | l2.md evidence appended w/ exact commands? | Rust unit/e2e added; no vendored bin/* edited; no Python mirror?

§9 DOD: [ ]S0 table+tool table+instprep call-order cited; expected trees frozen from O | [ ]l2-gpkg-docompress closed(S1); l2-gpkg-dostrip-splitdebug* closed(S2)—each w/ before/after evidence | [ ]RESTRICT=strip cell pinned by both-PM fixture(S3) | [ ]instprep: implemented+fixtured OR #38b filed w/ repro(S4) | [ ]no transform reimplemented∈Rust; no vendored bin/* edited | [ ]L2 porttest 0-unexplained; L1 unchanged; full verification green | [ ]docs updated(S5); no dead allowlist entries

§10 DELEGATION-BRIEF(for subagents): state #37S2 landed(commit hash); pass SRC as scope authority + G1-G6 answers + S0 table + O paths(§6) + rules: never edit vendored bin/*; never add transform switch outside resolved config; derive expectations from O archives; label claims provisional until container confirms. RETURN: diff, exact commands, gpkg-diff.sh output, archive listings, exact allowlist rows deleted. no container⇒implement/unit-test only + label **unverified end-to-end**

§11 FINDINGS-LOG(append as S0-S5 run: cmd|expected|actual|root-cause|fix-ref|backlog-id)
  S0 2026-09-13 ∷ in TEST/logs/l2-20260913T182502Z(post-#37) ∷ both gates fire all 3 fixtures
    docs: =O already(#37S2 deleted l2-gpkg-docompress rows)⇒S1=evidence+e2e only
    splitdebug: path-set=O; build-ids+.debug bytes ¬(+8B) ← WORKDIR /var/tmp/portage/portage/<cat>/<pf>/work; CLI defaults PORTAGE_TMPDIR=/var/tmp/portage vs real make.globals:35 /var/tmp ⇒ portuale-env ⇒ G6 #37 follow-up(default+resolved config) then S2 regrade; corrects #37S2 "compiled bytes" classification
    setuid: 3 identical bins 1 build-id; link target=estrip ___parallel race(O pt-setuid, real-L2 pt-sticky)⇒real-divergence, keep 1 narrowed row
    tools: all present except dwz(dedupdebug only); instprep call-order table recorded; harness ¬-binpkg-*⇒S4 default stands
