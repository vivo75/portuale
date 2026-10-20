# Porting Portage 3.0.82.2 → 3.0.83 in Portuale

> Documento di lavoro per un altro LLM esecutore.
> Origine analisi: `/home/vivo/repo/PORTUALE/Portage/` (copia di `portuale/3rdparty/portage`, branch `master`).
> Non modificare `3rdparty/portage` a mano: il porting in Portuale è meccanico + adattamenti.

## 0. Contesto e perimetro

- Tag upstream: `portage-3.0.82.2` = `bc5b19cf17b83e2f5f6069cea8d060d7932f1f95` → `portage-3.0.83` = `e409b5a188b723d49cf44c1eedd755be1b7be76`.
- Ampiezza: 205 commit, 124 file, `8676 insertions(+), 1989 deletions(-)` (`git diff --stat portage-3.0.82.2..portage-3.0.83`).
- Fonte primaria: `NEWS` del tag `3.0.83` (righe 1-110 per la 3.0.83, resto storico 3.0.82.1/3.0.82.2).
- Branch cleanroom esistente: `cleanroom-portage-3.0.82.2` = `73091925f` sopra `3.0.82.2`, contiene solo 13 file `cleanroom/*.md` (1929 righe, `git diff --stat portage-3.0.82.2..cleanroom-portage-3.0.82.2`). Nessun `cleanroom-portage-3.0.83` esistente al momento dell'analisi (2026-10-20).
- Obiettivo cleanroom: creare `cleanroom-portage-3.0.83` derivato da `portage-3.0.83` riapplicando i 13 `cleanroom/*.md`, poi aggiornarli per le nuove feature sotto.

Comandi di verifica (sola lettura, da `/home/vivo/repo/PORTUALE/Portage/`):

```bash
git rev-parse --verify refs/tags/portage-3.0.82.2
git rev-parse --verify refs/tags/portage-3.0.83
git log --oneline portage-3.0.82.2..portage-3.0.83 | wc -l
git diff --name-status portage-3.0.82.2..portage-3.0.83 | grep -v "^M"
```

## 1. Opzioni a linea di comando (aggiunte, nessuna rimozione finale)

Riferimenti su tag `3.0.83` (file:linea ottenuta con `git show portage-3.0.83:<file> | grep -n`).

### 1.1 `--binpkg-respect-user-patches [y|n]`
- `lib/_emerge/main.py:152` (tabella `insert_optional_args`), `lib/_emerge/main.py:400-408` (definizione `parse_opts`), `lib/_emerge/main.py:869-873` (normalizzazione y/n).
- Commit: `40edbe917 emerge: add --binpkg-respect-user-patches command line option`, collegati `526dd94dc`, `2b9c9b688`.
- Doc: `man/emerge.1:477-483`. Default attivo anche con `--usepkgonly`: ignora binpkg che revertano user-patch o built con user-patch diverse.
- Porting: aggiungere opzione, normalizzazione, plumbing in `depgraph.py` / `FakeVartree` / `Binpkg.py` (vedi §4).

### 1.2 `--circular-deps-report [text|json]`
- `lib/_emerge/main.py:157`, `lib/_emerge/main.py:429-434`.
- Commit: `5fcafeaa0 emerge: add --circular-deps-report=json` + serie resolver `4e1545f8a/a952df887/599c29020`.
- Doc: `man/emerge.1:534-541`.
- Porting: aggiungere opzione con choices `("True","text","json")`; output JSON consumato da tool automatici.

### 1.3 `--merge-wait-scope <deep|system|toolchain|none>`
- `lib/_emerge/main.py:533-551`.
- Commit: `3b1fc4238 emerge: add --merge-wait-scope to limit merge-wait barriers`.
- Doc: `man/emerge.1:773-799`, richiamo in `--jobs`/`--implicit-system-deps` e `man/make.conf.5` voci `merge-wait`/`parallel-install`.
- Default `deep` = comportamento precedente (@system + dipendenze runtime transitive).

