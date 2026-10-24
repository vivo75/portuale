# Residui OPEN #514–#555 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Chiudere o triage-are tutti i residui OPEN del port a Portage 3.0.83 lasciati dal piano `LLM/02.442-port-portage-3.0.83.opus.md` (#514, #516–#555).

**Architecture:** Un batch di slice indipendenti per area (gpkg, ebuild-merge, circular, depclean, atom/reducer, binhost, user-patches, scheduler, virtuals, test-hygiene), ognuna guidata dall'oracolo host 3.0.83 già catturato in `LLM/evidence/`, con pin hermetic in pmtest + unit Rust qui. Nessuna modifica condivisa tra slice salvo bless corpus.

**Tech Stack:** Rust workspace edition 2024 (`portuale`, `portage-repo`, `portage-profile`, `portage-dep`, `portage-use-reduce`), pmtest contract suite (`CASES` + `test_*_pinned`), differential bed L0/L1, host `emerge` 3.0.83.

**Spec:** `LLM/backlog-tasks.md` voci #514, #516–#555 OPEN + `LLM/02.442-port-portage-3.0.83.opus.md` S0–S13 + `LLM/portage-quirks.md` Q4–Q9 + oracoli in `LLM/evidence/2026-10-23-{500,505,506,507,508,509,510,511,512}/`, `LLM/evidence/2026-10-24-513/`.

## Global Constraints

