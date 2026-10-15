# Batch 2026-10-15 — piano consolidato per il proprietario

Documento da leggere per primo al ritorno. Riassume quattro piani (15 voci di backlog), le loro review, le domande aperte e i comandi per partire. Fonti: `LLM/02.355-reinstall-unmerge-parity.opus.md`, `LLM/02.359-default-solver-update-parity.opus.md`, `LLM/04.367-pubgrub-mutants.opus.md`, `LLM/02.346-ebuild-config-options-parity.opus.md` e i rispettivi `.review.md`. Tutti e quattro i piani terminano con una sezione `## Review applied` (per 02.359 l'ho verificato prima di leggerlo: era già presente, l'attesa non è servita).

Stato dei repo al momento della scrittura: portuale e pmtest sul branch `plan/batch-2026-10-15`, entrambi uguali a `main` (portuale `9370a7ca`, pmtest `f8c4bff`). Nessun file tracciato è modificato.

---

## 1. TL;DR

1. Sono pianificate 15 voci in quattro piani: **02.355** (#355 #356 #357 #358, unmerge/reinstall, *merge path*), **02.359** (#359 #360 #361 #362 #369, resolver di default), **04.367** (#367 #370, `--solver=pubgrub` e mutanti), **02.346** (#346 #347 #349 #351, config, opzioni CLI, `ebuild`, output). #352 resta fuori: decidi tu (§5).
2. Ogni piano è stato rivisto da un agente indipendente. Le review hanno trovato in totale **16 difetti (D), 30 lacune (G) e 14 deviazioni di processo (P)**: 02.355 4D/9G/4P, 02.359 5D/8G/4P, 04.367 3D/7G/3P, 02.346 4D/6G/3P. **Tutte risultano applicate** (sezione `## Review applied` in ogni piano).
3. Il difetto più serio, corretto, era in 02.355: la S4 originale poteva **cancellare `/bin`, `/lib64` con `emerge -C`** su un sistema merged-usr. R3 è stato spostato in S5, nello stesso commit di R2/R4/R4b, e la cella C1 (“`/bin` deve sopravvivere”) è un test failing-first da S4 in poi.
4. Altri difetti corretti: in 02.359 la formula `too_deep` era sbagliata di uno; in 02.346 l'ordine di lettura di `package.keywords` era invertito e la premessa sull'auto-clean era falsa (`AUTOCLEAN="no"` è il default).
5. Diverse voci del backlog erano sbagliate o incomplete: #361 è una regola sulla *presenza* dell'opzione `--usepkg`, #356 ha 4 regole mancanti in più (R5–R8), alcune affermazioni di #349/#351 sono false, e alcuni pin pmtest codificano comportamento non-Portage (§3).
6. I numeri di backlog sono assegnati a blocchi senza collisioni: 371–373 a 02.346, 374–379 a 04.367, 380–389 a 02.355, 390–399 a 02.359 (§4).
7. **Si può far girare senza sorveglianza:** tutti gli S0 (oracolo, sola lettura); 04.367 S0–S5; 02.359 S0–S4; 02.355 S0–S5, che vanno fermati su qualsiasi riga hard/unexplained del gate; 02.346 S0–S5, S7 (solo la metà non-pretend), S8, S9, più S6 fino alla fine del codice.
8. **Serve la tua presenza per:** 02.359 S5 (#369, coda di disgiunzioni differita); la revisione dei pin mossi da 02.346 S6; 02.346 S7 lato `--pretend`, S10 (listing merge/unmerge) e S11 (auto-clean); qualsiasi bless del corpus senza revisione del drift; commit e push.
9. Non c'è nessuna concessione di commit. Senza una tua concessione esplicita, quattro piani si accumulerebbero non committati: è la domanda numero 1 (§5).
10. Niente è stato eseguito: nessun commit, nessun codice, backlog non aggiornato, piani non tracciati (§6).

---

## 2. Ordine di esecuzione consigliato

### 2.1 Vincoli che valgono per tutti i piani

- **Un solo `target/release` e un solo banco.** Il banco monta `../portuale/target/release` dal vivo, e `managers.yaml` punta lì. Una build cargo durante una run del banco produce `portuale (deleted)` (memoria “no rebuild during bed runs”). I gate dei quattro piani (L0, L0 fixture-oracle, L1 merge, L2/L3) vanno quindi **eseguiti in serie**, mai sovrapposti. I worktree separati non risolvono il problema, perché il banco guarda sempre `../portuale`.
- **`cargo mutants --in-place` (04.367 S3)** modifica il sorgente: deve girare a macchina ferma (niente suite pmtest, niente banco, niente altro cargo) e con l'albero pulito, verificando con `sha256sum` prima e dopo.
- **Corpus e vdb condiviso.** 04.367 (`pg367-s-1`, `pg367-itop-1`) e 02.359 (`pg359-gsblk`, con un `-1.0` installato) aggiungono pacchetti installati fuori da `world` in `fixtures/var/db/pkg`. Entrambi muovono le stesse voci di corpus `--depclean`/`--prune`/`@installed` (precedente pmtest `f4cbd92`). Vanno eseguiti uno dopo l'altro, con un bless separato e catture Portage per ciascuno.
- **`/var/tmp` è tmpfs (~40G, condiviso con la RAM).** Ogni S0 lavora sotto `/var/tmp/<tag>` e cancella tutto alla fine. Mai scrivere in `/dev/shm`.

### 2.2 Dipendenze tra i piani

| tema | piani coinvolti | cosa succede | regola |
|---|---|---|---|
| `_want_update_pkg` / `_too_deep` | 02.359 S2 (#362), 04.367 S1 (#367) | è la stessa regola Portage (`depgraph.py:7304-7334`, `:8452-8457`). 04.367 la applica al solo switch pubgrub (`(deep && req.update) \|\| req.empty`), 02.359 al resolver di default | Sono compatibili. Però il residuo “default-solver `-pvu` new-slot” di 04.367 (§0.3, ultimo punto) è **la stessa divergenza di #362**: vedi la domanda D7 in §5 |
| pin `xfail(strict=True)` | 04.367 S2 → 02.359 S2 | se 04.367 aggiunge il pin di default `-pvu` come strict xfail e poi 02.359 S2 corregge #362, quel pin diventa XPASS e la suite fallisce | se 04.367 va prima, l'xfail cita `#362` e 02.359 S2 lo converte in pin normale (righe già prese da Portage) |
| `pubgrub -pvD` / docstring pubgrub | 02.359 S1 (riscrive le docstring `:27940-27947` e la label CASES `:451`), 04.367 S2 (nuovi pin pubgrub vicino ai `pg34`) | stesso file e stessa zona (`test_emerge_pretend_contract.py`) | in serie; nessun conflitto logico |
| `MergeOptions` / `unmerge_orphans` | 02.355 S4 (nuovo campo `unmerge_orphans`, `from_env` con fold incrementale, Q11), 02.346 S4 (D3: specchiare `merge_setup.rs:63-78` nel percorso diretto `ebuild`, **non** toccare `from_env` tranne il default di DISTDIR) | stesso file `ebuild_merge/options.rs`, stessa riga `ebuild.rs:393-399` | **02.355 S4 prima di 02.346 S4.** 02.346 S4 poi passa da `set_resolved_features` e riusa il campo di 02.355, senza duplicarlo. 355 Q11 e 346 Q4 sono la stessa modifica e va fatta una volta sola |
| `make.globals` nelle root ermetiche | 02.355 Q3 (strato base sintetico `FEATURES=unmerge-orphans`, `UNINSTALL_IGNORE=/lib/modules/*` quando `make.globals` non è letto), 02.346 §0.3/S4 (“settled design”: i pin copiano `make.globals` in `<cfg>/usr/share/portage/config/`) | lo strato sintetico cambia il FEATURES risolto in tutte le root ermetiche, e quindi anche quello che 02.346 S4 esporterà nel percorso `ebuild` | 02.355 S4 prima di 02.346 S4. La correzione generale (`make.globals` letto dall'installazione) è il residuo #384, che contraddice il “settled design” di 02.346: vedi B3 in §5 |
| profilo e `scalar_layers.rs` | 02.355 S4 (strato base), 02.346 S1 (catena dei profili, `scalar_layers.rs:96-110`) | stesso file, modifiche diverse | in serie. Consiglio 02.355 S4 prima di 02.346 S1, ma il vincolo è solo testuale |
| listing merge/unmerge `<<<`/`---` | 02.355 §1 (non estende più #351), 02.346 S10 (351-LIST, rinviato a una nuova voce) | risolto dalle review: la nuova voce di 02.346 possiede il listing. L'elog R4b viene portato in 02.355 S5 (Q2) | resta da dare un **numero** alla nuova voce, perché il blocco 371–373 è esaurito (G1 in §5) |
| gate merge-path (glibc + bash) | 02.355 S2–S5 (tutte e tre le invocazioni L1, più `sqlite`/`redb` dopo S3 e `l1-porttest.txt` dopo S4/S5), 02.346 S4 (L1 reinstall + nuova cella `portuale ebuild … merge`) e S10/S11 | le run L1 sono lunghe | in serie, mai durante una build. Se si riordina, 02.355 S1 (banco che vede le cancellazioni) va prima di ogni slice di 02.355 che giudica |
| fixture | 02.355 `repo-355` (`t355-*`), 02.359 `pg359-*`, 04.367 `pg367-*`, 02.346 nessuna nuova (configroot per test) | le review hanno verificato che nessun nome collide | — |

### 2.3 Ordine proposto (con motivazioni)

**Fase A: tutti gli oracoli (sola lettura, senza sorveglianza).** Una build a `9370a7ca`, poi gli S0 dei quattro piani: 04.367 S0 (container, ~1 h), 02.359 S0 (container più tre fixture, 2–3 h), 02.355 S0 (host, ~20 celle), 02.346 S0 (host, ~90 celle). Le catture lato Portage possono girare in parallelo (host e container sono separati), purché nessuno ricompili. Ogni S0 ha una “stop rule”: se Portage contraddice il piano, la cattura vince e la cosa ti viene riportata. Alla fine della fase A hai quattro `matrix.md` da leggere prima del codice. Motivazione: è lavoro senza rischi e fa emergere subito le sorprese che cambierebbero l'ordine.

**Fase B: 04.367 S1–S5 (#367, #370).** È il piano più piccolo (circa una giornata), tocca solo pubgrub, non ha gate merge-path ed è interamente sorvegliabile a distanza. Va per primo perché S3 (`cargo mutants --in-place`, 41 mutanti, ~5 min più la build) richiede la macchina ferma, ed è meglio farlo prima che altri piani occupino il banco. Il pin di default `-pvu` usa lo xfail con motivo `#362` (D7).

**Fase C: 02.359 S1 → S2 → S3 → S4 (#361, #362, #359, #360).** S1 è isolato e non prevede drift. S2 è il cambiamento più ampio del batch: ogni `-u` senza `--deep` smette di aggiornare le dipendenze già installate. Con la regola meccanica “bless solo se uguale a una cattura S0” può girare senza sorveglianza. S3 e S4 sono piccoli. Durata: 2–3 giorni in tutto. **S5 (#369) resta in attesa della tua presenza** (fase F).

**Fase D: 02.355 S1 → S2 → S3 → S4 → S5 (#358, #355, #357, #356).** Prima S1, perché senza di esso il banco non vede le cancellazioni (#358), e solo dopo i fix. Ogni slice da S2 in poi tocca il merge/unmerge path, quindi il gate glibc+bash con le tre invocazioni va eseguito dopo **ogni** slice. Il tempo di banco è il costo maggiore del batch. Va prima di 02.346 S4 per le dipendenze di §2.2.

**Fase E: 02.346 primo giro: S1 → S4 → S5 → S6 (solo codice, poi STOP).** S1 e S4 correggono configurazioni ignorate senza avviso e hanno impatto reale (es. `portuale ebuild` scriveva in `/var/tmp/portage` dell'host). S5 deve precedere S6 (rischio 6 del piano). S6 si ferma prima di ri-registrare qualsiasi pin e riporta la lista dei pin mossi (circa 98 test `ebuild`).

**Fase F (con te): revisione dei pin di 02.346 S6, poi 02.346 S3 → S2 → S8 → S7 → S9 → Z; 02.359 S5 (#369) e Z; decisioni su S10/S11 e #352.**

### 2.4 Sforzo per piano (dai piani)

| piano | slice | sforzo | senza sorveglianza? |
|---|---|---|---|
| 04.367 | S0 S · S1 S · S2 S · S3 S–M (~5 min run + 1–3 h triage) · S4 M (suite + 2 modi bed pubgrub) · S5 S | ~1 giorno | sì per S0–S5; commit/push e bless senza revisione no |
| 02.359 | S0 2–3 h · S1 1 h · S2 4–6 h · S3 2–3 h · S4 3 h · S5 8–12 h + `l0-resolver` (~1 h) · Z 1–2 h | ~3 giorni | S0–S4 sì; **S5 no**; Z sì quando tutto è verde |
| 02.355 | S0 M (~20 celle) · S1 S codice + L di banco (2–3 run del gate) · S2 S · S3 L (18 siti, 4 crate, re-derive DB) · S4 M · S5 M–L · Z M per run | il più lungo in tempo di banco | S0 sì; S1 sì (fermarsi se l'albero pre-#354 non mostra le righe `MISSING`); S2–S5 codice sì, “done” solo con gate verde; **stop su qualunque riga hard/unexplained, mai allowlist da soli** |
| 02.346 | S0 M (~90 celle) · S1 L · S2 S · S3 M · S4 M · S5 S · S6 L · S7 S · S8 S · S9 M · S10 L · S11 M | il più grande in codice | S0, S1, S2, S3, S4 (banco incluso), S5, S7 non-pretend, S8, S9 sì; S6 solo codice poi STOP; S6 revisione pin, S7 pretend, S10, S11 no |

---

## 3. Sorprese emerse (voci di backlog sbagliate o incomplete, rischi trovati)

| # | sorpresa | dove | conseguenza nel piano |
|---|---|---|---|
| 1 | **#361 è diagnosticato male.** Non è “un parent installato non trascina il BDEPEND”: in Portage `bdeps=auto` dipende dalla *presenza* della chiave `--usepkg` (`create_depgraph_params.py:97-103`; `--usepkg=n` la rende presente). Senza flag, Portage aggiorna `pg34-btool` anche lui. Le catture #34 passavano sempre `--usepkg=n` | 02.359 §0.1 | S1 tiene traccia di `usepkg_given`. Testo di #361 da correggere a Z. Pubgrub senza flag diverge: residuo #390 |
| 2 | **Pin pmtest che codificano comportamento non-Portage.** `test_update_threads_through_dependency_recursion_not_just_top_level` (`:17736-17756`) e il CASES `:2569-2572` (Portage stampa solo `N newpkg` + `N withdeps`). La label CASES `:451` e le docstring pubgrub `:27940-27947` sono false senza `--usepkg=n`. La DESCRIPTION di `pg34-blib-1.ebuild` è falsa. Il CASES `:441` (greedy slots) controlla solo il codice di uscita | 02.359 §0.1.4, §0.2.5, G6 | da riscrivere a partire dalle catture S0 (Q2 di 359). La DESCRIPTION cambia anche l'md5-cache |
| 3 | Pin portuale-authored in 02.346: l'rc 2 del set vuoto (`test_world_missing_file_expands_to_system_not_an_error`, `test_preserved_rebuild_set_is_empty_when_the_registry_is_empty`) e i 17 testi di errore argparse (13+4 pin, 17 voci di corpus) | 02.346 §0.1.4, S8 | set vuoto: solo registrato (#371); errori: S8 li riallinea a Portage |
| 4 | **Le catture 02.359 sono state fatte su un vdb “spostato”.** `actions.py:3669-3682` applica `_global_updates` anche alla root target, nonostante la docstring. La prima cella ha stampato `Performing Global Updates` e tutte le successive hanno girato con `oldmovepkg`→`newmovepkg` e `slotmovepkg` slot 0→1 | 02.359 §0.0 (D3) | S0 rifà lo staging per ogni cella oppure usa `--package-moves=n`, e ricontrolla `movedepconsumer` e `359-slots-slotmovepkg` |
| 5 | **`movedepconsumer` è una divergenza reale**, non un artefatto: Portage (moves attivi) dà rc 1, perché le global update non riscrivono gli ebuild, mentre portuale riscrive i `*DEPEND` in lettura e dà rc 0. Inoltre il pin di corpus `-pv --package-moves=n --deep dev-libs/movedepconsumer` (oggi rc 1) andrà verso Portage (rc 0) | 02.359 §0.4.3 | residuo #391 da registrare subito (a S0); il pin può muoversi solo verso la cattura S0 (d) |
| 6 | **#369 sta sotto `## Deliberate cuts — do NOT pick these`** (`backlog-tasks-2026-10.md:179`, voce `:197`). Ci stanno anche **#367 e #370** (`:195`, `:198`), più #363–#366, #368 e #321. 04.367 li chiama “Tier 4”, ma quel titolo non esiste | 02.359 header (P1); 04.367 review P1 | 02.359 Z sposta #369 nel blocco Tier 2. Per #367/#370 il piano non dice dove spostarli: domanda G2 in §5 |
| 7 | Un “controllo” di 02.359 in realtà diverge: `359-slots-unsatuseslotalt` stampa ` USE="-unsatuseorflag"` sulla riga `NS` senza `-v` | 02.359 G1 | residuo #392; il confronto dei controlli in S3 guarda solo le righe |
| 8 | **#355 è corretto, ma #356 è più ampio di quattro regole.** Mancano anche R5 `!md5`, R6 guardie di tipo (un `obj` diventato directory provoca EISDIR e **interrompe tutto l'unmerge**), R7 orphan arm per ogni tipo (fifo e dev compresi), R8 `FEATURES=-unmerge-orphans` **ignorato** (`unmerge_orphans` è sempre true in replace loop ed `emerge -C`) | 02.355 §0.2 | Q1 di 355: tutto viene incluso in #356, che cambia titolo |
| 9 | Note e documenti diventati falsi: `what-this-proves.md:19505` (“modified protected `/etc` files kept”) è vero solo a metà; in `code-notes/portuale/ebuild_unmerge.md` sono sbagliati “R4b dead code” e “fif/dev never unlinked, matching Portage”; anche la riga “Not established” fif/dev della matrix #354 è sbagliata | 02.355 §0.2, Z | da correggere a Z (la matrix con un'aggiunta datata, senza riscriverla) |
| 10 | **Rischio cancellazione `/bin`** (review 355 D1): con R3 da solo in S4, `emerge -C` di un pacchetto che registra `dir /bin` su merged-usr avrebbe scollegato il symlink `/bin`. Il gate glibc/bash non lo vede, perché il replace loop è protetto da `is_owned` | 02.355 S4/S5, cella C1 | corretto: R3 va in S5 insieme a R2/R4/R4b; C1 è failing-first e deve restare verde a ogni commit |
| 11 | **Trappola `make.globals`**: portuale lo legge sotto `PORTAGE_CONFIGROOT` (`scalar_layers.rs:80-83`), Portage dalla propria installazione. Nelle root ermetiche oggi `protect-owned`/`config-protect-if-modified` risultano già spenti, e un R8 fatto in modo ingenuo spegnerebbe anche l'orphan arm in tutti i pin | 02.355 §3 rischio 1, Q3 | strato base sintetico (Q3); correzione generale = #384 |
| 12 | preserve-libs **non ha un gate `FEATURES`** in portuale (Portage lo applica su `preserve-libs`, cella #354 `f0`) | 02.355 G9 | residuo #383 |
| 13 | `remote/merge_flow.sh` (il merge remoto `mrg`) è una **quarta implementazione dell'unmerge**: `awk '{print $2}'` e rimozione basata solo su mtime | 02.355 G3 | taglio esplicito, residuo #382 |
| 14 | Il banco stesso ha il bug di #357 (`consume.sh:234` `awk '{print $2}'`) e non vede i file cancellati (#358) | 02.355 §0.3/§0.4 | S1 lo corregge |
| 15 | **Premessa auto-clean falsa**: `make.globals:103` ha `AUTOCLEAN="no"`. L'auto-clean nelle catture #348 è un effetto collaterale delle global update in root ermetica (`actions.py:3671-3691`, bug #792195) | 02.346 D1 | S11 è legato solo a un `AUTOCLEAN=yes` impostato dall'utente; tutte le celle `emerge` di S0 girano con `--package-moves=n` |
| 16 | Il **profilo utente (`/etc/portage/profile`) non è un nodo di profilo** in portuale: le cinque letture “appiccicate” non hanno effetto. Il problema è più ampio del testo di #346 (tabella §0.1.1: `use.force`, `make.defaults`, `package.use`, … tutti diversi) | 02.346 §0.1.1 | S1 lo trasforma in un vero nodo |
| 17 | Affermazioni di #349/#351 da correggere: “perde l'infisso `/portage/`” è falso; “`emerge` rispetta la config” è falso per `DISTDIR` (lo prende da env su tutti i percorsi); `package.license` con build-id è già allineato; `Forcing test.` è già fatto (#348 S3); la grafia `<pf>/<pf>-<bid>` è imprecisa. `PORTAGE_REPO_NAME` manca nel percorso diretto (die senza `::repo`). `portuale ebuild` **ha scritto in `/var/tmp/portage` dell'host** | 02.346 §0.3, §0.4 | S4, e correzioni del testo a Z |
| 18 | Causa delle marcature `>>>` mancanti: `PORTAGE_QUIET=1` cablato in `phase_env.rs:183`. Se lo si toglie prima della S5, compaiono i testi di skip del bash vendorizzato, che non sono quelli di Portage | 02.346 §0.4 | S5 prima di S6, obbligatorio |
| 19 | Gli errori argparse differiscono anche nella riga d'errore e nella **riga vuota finale** (ogni messaggio di `parser.error` termina con `\n`, e argparse ne aggiunge un altro); il blocco `usage:` non è deterministico (`frozenset`) | 02.346 §0.2.3, D4 | S8 riallinea il testo; il blocco usage diventa un taglio (347-USG) |
| 20 | `binpkg-multi-instance`: il percorso `emerge` usa **già** i FEATURES risolti (`merge_setup.rs:47-52`), quindi il “deliberate conservatism” documentato (`ebuild_package.rs:56-60`) è già superato per `emerge` | 02.346 review “holds” 7 | S4 allinea `ebuild` (Q5) |
| 21 | La diagnosi S6 di #367 era troppo pessimista: il mutante **si vede** su una forma con 3 ebuild. Ne è emersa una **nuova divergenza pubgrub `-pvD`** (deep senza update tira lo slot più nuovo; Portage no) | 04.367 §0.3 | S1 corregge lo switch (condizionato a S0) |
| 22 | `--update-if-installed`: portuale accetta l'opzione e poi la ignora (`emerge_options.rs:57`) | 04.367 G7 | residuo (≥374) |
| 23 | Collisione dei numeri di backlog tra i piani (tre piani reclamavano tutti il 371) | review di 355/367/346 | risolta con l'assegnazione a blocchi (§4) |

---

## 4. Numeri di backlog

Il massimo numero registrato in tutti i tier è **#370** (verificato ora con `grep -hoE '^[0-9]+\. \*\*' LLM/backlog-tasks*.md`). Blocchi assegnati:

| blocco | piano |
|---|---|
| 371–373 | 02.346 |
| 374–379 | 04.367 |
| 380–389 | 02.355 |
| 390–399 | 02.359 |

Al momento della registrazione va ricontrollato con `grep -nE '^[0-9]+\. \*\*' LLM/backlog-tasks*.md` che il numero sia ancora libero. Mai usare un numero fuori dal proprio blocco.

### Residui previsti (dai piani)

**02.346 (371–373):**
- **#371** set vuoto: Portage `Total: 0 packages` con rc 0, portuale rc 2 (da riallineare anche ai due pin portuale-authored).
- **#372** `profile.bashrc` / `/etc/portage/bashrc` mai letti (`PORTAGE_BASHRC*` non impostati); correggere la riga obsoleta in `code-notes/portuale/ebuild_phases/mod.md:18`.
- **#373** `repos.conf` `usepkg-exclude` / `usepkg-include` per repository non letti.
- **senza numero (blocco esaurito):** 351-LIST (listing merge/unmerge, S10) e 351-CLEAN (auto-clean con `AUTOCLEAN=yes` utente, S11), se rinviati. Più quanto emergerà da S0. Vedi G1 in §5.
- da mettere in “Deliberate cuts” (senza numero nuovo): 347-USG (blocco `usage:` non deterministico), 351-UPD (`Performing Global Updates`), 351-NEWS (salvo cambio con Q8).

**04.367 (374–379), numeri non fissati nel piano (“#374 o il primo libero ≥374”):**
- divergenza del resolver di default `-pvu`: un parent nuovo porta la dipendenza installata nello slot più nuovo (`NS s-2`), mentre Portage tiene `s-1`. Da registrare solo se la riga Portage `-pvu` di S0 è soltanto `N top`. **È lo stesso caso di #362** (D7 in §5).
- `--update-if-installed` accettato ma ignorato (`req.update` resta false).
- eventuali mutanti #370 “uncovered” dopo il time box di 1 h per funzione.

**02.355 (380–389):**
- **#380** merge di un'immagine con path contenente `\n`/`\r` non rifiutato (`vartree.py:4627-4710`, “package … NOT merged”).
- **#381** `_match_contents`: matching per inode del parent e voci `dir` implicite dei parent (S5 ne implementa solo le forme strette).
- **#382** `remote/merge_flow.sh`: l'unmerge del merge remoto `mrg` (quarta implementazione).
- **#383** preserve-libs senza gate `FEATURES`.
- **#384** posizione generale di `make.globals` (letta dall'installazione di portuale, come Portage).
- #385–#389 liberi.

**02.359 (390–399):**
- **#390** `--solver=pubgrub` non piega il BDEPEND di un parent installato con bdeps auto (`-pvuD pg34-blib` senza flag: `Total: 0` contro `U pg34-btool-2`).
- **#391** (da registrare a S0) `movedepconsumer` con i moves attivi: Portage rc 1, portuale rc 0.
- **#392** (da registrare a S0) `359-slots-unsatuseslotalt`: ` USE="-unsatuseorflag"` sulla riga `NS` senza `-v`.
- **#393** (condizionale) fold `FEATURES=getbinpkg` → `--usepkg`, se la cella S0 diverge e S1 non lo assorbe.
- **#394** (condizionale, Q7) blocker lookahead di `_greedy_slots` nel resolver di default.
- **#395** suffisso ` for <root>` su `no ebuilds to satisfy` (`PORTAGE_RUNNING_ROOT` esiste solo in portuale).
- **#396** (condizionale) `--usepkgonly=n`: la presenza della chiave fa scartare a Portage il db degli ebuild.
- **#397** (condizionale) differimento degli atomi `virtual/*`.
- **#398** (condizionale) la scelta `||` nel percorso flip di `--autounmask-use` resta al momento dell'espansione.
- #399 libero.

---

## 5. Domande per il proprietario (consolidate)

Legenda: **[BLOCCA]** = senza risposta quella parte non parte o non si chiude; **[default sicuro]** = l'esecutore prende il default e prosegue. Tra parentesi il riferimento alla domanda nel piano originale.

### Le 8 che contano di più

1. **A1. Concessione di commit.** [BLOCCA i commit, non il lavoro] (355 Q9, 359 Q9, 367 Q12). Senza concessione, 02.359 accumula le slice non committate e scrive diff stat e gate in `LLM/evidence/2026-10-15-plan-359/slices.md`; 04.367 si ferma dopo S5; 02.355 non committa. Quattro piani accumulati non committati nello stesso albero sono ingestibili. **Default consigliato da approvare:** un commit per slice per repo, a coppie (prima pmtest, poi portuale che cita lo sha di pmtest), messaggi in inglese semplice, nessuno squash, **nessun push**.
2. **B1. Strato base sintetico per `make.globals`** [default sicuro] (355 Q3). Quando `make.globals` non è letto, sotto lo stack si aggiungono `FEATURES=unmerge-orphans` e `UNINSTALL_IGNORE="/lib/modules/*"`. **Default: sì.** Alternativa: la correzione generale #384 prima di S4, che però muove molti pin.
3. **D1. Riallineare a Portage i pin che codificano comportamento non-Portage** [default sicuro] (359 Q2), insieme a **D2. niente aggiornamento delle dipendenze senza `--deep`**, USE drift compreso (359 Q3). **Default: sì a entrambe.** Le righe vengono dalle catture S0 e il commit pmtest elenca ogni pin cambiato. È il cambiamento più visibile all'utente del batch.
4. **B2. In `portuale ebuild` i gate li guidano i FEATURES risolti** (sandbox, userpriv, `unmerge-orphans`, `binpkg-*` …) [default sicuro] (346 Q4) e **B4. `binpkg-multi-instance` dai FEATURES risolti** (346 Q5). **Default: sì.** Su un sistema reale `portuale ebuild` si comporterà come `emerge`. Richiede la nuova cella di banco `portuale ebuild … merge`.
5. **C1. Includere R5–R8 in #356** [default sicuro] (355 Q1). **Default: sì**, con il titolo cambiato in “`remove_contents` lacks Portage's unmerge arms”. In alternativa, tre voci nuove e qui solo R1–R4b.
6. **F1. Listing merge/unmerge (351-LIST)** [BLOCCA S10] (346 Q9), più **G1. il numero di backlog per quella voce**. **Default: rinviare a una voce nuova**, non eseguita senza sorveglianza; la raccomandazione del piano è di portarlo prima o poi. Il blocco 371–373 è esaurito: **propongo di prendere i numeri liberi #385/#386 dal blocco di 02.355** (351-LIST e 351-CLEAN; il listing è appunto il tema condiviso con 02.355). È una mia proposta, non è scritta in nessun piano.
7. **D7. Il residuo “default `-pvu` new-slot” di 04.367 è #362** [default sicuro] (367 Q2). Il piano propone di registrare un residuo nuovo più un pin `xfail(strict=True)`. **Default consigliato:** nessun numero nuovo, xfail con `reason="#362"`, e cella `pg367-top -pvu` aggiunta all'S0 di 02.359 (S2 converte poi lo xfail in pin normale). Se 02.359 S2 gira prima di 04.367 S2, il pin si scrive direttamente normale.
8. **H1. #352 (IDEPEND installato nella root target invece che nella running root).** [BLOCCA #352, fuori da questo batch] Opzioni: copiare Portage (scrive sull'host quando `ROOT≠/`), **taglio documentato (consigliato)**, oppure errore esplicito. Nessun piano lo tocca.

### A. Processo, commit, gate

| id | domanda | default | tipo |
|---|---|---|---|
| A1 | concessione di commit (sopra) | un commit per slice per repo a coppie, niente push | **BLOCCA i commit** |
| A2 | cadenza del gate merge-path in 02.355 (355 Q6) | gate completo (tre invocazioni L1) dopo **ogni** slice merge/unmerge (S2, S3, S4, S5); dopo S3 anche `sqlite`/`redb`; dopo S4/S5 anche `l1-porttest.txt` | default sicuro |
| A3 | `L1_CONSUME_INSTALL_MASK` nel gate obbligatorio (355 Q7) | obbligatorio per modifiche alla metà unmerge del merge path, opzionale altrimenti; documentato in `agent-context.md` | default sicuro |
| A4 | S0 di 02.355 mostra che portuale già coincide su una regola (355 Q10) | la regola si toglie dalla slice e si annota in `matrix.md` e nel paragrafo di chiusura | default sicuro |
| A5 | numero di equivalenti accettabili per #370 (367 Q5) | nessun tetto, ma ogni equivalente con un argomento di una riga; se più di metà dei missed sono “equivalenti”, segnare “equivalence-heavy, owner to confirm” e proseguire; dopo 1 h per funzione → “uncovered” con numero ≥374 | default sicuro |
| A6 | budget della run di mutanti (367 Q6) | ~5 min attesi; tetto `timeout 3600`, `--timeout 300` per mutante; nient'altro in esecuzione; se `--list` supera 200, fermarsi | default sicuro |
| A7 | dove registrare i mutanti #370 (367 Q7) | nuova sezione `## #370 mutants` in `LLM/code-notes/portage-repo/solver_bridge.md` | default sicuro |
| A8 | timeout contati come caught (367 Q8) | sì, con nota “inspect-don't-pin” | default sicuro |
| A9 | ambito del banco per 04.367 S4 (367 Q9) | solo i due modi L0 pubgrub (`-pvuD`, `-pve`) | default sicuro |
| A10 | fonte delle catture per 04.367 (367 Q10) | container (`stage.sh` + `podman run`); host `emerge` come ripiego, annotato in `cmd.txt` | default sicuro |

### B. `make.globals`, FEATURES, configurazione di `ebuild`

| id | domanda | default | tipo |
|---|---|---|---|
| B1 | strato base sintetico (355 Q3) | sì | default sicuro |
| B2 | FEATURES risolti guidano i gate in `ebuild` (346 Q4) | sì; siti elencati in 346 S4 (D3); `from_env` resta invariato tranne il default di DISTDIR | default sicuro |
| B3 | **tensione tra #384 e 02.346.** 02.346 dice che leggere `make.globals` sotto il configroot è “settled design” (i pin ne portano una copia); 02.355 registra come #384 la correzione generale (leggerlo dall'installazione, come Portage). Va scelta una linea | registrare #384 come voce aperta e non toccare nulla in questo batch | default sicuro (decisione futura) |
| B4 | `binpkg-multi-instance` dai FEATURES risolti (346 Q5) | sì | default sicuro |
| B5 | FEATURES di `ebuild <f> unmerge` (355 Q11) | stesso helper dei FEATURES risolti usato da `emerge -C` quando la config si risolve, altrimenti fold incrementale dell'env. **È la stessa modifica di B2: va fatta una volta sola** (02.355 S4 prima) | default sicuro |

### C. Semantica di unmerge / CONTENTS (02.355)

| id | domanda | default | tipo |
|---|---|---|---|
| C1 | R5–R8 dentro #356 (355 Q1) | sì | default sicuro |
| C2 | testo dell'elog R4b (355 Q2) | portarlo attraverso l'elog di portuale; i pin S5 controllano le righe dell'elog delle catture E1/E2 | default sicuro |
| C3 | righe `owner` sqlite/redb derivate con lo split vecchio (355 Q4) | all'apertura in scrittura, `rebuild_index` una volta sotto `meta.contents_parser = 2`, senza bump di schema; le aperture in sola lettura non scrivono (un avviso, righe vecchie) | default sicuro |
| C4 | newline in un path di merge (355 Q5) | residuo #380, fuori da #357 | default sicuro |
| C5 | cella `dev` (F2) (355 Q8) | verdetto solo da sorgente più unit test; alternativa: una run `sudo` con `--preserve-env=…` | default sicuro |

### D. Resolver: update, slot, installati (02.359, 04.367)

| id | domanda | default | tipo |
|---|---|---|---|
| D1 | riallineare i pin non-Portage (359 Q2) | sì | default sicuro |
| D2 | `-pvN` con USE drift su una dipendenza: seguire Portage, niente reinstall senza `--deep` (359 Q3) | sì | default sicuro |
| D3 | #361 trattato come regola di presenza dell'opzione, con riscrittura del backlog a Z (359 Q1) | sì | default sicuro |
| D4 | #360: togliere il gate `buildtime_hard` per pacchetti solo-installati (359 Q4) | segue la cattura S0 di `pg359-gbapp` | default sicuro |
| D5 | suffisso ` for <root>` (359 Q5) | fuori ambito, residuo #395 | default sicuro |
| D6 | `movedepconsumer` (359 Q6) | registrare #391 subito; il pin con moves spenti segue Portage (solo verso la cattura S0 (d)) | default sicuro |
| D7 | residuo default `-pvu` di 04.367 contro #362 (367 Q2) | xfail con `reason="#362"`, nessun numero nuovo (proposta mia; il piano dice residuo nuovo più strict xfail) | default sicuro |
| D8 | blocker lookahead di #359 (359 Q7) | entrambi i passaggi con `pg359-gsblk`; se la fixture non discrimina, solo il passaggio 1 e residuo #394 | default sicuro |
| D9 | approccio per #369 (359 Q8) | port vero della `_dep_disjunctive_stack` (walk principale e walk degli installati); il percorso flip va in #398. **S5 non si esegue senza sorveglianza** | **BLOCCA S5 (presenza)** |
| D10 | correggere lo switch pubgrub `-pvD` (367 Q1) | correggere in S1 **solo se** la riga Portage `-pvD` di S0 è `N top` (stop rule 2), altrimenti solo pin | default sicuro |
| D11 | vdb della fixture 04.367: condiviso o per test (367 Q3) | condiviso (precedente `pg34-*`), con revisione del drift del corpus | default sicuro |
| D12 | `compute_or_overrides` nell'ambito di #370 (367 Q4) | sì (+7 mutanti) | default sicuro |
| D13 | variante con parent installato `pg367-itop` (367 Q11) | sì, con un file `RDEPEND` nel vdb | default sicuro |

### F. Config, opzioni, output (02.346)

| id | domanda | default | tipo |
|---|---|---|---|
| F1 | listing merge/unmerge, S10 (346 Q9) | rinviare a una voce nuova (numero: G1) | **BLOCCA S10** |
| F2 | auto-clean, S11 (346 Q10) | rinviare a una voce nuova, piccola, legata a `AUTOCLEAN=yes` impostato dall'utente; niente opzione “solo testo” | **BLOCCA S11** |
| F3 | preambolo `--pretend` sugli abort (346 Q6) | correggere solo il ramo non-pretend; il `--pretend` resta com'è e l'incoerenza va annotata (un preambolo completo muoverebbe la maggior parte dei 3758 pin) | **BLOCCA la metà pretend di S7** |
| F4 | profilo utente come vero nodo (346 Q1) | sì, `ProfileNode { kind: User }`; le letture appiccicate vengono rimosse | default sicuro |
| F5 | (346 Q2) profilo utente ignorato senza `make.profile`: **non è una domanda**, è la regola Portage (`LocationsManager.py:169`), già dentro S1 | — | — |
| F6 | testo dell'avviso di deprecazione di `package.keywords` (346 Q3) | solo il messaggio, senza il frame Python, e solo se `package.keywords` produce almeno una voce valida | default sicuro |
| F7 | chiavi `getbinpkg-exclude/include` di `binrepos.conf` (346 Q7) | sì, dentro S9 | default sicuro |
| F8 | notifica news a fine run (346 Q8) | taglio | default sicuro |
| F9 | set vuoto rc (346 Q11) | solo registrare #371 | default sicuro |
| F10 | `bashrc` mai letti (346 Q12) | solo registrare #372; il piano consiglia di farlo subito dopo questo batch | default sicuro |
| F11 | revisione dei pin mossi da 02.346 S6 (~98 test `ebuild`): non è una domanda del piano, ma una regola di stop | l'esecutore si ferma alla fine del codice di S6 e consegna la lista | **BLOCCA la seconda metà di 02.346** |

### G. Contabilità del backlog

| id | domanda | default | tipo |
|---|---|---|---|
| G1 | numeri per 351-LIST e 351-CLEAN (il blocco 371–373 è esaurito; 346 Z dice “chiedere al coordinatore”) | proposta: #385 e #386 dal blocco libero di 02.355 | default sicuro, se approvi |
| G2 | #367 e #370 stanno sotto “Deliberate cuts” come #369; 02.359 Z sposta solo #369 nel Tier 2 | 04.367 S5 li sposta nello stesso modo, nel blocco Tier 4 (da creare) o nel Tier 2; consiglio Tier 4, coerente con il prefisso `04.` del piano | default sicuro |
| H1 | **#352** IDEPEND (vedi sopra) | **taglio documentato (consigliato)**; alternative: copiare Portage, errore esplicito | **BLOCCA #352** |

---

## 6. Cosa NON è stato fatto, e come partire

### Stato

- **Nessun commit e nessun push**, in nessuno dei due repo.
- I quattro piani, le quattro review, `LLM/evidence/2026-10-15-plan-359/` e questo documento sono **non tracciati** in portuale sul branch `plan/batch-2026-10-15`. pmtest è sullo stesso branch ed è pulito.
- **Backlog non aggiornato**: nessun residuo registrato (né 371–399, né #391/#392, che 02.359 registra a S0). I testi sbagliati di #349/#351/#356/#361 sono ancora quelli vecchi. Tutto si fa al momento dell'esecuzione, ricontrollando i numeri liberi.
- **Esecuzione non iniziata**: nessuno S0 catturato (le uniche catture esistenti sono le 53 celle di 02.359 in `LLM/evidence/2026-10-15-plan-359/`, alcune da rifare per il vdb spostato), nessuna fixture, nessun codice, nessun gate eseguito.
- Le tabelle §0 di 02.346 e di 04.367 vengono da probe la cui scratch è stata cancellata: valgono come “likely” finché S0 non le ricattura.

### Comandi per partire

I branch partono da `main` (oggi `main` = `plan/batch-2026-10-15` in entrambi i repo). Ogni branch va aperto **in entrambi i repo contemporaneamente, con lo stesso nome**. I file non tracciati dei piani restano nell'albero anche dopo il cambio di branch.

```sh
# 04.367 (consigliato per primo dopo la fase A)
cd /home/vivo/repo/PORTUALE/portuale && git checkout -b backlog/367-370-pubgrub-mutants main
cd /home/vivo/repo/PORTUALE/pmtest   && git checkout -b backlog/367-370-pubgrub-mutants main

# 02.359
git -C /home/vivo/repo/PORTUALE/portuale checkout -b backlog/359-default-solver-update main
git -C /home/vivo/repo/PORTUALE/pmtest   checkout -b backlog/359-default-solver-update main

# 02.355
git -C /home/vivo/repo/PORTUALE/portuale checkout -b backlog/355-358-unmerge-parity main
git -C /home/vivo/repo/PORTUALE/pmtest   checkout -b backlog/355-358-unmerge-parity main

# 02.346 (nome suggerito dal piano)
git -C /home/vivo/repo/PORTUALE/portuale checkout -b backlog/346-351-config-output main
git -C /home/vivo/repo/PORTUALE/pmtest   checkout -b backlog/346-351-config-output main
```

Se i piani vengono eseguiti in serie, ognuno parte dal `main` aggiornato con il precedente (dopo il merge, se concesso), così le dipendenze di §2.2 risultano già soddisfatte. Prima di ogni piano: build a HEAD, baseline della suite pmtest presa **per nome dei test** (`python3 -m pytest pytests-contract-suite -q` in `../pmtest`; `git clean -fdq fixtures/` se compaiono inquinamenti), poi S0.

Primo passo concreto della fase A, 04.367 S0 (controllo collisioni):
```sh
ls /home/vivo/repo/PORTUALE/pmtest/fixtures/repo/dev-libs | grep -E 'pg367|pg359'   # deve essere vuoto
```

### Regole di AGENTS.md che vincolano l'esecuzione

- **Commit e push solo su richiesta esplicita** (regola 9). Ogni richiesta è separata, e finire una slice non la implica.
- **Due repo, un progetto:** branch con lo stesso nome in entrambi, commit a coppie (prima pmtest; il messaggio portuale cita lo sha corto di pmtest, quello pmtest cita branch e slice di portuale), push insieme (prima portuale, poi pmtest). Il bless del corpus (`PORTUALE_CORPUS_BLESS=1`) sta solo nel commit pmtest e va fatto dopo aver rivisto il drift.
- **Oracolo prima del codice:** i valori attesi vengono da Portage 3.0.82.2 (host, container o `ResolverPlayground`), mai dall'output di portuale. Nei documenti si scrive “Portage”, non “real”.
- **Gate completo per slice** (regola 8): `cargo fmt --check`, `cargo clippy --release --all-targets` con 0 warning, `cargo build --release && cargo test --release` sull'intero workspace, poi la suite pmtest. Banco L0 dopo modifiche al resolver; **gate merge-path glibc + bash** dopo ogni modifica al merge/unmerge path. Mai ricompilare durante una run del banco.
- **Gate fallito:** fermarsi e riportare. Mai aggiungere da soli una voce a `known-divergences.yaml` o un'allowlist.
- **Numerazione:** numeri globali mai riusati; ricontrollare su tutti i tier prima di registrare; ogni piano usa solo il proprio blocco.
- **Chiusura del piano** (regola 11): nello stesso commit che porta la voce a `DONE <data>`, lo `Status:` del piano passa da `proposed` a `done <data>` (range delle slice, branch, commit), e gli hash citati devono essere raggiungibili da `main`.
- **Commenti nel codice brevi**; il contesto e le citazioni Portage vanno in `LLM/code-notes/…`, poi `python3 LLM/tools/code_notes_index.py`. `what-this-proves.md` si aggiorna solo aggiungendo in coda, con un esempio eseguibile verificato dal vivo.
- **`/dev/shm` mai**; `/var/tmp` è tmpfs: ripulire la propria scratch a fine slice. Per i test che scaricano, `GENTOO_MIRRORS=http://<IP eth0>:8080` (proxy dei distfiles). Con sudo si usa `--preserve-env=VAR`, mai il prefisso `VAR=x sudo`.
- **Agenti esterni:** lancio a scaglioni (60 s), via `LLM/tools/scope/oc.sh`, mai `pkill`. Non modificare script in esecuzione.