### 1.4 Solo ritocchi help
- `--binpkg-changed-deps` / `--binpkg-respect-use` riscritti in `lib/_emerge/main.py:393-399` (stesso set commit). `--binpkg-respect-use` spostato ma non rimosso; verificare che in Portuale non resti la vecchia stringa multilinea con tab.
- `man/emerge.1`: `--getbinpkg` ora cita `binrepos.conf` prima di `PORTAGE_BINHOST` (commit `74bbbec66`); `--getbinpkgonly` implica `--usepkgonly` + `--binpkg-respect-use=n` (commit `233848cf8`); `--usepkgonly` implica `--binpkg-respect-use=n` (commit `e34de37be`); `--getbinpkg-{include,exclude}` supporto sperimentale in `binrepos.conf` (commit `891a3ed6b`).

### 1.5 Da NON portare
- `--cycle-break` e tutto il "circular dependency solving" automatico: revertato in `2b0a68a12 Revert circular dependency solving` (revert di 10 commit, 1540 righe tolte, bug 175808/407351/515630/647824/888177). In `3.0.83` finale non esiste. Se Portuale contiene tracce intermedie, rimuoverle.

## 2. File di configurazione (nessun file aggiunto/rimosso)

- `git diff --name-only portage-3.0.82.2..portage-3.0.83 -- cnf/` = solo `cnf/make.conf.example`, `cnf/make.globals`.
- `cnf/make.globals:63-64`, `cnf/make.conf.example:157-160`, commit `b91ee949a`: User-Agent `Portage (Gentoo, https://www.gentoo.org)` → `Portage (Gentoo, gentoo.org)` (bug 983844). Riportare identico.
- Nuovi settaggi documentati (nessun nuovo file in `etc/`):
  - `FEATURES=merge-use-vdb` — `man/make.conf.5:627-636`, commit `c854e0908`. Default disabilitato. Usa hash+mtime dal VDB per saltare scritture identiche; disabilitare se si riparano file corrotti.
  - `FEATURES=userfetch` per binpkg citato in `e4ef338a4` poi interamente revertato in `ef326d1b1` (vedi §7). Non portare.
  - `PORTAGE_CIRCULAR_MAX_USE_FLAGS` (default 10) — `man/make.conf.5:1194-1200`, `man/emerge.1:1579-1587`, commit `599c29020`.
  - `PORTAGE_FORK_DEP_CALC` — `man/emerge.1:1599-1611`, commit `d6b630b46`. Default fork attivo; `0` = calcola in-process. Solo `--pretend`/`--resume` non forkkano mai.
  - `verify-signature` default `true` chiarito — `man/portage.5:699`, commit `d4e2062f3`; nota `cnf/binrepos.conf` shipped con `true` in `[DEFAULT]`.
  - `PORTAGE_EBUILD_EXIT_FD`, `PORTAGE_IPC_ALIVE_FD`, `USERLAND`, `XARGS`, `LOCPATH` in whitelist/blacklist — `lib/portage/package/ebuild/_config/special_env_vars.py:55,58,156,164,205,207,243`, commit `5dc9b5572` + `c0d436235` + `f7bfe34de`.
  - `man/make.conf.5`: `keepwork`/`keeptemp` warning esteso (commit `7293dae07`, `82d6dd454`); `parallel-install`/`merge-wait` richiamano `--merge-wait-scope`.
  - `man/portage.5:1510-1514`: digest user-patch salvati nei binpkg locali (`emerge --buildpkg`, `quickpkg`).

## 3. Cache (meccanismi, nessun file di cache aggiunto/rimosso nel repo)

### 3.1 Nome stabile — breaking per percorsi esistenti
- `lib/portage/cache/fs_template.py:79-90`, commit `910720a5a cache/anydbm: give databases a stable file name`.
- Prima: `f"{tail}-{abs(label.__hash__()):X}"` (hash Python random per processo). Dopo: `hashlib.md5(label.encode(...)).hexdigest()[:16].upper()`.
- Effetto: tutti i percorsi cache cambiano in modo deterministico. Portuale deve invalidare/ricreare o migrare; non mantenere il vecchio schema.

