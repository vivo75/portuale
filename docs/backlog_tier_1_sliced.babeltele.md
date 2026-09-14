# T1-SLICED ∷ BabelTele mirror — LLM→LLM consult-only
SRC=docs/backlog_tier_1_sliced.opus.md @ main:5a1f329 2026-09-14; AUTH=SRC (divergence→SRC wins, never cite THIS as evidence)
PROJ: readability-relaxed semantic projection (BabelTele, arXiv:2606.19857); glyph-light ⇒ savings from dropped prose; ALL ids/paths/line-refs byte-exact; line-refs drift→relocate by symbol
LEGEND: `∷`is `;`fence `|`field `→`leads/causes `⇒`decision `←`from `∈`in `∉`not-in `¬`not `∧`and `∨`or `:=`def `!`must-not `?`unverified `+`add `-`remove `@`anchors file:sym:line `F/M/S`=tiers(F Opus5/Fable5.1, M Sonnet5, S Haiku4.5; Frev=F reads full diff pre-user-commit) `U`=user-owned action `BR`=3rdparty/brush checkout `PF`=rust/portage-fetch/src/lib.rs `FR`=rust/portuale/src/fetch.rs `FP`=3rdparty/portage/lib/portage/package/ebuild/fetch.py `EP`=rust/portuale/src/ebuild_phases.rs

