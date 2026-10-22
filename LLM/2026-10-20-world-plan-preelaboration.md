# Preelaborazione e cache di stato per `emerge -uDpvN @world`

Status: **done 2026-10-22** (S0 `67a34067`, S1 `31c66595` `b0f4b950` `682149ab`, S2 measured and deferred `27a76963`, S3 plan cache `081f3710`; branch `backlog/441-s0-s1`). S4-S7 not done: S4 is #473, residues #474, #475. Original design below; the shipped S3 is the conservative variant of the strong review (§2.1), see `LLM/code-notes/portuale/pretend/plan_cache.md`. Data: 2026-10-20.
Scopo: documento di ripresa per l'evoluzione "piano `@world` in ~1 s".
Spostato da `helpers/` (gitignored) in `LLM/` il 2026-10-20 (backlog #441); scritto mentre un altro agente lavorava sul
working tree: non tocca codice tracciato.

Documento collegato, ma con un workload diverso:
[`docs/cleanroom/parallelism.md`](../docs/cleanroom/parallelism.md). Quel
documento misura `net-libs/rest` (~2 s) e conclude "non parallelizzare il
resolver". Il presente documento parte da un workload molto più grande e
CPU-bound (`@world`, 210 atomi, 2099 pacchetti installati) e arriva a una
conclusione diversa: il problema non è il parallelismo, è **il ricalcolo
della stessa pianificazione**.

---

## 0. Definizione: "premastica" = preelaborazione

"Premastica" è un verbo, non un nome: significa preparare i dati perché il
consumo successivo sia più rapido. Nel documento si usano i termini
**preelaborazione**, **snapshot** e **cache di stato**.

La tesi:

> `emerge -uDpvN @world` impiega minuti perché ricalcola da zero ogni volta
> una pianificazione che dipende da un numero relativamente piccolo di stati
> (repo, binpkg, VDB, config, world, argv/env). Se quegli stati vengono
> preelaborati e la pianificazione viene materializzata, i run ripetuti e il
> primo run dopo un sync possono stare sotto il secondo.

---

## 1. Misure e contesto

### 1.1 Misura fornita

Sul container LXC:

```sh
time ./target/release/portuale emerge -uDpvN @world &> /dev/null
real    3m13.820s
user    2m28.214s
sys     0m31.206s
```

- 210 pacchetti in `/var/lib/portage/world`;
- 2099 pacchetti installati (`eix -I --only-names | wc -l`, `EIX_LIMIT=0`);
- output su `/dev/null`, quindi il costo non è la scrittura su terminale.

Il rapporto user/sys (~4.8:1) dice che il tempo è **CPU-bound**, non I/O-bound.
Questo è il dato che cambia la priorità rispetto al documento `parallelism.md`.

### 1.2 Obiettivi

| Priorità | Obiettivo | Note |
|---|---|---|
| Primario | `emerge -uDpvN @world` **≤ 1 s** | anche al **primo run dopo un sync** |
| Secondario | run ripetuti **≤ 1 s** | molti run sullo stesso stato |
| Terziario | `mrg` planning **≤ 1 s** lato server | il piano è calcolato sul server |
| Non obiettivo | 1 s per qualunque atomo mai visto | cold ammesso più lento |

### 1.3 Vincoli

- `emerge`: output **byte-identico** a Portage. Nessun riordino visibile.
- `mrg`: l'ordine di build/installazione può cambiare, ma ogni dipendenza
  hard deve restare rispettata (`gcc` prima di `eix` se è BDEPEND/DEPEND;
  PDEPEND può essere rilassato).
- Niente demone. La preelaborazione deve essere un comando/processo breve,
  invocabile da un hook di sync, non un servizio sempre attivo.
- Tempo di build della cache: **non un problema** (tempo macchina, nessun
  umano in attesa).
- Dimensione cache: **centinaia di MB accettabili**.
- Repo sorgente e binpkg **separati**: aggiornamenti indipendenti, cache
  indipendenti, nessuna fusione in un unico snapshot.
- `mrg`: piano calcolato sul **server**; multi-client rimandato.
- Dopo un merge è accettabile una finestra "fredda" (nessun demone che
  ricalcola in background obbligatorio).
- Musl-static, dipendenze pure-Rust; niente C linkage nuovo.

---

## 2. Perché il documento `parallelism.md` non basta

`parallelism.md` ha misurato:

- `--pretend` su un atomo singolo: ~2 s, già più veloce di Portage;
- `read_md5_cache` 36.811 chiamate / 33.608 hit / 3.203 letture reali;
- `list_candidates` 17.765 / 17.157 hit / 608 miss;
- memo thread-local `Rc` (non condivisibili), quindi un pool di worker
  partirebbe con memo fredde;
- conclusione: non scrivere un pool nel resolver; il candidato di
  parallelismo è il merge copy loop (`§3`).

Queste conclusioni restano valide **per quel workload**. Su `@world` il
profilo cambia per cinque motivi che il workload piccolo non esercita:

### 2.1 `-N` (`--newuse`) tocca ogni installato, senza memo

`crates/portage-repo/src/resolve/pretend.rs::already_installed_or_reinstall`
chiama `reinstall_flags_for_use_change` quando `newuse`/`changed_use` sono
attivi. La funzione (`crates/portage-repo/src/reinstall.rs::reinstall_flags_for_use_change`)
per ogni pacchetto:

- legge dal VDB `USE` e `IUSE` (`read_vdb_flag_set`);
- legge l'md5-cache corrente (`repo_aux_metadata`);
- calcola `effective_use_flags`;
- confronta presenza e stato dei flag.

Non c'è memo. Con 2099 installati e `@world` come top-level, sono ~2099
esecuzioni, potenzialmente ripetute a ogni passata e nella fase 2.

### 2.2 `-D` + `--complete-graph-if-new-ver` = seconda passata completa

`crates/portuale/src/pretend/resolve_phases.rs::run_resolve_phases` esegue la
fase 1 (autorevole) e, se un pacchetto installato cambia e
`--complete-graph-if-new-ver` è attivo (default), la fase 2
`_complete_graph`. Su `@world` significa **due risoluzioni complete**.

### 2.3 Scan lineari dentro il walk: possibile O(n²)

Molti punti per-nodo/per-disgiunzione fanno `entries.iter().find/any/position`
sul grafo intero. Esempi (simboli, non righe):

- `crates/portage-repo/src/resolve/or_preference.rs::alternative_downgrade_demoted`
  scandisce `entries` per ogni atomo di ogni alternativa `||`;
- `crates/portage-repo/src/resolve/pass/slot_conflicts.rs`;
- `crates/portage-repo/src/resolve/blockers.rs`;
- `crates/portage-repo/src/resolve/cycles.rs`;
- `crates/portage-repo/src/resolve/withhold.rs`;
- `crates/portage-repo/src/resolve/direct_solve.rs`;
- `crates/portage-repo/src/resolve/autounmask.rs`;
- `crates/portage-repo/src/resolve/entry.rs`;
- `crates/portage-repo/src/merge_order/select_nodes.rs` (fallback roots:
  `g.order.iter().position` dentro un sort).

Con migliaia di entry questi scan possono dominare. La correzione è un
**indice del grafo** costruito una volta per passata, non un pool di thread.

### 2.4 Display `-v` su migliaia di entry

`-v` calcola per ogni entry `use_flags_display`, `use_expand_display`,
`use_expand_display_p`, decorazioni slot/repo, contatori. Con output su
`/dev/null` il costo di calcolo e formattazione resta. Una cache del piano
(o dell'output renderizzato) lo elimina.

### 2.5 `run_pass` non è l'unica voce

Il profilo di `parallelism.md` dice `run_pass` ~81% con figli su un atomo
singolo. Su `@world` il peso relativo può cambiare; senza un profilo
dedicato non si può estrapolare. La S0 (§9) esiste per questo.

---

## 3. Architettura: piramide L0–L4

```
L0  Dati grezzi autorevoli
    repo md5-cache | binpkg Packages+payload | VDB | profilo/make.conf/package.*
        |
        v  (cache build, dopo sync / dopo merge)
L1  Snapshot preelaborati, indipendenti
    RepoSnapshot | BinpkgSnapshot | InstalledSnapshot | ConfigSnapshot | WorldSnapshot
        |
        v  (per fingerprint)
L2  Viste derivate
    CandidateView | ReinstallView | DepsChangedView | SlotOperatorView | GraphIndex
        |
        v  (per StateFingerprint)
L3  Plan cache
    RenderedPlanCache (--pretend) | GraphResultCache (esecuzione / mrg)
        |
        v  (opzionale, dopo merge)
L4  Delta / incremental
    aggiorna solo la closure dei pacchetti cambiati
```

Regola: **L0 è la verità, L1–L4 sono derivate e opzionali**. Se una cache è
mancante, corrotta o di schema diverso, si ricade sul percorso attuale senza
errore.

---

## 4. L1 — Snapshot

### 4.1 RepoSnapshot (sorgente ebuild)

Per ogni repo/overlay, un file immutabile per revisione.

Contenuto per `(category, package, version)`:

- campi md5-cache: `EAPI`, `SLOT`, `KEYWORDS`, `IUSE`, `LICENSE`,
  `PROPERTIES`, `RESTRICT`, `REQUIRED_USE`, `SRC_URI`, `DEFINED_PHASES`,
  `INHERIT`, `HOMEPAGE`, `DESCRIPTION`, `DEPEND`, `RDEPEND`, `BDEPEND`,
  `PDEPEND`, `IDEPEND`, `_md5_`, `_eclasses_`;
- `repo_name`, `repo_priority`, `repo_location`;
- dipendenze **già parsate**: una struttura compatta per ogni classe, non
  stringhe da ritokenizzare;
- indice `(category, package) -> range di versioni`;
- indice `(category, package, slot) -> range`;
- `reverse_index`: per ogni `(category, package)` nominato in una qualunque
  dipendenza (entrambi i rami USE, tutte le classi), l'elenco dei version id
  che lo nominano. È un over-approximation: serve per discovery e scan
  inverse, non per decidere.
- `atom_table`: atomi deduplicati e parsati (categoria, package, operatore,
  versione, slot, use-deps, blocker).

Non contiene: visibilità, USE risolte, preferenze `||`, scelte del resolver,
merge order. Sono dipendenti da config/stato.

**Chiave di invalidazione:** revisione del repo + hash di `metadata/layout.conf`
+ hash di `profiles/repo_name` + masters chain + schema version. La revisione
può essere:
- `git rev-parse HEAD` se `.git` esiste;
- contenuto di `metadata/timestamp` (o `metadata/timestamp.chk`);
- in mancanza, un file `metadata/.portuale-revision` scritto dal cache build,
  validato contro l'mtime massimo di `metadata/md5-cache`.

### 4.2 BinpkgSnapshot (sorgente binaria, separata)

Per ogni sorgente binaria (`$PKGDIR` locale, ogni binhost configurato), un file
separato. **Non si fonde con RepoSnapshot.**

Contenuto per cpv:

- `SLOT`, `SUBSLOT`, `EAPI`, `USE` baked, `IUSE`;
- `BUILD_ID`, `BUILD_TIME`, `SIZE`;
- `*DEPEND` registrate (già USE-ridotte al build);
- digest `MD5`/`SHA1`/`SHA256`/`SHA512`;
- path locale o URI remota;
- `DEFINED_PHASES`.

Indici: `(cat,pkg)`, `(cat,pkg,slot)`, `(cat,pkg,build_id)`.

**Chiave di invalidazione:** mtime/hash di `$PKGDIR/Packages` o
`TIMESTAMP`/`TTL` del binhost; per il remoto, la revisione del file
`Packages` scaricato. Un sync del repo non lo invalida; un aggiornamento del
binhost non invalida il repo.

### 4.3 InstalledSnapshot (VDB)

Contenuto per entry VDB:

- `CATEGORY`, `PF`, `SLOT`, `USE`, `IUSE`, `repository`, `COUNTER`;
- `*DEPEND` registrate;
- `CONTENTS` (path, tipo, md5/target, mtime);
- binding `:=` registrati (provider cpv/slot/subslot);
- mtime della directory dell'entry.

Indici:
- `(cat,pkg) -> entry`;
- `(cat,pkg,slot) -> entry`;
- `reverse_dependents`: consumer cpv -> provider cpv/slot, calcolato dalle
  dipendenze registrate;
- `contents_by_path`: path -> owner cpv, per collision check e depclean.

**Chiave di invalidazione:** mtime delle directory categoria del VDB +
`counter` + numero di entry. Portage mantiene questi mtime con `_bump_mtime`
per i consumer; portuale li usa già in `installed_candidates`. In caso di
dubbio, rebuild (2099 entry è poco).

### 4.4 ConfigSnapshot

Il `Config` risolto è grande e composto da `HashSet`/`HashMap`; serializzarlo
richiede un formato dedicato. Due fasi:

- **Fase A (raccomandata per partire):** non serializzare `Config`; calcolare
  un fingerprint dei file di config e rieseguire `resolve_config` a ogni run.
  La risoluzione del profilo è piccola rispetto al resolve del mondo.
- **Fase B (solo se il profilo mostra costo):** serializzare un sottoinsieme
  stabile di `Config` (USE, maschere, licenze, keywords, package.*, system
  set) con schema versionato. I fingerprint `use_context_base` e
  `is_visible_base` esistono già in
  `crates/portage-profile/src/fingerprints.rs` e sono il modello da seguire.

File di config da includere nel fingerprint:
profilo (catena `parent`), `make.conf`, `make.globals`, `package.*`,
`repos.conf`, `world`, `world_sets`, e le variabili d'ambiente rilevanti
(§6.1).

### 4.5 WorldSnapshot

- `@world`, `@system`, `@selected`, set utente espansi in atomi;
- appartenenza agli set (per `requested` e per `--json`);
- `profiles/updates/` già applicati.

Chiave: hash di `world`/`world_sets` + profilo + config + revisione repo.

---

## 5. L2 — Viste derivate

Queste viste sono ciò che toglie lavoro al resolver senza cambiare le sue
decisioni. Sono keyed da fingerprint, non da run.

### 5.1 CandidateView

Per `(repo_snapshot_id, use_context_fingerprint, is_visible_fingerprint, cp)`:

- elenco candidati ordinato per versione e priorità repo;
- per ogni candidato: `is_visible` (bool), `effective_use_flags` (set),
  `metadata_key_accepted` (bool);
- `tree_best` (migliore visibile) e `installed_best` (migliore installato),
  per i due percorsi `--update` / non-`--update`.

Attenzione: la vista non deve congelare scelte che dipendono da `--deep`,
`--newuse`, `--changed-use`, `--usepkg`, `--getbinpkg`, `--binpkg-respect-use`.
Il fingerprint deve includere queste opzioni, oppure la vista si limita ai
fatti per-candidato (visibilità/USE) e lascia la selezione al resolver.

### 5.2 ReinstallView (`-N` e affini)

Per `(installed_snapshot_id, repo_snapshot_id, use_context_fingerprint, cpv)`:

- `reinstall_flags_for_use_change` -> `Option<Vec<String>>`;
- `deps_changed` -> bool;
- `slot_changed` -> bool;
- `rebuilt_binary` -> bool;
- `new_repo` -> bool.

Questa è la cache che attacca direttamente `-N`. Va invalidata quando cambia
l'entry VDB (mtime), il candidato repo (metadata hash), il fingerprint USE, o
il flag `newuse`/`changed_use`.

### 5.3 SlotOperatorView

- per ogni installato: provider e binding `:=` registrato;
- per ogni provider: consumer che lo nominano (reverse index);
- per ogni cp: slot disponibili nel repo e nell'installato.

Serve a `slot_operator_rebuild_scan`, `rev_dep`, `complete_graph`, e al
partizionamento `mrg`.

### 5.4 GraphIndex (runtime, non persistito)

Costruito una volta per passata dal grafo corrente:

- `cp -> Vec<entry_idx>`;
- `(cp, slot) -> Vec<entry_idx>`;
- `cpv -> entry_idx`;
- `atom -> candidate` (dove serve).

Sostituisce gli scan `entries.iter().find/any/position` nei punti elencati in
§2.3. È un prerequisito algoritmico, indipendente dalla cache.

---

## 6. L3 — Plan cache

### 6.1 StateFingerprint

Chiave esatta del piano. Deve coprire **tutto** ciò che cambia la risoluzione
o l'output. Meglio un falso miss che un falso hit.

Componenti:

1. **Versione di portuale**: `CARGO_PKG_VERSION` + revisione git di build, se
   disponibile.
2. **Argv canonico**: dopo `EMERGE_DEFAULT_OPTS`, con l'ordine preservato.
3. **Env di risoluzione** (whitelist:
   `ROOT`, `EROOT`, `SYSROOT`, `ESYSROOT`, `BROOT`, `PORTAGE_CONFIGROOT`,
   `PORTAGE_RUNNING_ROOT`, `DISTDIR`, `PKGDIR`, `PORTAGE_BINHOST`,
   `USE`, `USE_EXPAND`, `ACCEPT_KEYWORDS`, `ACCEPT_LICENSE`,
   `ACCEPT_PROPERTIES`, `ACCEPT_RESTRICT`, `FEATURES`,
   `EMERGE_DEFAULT_OPTS`, `PORTAGE_DEBUG`, `LANG`/`LC_*` per l'output).
   Escludere solo ciò che è dimostrabilmente irrilevante; in dubbio, includere.
4. **Config**: hash di tutti i file elencati in §4.4 + fingerprint
   `use_context_base`/`is_visible_base` se disponibili.
5. **World**: hash di `world`/`world_sets`.
6. **RepoSnapshot id**: per ogni repo, `(name, revision, schema)`.
7. **BinpkgSnapshot id**: per ogni sorgente, `(source, revision, schema)`.
8. **InstalledSnapshot id**: root + VDB revision (§4.3).
9. **Root/config_root** canonici.
10. **Modalità di output**: `--json`, `--tree`, `--verbose`, `COLUMNWIDTH`,
    `NOCOLOR`/`TERM`/`--color`, `--quiet`, `--ask` (quest'ultimo non
    cacheabile: interattivo).

Implementazione: hash SHA-256 (o BLAKE2) su una serializzazione canonica
(sorted keys, length-prefixed) e hex come nome file. `sha2`/`blake2` sono già
nel workspace; evitare `DefaultHasher` (non stabile tra versioni).

### 6.2 Due livelli di cache

**L3a — RenderedPlanCache (per `--pretend`)**

Valore: stdout, stderr, exit code della run, già renderizzati. È il modo più
semplice per garantire <1 s: si saltano resolve, merge order e display.

- Chiave: StateFingerprint + opzioni di display + `COLUMNWIDTH`.
- Non cacheare `--ask` (prompt interattivo) né azioni che mutano lo stato.
- Il contenuto deve essere deterministico: niente timestamp, PID, path
  temporanei, ordine di `HashMap` non ordinato.
- Se un warning di config dipende da un file non incluso nella chiave, è un
  bug della chiave: includerlo.

**L3b — GraphResultCache (per esecuzione e `mrg`)**

Valore: `GraphResult` serializzato + `ResolvePlan` necessario a eseguire.
Serve a `emerge` non-`--pretend` e al server `mrg`, che devono eseguire o
riordinare, non solo stampare.

- Serializzazione: formato binario versionato, oppure JSON compatto.
  `GraphResult` ha molti campi (`entries`, `outcome`, conflitti, autounmask,
  skipped, changed-deps, ABI rebuild, ecc.); va definito uno schema stabile
  con `schema_version`.
- Per `emerge`: il piano cachato deve produrre lo stesso output e lo stesso
  ordine; il rendering può essere rifatto o cachato in L3a.
- Per `mrg`: il server usa il `GraphResult` per calcolare
  `execution_order`/compartimenti e per eseguire; l'ordine può differire da
  Portage.

### 6.3 Precalcolo dei piani comuni

Dato che il tempo di build non è un problema, dopo sync/merge si calcolano in
anticipo i piani per un set configurabile:

```toml
# /etc/portuale/cache.conf (esempio)
[plans]
targets = [
  "@world -uDpvN",
  "@system -uDpvN",
  "@selected -uDpvN",
]
```

Il comando `portuale cache build --plans` esegue la stessa pipeline in
modalità `--pretend`, cattura l'output e scrive L3a/L3b. Per `mrg` server,
lo stesso set può essere calcolato all'avvio e dopo ogni sync/merge.

Questo è il meccanismo che permette il **primo run dopo un sync** sotto il
secondo: il sync avvia il build, il run trova il piano pronto.

### 6.4 Dopo un merge

Il VDB cambia, quindi lo StateFingerprint cambia e il piano scade. Il costo
cold è accettabile. Due opzioni, in ordine di semplicità:

1. **Nessun precalcolo**: il run successivo è cold, ma al termine scrive il
   nuovo piano; il run dopo è caldo.
2. **Warm breve**: dopo un merge riuscito, un processo breve
   (`portuale cache warm --plans`) ricalcola in background. Non è un demone:
   è un one-shot che termina. Accettabile perché il tempo macchina non è
   monitorato.

L'incrementalità vera (L4) è rimandata: prima si misura quanto costa il cold
dopo un merge.

---

## 7. L4 — Delta (rimandato)

Solo se il cold dopo merge è troppo lento. Idea:

- confrontare `InstalledSnapshot` vecchio/nuovo e `RepoSnapshot` vecchio/nuovo;
- calcolare la closure dei pacchetti cambiati via `reverse_index`;
- ricomputare solo quel sottografo, riusando le entry non toccate;
- ricomporre il merge order globale;
- verificare che il risultato sia **identico** al resolve completo su un
  corpus; per `emerge` usarlo solo se la parità è dimostrata, per `mrg`
  allentare il vincolo (basta la validità delle dipendenze).

Rischi: la risoluzione Portage è globale (backtracking, slot conflict,
autounmask); un delta può divergere. Va trattato come ottimizzazione
sperimentale, non come base del design.

---

## 8. Storage, formato, concorrenza

### 8.1 Layout

```
/var/cache/portuale/                  (override: PORTUALE_CACHE_DIR)
  schema                              # intero, schema globale
  repos/<repo-key>/<revision>.snap
  binpkgs/<source-key>/<revision>.snap
  installed/<root-key>/<revision>.snap
  config/<config-root-key>/<fingerprint>.snap
  plans/<state-fingerprint>.plan
  plans/<state-fingerprint>.out       # L3a: stdout/stderr/exit code
  locks/
  tmp/
```

`<repo-key>` = hash di `(name, canonical location)`. `<revision>` = hash della
revisione. I nomi sono content-addressed: una cache vecchia è innocua e può
essere GC-ata.

### 8.2 Formato

Raccomandazione: **file binario hand-rolled, immutabile**, con:

```
magic (8 byte) | schema (u32) | flags (u32) | header_len (u32)
header (JSON o binario length-prefixed)
indice (sorted, offset/length)
record (length-prefixed)
string table / atom table
```

Motivi:
- zero nuove dipendenze, stile del repo (gpkg/xpak/JSON hand-rolled);
- mmap o lettura integrale, nessun lock per i reader;
- deterministico, versionabile.

Alternativa: `redb` (pure Rust, già usato da `portage-vdb` come feature). Se
il formato hand-rolled diventa troppo codice, redb è il fallback naturale; il
build minimale può spegnere la feature e ricadere su md5-cache.

Da evitare: SQLite/rusqlite (C linkage), formati non versionati, JSON enorme
per i record.

### 8.3 Scrittura atomica e lock

- Ogni file: temp nella stessa directory + `rename` (come `write_atomic` del
  repo).
- Un lock per cache build (`locks/build.lock`, `flock`); i reader non
  prendono lock: vedono il file vecchio o quello nuovo.
- Schema mismatch o file corrotto: miss, non errore.
- Permessi: directory 0755, file 0644; se la cache è scrivibile da utenti non
  privilegiati, è un rischio di poisoning: la cache va sotto una directory
  root-owned o `PORTUALE_CACHE_DIR` fidato.

### 8.4 GC

- Tenere le ultime N revisioni per repo/binpkg e gli ultimi M piani.
- GC a fine `cache build`, non a ogni run.
- Non cancellare file in uso: rename/immutabilità rende sicuro.

---

## 9. S0 — Profiling (prima slice, nessun cambiamento di comportamento)

Obiettivo: capire dove vanno i 2m28s user **prima** di scegliere tra S1
(algoritmo) e S2 (snapshot). Da fare quando il terminale che implementa il
backlog si libera.

Strumenti:

```sh
perf record -g -- ./target/release/portuale emerge -uDpvN @world
perf report --stdio
/usr/bin/time -v ./target/release/portuale emerge -uDpvN @world
strace -c -f ./target/release/portuale emerge -uDpvN @world
```

Contatori dietro `PORTUALE_PROFILE=1` (temporanei, poi rimossi o dietro env):

- tempo per fase: config, expand targets, resolve fase 1, fase 2, merge order,
  display;
- numero di chiamate a `reinstall_flags_for_use_change`, `deps_changed`,
  `slot_changed`, `rebuilt_binary`, `new_repo`;
- numero di `entries.iter().find/any/position` per sito;
- `is_visible`, `effective_use_flags`, `repo_aux_metadata`, `list_candidates`,
  `all_installed_packages`;
- profondità e numero di entry del grafo, numero di passate/backtrack;
- tempo di `merge_order` e di `select_nodes`.

Output: `helpers/2026-10-XX-world-profile.md` con i top 10 costi e la
decisione S1/S2.

Criterio di uscita: sappiamo dire se il costo è (a) scan O(n²), (b) lavoro
per-package `-N`, (c) doppia passata, (d) display, (e) altro, con percentuali.

---

## 10. S1 — Prerequisiti algoritmici (indipendenti dalla cache)

Anche con la cache, il cold e il build della cache beneficiano di queste
correzioni. Sono deterministiche, non parallelismo.

1. **`GraphIndex`** per passata e sostituzione degli scan lineari nei siti di
   §2.3. Chiave `(cp, slot)` per non collassare gli slot.
2. **Memo `reinstall_flags_for_use_change`** (e `deps_changed`,
   `changed_slot`, `rebuilt_binary`, `new_repo`) per
   `(root, cpv, config use fingerprint, newuse)`.
3. **Riuso della fase 2**: se lo stato è invariato, la plan cache la salta;
   nel cold, valutare se la fase 2 può riusare il grafo della fase 1 invece
   di riattraversarlo.
4. **`required_set_reachable_cps` / world closure** calcolate una volta per
   stato, non per passata.
5. **`select_nodes` roots fallback**: mappa `order-index` invece di
   `position` dentro il sort.
6. **Display**: non ricalcolare `effective_use_flags`/USE string se già
   calcolati per l'entry; la cache L3a li salta del tutto.

Test: output byte-identico, `PORTUALE_SHUFFLE_DIRS`, L0. Misura: riduzione
del tempo `@world` prima/dopo, riportata nel commit.

---

## 11. S2 — Snapshot (Repo, Binpkg, Installed, World)

### 11.1 Comando

```
portuale cache build [--repo] [--binpkg] [--installed] [--world] [--plans]
                     [--force] [--jobs=N] [--load-average=LA]
```

- `--repo`: per ogni repo, legge `metadata/md5-cache`, valida, parsa, scrive
  `RepoSnapshot`.
- `--binpkg`: per ogni sorgente, legge `Packages` (locale/remoto), scrive
  `BinpkgSnapshot`.
- `--installed`: scansiona il VDB, scrive `InstalledSnapshot`.
- `--world`: espande `@world`/`@system`/`@selected`, scrive `WorldSnapshot`.
- `--plans`: calcola i piani configurati (§6.3).
- Parallelismo: `std::thread::scope` + join indicizzato, come
  `crates/portuale/src/regen.rs::run_parallel`; `--jobs`/`--load-average`
  riusano `SchedulerPolicy`.

### 11.2 Lettura

- `RepoSnapshot::open(repo, revision)`; se manca/mismatch, fallback al
  percorso md5-cache attuale.
- `list_candidates` e `repo_aux_metadata` guadagnano un fast path: leggono
  dal snapshot invece che da disco. Il percorso attuale resta come fallback.
- `InstalledSnapshot` alimenta `installed_candidates`, `all_installed_packages`,
  `installed_reverse_dependents`, `read_vdb_flag_set` (fast path).
- `BinpkgSnapshot` alimenta `local_binpkg`/`RemoteBinhostIndex`.

### 11.3 Verifica

- Per ogni fixture/entry: snapshot == parsing diretto di md5-cache (stessi
  campi, stessi atomi).
- Reverse index: per ogni versione, tutti i cp nominati sono presenti; un
  test di completezza su un campione.
- Corruzione/versione schema: fallback, nessun errore.
- Build parallelo: risultato identico al seriale.

---

## 12. S3 — Plan cache e precalcolo post-sync

### 12.1 Flusso `emerge`

```
emerge -uDpvN @world
  -> config (o ConfigSnapshot)
  -> StateFingerprint
  -> L3a hit?  sì -> stampa stdout/stderr/exit code, esci
               no -> L3b hit? sì -> renderizza
                     no -> resolve completo -> scrivi L3a/L3b -> stampa
```

Il fast path non deve toccare il repo: solo fingerprint, cache e VDB mtime.

### 12.2 Flusso `mrg` server

```
server startup / dopo sync / dopo merge
  -> costruisce L1/L2 in memoria
  -> precalcola i piani configurati (L3b)
client -> richiesta (argv, env, root, stato)
  -> server calcola/valida StateFingerprint
  -> hit: restituisce GraphResult
  -> miss: resolve, salva, restituisce
```

Multi-client: rimandato. Prima versione: un lock attorno a cache/resolve, o
una coda; nessuna promessa di concorrenza.

### 12.3 Trigger post-sync senza demone

Opzioni, in ordine di preferenza:

1. **Hook `postsync.d` / `repo.postsync.d`** (Portage li esegue in
   `portage/sync/controller.py::perform_post_sync_hook`): uno script
   `/etc/portage/postsync.d/portuale-cache` esegue
   `portuale cache build --repo --binpkg --installed --world --plans`.
2. **Wrapper di sync**: se portuale/mrg implementa un comando di sync, chiama
   `cache build` al termine.
3. **systemd timer** (oneshot) come fallback per chi non usa hook: non è un
   demone, ma richiede systemd; documentarlo come opzione.

Senza hook, il primo run dopo sync è cold; il documento deve dirlo
esplicitamente.

### 12.4 Criteri di accettazione

- Con hook eseguito: primo `emerge -uDpvN @world` dopo sync **≤ 1 s**.
- Run ripetuti **≤ 1 s**.
- Output byte-identico al resolve completo (stdout, stderr, exit code).
- `--json`, `--tree`, `-v`, `-q` coperti o esclusi dalla cache in modo
  esplicito.
- Mutazioni: toccare repo/binpkg/VDB/config/world/argv/env produce un miss;
  nessun falso hit nel corpus di test.

---

## 13. S4 — `mrg` server

- Il server tiene L1/L2 in memoria e L3b su disco/memoria.
- Dopo sync: ricostruisce solo gli snapshot cambiati (repo e binpkg
  indipendenti).
- Dopo merge: aggiorna `InstalledSnapshot` e invalida i piani che dipendono
  da quel root.
- Precalcola `@world`, `@system`, `@selected` e le richieste configurate.
- Restituisce `GraphResult`; il client può solo eseguire o mostrare.
- Ordine: il server può scegliere un `execution_order` topologico qualsiasi;
  deve esporre le dipendenze hard e rispettarle.
- Multi-client e isolamento: fuori scope per la prima versione; documentare
  il limite.

---

## 14. Correttezza: la regola d'oro

**Un falso hit è un bug; un falso miss è solo lentezza.**

Misure:

- La chiave include tutti gli input; se un input non è rappresentabile,
  non cachare quella richiesta.
- Test di sensibilità: per ogni componente della chiave, una mutazione deve
  produrre un miss. Un test per componente.
- Test di equivalenza: per un corpus di richieste, piano cachato == piano
  fresco, byte per byte (stdout/stderr/exit code) e `GraphResult` uguale.
- L0 differential: `emerge -pv` cached vs fresh vs Portage; byte-identico.
- Determinismo: `PORTUALE_SHUFFLE_DIRS`.
- Fallback: cache assente/corrotta/schema diverso/permessi negati -> percorso
  attuale, exit code invariato.
- `--no-cache` e `PORTUALE_CACHE_DIR` per debug e test.
- `--cache-stats` (o `PORTUALE_CACHE_DEBUG=1`) per hit/miss e motivo.

---

## 15. Parallelismo: dove ha senso e dove no

La cache toglie la maggior parte del lavoro; il parallelismo resta utile nel
**build della cache** e nel cold:

- **Build cache**: per repo, per categoria, per pacchetto `-N`, per
  candidato/visibilità. `std::thread::scope` + join indicizzato, niente
  `rayon`. Determinismo: ogni worker restituisce `(indice, risultato)`, il
  joiner ordina.
- **Cold resolve**: il resolver resta sequenziale. Un pool nel resolver è
  stato valutato e respinto in `parallelism.md` per il workload piccolo; per
  `@world` si può rivalutare **solo dopo** S1/S2, e solo se il profilo mostra
  un nucleo parallelizzabile. Le memo `Rc`/thread-local sono un ostacolo
  reale: un pool partirebbe con memo fredde.
- **Merge**: il merge copy loop è la slice di parallelismo consigliata da
  `parallelism.md` §3 (`--install-jobs`, due fasi scan/copy). Non è oggetto
  di questo documento, ma resta la priorità per (b)/(c) merge.
- **Mai**: merge concorrenti di pacchetti diversi sullo stesso `${ROOT}`
  (collision-protect, `CONTENTS`, `cfgfiledict`, preserve-libs).

---

## 16. HordeSat: esito dell'indagine

### 16.1 Cosa è

HordeSat (Balyo, Sanders, Sinz, SAT 2015, arXiv:1505.03340) è un **portfolio
SAT massivamente parallelo**:

- più solver CDCL (MiniSat, Lingeling) sulla **stessa** formula, diversificati
  per fasi/seedi/ordine delle clausole;
- parallelismo gerarchico: thread in shared memory + MPI tra nodi;
- **clause sharing** periodico (≈1 s) con `MPI_Allgather`, buffer fisso,
  Bloom filter per i duplicati, preferenza per clausole corte/LBD basso;
- scala su centinaia/migliaia di core, con speedup significativi sulle
  istanze difficili.

Successore: **MallobSat** (Schreiber & Sanders, JAIR 2024), che ripensa il
clause sharing (volume sublineare, exchange compatto, filtro esatto, backend
moderni), raddoppia lo speedup medio di HordeSat e scala fino a 3072 core.

### 16.2 Perché non è la prima strada per portuale

1. **Il problema non è un'istanza CNF difficile.** La risoluzione Gentoo è
   version solving con preferenze (versione più alta visibile, installato
   preferito, priorità repo, `||`, USE, slot, blocker, REQUIRED_USE). Il SAT
   puro restituisce **un** modello, non quello preferito da Portage; servirebbe
   MaxSAT/PB o un layer di ottimizzazione.
2. **I 2m28s user non sembrano search SAT.** Sembrano scan globali, `-N` per
   ogni installato, doppia passata, display. Un portfolio SAT non li tocca.
3. **Non deterministico.** Il clause sharing asincrono rende tempo e modello
   non riproducibili. Per `emerge` l'output deve essere byte-identico; per
   `mrg` l'ordine può cambiare, ma le scelte devono restare riproducibili per
   i test. Esistono solver paralleli deterministici (ManySAT 2.0 con barriere;
   ManyGlucose/DPS con delayed clause exchange a intervalli basati sui
   conflitti), ma pagano in prestazioni e richiedono una selezione canonica
   del modello.
4. **Non è embeddabile.** C++/MPI, non una libreria Rust; il goal musl-static
   rende MPI e C++ un costo architetturale.
5. **Il parallelismo utile è un altro.** HordeSat eccelle su **una** istanza
   difficile con molti core. Qui ci sono molte unità indipendenti
   (compartimenti, pacchetti, piani): conviene parallelizzare i **job** con
   solver sequenziali deterministici.

### 16.3 Alternativa pragmatica

- **Portfolio dei solver già in casa** per `mrg`: `BacktrackingResolver`
  (Portage), `PubGrubResolver`, `ResolvoResolver`, già dietro `Resolver` e
  `--solver=`. Eseguirli in parallelo con una regola di vittoria deterministica
  (per `emerge` vince sempre il piano Portage; per `mrg` vince il primo piano
  valido secondo un ordine fisso, non "chi finisce prima").
- **Clause sharing deterministico** solo se un caso reale resta search-bound:
  layer attorno a `resolvo` (CDCL) con intervalli a conflitti, barriere, seed
  fissi. Progetto di ricerca, non slice.
- **Condizione per riconsiderare HordeSat:** un caso in cui il tempo di
  ricerca superi stabilmente il 30–50% del totale **e** il modello preferito
  sia esprimibile come ottimizzazione. Finché il costo è scansione/ripetizione,
  HordeSat è la soluzione a un problema che non abbiamo.

---

## 17. Roadmap

| Slice | Contenuto | Accettazione | Dipendenze |
|---|---|---|---|
| **S0** | Profiling `@world`, contatori per fase/sito; report | Top 10 costi con percentuali; decisione S1/S2 | terminale libero |
| **S1** | `GraphIndex` + memo `-N`/`deps_changed` + fix `select_nodes`/closure | `@world` più veloce, output byte-identico, L0 | S0 |
| **S2** | `RepoSnapshot` + `BinpkgSnapshot` + `InstalledSnapshot` + `WorldSnapshot`; fast path lettura; fallback | snapshot == md5-cache; cold più veloce; nessun cambio output | S0/S1 |
| **S3** | `RenderedPlanCache` (L3a) + `GraphResultCache` (L3b) + `cache build --plans` + hook post-sync | primo run dopo sync ≤1 s (con hook); warm ≤1 s; byte-identico; test di sensibilità chiave | S2 |
| **S4** | `mrg` server: L1/L2 in memoria, precalcolo, `execution_order` | piano server ≤1 s; dipendenze hard rispettate; multi-client documentato come limite | S3 |
| **S5** | (Opzionale) delta/incremental dopo merge | delta == full su corpus; per emerge solo se parità dimostrata | S3/S4 |
| **S6** | (Opzionale) parallelismo cold/build: `-N` per package, visibilità, repo build | deterministico, speedup misurato | S2 |
| **S7** | (Condizionale) portfolio deterministico per `mrg`; HordeSat solo come riferimento | caso search-bound; piano valido e riproducibile | S6 |

Priorità: S0 → S1 → S2 → S3. S4 è il valore per `mrg`; S5–S7 sono
ottimizzazioni successive.

---

## 18. Rischi e decisioni aperte

### Rischi

- **Falso hit** per input non incluso nella chiave: piano sbagliato. Mitigare
  con chiave conservativa, test di sensibilità, `--no-cache`.
- **Revisione repo inaffidabile** (edit manuale senza timestamp/git): cache
  stale. Documentare l'assunzione; `cache build --force`.
- **Schema `GraphResult`** che cambia: versionare e invalidare.
- **Cache grande**: centinaia di MB accettabili, ma GC e tempi di build vanno
  misurati.
- **Concorrenza** tra processi: scritture atomiche, lock solo in build,
  reader lock-free.
- **Output dipendente dal terminale** (`COLUMNWIDTH`, colori, `--quiet`):
  includere nella chiave L3a o cachare solo `GraphResult`.
- **`--ask`**: non cachare; il prompt è interattivo.
- **Sicurezza**: cache scrivibile da utenti non privilegiati = poisoning;
  directory root-owned o `PORTUALE_CACHE_DIR` fidato.
- **VDB in-place**: un edit esterno con mtime invariato può produrre un falso
  hit; Portage usa `_bump_mtime`, portuale può validare `counter` e numero
  entry.

### Decisioni aperte

1. Formato: hand-rolled binario vs `redb` vs JSON compatto.
2. Cache root: `/var/cache/portuale` vs `$PORTAGE_TMPDIR/portuale-cache` vs
   configurabile.
3. L3a (output renderizzato) da subito, o prima L3b (`GraphResult`) e
   rendering a ogni run?
4. `ConfigSnapshot` serializzato: necessario per l'1s o basta il fingerprint?
5. Hook post-sync: `postsync.d` / `repo.postsync.d`, wrapper di sync, o timer oneshot?
6. Dopo merge: cold accettato o `cache warm` one-shot?
7. `mrg`: modello multi-client (lock globale vs coda vs shard) — rimandato.
8. GC: quante revisioni/piani tenere.

---

## 19. Appendice: mappa dei simboli utili

| Area | Simbolo / file |
|---|---|
| Entry point emerge | `crates/portuale/src/pretend/mod.rs::run` |
| Fasi resolve | `crates/portuale/src/pretend/resolve_phases.rs::run_resolve_phases` |
| Resolver trait | `crates/portage-repo/src/resolve/resolver.rs::Resolver` |
| Loop backtracking | `crates/portage-repo/src/resolve/driver.rs::backtracking_resolve` |
| Passata | `crates/portage-repo/src/resolve/pass/mod.rs::run_pass` |
| Contesto | `crates/portage-repo/src/resolve/ctx.rs::ResolveCtx` |
| Entry/grafo | `crates/portage-repo/src/resolve/entry.rs::GraphEntry` |
| Risultato | `crates/portage-repo/src/resolve/result.rs::GraphResult` |
| `-N` | `crates/portage-repo/src/reinstall.rs::reinstall_flags_for_use_change` |
| Preferenza `||` | `crates/portage-repo/src/resolve/or_preference.rs::disjunction_preference` |
| Merge order | `crates/portage-repo/src/merge_order/mod.rs::serialize_merge_order` |
| Digraph | `crates/portage-repo/src/merge_order/digraph.rs::Digraph` |
| Select | `crates/portage-repo/src/merge_order/select_nodes.rs::select_nodes` |
| Scheduler build | `crates/portuale/src/emerge_build/scheduler.rs::run_build_scheduler` |
| Merge copy | `crates/portuale/src/ebuild_merge/tree.rs::merge_tree` |
| Move atomico | `crates/portuale/src/ebuild_merge/protect.rs::replace_file_atomic` |
| Binpkg merge | `crates/portuale/src/ebuild_merge/from_binpkg.rs::merge_binpkg` |
| Estrazione binpkg | `crates/portuale/src/binpkg/extract.rs::extract_binpkg` |
| Md5-cache | `crates/portage-repo/src/metadata_cache.rs::read_md5_cache` |
| Candidati | `crates/portage-repo/src/candidates.rs::list_candidates` |
| Fingerprint config | `crates/portage-profile/src/fingerprints.rs` |
| `--regen` parallelo | `crates/portuale/src/regen.rs::run_parallel` |
| Direttore mrg | `crates/mrg-director/src/lib.rs::Director` |
| `mrg` applet | `crates/portuale/src/mrg.rs::run` |
| Doc parallelismo | `docs/cleanroom/parallelism.md` |
| Doc performance | `docs/performances-tuning.md` |
| Doc cache on-disk | `docs/on-disk-caches.md` |

---

## 20. Glossario

- **Preelaborazione / premastica**: trasformare i dati grezzi in una forma
  consumabile più rapidamente.
- **Snapshot**: file immutabile che rappresenta uno stato (repo, binpkg, VDB,
  config, world) a una revisione.
- **StateFingerprint**: hash di tutti gli input che determinano un piano.
- **Plan cache**: piano (`GraphResult` o output renderizzato) keyed da
  StateFingerprint.
- **Cold / warm**: senza / con piano in cache.
- **Hook post-sync**: script eseguito al termine del sync, prima che un umano
  lanci `emerge`.
- **L0–L4**: livelli della piramide (grezzo, snapshot, viste, piano, delta).

---

## 21. Non-goals

- Sostituire l'md5-cache o il formato `Packages` come formato di interscambio.
- Introdurre un demone sempre attivo per `emerge`.
- Parallelizzare il merge di pacchetti diversi sullo stesso `${ROOT}`.
- Cambiare le decisioni del resolver (slot conflict, backtracking,
  autounmask) o l'ordine di `emerge`.
- Adottare HordeSat/MallobSat come dipendenza.
- Ottenere 1 s per qualunque atomo a freddo.