### 3.2 Creazione atomica
- `lib/portage/cache/anydbm.py:49-89` `_create()/_create_at()` via tmp + `os.link`, commit `76b9f8e5f`; mode factored in `d9b567c23` (`_open_mode()`).
- Porting: riportare `errno/glob/secrets` import e logica `FileExistsError`/`EPERM/EOPNOTSUPP/ENOTS` fallback.

### 3.3 Chiusura esplicita
- `lib/portage/cache/anydbm.py:135-145` `close()`, `lib/portage/cache/sqlite.py:324-334`, `lib/portage/cache/template.py:195-203`, commit `6552b213c`, `cfbf4ed1d`. Test `ba7ed6546 tests: check that close_caches() closes the auxdb`.
- `sqlite.py`: `commit()` non apre più connessione solo per commit; `close()` chiude solo se stesso pid.

## 4. Cambiamenti non visibili / solver

File nuovi da aggiungere tali e quali (linee su `3.0.83`):
- `lib/_emerge/_depgraph_fork.py:1-345` (345 righe), commit `d6b630b46`. Chiamato da `lib/_emerge/actions.py:78-79,177-234`.
- `lib/_emerge/_run_in_child.py:1-256` (256 righe), commit `48f1d2b89`.
- `lib/portage/package/ebuild/_config/UserPatches.py:1-127` (127 righe), commit `40edbe917`; fix crash `7964f58e2` per patch fuori `cat/pkg`.
- `lib/portage/tests/...` 22 file `A` (vedi `git diff --name-status`): sono solo test, ma i file `test_binpackage_patches.py:1-232`, `test_circular_dependency_display.py:1-534`, `test_depgraph_fork.py:1-168`, `test_merge_wait_scope.py:1-114`, `test_gpkg_hostile.py:1-486` documentano il comportamento atteso — usarli come oracle.

Dettaglio per file modificati (commit principali):
- `lib/_emerge/depgraph.py` (485 righe): `a7c346aa4` non mutare `ignored_binaries`; `298e08c88` report solo binari con cat/pkg in merge; `4e6b92a71` FakeVartree assorbe preload; `526dd94dc` skip binpkg con user-patch locali; `3980ca8a9` split world favorites.
- `lib/_emerge/resolver/circular_dependency.py` (328 righe): `4e1545f8a` nomina USE per edge; `ab7a4d4fc` punta a `FEATURES=test`; `a952df887` masked alternatives; `599c29020` give-up oltre `PORTAGE_CIRCULAR_MAX_USE_FLAGS`; `3b6976a5c` always-show; `e0689630f` colorize; `f9d65f4c5` refactor lookup.
- `lib/portage/util/digraph.py:387-443` `strongly_connected_components()` Tarjan iterativo, commit `af2a57741`; `96c320967` depclean nomina ciclo intero (bug 346351); `3d677b7b2` scheduler non attende post-merge deps in ciclo (bug 452172).
- `lib/_emerge/Scheduler.py` (715 righe): `1c8b75d73/3ea2010e1` pkg_pretend parallelo (bug 579526); `0876923be` live-deps; `8ac550470/bfa615f06` SIGINT merge-wait (bug 961044); `cb4e9e1d3/1f44396ec` fetch pkg_pretend; `147c305ba/0ef099992` sandbox log / tmpdir.
- Perf/interning: `lib/portage/dep/__init__.py` 415 righe (`9f91ad4094`, `9d109000e`, `c181095ba`, `65ffb368b`, `b675df829`, `b7cfa5270` QEMU filter); `src/dep_atom.h:1-9` + `src/meson.build:9` (`09fb80ec4` Py_NewRef py3.9, `26b5453a2` meson C-errors, `9e6ef5280` test import); `22449ce9b` IPv6 fix.
- `bin/ebuild-ipc.py` riscritto stdlib-only (commit `d06798b1c`, `1bad86d92`, `b7052a33d`, `050a653a6`, `efae859af`, `8264f0162`): niente `import portage`, exit-status su pipe, alive-fd. Cancellato symlink `bin/ebuild-ipc → ebuild-pyhelper` in `afdc1ddb5` (1 riga). Aggiornare `phase-helpers.sh:58`, `isolated-functions.sh:20`, `meson.build` di conseguenza (`6178a11cb`, `ed88f2ba6`).
- `lib/portage/gpkg.py:679-810`: `_check_member/_check_symlink_path/_check_hardlink`, `_check_metadata_files`, `os.rename` invece di `shutil.move` (commit `5d0458b4b/e52f24f03/4bc42673b/993d098bd`, bug 982208).
- `lib/portage/dbapi/bintree.py:124` (commit `b6071d4d1` skip index senza build-id, `78607215b/966e93bba/7e2dcd0bd/1578cd55b/b83ea973d` gpkg-only/verify, `50a54fd87` basename_index); `lib/_emerge/Binpkg.py:375-431` report patch; `lib/_emerge/BinpkgVerifier.py:20` rifiuta remote senza digest (`8af2c46ec`); `lib/_emerge/BinpkgFetcher.py:44` (`d4370a42b`, `f838393df`, `211d8a0f8`).
- `lib/portage/package/ebuild/doebuild.py`: `_xargs_command()` + `XARGS` in env (`c0d436235`), `get_emerge_tmpdir()` estratto (`147c305ba`), `USERLAND` passthrough (`5dc9b5572`), `spawn`/`_fetch_subprocess` forkserver check (`a20e27af/7a6f5cfa1`, bug 973043), rimozione parametro deprecato `d66562333`.
- Deprecation cleanup (~25 commit `c29a72de1…d5fe4b2ba`, `abbdde992/a8ddfffdc/98c6ed58d/...`): rimossi `mergelist`, `reversed`, `virts_p/virtuals`, `async`, `prevent_collision`, `fixdbentries` (`656f5f518`), ecc. Verificare che Portuale non chiami più queste API.