- Ogni slice che cambia output `emerge` prende l'expected da Portage reale (bed / host / `lib/portage/tests/resolver/`), citato nel docstring del test.
- Determinismo: niente timing/secret/hash-order nei pin (`Dependency resolution took` tagliato, `Change USE:` multi-flag come set salvo #517f).
- Coppia di commit portuale+pmtest sullo stesso branch name; bless corpus solo in pmtest con `PORTUALE_CORPUS_BLESS=1` dopo review del drift.
- Gate prima di DONE: `cargo fmt --check`, `cargo clippy --release --all-targets` 0 warning, `cargo test --release`, `scripts/pytest-parallel.sh` in `../pmtest`.
- Merge-path gate per ogni tocco di merge/ebuild-phase: reinstall glibc + bash rc 0.

## Review Focus

- Contenimento gpkg più stretto di Portage accetta/rifiuta il bordo opposto — Task 1 pinna entrambi i lati.
- Preambolo/timing `Dependency resolution took` nascosto da `compare.sh`/`strip_pretend_preamble` — Task 3 lo riespone.
- `Maybe you meant` per singolo atom mancante vs multi-atom — Task 6 controlla prima il singolo.
- `USER_PATCHES` writer senza digest riproducibile — Task 7 confronta byte con host `--buildpkgonly`.
- `tmp_path` troncato a 30 char maschera collisioni — Task 10 rinomina + self-test di vacuità.

---

### Task 1: #514 — gpkg hostile residue (bed cell + 2 bordi)

**Files:**
- Modify: `crates/portuale/src/binpkg/extract.rs`, `crates/portuale/src/binpkg/inner.rs`
- Test: Rust `binpkg::` craft tests + pmtest L5 fault cell (pmtest side) + `LLM/evidence/2026-10-23-500-501/` nuove righe
- Code notes: `LLM/code-notes/portuale/binpkg/extract.md`, `inner.md`

**Interfaces:**
- Consumes: oracle `LLM/evidence/2026-10-23-500-501/` (23 shapes, `build-hostile.py`).
- Produces: `containment_root` semantics (consumata da Task 6 Step 2 e Task 7 Step 1 per PATH/containment).

- [ ] **Step 1: Scrivi il failing test Rust per (b) e (c)**

```rust
// (b) symlink che sale esattamente a temp-root Portage: accettato lì, rifiutato qui
// (c) hardlink-di-symlink con membro sotto: Portage estrae, GNU tar tronca
#[test]
fn hostile_residue_b_and_c_match_portage_3_0_83_r1() { /* rc + Danger line + stato dir */ }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --release -p portuale binpkg::`
Expected: FAIL sulle 2 nuove righe.

- [ ] **Step 3: Implement `scan_image_tar` root = parent di `image/` in `crates/portuale/src/binpkg/extract.rs`**

Portage temp-root semantics + messaggio `Danger:` sul path Portage; gestisci (c) come Portage o documenta divergenza GNU-tar.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --release -p portuale binpkg::` + `python3 -m pytest pytests-contract-suite/test_portuale.py -q -k hostile_gpkg` (da `../pmtest`)
Expected: PASS.

- [ ] **Step 5: Aggiungi bed L5 fault cell per shape hostile (control Portage, candidate portuale, compare rc) — pmtest side**
- [ ] **Step 6: Merge-path gate (tocco extract su ogni binpkg merge): reinstall glibc + bash rc 0, findings reviewed**
- [ ] **Step 7: Commit pair portuale+pmtest**

### Task 2: #516 — `ebuild merge` standalone muore in 60noop-testsuites senza `IUSE=test`

**Files:**
- Modify: `crates/portuale/src/ebuild_merge/*` (QA check path), `crates/portuale/src/remote/merge_script.rs` (`REGEN_LOCALE_VARS`/`LOCPATH`)
- Test: overlay one-ebuild di `LLM/evidence/2026-10-23-505/`, pmtest e2e standalone merge

**Interfaces:**
- Consumes: vendored `bin/` S6 (già 3.0.83).
- Produces: `LOCPATH` forwarding per merge-path gate.

- [ ] **Step 1: Repro standalone merge dell'ebuild senza `IUSE="test"` vs host**

Run: `portuale ebuild merge` dell'overlay `LLM/evidence/2026-10-23-505/` su entrambi i manager.
Expected: portuale FAIL QA (`USE Flag 'test' not in IUSE`), host PASS.

- [ ] **Step 2: Implement handling implicito `test` come Portage per run standalone + inoltra `LOCPATH` via `REGEN_LOCALE_VARS` in `merge_script.rs`**
- [ ] **Step 3: Run gate**

Run: `cargo test --release -p portuale -- ebuild_merge` (sotto-modulo esatto, non tutto `ebuild`) + merge gate glibc+bash
Expected: PASS, 0 unexplained.

- [ ] **Step 4: Commit**

### Task 3: #517–#520 — residui report circolari

**Files:**
- Modify: `crates/portage-repo/src/resolve/cycle_report.rs`, `crates/portuale/src/pretend/display/circular.rs`, `circular_json.rs`, `crates/portuale/src/pretend/mod.rs`, CLI `choices=` usage rendering
- Test: pmtest `test_506_*`, `test_507_*` + CASES bed `l0-fixture-oracle-506.txt`

**Interfaces:**
- Consumes: captures `LLM/evidence/2026-10-23-506/`, `LLM/evidence/2026-10-23-507/captures/` (shapes 33,34,45,46,52,53,54).
- Produces: preambolo condiviso (consumato da Task 4 Step 1 per `Using system located in…` helper).

- [ ] **Step 1: #517 — pinna i sotto-punti (a)–(d) come failing byte-pin; (e) RDEPEND/binary/fallback resta cut registrato (nessun pin byte, solo pin di assenza); (f) `Change USE:` come set-compare (Q4), mai byte-order**

(a) start ring `sbrB`; (b) remainder flat `--backtrack=0`/`ccd5a`; (c) blank-line REQUIRED_USE; (d) ordine warning `MAX_USE_FLAGS`; (e) RDEPEND/binary/fallback come wontfix; (f) `Change USE:` hash-order vs sort.

- [ ] **Step 2: #518 — pinna `--onlydeps` rotation `[a,b]` prima di `[b,a]` + `shortest_cycle` da `usecycleb`**

Oracle: `LLM/evidence/2026-10-23-507/captures/54-onlydeps.*`.

- [ ] **Step 3: #519 — stampa preambolo `These are the packages…`/`Dependency resolution took` prima di `[nomerge]` rows e blocco unmet-requirements; review corpus drift poi bless SOLO nel commit pmtest di questo Task (cfr. Task 10 Step 2: lì solo re-run clean)**
- [ ] **Step 4: #520 — `invalid choice` stampa 78-line usage block Portage 3.0.83 per ogni `choices=` S4 (Tier 3, generico)**

Oracle: `captures/53-none.*`, rc 2 entrambi.

- [ ] **Step 5: Run**

Run: `cargo test --release -p portage-repo cycle_report` + `cargo test --release -p portuale circular` + pmtest `-k "506 or 507"`
Expected: PASS.

- [ ] **Step 6: Commit pair**

### Task 4: #521–#524 — depclean

**Files:**
- Modify: `crates/portuale/src/pretend/removal/depclean.rs`, `crates/portage-repo` `depclean_cleanlist`/`CycleSuggestion`, `pretend/post_merge.rs`
- Test: hermetic ROOT vdb hand-written (come #508), pmtest `test_508_*` estesi

**Interfaces:**
- Consumes: captures `LLM/evidence/2026-10-23-508/captures/03-pair-both.portage.out` + preambolo condiviso da Task 3.
- Produces: `Using system located in…` helper riusabile per `--unmerge/--clean -p`.

- [ ] **Step 1: #521 — `Using system located in ROOT tree <root>/` + blank line per ROOT≠`/` anche nel manual path (verifica `--unmerge -p`, `--clean -p` non-`/` ROOT prima)**
- [ ] **Step 2: #522 — `Calculating dependencies ... done!` su stdout depclean (o pin esplicito dell'assenza se deciso no)**
- [ ] **Step 3: #523 — fixture linkmap con `NEEDED.ELF.2` consumer → verifica se 2nd pass dopo `--depclean-lib-check` duplica `show_parents`+cycle notices (oggi unverified)**

Run: host `emerge -p --depclean --verbose` vs portuale sulla fixture linkmap.

- [ ] **Step 4: #524 — rendi l'oracolo usabile con fixture pmtest (profile parsabile da host: fix `updates/2Q-2024` + `crossrepo-parent`) oppure registra oracle config nel bed; risolvi vdb runtime-deps irrisolte per `--depclean ARG`**
- [ ] **Step 5: Run Rust `depclean` + pmtest depclean pins**

Run: `cargo test --release -p portage-repo depclean`
Expected: PASS.

- [ ] **Step 6: Commit pair**

### Task 5: #525–#532 — S10b atom/reducer parity

Done 2026-10-24 on branch `residui-514-555` (portuale + pmtest pair; ledger `.superpowers/sdd/plan-residui-514-555/progress.md`).

**Files:**
- Modify: `LLM/02.442-port-portage-3.0.83.opus.md` (testo S10b), `portage-dep` battery `testdata/atoms-3.0.83.tsv`, `portage-use-reduce` tests, `LLM/tools/atom_oracle_table.py`, `LLM/evidence/2026-10-23-510/build-3.0.82.2.sh`, pmtest `3rdparty/repos.toml:24`
- Test: `tests_grammar.rs`, `portage-use-reduce` reducer rows

- [x] **Step 1: #525 — registra errata S10b senza riscrivere il piano chiuso (3.0.82.2 constructor≈reducer sui 3 atomi; cita `diffpaths-3.0.82.2.txt`); in-place edit di `LLM/02.442-port-portage-3.0.83.opus.md` solo con sign-off owner**
- [x] **Step 2: #526 — decidi `use_reduce` string-path (`cat/pkg[a(+),-a]` senza `token_class`) — mimic in `mrg` o wontfix con quirk Q8**

Oracle: `LLM/evidence/2026-10-23-510/host-3.0.83.txt`.

- [x] **Step 3: #527 — sposta pin reducer-level in `portage-use-reduce` accanto a `use_reduce_flat_validated`**

Oracle: `probe.py` (`reduce-Atom` rows): `cat/pkg[a(+),-a]` rejected, i due buoni accepted.

- [x] **Step 4: #528 — aggiungi i 3 atomi probe alla battery 83-row (estendi `probe_atoms.py`, sposta count assert, rigenera)**

Oracle: `LLM/evidence/2026-10-11-341-oracle/probe_atoms.py`.

- [x] **Step 5: #529 — rebuild parser C 3.0.82.2 via meson `_parser` target, non gcc manuale**
- [x] **Step 6: #530 — fissa wording versione `3.0.83` vs `3.0.83-r1` (VERSION vs ebuild rev)**

Oracle: `LLM/evidence/2026-10-23-510/emerge-version.txt`.

- [x] **Step 7: #531 — pinna/decidi testo errore reducer 3.0.83 (`invalid use dep token`) vs wrapper 3.0.82.2**
- [x] **Step 8: #532 — pin battery punta a 3.0.83 (pmtest side, owner pmtest agent): CASES/fixture/command come gli altri step; se resta owner esterno, sposta con pointer invece di step vuoto**
- [x] **Step 9: Run**

Run: `cargo test --release -p portage-dep` + `cargo test --release -p portage-use-reduce`
Expected: PASS.

### Task 6: #535–#539 — binhost

Done 2026-10-26 on branch `residui-514-555` (ledger `.superpowers/sdd/plan-residui-514-555/progress.md`).

**Files:**
- Modify: `crates/portage-profile/src/binrepo.rs`, `crates/portuale/src/emerge_getbinpkg.rs`, `crates/portage-repo` `populate_local_pkgdir`, `crates/portuale/src/binpkg/local_index.rs`
- Test: scratch-repo shapes `LLM/evidence/2026-10-23-511/` + nuovo shape overlap

**Interfaces:**
- Consumes: captures `13-sig-multi-missing.*`, layout overlap `s511-e/f` history + `containment_root` semantics da Task 1.

- [x] **Step 1: Check singolo atom mancante: se `Maybe you meant` differisce, fix quello; altrimenti #535 wontfix (Portage first-in-argv deterministico su PYTHONHASHSEED 1–6, valore basso)**

Run: `emerge -p --color=n app-misc/zznope1 app-misc/zznope2` su entrambi + capture singolo-atom `emerge -p --color=n app-misc/zznope1` su entrambi.
Expected post-fix: singolo-atom identico a Portage su entrambi i lati; multi-atom resta wontfix #535 salvo decisione owner.

- [x] **Step 2: #536 overlap — porta `cpv_exists` skip pre-`gpkg_only` in ordine descending-priority; probe dedicato overlap con policy `gpkg_only` diverse**
- [x] **Step 3: #537 — `gpkg_only` locale `$PKGDIR` sotto `FEATURES=binpkg-request-signature` (ignora xpak con warning `Local XPAK packages are ignored…`)**
- [x] **Step 4: #538 — `basename_index` key `(dir, basename)` non solo basename (bug 978985)**

Test: due same-basename in diverse subdir `$PKGDIR` con uguali SIZE/mtime.

- [x] **Step 5: #539 — eredita `[DEFAULT]` di `binrepos.conf` config-root (configparser inheritance) per `verify-signature`/`frozen`**

Test: `[DEFAULT] verify-signature = false` + sezione keyless xpak-serving.

- [x] **Step 6: Run**

Run: `cargo test --release -p portage-profile binrepo` + pmtest getbinpkg pins
Expected: 12/13→13/13 + nuovi shape.

- [x] **Step 7: Commit pair**

### Task 7: #540–#542 — user patches writer/VDB/extraction

Done 2026-10-26 on branch `residui-514-555` (ledger `.superpowers/sdd/plan-residui-514-555/progress.md`).

**Files:**
- Modify: `crates/portuale/src/helpers/gpkg.rs` (metadata assembly + `Packages` inject), `ebuild_merge/from_binpkg.rs` (VDB copy), merge extraction rendering
- Test: real `--buildpkgonly` oracle tree `LLM/evidence/2026-10-23-509/`; riusa `containment_root` semantics da Task 1 per PATH/containment.

- [x] **Step 1: #540 writer — includi `build-info/USER_PATCHES` + `user_patch.digests` in `metadata.tar` e key `USER_PATCHES` in `Packages`; digest byte-identico a host**

Oracle: `build-info-*.txt`, `gpkg-metadata-patch-members.txt`.

- [x] **Step 2: #541 VDB — copia entrambi i membri su binary merge (`USER_PATCHES`, `user_patch.digests`); pin con merge `-K` di binario patchato**

Oracle: `captures/14-merge-match.*` + VDB listing S10.

- [x] **Step 3: #542 extraction — tabella `This binary package was built with user patches applied:` (troncata a terminal width) + warning mismatch + warning digest malformato; pin con merge `14-merge-match` e `16-merge-mismatch`**
- [x] **Step 4: Run Rust binpkg + pmtest user-patch pins**

Run: `cargo test --release -p portuale binpkg::`
Expected: PASS.

- [x] **Step 5: Commit pair (merge-path gate: reinstall glibc + bash rc 0, findings reviewed; il merge del binario patchato è solo un caso del gate)**

### Task 8: #543–#546 — scheduler

**Files:**
- Modify: `crates/portuale/src/emerge_build/merge_wait.rs`, `run_build_scheduler`, `run_merge_loop`, `crates/portuale/src/emerge_build/buildpkgonly.rs`, `merge_binpkg` pretend hook
- Test: oracle `LLM/evidence/2026-10-23-512/` (`10-pretend` tree, mixed source+binpkg `-j`)

- [ ] **Step 1: #543 Tier 3 — cgroup `=== Resource usage…` — o porta `CgroupManager` reale o wontfix documentato (no counter fabbricati)**

Oracle: `captures/14-cgroup.portage.*` (root) e `14-cgroup-degraded.portage.*` (unprivileged silent rc 0) + `test_observability.py`.

- [ ] **Step 2: #544 — `pkg_pretend` binari nel pass parallelo up-front con replay merge-list-order (oggi inline in `merge_binpkg`); probe mixed `-j` con marker su entrambi i lati**
- [ ] **Step 3: #545 Tier 3 low-value — serial `-j1` pretend up-front unbuffered in-order (oggi dentro ogni build)**

Oracle: `-j1` run del tree `10-pretend` su entrambi.

- [ ] **Step 4: #546 Tier 3 — conferma se `_run_pkg_pretend` copre `--buildpkgonly`; se sì, threading pass in `run_buildpkgonly`**

Oracle: `--buildpkgonly -j` run del tree `10-pretend` su entrambi.

- [ ] **Step 5: Run**

Run: `cargo test --release -p portuale merge_wait`
Expected: PASS.

- [ ] **Step 6: Merge-path gate (tocco merge-loop/buildpkgonly): reinstall glibc + bash rc 0, findings reviewed**
- [ ] **Step 7: Commit pair**

### Task 9: #547–#554 — virtuals e dintorni

**Files:**
- Modify: `crates/portage-profile/src/virtuals.rs`, `crates/portage-profile/src/resolve/package_masks.rs`, `crates/portuale/src/pretend/early_actions.rs`, `pretend::run` action dispatch, news/unmerge/version paths
- Test: `LLM/evidence/2026-10-24-513/` 29 shapes + nuovi shape per azione

- [ ] **Step 1: #547 — verifica se `package.keywords` stampa frame Python; se sì, mimica come #513**

Oracle: host `emerge -p` con `/etc/portage/package.keywords` non-empty in hermetic root.

- [ ] **Step 2: #548 — owner decision prefisso warning (`/usr/lib/python3.14/...` host vs `portage/...` hermetic)**

Oracle: `captures/02-profile-nonempty.portage.err`.

- [ ] **Step 3: #549 — `virtuals` illeggibile mode 000 → `PermissionDenied` abort, non empty (run non-root)**
- [ ] **Step 4: #550 — cattura `--regen/--config/--deselect/--resume` con `/etc/portage/virtuals` (oggi non runnati)**
- [ ] **Step 5: #551 — decidi doppia stampa invalid profile atom (mimic 2nd read o keep single, owner Q3)**

Oracle: `captures/16-profile-virt-badmask.portage.err`, `17-user-virt-badmask`.

- [ ] **Step 6: #552 — `emerge --version` → `Portage 3.0.83 (…)` rc 0, non usage error 2**

Oracle: `captures/12-version.portage.out`.

- [ ] **Step 7: #553 — `-C` caution + `Couldn't find…to unmerge`; `--rage-clean` senza atomi stampa niente**

Oracle: `captures/24-*`, `27-*`.

- [ ] **Step 8: #554 — `--check-news` righe `!!! Invalid news item:` + `line 7:` detail**

Oracle: `captures/25-*`.

- [ ] **Step 9: Run**

Run: `cargo test --release -p portage-profile virtuals` + `cargo test --release -p portuale user_virtuals` + pmtest `test_513_*`
Expected: PASS.

- [ ] **Step 10: Commit pair**

### Task 10: #555 + closeout — test hygiene e verifica finale

**Files:**
- Modify: pmtest `test_depclean_package_provided_pinned_output` rename (primi 30 char unici)
- Test: run singolo del test rinominato

- [ ] **Step 1: Rinomina test così i primi 30 char di `tmp_path` non collidono; run isolato PASS**

Run (da `../pmtest`): `python3 -m pytest pytests-contract-suite/test_depclean*.py -q -k <nuovo_nome>`
Expected: PASS da solo e in suite.

- [ ] **Step 2: Full gate qui + `scripts/pytest-parallel.sh` in pmtest; Task 10 NON blessa mai — solo re-run clean (il bless di Task 3 vive già nel commit pmtest di Task 3)**
- [ ] **Step 3: Aggiorna `LLM/scope-backlog.md` voci chiuse, `LLM/what-this-proves.md` paragrafo per slice con esempio live runnable live-verified, `LLM/backlog-tasks.md` DONE, code-notes + `code_notes_index.py`, `LLM/portage-quirks.md` se nuovi quirk; run `doc_status_check.py` + `cut_registry_check.py` (02.442 §0 regola 22)**

## Self-Review

- Copertura: ogni OPEN #514,516–555 ha un Task; #533–534 non esistono; #535 wontfix-condizionale esplicito.
- Proporzione: step danno solo signature/file/oracle/test, non corpi.
- Tipi: `CycleSuggestion`, `UserPatchSet`, `verify_signature`, `gpkg_only`, `basename_index`, `merge_wait` nomi già in tree.