STATUS: proposed 2026-09-14 ∷ PROGRESS: track F COMMITTED ∈main {F1a F1b F2 F3(1)(2) F4 F5 DONE; D3=deterministic suffix landed 9f0e477; D4=shuffle deliberate cut; F4 free-space 79ce808; oracle real fetch(listonly=1): literals tried LAST-LISTED FIRST} ∷ TRACK-B B0-B5 DONE {pin=vivo75/brush@b9524ad5 (25bffd54+5 fixes); branches bc99e6c1/df830c59/962051c9/dfbca97c/2073877d pushed; verified cargo test --release ∧ pytest ∧ compat 0-unexpected ∧ eclass sweep 0-fail incl synthetics ∧ #38 G3 smoke non-empty} ∷ B6=U; B7 upstream-blocked
SCOPE: docs/backlog-tasks.md Tier 1 OPEN only
RF: AGENTS.md(step4 real-exec carve-out, step8 verify); docs/agent-context.md; docs/brush-pin.md; docs/brush-pr/README.md; PF module doc

§1 TIER1-STATE (audit 2026-09-13 re-spot-checked 2026-09-14: lto/codegen-units∈rust/Cargo.toml, expand_package_env_files, slot_conflict_caret_idx, colorize_marked_spans, find_hard_cycles, elog mail_summary ⇒ hold)
  DONE {1,2,3,4,7,8,9,10,11,12,13,16} | NOT-A-GAP {15} | OPEN {5 brush PRs, 6 brush re-pin+src_compile no-op, 14 fetch cuts}
  TRACKS: B:={#5,#6} (same work) ∷ F:={#14} ∷ independent; ¬resolver ⇒ ¬Python mirror ¬CASES

§2 FINDINGS (reproduced 2026-09-14; change the plan)
  2.1 BRUSH-BUG-4 ∷ `declare -f` emits quoted heredoc TERMINATOR with quotes (`'EOF'`; bash `EOF`)
    repro: inherit-free copy of TEST/images/overlay/porttest/porttest/splitdebug (sed -e '/^inherit/d' -e 's/$(tc-getCC)/${CC:-gcc}/'); `ebuild --shell bash … compile`→work∋pt-splitdebug,libptsd.so.0.0.0 ∷ `--shell brush`→work empty rc0 + `…/temp/environment: unterminated here document sequence; tag(s) ['EOF']`
    minimal: `f(){ cat > a.c <<-'EOF'⏎int x;⏎EOF⏎}; declare -f f` under BR/target/debug/brush → terminator `'EOF'`
    mechanism: saved env unparseable→next phase source fails→ebuild src_compile lost→`default` runs (saved env shows `src_compile () { default }`)
    OWNER-OF-BUG: OUR staged fix 02 `fix/declare-f-heredoc-serialization` 3d2bde47 ∷ brush-parser/src/ast.rs `block.push_str(&here_doc.here_end.value)` ~:1733@pin = raw quoted word; need quote-removed delimiter ∷ 211-eclass/1843-fn sweep had ¬quoted tag ⇒ fix BEFORE PR 02
  2.2 SOURCE-PARSE-ERROR-SILENT ∷ bash: `source bad.sh`→rc2, `|| die` fires (bin/ebuild.sh:580 `source "${T}"/environment || die "error sourcing environment"`) ∷ brush standalone: parse error aborts whole -c script rc2, `||` never runs ∷ portuale embedded: phase CONTINUES rc0 — EP:run_one_phase_brush maps only Err of run_string/source_script, ignores nonzero ExecutionResult ⇒ 2.1 became silent wrong output
  2.3 EMPTY-$BASH ∷ bin/phase-functions.sh:__filter_readonly_variables runs `env -i -- "${BASH}" -c 'printf …'` ∷ smoke log `env: '': No such file or directory` ⇒ embedded brush_core::Shell BASH empty(?confirm B3.1; standalone brush sets BASH) → bash_vars empty → BASHOPTS/EUID/PPID/SHELLOPTS/UID saved → re-source `declare: cannot mutate readonly variable` noise
  2.4 GENTOO_MIRRORS-FALLBACK-BROKEN ∷ PF doc claim "flat = what GENTOO_MIRRORS use" FALSE
    `curl -s http://distfiles.gentoo.org/distfiles/layout.conf` → `[structure] 0=filename-hash BLAKE2B 8`
    `…/distfiles/which-2.23.tar.gz`→404(portuale flat) ∷ `…/distfiles/80/which-2.23.tar.gz`→200(real)
    host /var/cache/distfiles/.mirror-cache.json (real-written) ∷ 4 mirrors all filename-hash BLAKE2B 8
    ⇒ public-mirror fallback ∧ mirror://gentoo ∧ dead-upstream fetch can NEVER succeed on stock GENTOO_MIRRORS ⇒ most important functional gap in T1
  2.5 DUP-ATTEMPTS ∷ FR:assemble_candidates lists thirdparty expansions twice (inline + primaryuri tail) — real same shape BUT FP `tried_locations` skips repeats; portuale ¬set ⇒ each unreachable 3rd-party mirror tried 2×
  2.6 SLASH-GENTOO_MIRRORS ∷ real `/`-rooted GENTOO_MIRRORS→fsmirrors(copyfile FP:1503-1513) ∷ portuale PF:gentoo_mirror_fallback ¬filters `/` → wget gets bare path as URL; FR filters `/` only for custommirrors["local"]

§3 TRACK-B brush (#5+#6)
  GOAL: brush builds compiled ebuild =bash; broken saved env fails loud; 4(+1) upstream fixes correct+rebased; PRs openable
  OUT: flipping `--shell` default→brush (separate owner decision post-merge)
  INV: fix brush bugs IN brush (BR fix/* branches) ¬portuale workaround ∷ only allowed tree workaround = "brush strategy #2" w/ user sign-off ∷ `--shell bash` byte-unchanged every slice ∷ PR open / push vivo75/brush / comment #1276 = U
  B0(S,0.5h,docs) stale pin records→`vivo75/brush@67c301a7` thin fork
    rust/portuale/Cargo.toml comment above brush-core ∷ "fork is gone"→thin fork
    3rdparty/repos.toml [brush] comment ∷ "No fork: plain upstream main"→thin fork
    docs/brush-pr/README.md ∷ "pinned to 5af3f6c1 (commit 8184c11)"→67c301a7 + per-branch bases
    docs/brush-pin.md "Root-caused + fixed 2026-09-05" ∷ "All three now in pin (5af3f6c1)"→ref "Current pin"
    tests/test_portuale.py:test_ebuild_install_does_not_deadlock_on_a_large_eclass_scope docstring ∷ "fixed in pinned fork"→staged fix 03/#1276
    ACCEPT: grep 5af3f6c1|"fork is gone"|"No fork" → history only; ¬code
  B1(M,Frev,2-4h) fix bug 4
    1 build 3d2bde47 alone AND pin 67c301a7; run §2.1 minimal; expect BOTH broken; pin-only⇒merge regression⇒STOP re-plan w/ user
    2 ast.rs: deferred terminator = quote-removed delimiter (bash: any quoting 'EOF' "EOF" \EOF E"O"F removed for terminator; command line keeps quoted form); amend∨follow-up commit ⇒D2
    3 +compat cases brush-parser/tests/cases/compat/builtins/declare.yaml: <<'EOF', <<"EOF", <<\EOF, <<-'EOF' tab body; assert declare -f =bash ∧ `declare -f | source` idempotent
    4 extend eclass sweep w/ synthetic fn per quoting form; rerun 211-eclass sweep
    5 cherry-pick→vivo75/brush main; re-pin portuale (brush-pin.md checklist); refresh docs/brush-pr/patches/02-*.patch + 02-*.md
    ACCEPT: §2.1 prints EOF; compat 0 new fail; `ebuild --shell brush` inherit-free splitdebug copy → pt-splitdebug+libptsd.so.0.0.0 (e2e pin=B4)
  B2(F,3-6h) failed saved-env source must fail phase
    1 characterise brush parse-error-in-source: top-level | in-function | lhs of `||` vs bash(rc2, `||` runs) ⇒ upstream divergence write-up brush-pr/0N-*.md style
    2 fix site: (a) upstream brush `source`→nonzero status ¬abort enclosing (faithful; real ebuild.sh:580 `|| die` fires) ∷ (b) portuale EP:run_one_phase_brush treat nonzero ExecutionResult of run_string(setup) ∧ source_script(ebuild.sh) as phase failure before invoke_function("__ebuild_main") ⇒ REC both: (a) as fix-05 branch, (b) now (guard stays valid); prove (b) ¬fires on healthy run
    3 regression test: `ebuild --shell brush <fixture> unpack` → corrupt ${T}/environment (unterminated heredoc) → `compile` ⇒ BOTH shells nonzero w/ real "error sourcing environment" die; bash = control (should already pass)
    ACCEPT: test green both shells; ¬brush phase test behaviour change on healthy env
  B3(M,2-3h) embedded shell $BASH
    1 confirm: fixture pkg_setup einfo ${BASH} under brush → expect empty
    2 choose BASH: (i) real bash path (lists bash specials, brush emulates; matches real "hygienic instance of bash" intent) ∨ (ii) portuale binary if brush-subprocess entry exists ⇒ REC (i); BASH is shell-set ¬env ⇒ set inside run_one_phase_brush setup ¬phase_setup_script env export; if brush forbids setting BASH ⇒ +upstream issue (embedded Shell no $BASH)
    3 test: after `ebuild --shell brush <phasepkg> install` saved ${T}/environment ¬declare lines for BASHOPTS EUID PPID SHELLOPTS UID ∧ stderr ¬`env: ''` ¬`cannot mutate readonly variable`
    ACCEPT: _l2-brush-smoke noise gone; bash unchanged
  B4(S/M,1-2h, after B1∧B2∧B3) e2e pin
    +fixture ∈fixtures/repo shaped like inherit-free splitdebug (src_compile `<<-'EOF'` heredoc + ${CC:-gcc}; skip w/ explicit reason if no compiler) ∷ extend test_ebuild_shell_bash_and_brush_produce_the_same_real_result ∨ sibling: same image/ file set both shells ∷ `git add` fixture BEFORE any `git clean -fdq fixtures/`
    container: rerun #38 G3 smoke `emerge --shell brush --buildpkgonly porttest/splitdebug` → append TEST/findings/l2.md "#38 S2" + brush-pin.md "What is *not* tracked here" mark resolved
    ACCEPT: test green; container image non-empty; brush-pin.md 2026-09-13 bullet closed w/ evidence
  B5(M,2-4h, after B1∧B2a) rebase fix branches + re-pin (#6 recurring)
    fact: upstream reubeno/brush main=25bffd54 @2026-09-14 (git ls-remote); pin upstream base 812336dd; every fix/* branch base a250b84e
    1 BR `git fetch upstream`; rebase each fix branch {01 tokenizer, 02 declare-f(+B1), 03 deadlock, 05 source-status if B2a} onto upstream/main; resolve
    2 per-branch isolation: cargo test -p brush-parser -p brush-core; clippy; compat 0 new fail
    3 rebuild vivo75/brush main = upstream + rebased fixes; re-pin portuale (Cargo.toml both crates, Cargo.lock, 3rdparty/repos.toml, brush-pin.md "Current pin"); checklist: fmt, clippy 0w, `cargo test --release -p portuale` incl deadlock guard + B2/B4, pytest brush tests
    4 re-export docs/brush-pr/patches/*.patch; update brush-pr/README.md (bases, ids, verification numbers)
    ACCEPT: each fix = 1 commit on current upstream/main, passes brush suite alone; portuale green on new pin; force-push origin = U go-ahead
  B6(U; agent prep ~1h) open upstream PRs ∷ blocked B1-B5 ∧ D1
    agent: per fix `gh pr create -R reubeno/brush --head vivo75:<branch>` + body ← brush-pr/0N-*.md (root cause, minimal repro, bash comparison, tests) ∷ user reviews+runs ∷ #1276 (OPEN, head fix/pipeline-function-stage-deadlock2) update∨close-superseded per D1
    ACCEPT: PR URLs recorded brush-pr/README.md + brush-pin.md; backlog #5→"PRs open, awaiting upstream"
  B7(S,1h, upstream-blocked) all fixes ∈reubeno/brush main ⇒ re-pin upstream direct; remove thin-fork wording at 5 B0 sites; close #5; file `--shell` default flip as separate owner decision (¬this plan); #6 stays recurring

§4 TRACK-F fetch (#14)
  GOAL: candidate list+retry = real FP:fetch on every DETERMINISTIC axis; public mirror fallback works vs real Gentoo mirrors
  INV: real-exec only ¬Python mirror ¬CASES ∷ downloads stay portage_fetch::download_via_wget (wget subprocess) ¬in-process HTTP ∷ EXPECTED ORDERS ← real portage oracle ¬reading FP: real `emerge -pf <atom>` (listonly prints every candidate URI in try-order) ∨ direct `portage.package.ebuild.fetch.fetch(..., listonly=1)` on container image; thirdpartymirrors entries w/ ONE url each (shuffle-proof) ∷ unit tests ¬network: FR test helpers serve_once/closed_port + pre-seeded .mirror-cache.json
  F1a(M,2-3h) layout math ∈rust/portage-fetch
    port FlatLayout/FilenameHashLayout/ContentHashLayout (get_path, verify_args) + MirrorLayoutConfig ([structure] 0=,1=… parse, validate_structure, get_best_supported_layout w/ flat fallback) @FP:465-630 ∷ deps blake2+sha2 present; hash names per real checksum_str; content-hash needs Manifest digest (DistfileDigests has it)
    oracle real python: `FilenameHashLayout('BLAKE2B','8').get_path('which-2.23.tar.gz')`→`80/which-2.23.tar.gz` ∷ pin several names × cutoffs {8, 8:8, 16} + invalid structure (cutoff %4≠0, unknown algo)→flat
  F1b(F,4-6h, after F1a) negotiation+cache ∈FR ← FP:async_mirror_url :731-785
    cache ${DISTDIR}/.mirror-cache.json := {mirror_url: [ts, [[layout args…]…]]}; ts ≥ now-86400 ⇒ use ∷ else fetch `<mirror>/distfiles/layout.conf`→${DISTDIR}/.layout.conf.<host> via same wget, no mirrors ∷ `/`-rooted mirror reads <dir>/layout.conf ∷ success⇒atomic cache write; any failure⇒flat, ¬write
    cache SHARED w/ real portage same DISTDIR ⇒ JSON format-compatible BOTH directions: portuale reads real-written (host file) ∧ real reads portuale-written (container)
    urlquote path only for ftp/http/https
    applies to local_mirrors+public_mirrors (assemble_candidates steps 1-2) ¬mirror:// expansions ¬literals ∷ LAZY like real functools.partial: file fetched from 1st candidate ⇒ ¬layout.conf download
    now = parameter (TTL deterministic in tests); only call site reads clock
    DOC: correct false flat claim in PF module doc + FR doc comments, cite §2.4 evidence
    ACCEPT: unit {cache hit; stale→local-server layout.conf→hashed path fetched; unreachable layout.conf→flat; cache round-trip} ∷ container: literal SRC_URI unreachable → portuale fetches which-2.23.tar.gz from …/distfiles/80/… ∧ real `emerge -pf` lists same URL
  F2(S,1h, independent, may go FIRST) tried_locations
    FR:fetch_src_uri candidate loop: skip candidate already tried for this file (= FP `if loc in tried_locations: continue`) ∷ keep assemble_candidates list shape incl dups (real builds same) ∷ dedup attempts only
    test: mirror:// URI w/ single thirdparty root = closed_port ⇒ exactly 1 attempt, error list names it once
  F3(M,2-4h, after F2) checksum-failure semantics ← FP:896-934, :1975-2000, _checksum_failure_temp_file:293
    (1) count failures; at 2nd (checksum_failure_primaryuri=2) append reversed primaryuris to remaining list ("switch to primaryuri mode")
    (2) stop after PORTAGE_FETCH_CHECKSUM_TRY_MIRRORS failures (default 5; invalid→warn+default)
    (3) RENAME bad file `<file>._checksum_failure_.<random>` ∈DISTDIR + `Refetching... File renamed to '…'` ∷ portuale today deletes + tries every candidate
    port (1)(2) deterministic ∷ (3)⇒D3 DECIDED deterministic suffix
    ACCEPT: local server bad bytes on N candidates ⇒ exact real attempt count+order; primaryuri switch pinned by fixture where literal reached only post-switch
  F4(M,2-3h, after F1a; uses F1b for dir layout.conf) fsmirrors
    before any remote candidate, file absent ∧ space: try fsmirrors in real order = custommirrors["local"] `/` entries then `/`-rooted GENTOO_MIRRORS; path via that dir's layout.conf; plain copy; print real `Local mirror has file: <f>`; first hit stops; copied file still digest-verified
    FIX §2.6: `/`-rooted GENTOO_MIRRORS leave gentoo_mirror_fallback, never reach wget
    ACCEPT: unit temp-dir mirror under flat ∧ filename-hash; `/`-rooted GENTOO_MIRRORS ⇒ ¬wget candidate
  F5(M,Frev,3-5h, after F2) multi-URI-per-file grouping ← FP:1099-1192
    real: filedict OrderedDict[filename→uris]; local+public mirror lists added ONCE on first sight of filename; each SRC_URI entry for that filename appends its mirror:// expansions in order; primaryuris per filename REVERSED (uris.reverse()) + thirdparty appended; merged head (primaryuri) ∨ tail
    portuale loops per entry ⇒ 2 entries/1 filename re-try shared mirror lists + wrong relative literal order both modes
    1 per-filename grouping ∈fetch_src_uri (∨ assemble_candidates takes group)
    2 expected orders ← oracle real `emerge -pf` on 2-URI fixture ± RESTRICT=primaryuri ! derive from reverse() comment (misreadable)
    3 check `A` (EP extra_env.push(("A", …)) from fetch_src_uri filenames) lists multiply-sourced file ONCE like real; twice⇒fix here; cross-check #45 (vdb environment A w/ distfile 3×) same∨separate root cause?
    ACCEPT: oracle order pinned 2-URI+3-URI both modes; A deduped
  F6(S,0.5h,docs, anytime) shuffle → Deliberate cuts (backlog-tasks.md) + scope-backlog.md Part 3 w/ reason (load-balancing only; every candidate digest-verified; try-order ¬observable in result) ⇒ D4 DECIDED cut

§5 ORDER/DEPS/EFFORT
  B: B0 → {B1, B2, B3} (mutually indep) → B4 → B5 (needs B1 ∧ B2a) → B6(U) → B7(upstream)
  F: F2→F3 ∷ F1a→F1b→F4 ∷ F2→F5 ∷ F6 anytime
  tracks independent ⇒ parallel sessions/worktrees OK
  slice|tier|est|container: B0 S 0.5h no | B1 M(Frev) 2-4h no | B2 F 3-6h no | B3 M 2-3h no | B4 S/M 1-2h YES(G3 smoke) | B5 M 2-4h no | B6 U — no | B7 S 1h no | F1a M 2-3h no(host python oracle) | F1b F 4-6h YES(real mirror + cache cross-read) | F2 S 1h no | F3 M 2-4h optional | F4 M 2-3h no | F5 M(Frev) 3-5h YES(emerge -pf oracle) | F6 S 0.5h no
  TOTAL ≈27-45 agent-h + upstream wait B6/B7 ∷ #14 ≠ "one sitting" ⇒ D5
  FIRST SESSION: B0 + F2 (both S, ~zero risk) then B1 (unblocks smoke, gates PRs)

§6 DECISIONS (owner)
  D1 #1276 ∷ (a) force-push fix 03 onto its head ∨ (b) close superseded + open fix 03 fresh ⇒ REC (b): different branch+base, fresh write-up cleaner
  D2 B1 on pushed branch ∷ amend 3d2bde47 ∨ follow-up commit ⇒ REC amend before PR (1 commit/fix stated structure; needs force-push origin=U)
  D3 checksum rename F3(3) ∷ random suffix ∨ deterministic suffix ∨ keep delete(cut) ⇒ DECIDED 2026-09-14 owner: deterministic suffix (keeps evidence ∈DISTDIR, ¬nondeterminism; documented divergence)
  D4 shuffle F6 ∷ cut ∨ seeded opt-in ⇒ DECIDED 2026-09-14 owner: confirmed, third-party mirror shuffle stays deliberate cut
  D5 #14 scope ∷ all F1-F5 ∈T1 ∨ F3+F5→T2 ⇒ REC keep F1 F2 F4 ∈T1 (real bugs), move F3 F5 → T2

§7 HOUSEKEEPING (first commit touching backlog-tasks.md)
  #14 rewrite around §2.4: functional bug ¬cosmetic cut; slices F1-F6
  #6 rewrite: src_compile no-op root cause = §2.1-2.3, owned B1-B4
  #12 cites standalone_phase_env_layers_matching_package_env_build_vars @EP:4761 as production ∷ actually #[test] fn (~:5670) ⇒ point at production matcher
  outside T1 same file: Tier 5 lists #40 TWICE (DONE 2026-09-13 + stale open line) ⇒ delete stale

§8 VERIFY (every slice)
  AGENTS.md step8: cargo fmt --check; cargo clippy --release --all-targets 0w; cargo test --release; python3 -m pytest tests -q ∷ compare failing test NAMES vs clean-main baseline ¬counts (contract-suite pollution) ∷ pub sig change ∈portage-* ⇒ build/test at workspace ROOT
  B: + brush-compat-tests ∈BR
  F: F1b ∨ F4 ∨ F5 (changes what is downloaded) ⇒ + L2 porttest track (L2/L3 builds fetch distfiles)