## 5. Bug corretti (riferimenti)

Solo trailer `Bug: https://bugs.gentoo.org/N` nel range `3.0.82.2..3.0.83` (`git log --pretty="%H %s%n%b" | grep -oE "bugs.gentoo.org/[0-9]+"`):
`142473, 175808×3, 240320×4, 280478, 310613, 335620, 346351, 377907, 407351×2, 416871, 452172, 463964, 463976, 468052, 515630×2, 549906×3, 579526×4, 582624, 604566, 607922, 647824×2, 668164, 703348, 722270, 766459, 877549, 888177×4, 917047×4, 929010, 930179×5, 945384×2, 961044×2, 968870, 969628, 970606×2, 971256, 973043×2, 976616×3, 978685, 978985, 980835×3, 981223, 981272×4, 981298, 981369×2, 981521, 981694, 982208×6, 982328, 982512×2, 982753×2, 982999, 983036, 983372, 983844`.

I più rilevanti per Portuale (già citati sopra con file):
- 983844 User-Agent; 961044 SIGINT; 978985 Packages index; 970606 xpak/gpkg; 280478 --info passwd; 945384/982208 firma+gpkg; 981694 qemu-user; 978685 QEMU_*; 973043 multiprocessing; 240320 interning-mem; 579526 pkg_pretend; 549906 fork-calc; 917047 user-patches; 346351/452172 cicli.

## 6. Visualizzazione

- `man/emerge.1:89-91` bold per set (commit `8a4b5baa8`).
- `man/emerge.1:1310-1315` `b` soft vs `B` hard blocker (commit `ad28f81d0`).
- `lib/portage/output.py:346-359` `renderPath()`, `633-646` `ebinfo()` (commit `3071bc985`); `lib/_emerge/Binpkg.py:375-431` tabella patch+digest con `PKG_BINARY_MERGE`.
- `lib/_emerge/JobStatusDisplay.py:92-99,160-167` `displayMessage(raw=)` + fix non-tty (commit `a6fca880a` collegato).
- Scheduler cgroup: `55beac58c` distingue build/install, `a6fca880a` summary in background solo con `--verbose`; `man/emerge.1:1184-1188` `--verbose` esteso.
- `emerge --info`: `ae78e95a8/fce6ff1f9/0ef099992/c4760f80e` consolida `Repositories` + filtra `sync-uri`/passwd (bug 280478).

## 7. Bug noti introdotti / cosa non portare

- `ef326d1b1 Revert FEATURES=userfetch for binpkgs for now` (revert di `e4ef338a4/08ec804d7/b83ea973d/0ff120f80`, bug 930179/982328 permessi). In `3.0.83` la feature non c'è. Non portare i 4 commit revertati.
- `2b0a68a12 Revert circular dependency solving` (revert di 10 commit, vedi §1.5). Non portare `--cycle-break` né `pending_rebuilds`.
- Nessuna sezione "known issues" in NEWS; i due revert sono il segnale di regressione evitata. Da testare in Portuale: permessi `userfetch`+binpkg e cicli con `--jobs`.

## 8. Piano per `cleanroom-portage-3.0.83`

1. `git checkout -b cleanroom-portage-3.0.83 portage-3.0.83` (base `e409b5a`).
2. Riportare i 13 file da `cleanroom-portage-3.0.82.2` tali e quali come primo commit:
   ```bash
   git checkout cleanroom-portage-3.0.82.2 -- cleanroom/
   git commit -m "cleanroom: re-apply 3.0.82.2 spec onto 3.0.83"
   ```
   Atteso: zero conflitti (solo `A cleanroom/*` vs `D bin/ebuild-ipc` upstream, nessun overlap).
3. Aggiornare le spec che descrivono comportamento cambiato (secondo commit o serie):
   - `03-dependency-resolution.md`: `--circular-deps-report`, `strongly_connected_components`, `PORTAGE_CIRCULAR_MAX_USE_FLAGS`, fork-calc, pkg_pretend parallelo, skip binpkg con user-patch.
   - `05-task-scheduler.md`: `--merge-wait-scope`, cgroup log, SIGINT merge-wait.
   - `06-fetch-phase.md` / `07-build-phase.md`: `ebuild-ipc` stdlib-only, `PORTAGE_IPC_ALIVE_FD`, `PORTAGE_EBUILD_EXIT_FD`, `XARGS`/`USERLAND`.
   - `10-portage-lib-support.md`: cache stabile+atomica, `UserPatches.py`, `gpkg` hardening, deprecazioni rimosse.
   - `02-actions-and-config.md`: nuove CLI, `merge-use-vdb`, User-Agent, `verify-signature` default.
   - `11-end-to-end-flows.md`: `--info` consolidato, report binpkg patch.
4. Verifica: `git diff --stat portage-3.0.83..cleanroom-portage-3.0.83` deve mostrare solo `cleanroom/*` (+ eventuale `TEST-NOTES` se serve).

## 9. Checklist per l'esecutore Portuale

- [ ] Tutte le 3 nuove CLI parsate e documentate; nessun riferimento residuo a `--cycle-break`.
- [ ] `FETCHCOMMAND`/`RESUMECOMMAND` aggiornati; `merge-use-vdb` dietro flag.
- [ ] Cache: nuovo `gen_label` md5, creazione atomica, `close()`; test `close_caches` passa.
- [ ] `_depgraph_fork` + `_run_in_child` integrati dietro `PORTAGE_FORK_DEP_CALC`; `--pretend`/`--resume` restano in-process.
- [ ] `UserPatches` + `--binpkg-respect-user-patches` + report `ebinfo` funzionanti.
- [ ] `ebuild-ipc` senza `import portage`; symlink rimosso; `phase-helpers`/`isolated-functions` usano binario diretto.
- [ ] `gpkg` rifiuta symlink-escape e metadata non-regolari (test `test_gpkg_hostile` come oracle).
- [ ] Display: bold set, b/B, `renderPath`, cgroup verbose.
- [ ] Nessuna chiamata a API deprecate rimosse (vedi §4 ultimo punto).
- [ ] `cleanroom-portage-3.0.83` creato come sopra e spec aggiornate.
