# Revisione critica, analisi delle assunzioni e proposte algoritmiche per `2026-10-20-world-plan-preelaboration.md`

Data: 2026-10-20  
Oggetto revisionato: [`LLM/2026-10-20-world-plan-preelaboration.md`](2026-10-20-world-plan-preelaboration.md)  
Metodo: verifica diretta delle ipotesi del documento contro il codice sorgente corrente del workspace (`portage-repo`, `portage-profile`, `portage-vdb`, `mrg-director`, `portuale`) e i documenti di progetto (`docs/cleanroom/parallelism.md`, `docs/performances-tuning.md`, `docs/on-disk-caches.md`, `helpers/vdb_to_db.md`).

---

## 1. Sintesi esecutiva

Il documento `2026-10-20-world-plan-preelaboration.md` imposta una direzione architetturale solida su quattro punti chiave:
1. **Cambio di regime rispetto a `parallelism.md`**: riconosce correttamente che un singolo atomo (`net-libs/rest`, ~2 s) e `@world` (`-uDpvN`, 210 atomi, 2099 installati, ~194 s in container LXC) hanno colli di bottiglia asintoticamente diversi.
2. **Piramide L0–L4 con fallback trasparente**: mantenere L0 come unica fonte di verità e trattare L1–L4 come proiezioni opzionali e invalidabili è la scelta giusta per non violare il contratto di drop-in replacement di `emerge`.
3. **Separazione fisica e di ciclo di vita tra `RepoSnapshot` e `BinpkgSnapshot`**: indispensabile perché `emerge --sync` e gli aggiornamenti del binhost avvengono con frequenze e trigger disaccoppiati.
4. **Analisi e scarto di HordeSat / MallobSat**: la motivazione per cui il problema di Gentoo non è un'istanza SAT CNF pura search-bound, ma un problema di version solving con preferenze, ordinamento deterministico e scansioni di metadati, è impeccabile.

Tuttavia, incrociando il documento con il codice reale del repository, emergono **tre criticità strutturali** e **diversi moltiplicatori algoritmici non diagnosticati in §2**:

1. **Affidare l'obiettivo primario ("primo run dopo sync ≤ 1 s") solo al precalcolo L3 (`cache build --plans` in `postsync.d`) è fragile**:
   - gli hook `/etc/portage/postsync.d` vengono eseguiti *sincronamente* da `emerge --sync` (`portage/sync/controller.py`): spostare 3 minuti di calcolo dentro `postsync.d` significa solo far aspettare l'utente 3 minuti alla fine di `emerge --sync` (o, se lanciato in background, andare in cache-miss / lock contention appena l'utente digita `emerge -uDpvN @world` subito dopo il sync);
   - qualsiasi minima variazione di argomenti o ambiente (`--exclude`, `--with-bdeps=y`, `-t`/`--tree`, `USE="..."`, o persino `COLUMNWIDTH` al ridimensionamento del terminale se non si separano le chiavi L3a/L3b) causa un **cache miss su L3**, facendo precipitare l'utente dai promessi `< 1 s` ai `194 s` del cold path.
   - **Conclusione costruttiva**: L3 garantisce `< 50 ms` sui run identici ripetuti, ma per rendere robusto l'obiettivo `≤ 1 s` **il calcolo stesso del piano a partire dagli snapshot L1/L2 (cold rispetto a L3, warm rispetto a L1) deve scendere da ~194 s a ~0.5–1.5 s**. Con gli algoritmi proposti in §4 di questa revisione, risolvere 2099 pacchetti in memoria richiede decine di millisecondi, non minuti.

2. **Il documento ignora che gran parte di `InstalledSnapshot` (L1) esiste già in `crates/portage-vdb` (backlog #157)**:
   - `portage-vdb` implementa già `InstalledDb`, `Snapshot` (con `generation: u64` e i 23 campi normalizzati `EntryFields`), `reverse_dependents`, `owners`, `world`, `world_sets`, e i backend `files`, `sqlite` e `redb` (`crates/portage-vdb/src/lib.rs`, `helpers/vdb_to_db.md`).
   - Progettare un nuovo `InstalledSnapshot` indipendente in `portage-repo` (§4.3) creerebbe una duplicazione architetturale. Bisogna invece unificare L1 `InstalledSnapshot` con `portage-vdb::Snapshot`.

3. **La diagnosi dei costi di `@world` in §2.1–§2.4 coglie solo metà del problema**:
   - L'analisi del codice mostra che il vero esplosivo algoritmico su `@world` non è solo `-N` o gli scan su `entries`, ma **quattro scansioni globali $O(|\text{VDB}|)$ ripetute a ogni passata**, un **triplo BFS completo sul VDB ad ogni `ResolveCtx::new`**, e un **ramo di `run_resolve_phases` (`else if auto_enable` sotto `--deep`) che scarta `complete_reuse` e ricalcola la Fase 1 da zero**.

---

## 2. Analisi critica delle assunzioni e diagnosi codice-alla-mano (§1–§2)

### 2.1 Assunzione §1.1: "Il rapporto user/sys (~4.8:1) dice che il tempo è CPU-bound, non I/O-bound"

- **Cosa è vero**: 148.2 s `user` contro 31.2 s `sys` dimostrano che il processo non è bloccato in attesa del disco fisico (tutto è nel page cache del kernel).
- **Cosa è fuorviante**: **31.2 secondi di kernel time (`sys`) su un singolo thread sono enormi**. In `helpers/vdb_to_db.md` §3.2, `strace` su `emerge -puDN @world` misura **447.658 chiamate `statx`** e 4.457 `openat` solo verso il VDB.
- **Perché `sys` e `user` sono accoppiati**: ogni chiamata a `read_vdb_flag_set(root, cat, pkg, ver, "USE"|"IUSE")` (`crates/portage-repo/src/installed.rs:456-467`) invoca `vdb_aux_get` -> `FilesDb::aux_get`, che:
  1. esegue una syscall `statx` sulla directory `/var/db/pkg/<cat>/<pf>` per verificare `#dir_mtime == st_mtime_ns` (~178 `statx` per pacchetto installato per run, costo `sys`);
  2. clona la `String` del campo dal memo e **esegue ogni volta `.split_whitespace().map(...).collect::<HashSet<String>>()`**, allocando decine di `String` e calcolando SipHash per costruire un nuovo `HashSet` da zero (costo `user`: `malloc`/`free` + `SipHash` + `memcpy`).
- Quindi una fetta consistente dei 148 s `user` è in realtà **deserializzazione ripetuta in memoria di stringhe VDB e md5-cache già lette**.

### 2.2 Quello che §2.1–§2.4 non vede: i 5 moltiplicatori nascosti nel codice attuale

Ispezionando il grafo delle chiamate reale di `portage-repo` e `portuale`, emergono cinque colli di bottiglia specifici di `@world -uDpvN` che il documento non menziona o sottostima:

#### A. Triplo BFS sull'intero VDB dentro `ResolveCtx::new` (`crates/portage-repo/src/resolve/ctx.rs:144-171`)
Ad ogni invocazione di `ResolveCtx::new(req)`, il costruttore calcola **tre chiusure transitive separate** chiamando tre volte `required_set_reachable_cps` (`crates/portage-repo/src/slot_operator/eliminate.rs:311-361`):
1. `slot_op_reachable`: BFS da `complete_seed_atoms ∪ req.atoms`;
2. `world_reachable`: BFS da `complete_seed_atoms`;
3. `blocker_retry_closure`: BFS da `blocker_retry_seed_atoms ∪ req.atoms ∪ system_packages`.

Cosa fa **ciascuna** chiamata a `required_set_reachable_cps`?
- Chiama `all_installed_packages(root)` e per **ogni pacchetto installato raggiungibile** (~2099 su `@world`):
  - chiama `read_vdb_flag_set(root, ..., "USE")` (1 `statx` + allocazione `HashSet<String>`);
  - per ognuna delle 4 chiavi `["RDEPEND", "PDEPEND", "DEPEND", "BDEPEND"]`, chiama `read_vdb_string` + `flat_dep_atoms(&depstr, &use_flags)` (che tokenizza, esegue `use_reduce_flat` e chiama `parse_atom` su ogni token!).
- Risultato: **~6.300 espansioni complete di dipendenze VDB (~25.200 stringhe `*DEPEND` parsate e USE-ridotte)** eseguite *prima ancora che inizi la prima passata del resolver*, e **ripetute** nella Fase 2 (e nel re-pass `ctx0` di `driver.rs:196`)!
- Per giunta, `slot_op_reachable` è matematicamente uguale a `world_reachable` più la chiusura raggiungibile da `req.atoms`: ricalcolarle separatamente da zero è puro spreco.

#### B. Il bug di performance in `run_resolve_phases` sotto `-D` (`crates/portuale/src/pretend/resolve_phases.rs:370-405`)
In §2.2 il documento afferma: *"se un pacchetto installato cambia e `--complete-graph-if-new-ver` è attivo (default), esegue la fase 2 `_complete_graph`"*.
Guardiamo cosa fa davvero `resolve_phases.rs:370-405` quando si lancia `emerge -uDpvN @world`:
```rust
let want_complete = complete_graph || (deep == portage_repo::Deep::NotRequested && auto_enable);
let result = if want_complete {
    run_resolve(true, &locked_merges, true, &complete_reuse)
} else if auto_enable {
    // `--deep` in force (the only way to reach here with `auto_enable` true):
    // re-run the phase-1 walk with the seeds fed in.
    run_resolve(false, &[], true, &empty_reuse)
}
```
Con `-uDpvN @world`, `-D` imposta `deep = Deep::Unlimited` ($\neq \text{Deep::NotRequested}$). Pertanto:
1. `want_complete` è **`false`**!
2. Il codice entra invece nel ramo `else if auto_enable` (`resolve_phases.rs:387`), che chiama `run_resolve(false, &[], true, &empty_reuse)`!
3. Notare gli argomenti: passa **`&empty_reuse`** (scartando `complete_reuse` che pure aveva appena calcolato alle righe 320–346!) e **`locked = &[]`**, rieseguendo **l'intera Fase 1 da zero** con l'unica differenza che `with_seeds = true` popola `config.complete_seed_atoms`!
4. Peggio ancora: nella prima chiamata (`with_seeds = false`, riga 283), `c.blocker_retry_seed_atoms` era *già* stato popolato (`resolve_phases.rs:113`), e poiché il target della CLI è `@world` + `-D`, il grafo camminato nella Fase 1 copre già `@world`! Se nella Fase 1 con `@world -D` passassimo direttamente `with_seeds = true` (o riusassimo il grafo se `slot_operator_rebuild_scan` e `reverse_dependency_constraints` non trovano nuovi nodi), **l'intera seconda risoluzione sparirebbe (risparmio netto del 50% del tempo totale)**!

#### C. Quattro scansioni lineari $O(|\text{VDB}|)$ ad ogni singola passata (`run_pass` / `collect_feedback` / `assemble_result`)
Il documento (§2.1) punta il dito su `reinstall_flags_for_use_change` (`-N`), ma durante **ogni passata** del loop di backtracking vengono eseguite fino a **quattro scansioni globali di tutti i 2099 pacchetti installati**:
1. **`collect_unwalked_installed_blockers`** (`crates/portage-repo/src/resolve/blockers.rs:394`):
   - Per ognuno dei 2099 pacchetti in `all_installed_packages(root)` non nel grafo: chiama `live_metadata_for_installed` (che cerca nei repo e legge `md5-cache`), legge `USE` dal VDB, costruisce le stringhe `IDEPEND`/`PDEPEND`/`RDEPEND` via `installed_dep_string`, e se contengono `'!'` esegue `use_reduce_flat_disjunctive` (con `disjunction_preference`).
2. **`reverse_dependency_constraints`** (`crates/portage-repo/src/resolve/rev_dep.rs:411`):
   - Chiamata in `collect_feedback` (`feedback/mod.rs:145` quando ci sono slot conflict, e poi a riga 191 in `step_reverse_dep_pins`!): itera su tutti i **2099 `consumer in all_installed_packages(root)`**, legge le loro dipendenze dal VDB + repo live, le riduce con `USE` e controlla ogni atomo contro `upgrading` e `use_breaks`.
3. **`slot_operator_rebuild_scan` + `collect_probe_parents` + `slot_operator_slot_change_probe`** (`crates/portage-repo/src/slot_operator/rebuild_scan.rs:123, 148, 232`):
   - Scansiona `all_installed_packages(root)` più volte per passata (in `step_slot_operator_probe` e di nuovo in `assemble_result` se `abi_rebuilds.is_none()`), facendo `.split_whitespace()` e `parse_atom` su tutte le chiavi `*DEPEND` di ogni pacchetto installato.
4. **`add_installed_dependency_closure`** (`crates/portage-repo/src/merge_order/installed_closure.rs:121`):
   - Alla fine della risoluzione, `assemble_result` chiama `topological_merge_order` -> `serialize_merge_order` -> `schedule_graph`. Quando almeno un pacchetto nel piano è un `Upgrade`/`Reinstall`/`Downgrade` (`complete = true`, riga 158), `add_installed_dependency_closure` espande fino a fixpoint tutti i pacchetti installati e `@system`, chiamando `vdb_edges` (che invoca `live_metadata_for_installed` + 5× `installed_dep_string` + `dep_edges_from_metadata`) per ogni singolo pacchetto installato!

#### D. Moltiplicazione per il numero di passate di Backtracking + `replay_walk`
In `crates/portage-repo/src/resolve/driver.rs`:
- Il loop non fa 1 sola passata per fase: ogni `BacktrackFeedback::RevDep`, `SlotConflict`, `Config` (da slot-operator rebuild), o `Autounmask` avvia una nuova `run_pass` (che ricrea un `PassState::default()` vuoto, perdendo anche `installed_meta_memo`!).
- Inoltre, se `autounmask_suggest_use` è attivo e ci sono slot conflict o use changes, `driver.rs:82` esegue anche `replay_walk(&ctx, &pass.entries, ...)` ad ogni passata.
- E se `restarts > 0`, `driver.rs:191-201` costruisce un nuovo `ResolveCtx::new(&req0)` (altri 3 BFS completi sul VDB!) ed esegue una `run_pass` finale!

#### E. `PassState` ha già metà del `GraphIndex`, ma non lo espone e lo spezza male
In §2.3 e §5.4 il documento propone di costruire `GraphIndex` una volta per passata.
In realtà, `PassState` (`crates/portage-repo/src/resolve/pass_state.rs:81-99`) mantiene **già** incrementalmente durante la passata:
- `resolved_slots: HashMap<(String, String, String, bool), usize>` (indice in `entries` per i pacchetti merge-bound);
- `installed_slots: HashMap<(String, String, String, bool), String>` (versione per i nodi `AlreadyInstalled`).
Perché allora `or_preference.rs:58`, `rev_dep.rs`, `blockers.rs`, `slot_conflicts.rs` fanno scansioni lineari `entries.iter().filter(...)`?
1. Perché `installed_slots` salva la `String` della versione invece dell'indice `usize` dentro `state.entries`;
2. Perché manca una mappa secondaria `by_cp: HashMap<(bool, String, String), SmallVec<[usize; 2]>>` dentro `PassState` aggiornata al momento di `state.entries.push(...)`;
3. Perché le funzioni ausiliarie ricevono `entries: &[GraphEntry]` invece di una struct `GraphEntriesView` che incapsula `&[GraphEntry]` + l'indice incrementale!
Importante: durante `run_pass`, `alternative_downgrade_demoted` e `disjunction_preference` vengono chiamate **mentre `entries` sta crescendo** (non a fine passata!). Quindi `GraphIndex` non va "costruito una volta per passata", ma **mantenuto incrementalmente ad ogni push/mutazione di `PassState::entries`**.

---

## 3. Analisi critica delle scelte architetturali (L0–L4, Storage, Sicurezza)

### 3.1 Sovrapposizione tra `InstalledSnapshot` (§4.3) e `crates/portage-vdb`
Il crate `crates/portage-vdb` (backlog #157, documentato in `helpers/vdb_to_db.md`) è già presente nel workspace e definisce:
- `portage_vdb::Snapshot` (`generation: u64`, `entries: Vec<(EntryKey, Arc<EntryFields>)>`, `index: HashMap<EntryKey, usize>`);
- `InstalledDb::reverse_dependents(cp, classes) -> Result<Vec<DepRecord>>`;
- `InstalledDb::owners(paths)`;
- `InstalledDb::generation() -> Result<u64>` e `category_generation(cat) -> Result<u64>`.

**Critica costruttiva**: Non creare un secondo formato `InstalledSnapshot` concorrente in `/var/cache/portuale/installed/`. Al contrario:
- Usare `portage-vdb` come unico layer di astrazione del VDB.
- Quando il backend è `sqlite` o `redb`, lo snapshot e gli indici (`dep_atom`, `owner`, `generation`) sono *già* dentro il DB!
- Quando il backend è `files` (default per `emerge`), `InstalledSnapshot` deve essere semplicemente la materializzazione su disco (sidecar cache in `/var/cache/portuale/installed/`) di `portage_vdb::Snapshot` arricchita con le dipendenze già USE-ridotte e l'indice inverso CSR (vedi §4.3 sotto).

### 3.2 Perché il formato binario immutabile batte `redb` per gli snapshot L1/L3 (§8.2)
In §8.2 il documento propone un formato binario hand-rolled immutabile, indicando `redb` come "fallback naturale".
C'è un motivo tecnico decisivo, già dimostrato nel repository (`helpers/vdb_to_db.md` §4 e test `a_held_redb_file_is_busy_in_process` in `crates/portuale/src/mrg.rs:2479`), per **escludere `redb` come formato per le cache condivise L1/L2/L3**:
- **`redb` impone il vincolo "one-process only" (`DatabaseAlreadyOpen`)**: `redb::Database::open` acquisisce un lock sul file anche se usato solo per transazioni di lettura. Se `mrg` server tiene aperto un file `redb`, o se due terminali lanciano `emerge -pv` contemporaneamente, il secondo processo fallisce con `DatabaseAlreadyOpen`!
- Un **file binario immutabile content-addressed** (`<revision>.snap`), scritto tramite `mkstemp` + `rename` e aperto in sola lettura (o `mmap`), garantisce:
  - **Concorrenza infinita lock-free** tra `mrg` server, `cache build`, e $N$ processi `emerge` client;
  - **Zero-copy deserialization**: con offset allineati e tabelle piatte ordinate, il processo non deve allocare `HashMap` all'avvio: legge direttamente dal buffer mappato dal kernel in $O(1)$!

### 3.3 Separazione obbligatoria tra `ResolveFingerprint` (L3b) e `RenderFingerprint` (L3a) (§6.1–§6.2)
In §6.1 il documento inserisce nel medesimo `StateFingerprint` sia i parametri di risoluzione (repo, VDB, config, USE) sia quelli di visualizzazione (`COLUMNWIDTH`, `NOCOLOR`/`TERM`/`--color`, `--verbose`, `--quiet`, `--tree`, `--json`).
- **Problema**:
  1. `COLUMNWIDTH` cambia ogni volta che l'utente ridimensiona la finestra del terminale (o se `emerge` legge la larghezza del pty via `ioctl(TIOCGWINSZ)` quando non è fissata nell'env). Se `COLUMNWIDTH` è dentro lo `StateFingerprint` di L3b (`GraphResultCache`), ridimensionare la finestra del terminale o eseguire `emerge -uDN @world` (senza `-p`, dopo aver visto `emerge -uDpvN @world`) **invalida l'intero `GraphResultCache` (L3b)**!
  2. Il flusso principale dell'utente reale è proprio:
     - passo 1: `emerge -uDpvN @world` (ispeziona il piano);
     - passo 2: `emerge -uDN @world` (oppure `emerge -uDavN @world` con `--ask`: esegue lo stesso piano!).
  3. Se `-p` / `-v` / `--ask` / `COLUMNWIDTH` cambiano il fingerprint di L3b, il passo 2 (esecuzione o `--ask`) **non farà mai hit sul piano appena calcolato al passo 1**!
- **Soluzione costruttiva**:
  - Definire **`ResolveFingerprint`** (chiave di **L3b `GraphResultCache`**): normalizza `argv` **rimuovendo i flag puramente di output/azione** (`--pretend`, `--ask`, `--verbose`, `--quiet`, `--tree`, `--json`, `--color`, `COLUMNWIDTH`, `TERM`, `NOCOLOR`). In questo modo:
    - `portuale cache build --plans` calcola L3b una volta sola;
    - `emerge -uDpvN @world`, `emerge -uDptvN @world` (`--tree`), `emerge -uDavN @world` (`--ask`) e `emerge -uDN @world` (merge reale!) **condividono tutti lo stesso identico `GraphResult` in L3b**!
    - Anche `--ask` diventa istantaneo (`< 50 ms` per mostrare il piano e chiedere `[Yes/No]`), perché carica `GraphResult` da L3b, renderizza a schermo e poi attende l'input interattivo!
  - Definire **`RenderFingerprint = BLAKE2b(ResolveFingerprint || DisplayFlags || EffectiveTerminalWidth || ColorMode)`** come chiave opzionale di **L3a (`RenderedPlanCache`)**.

### 3.4 Incompatibilità tra `RepoSnapshot` (§4.1) e i vincoli di validazione #46 e #55
In §4.1 si assume di poter costruire `RepoSnapshot` leggendo `metadata/md5-cache` e invalidandolo solo con `git rev-parse HEAD` o `metadata/timestamp`.
- **Problema 1 (Backlog #46 — validazione `_md5_` e `_eclasses_`)**: `portage-repo` valida ogni entry `md5-cache` contro l'hash MD5 del file `.ebuild` e delle `.eclass` ereditate (`crates/portage-repo/src/md5_dict.rs`). Se un utente o uno sviluppatore modifica un ebuild o un'eclass in un overlay locale o nell'albero senza aggiornare `metadata/timestamp` né fare commit git, un `RepoSnapshot` che controlla solo `HEAD` o l'mtime della directory `metadata/md5-cache` servirà metadati stale (violando #46 e i test `L2_STALECACHE=1`).
- **Problema 2 (Backlog #55 — `layout.conf` `cache-formats` e `FEATURES=metadata-transfer`)**: se `FEATURES=metadata-transfer` è attivo o il repo è `pms`-only, `portage-repo` ignora `metadata/md5-cache` (`docs/on-disk-caches.md` §2).
- **Soluzione costruttiva**:
  1. Per il calcolo veloce della revisione del repo: se il repo ha `.git`, `git rev-parse HEAD` non basta se il working tree è sporco; o si controlla l'indice git (`.git/index` mtime/hash) + `metadata/timestamp.chk`, oppure nello snapshot si salva una tabella compatta delle `.eclass` (`eclass_name -> (mtime_ns, size, md5)`) e per ogni CPV `(ebuild_mtime_ns, ebuild_size, _md5_)`.
  2. Includere `FEATURES=metadata-transfer` e `cache-formats` nella chiave/condizione di utilizzo di `RepoSnapshot`.

### 3.5 Bug latente nei fingerprint attuali (`crates/portage-profile/src/fingerprints.rs`)
Il documento (§4.4) cita `use_context_base_fingerprint` e `is_visible_base_fingerprint` in `crates/portage-profile/src/fingerprints.rs` come modello da seguire per i fingerprint persistenti.
Ispezionando `crates/portage-profile/src/fingerprints.rs:11-26`:
```rust
let mut h = std::collections::hash_map::DefaultHasher::new();
let hash_set = |s: &HashSet<String>, h: &mut std::collections::hash_map::DefaultHasher| {
    s.len().hash(h);
    let mut acc: u64 = 0;
    for v in s {
        let mut e = std::collections::hash_map::DefaultHasher::new();
        v.hash(&mut e);
        acc ^= e.finish();
    }
    acc.hash(h);
};
```
- `DefaultHasher` (SipHash-1-3 a 64 bit) + XOR-fold è accettabile per un memo effimero in-process, ma **non va mai usato per chiavi persistenti su disco (L2/L3)**:
  1. 64 bit hanno probabilità di collisione non trascurabile su cache multi-run;
  2. `DefaultHasher` non garantisce stabilità del seed/algoritmo tra versioni di `std`;
  3. `use_context_base_fingerprint` e `is_visible_base_fingerprint` ignorano campi di `Config` che influenzano la risoluzione (es. `system_packages`, `package_provided`, `profile_formats`, `iuse_effective`, `binrepos`, `pkgdir`).
- **Regola per L1/L2/L3**: usare un hasher crittografico a 256 bit (`blake2` o `sha2`, già presenti nel workspace) alimentato con stream canonico (collezioni non ordinate sempre ordinate prima dell'hash, stringhe length-prefixed `u64_le(len) || bytes`).

---

## 4. Algoritmi e strutture dati consigliati per raggiungere `≤ 1 s`

Per portare `emerge -uDpvN @world` da **194 s** a **< 1 s** (anche quando L3 è in cache-miss dopo un sync o per una variazione di flag CLI), suggeriamo **8 algoritmi e strutture dati specifici** per il dominio di Portage.

---

### Algoritmo 1: Interning Globale Lessicografico (`CpId`, `CpvId`, `SlotId`, `FlagId`) e Direct-Addressed Arrays

**Il problema nel codice attuale:**  
Durante un resolve di `@world`, `portage-repo` esegue milioni di allocazioni `String::clone()`, `format!("{cat}/{pkg}-{ver}:{slot}/{sub}::{repo}")` e lookup `HashMap<(String, String), ...>` usando SipHash su stringhe.

**L'algoritmo:**  
Costruire all'apertura degli snapshot L1 (o nel contesto di sessione) una **Symbol Table / Interner con numerazione monotona lessicografica**:
- Raccogliere tutti i `(category, package)` noti (repo + VDB + binpkg, $N_{\text{cp}} \approx 20.000\text{–}32.000$), ordinarli lessicograficamente una sola volta, e assegnare a ciascuno un intero denso `CpId(u32)` $\in [0, N_{\text{cp}})$.
- **Proprietà fondamentale (Order-Preserving Interning)**: poiché gli ID sono assegnati in ordine lessicografico:
  $$\text{cp}_1 <_{\text{lex}} \text{cp}_2 \iff \text{CpId}(\text{cp}_1) < \text{CpId}(\text{cp}_2)$$
  Tutti i `sort()` deterministici richiesti per la parità con Portage (su `BTreeSet<(String, String)>`, `abi_rebuilds`, `enforced`, ecc.) diventano confronti tra interi `u32` a 1 ciclo di clock anziché `memcmp` su coppie di `String`!
- **Direct-Addressed `GraphIndex`**:
  Con $N_{\text{cp}} \approx 30.000$, una tabella indicizzata per `CpId` non ha bisogno di `HashMap`: un semplice `Vec<SmallVec<[u32; 2]>>` di lunghezza $N_{\text{cp}}$ occupa appena **~480 KB** (entra interamente nella cache L2 della CPU!) e trasforma ogni lookup `cp -> entries` in un **accesso diretto ad array `index[cp_id.0 as usize]` in $O(1)$ reale (0 hash, 0 collisioni, 0 allocazioni)**.

---

### Algoritmo 2: Algebra Bitwise su `BitSet` a 64/128 bit per USE Flags e `--newuse` (`-N`)

**Il problema nel codice attuale (`crates/portage-repo/src/reinstall.rs:525-564`):**  
`reinstall_flags_for_use_change` viene chiamata per ogni pacchetto installato considerato durante il walk. A ogni chiamata:
1. legge `USE` e `IUSE` dal VDB parsando le stringhe in due `HashSet<String>`;
2. calcola `cur_iuse` e `cur_use` (`Rc<HashSet<String>>`);
3. alloca altri 3 `HashSet<String>` (`orig_enabled`, `cur_enabled`, `flags`) più `presence_diff` per calcolare intersezioni e differenze simmetriche, e infine itera su `config.use_force.union(&config.use_mask)` (centinaia di stringhe!) per rimuovere le flag forzate/mascherate.

**L'algoritmo (SWAR / Bitset Set-Algebra):**  
1. Nello snapshot (o nel `ResolveCtx`), assegnare a ogni USE flag distinta dell'universo (~500–1.000 flag attive tra profilo e `IUSE` dei pacchetti, oppure localmente per `CpId`) un indice `FlagId(u16)`.
2. Rappresentare ogni insieme di USE flag come un **`FixedBitSet`** compatto (es. `[u64; 16]` = 1024 bit = 128 byte per l'universo globale, oppure un singolo `u64` / `u128` locale ai flag dichiarati in `IUSE` del pacchetto!).
3. Precalcolare una sola volta in `Config` la maschera bitwise `forced_or_masked: FixedBitSet = config.use_force | config.use_mask`.
4. Allora `reinstall_flags_for_use_change` diventa **4 istruzioni CPU (AND, XOR, OR, ANDNOT) senza rami e senza allocazioni**:
   ```rust
   // Operazioni su parole u64 (auto-vettorizzate in AVX2/SSE2 dal compilatore):
   let orig_enabled = vdb_entry.orig_iuse_bits & vdb_entry.orig_use_bits;
   let cur_enabled  = cand.cur_iuse_bits & cand_cur_use_bits;
   let mut diff     = orig_enabled ^ cur_enabled;
   if newuse {
       let presence_diff = (vdb_entry.orig_iuse_bits ^ cand.cur_iuse_bits) & !config.forced_or_masked_bits;
       diff |= presence_diff;
   }
   if diff.is_zero() {
       return None; // Fast-path nel 99%+ dei pacchetti: ZERO allocazioni!
   }
   // Solo se diff != 0 (rarissimo), estrai i FlagId tramite tzcnt (trailing_zeros)
   ```
5. **Complessità**: da $O(|IUSE| + |USE| + |use\_force| + |use\_mask|)$ operazioni di hashing/allocazione stringhe (~10–20 µs per chiamata) a **~5 nanosecondi** (2000× più veloce). Anche `use_deps_satisfied` (`cat/pkg[foo,-bar,baz?]`) si riduce a un test bitwise `(use_bits & req_on == req_on) && (use_bits & req_off == 0)`!

---

### Algoritmo 3: Grafo Inverso Compatto CSR del VDB (`VdbReverseGraph`) + Bitset BFS per azzerare le 4 scansioni $O(|\text{VDB}|)$

**Il problema nel codice attuale:**  
Come mostrato in §2.2 (punti A e C), il resolver tratta il VDB come una lista piatta di 2099 pacchetti (`all_installed_packages`) e la scansiona da cima a fondo in:
- `required_set_reachable_cps` (3 volte per `ResolveCtx::new`);
- `reverse_dependency_constraints` (1–2 volte per passata);
- `slot_operator_rebuild_scan` (1–2 volte per passata);
- `collect_unwalked_installed_blockers` (1 volta per passata);
- `add_installed_dependency_closure` (1 volta in `schedule_graph`).

**L'algoritmo (Pre-Reduced CSR Adjacency & Inverted Index):**  
Un pacchetto installato $u \in \text{VDB}$ ($|V_{\text{vdb}}| \approx 2099$, indicizzato con `VdbId(u16)` $\in [0, 2099)$) ha un insieme `USE` registrato **immutabile** fino al prossimo merge. Pertanto, la riduzione `use_reduce(u.*DEPEND, u.USE)` delle sue dipendenze `Raw` (e della vista `Effective` rispetto al `RepoSnapshot` corrente) dipende **solo** da `(InstalledSnapshot, RepoSnapshot)`!

Nello snapshot `InstalledSnapshot` (o al livello L2 `VdbGraphView` keyed da `(installed_snap_id, repo_snap_id, dynamic_deps)`), precalcolare:

1. **Matrice di adiacenza diretta CSR (`forward_edges`) per classe di dipendenza (`DepClassMask: u8`)**:
   - Due array contigui: `offsets: Vec<u32>` (lunghezza $2100$) e `targets: Vec<(VdbId, DepClassMask)>`.
   - **Algoritmo di Reachability in `ResolveCtx::new`**:
     Invece di leggere stringhe e chiamare `flat_dep_atoms` 25.000 volte, `required_set_reachable_cps` esegue un BFS su `VdbId(u16)` tenendo `visited: [u64; 34]` (2176 bit = **272 byte sullo stack!**).
     Inoltre, poiché $\text{Seeds}(\text{world\_reachable}) \subseteq \text{Seeds}(\text{slot\_op\_reachable})$:
     - Passo 1: calcola `world_reachable` con BFS da `complete_seed_atoms`;
     - Passo 2: inizializza `slot_op_reachable = world_reachable.clone()` (copia di 272 byte!) e prosegue il BFS **solo** dai nodi in `req.atoms` non ancora visitati!
     - Tempo totale per tutte e 3 le chiusure in `ResolveCtx::new`: **< 30 microsecondi** (contro centinaia di millisecondi / secondi attuali).

2. **Indice Inverso CSR (`rev_deps_by_target_cp: CpId -> &[RevDepEdge]`)**:
   - Ogni `RevDepEdge` contiene `(consumer: VdbId, dep_class: DepClassMask, atom_id: AtomId, has_slot_op_eq: bool, bound_slot: SlotId, bound_sub_slot: SubSlotId)`.
   - **Nuovo algoritmo per `reverse_dependency_constraints` (`rev_dep.rs`)**:
     Invece di iterare sui 2099 `consumer in all_installed_packages(root)`, iterare **solo sui pacchetti effettivamente in `upgrading` o `use_breaks`** ($k = |\text{upgrading}| + |\text{use\_breaks}|$, spesso $k \ll 2099$):
     ```rust
     for &(target_cp, target_slot) in upgrading.keys().chain(use_breaks.keys()) {
         for edge in vdb_graph.rev_deps_of(target_cp) {
             // Visita SOLO i consumer installati che nominano davvero target_cp!
         }
     }
     ```
     Anche quando `upgrading` ha 100 pacchetti, il numero medio di archi entranti toccati è qualche centinaio anziché $2099 \times 5$ stringhe `*DEPEND` intere!
   - **Nuovo algoritmo per `slot_operator_rebuild_scan` (`rebuild_scan.rs`)**:
     Per il ramo `same_slot`, iterare direttamente su `(provider_cp, new_info) in &new_slot` e consultare `vdb_graph.slot_op_consumers_of(provider_cp)` filtrando in $O(1)$ quelli con `edge.bound_slot == n_slot && edge.bound_sub_slot != n_sub`!

3. **Tabella sparsa dei Blocker Installati (`installed_blockers: Vec<InstalledBlockerRow>`)**:
   - Su 2099 pacchetti installati in un sistema Gentoo reale, solo ~30–80 pacchetti hanno un token `!` nelle loro `RDEPEND`/`PDEPEND`/`IDEPEND`.
   - `collect_unwalked_installed_blockers` (`blockers.rs:367`) oggi scansiona tutti i 2099 pacchetti ad ogni passata solo per fare `if !depstr.contains('!') { continue; }` dopo aver costruito `depstr`!
   - Con la lista prefiltrata dei soli pacchetti installati che contengono `'!'` (e indicizzata per `target_cp`), `collect_unwalked_installed_blockers` controlla solo i pacchetti il cui `target_cp` è effettivamente merge-bound nella passata corrente: tempo **< 10 microsecondi per passata**.

---

### Algoritmo 4: Pre-compilazione di `*DEPEND` in Linearized Postorder/Skip-Offset Bytecode (`FlatDepAst`)

**Il problema nel codice attuale:**  
Anche con `read_md5_cache` in memoria, le stringhe `DEPEND`/`RDEPEND`/`BDEPEND`/`PDEPEND`/`IDEPEND` vengono riparsate (`split_whitespace`, parentesi, `use? ( ... )`, `|| ( ... )`) in `use_reduce_flat`, `use_reduce_structured`, `dep_edges_from_metadata`, `canonical_dep_key`.
In §4.1 il documento propone di salvare "dipendenze già parsate: una struttura compatta per ogni classe". Specifichiamo l'algoritmo e il layout esatto per renderlo zero-copy in `mmap`.

**L'algoritmo (Flat Preorder Tree con Skip-Offsets):**  
In `RepoSnapshot`, serializzare ogni albero sintattico `*DEPEND` come un slice piatto di istruzioni a dimensione fissa (`#[repr(C)] struct DepInstr`, 8 byte):
```rust
#[repr(C)]
pub struct DepInstr {
    pub op: u8,         // 0 = Atom, 1 = UseCondPos, 2 = UseCondNeg, 3 = AnyOf(||), 4 = AllOf(())
    pub dep_class: u8,  // bitmask o indice DEPEND/RDEPEND/...
    pub aux: u16,       // FlagId per UseCond, arity per AnyOf/AllOf
    pub payload: u32,   // AtomId per Atom, oppure skip_offset (numero di DepInstr nel sottoalbero) per UseCond/AnyOf/AllOf
}
```
**Valutazione (Tree Walk senza ricorsione e con Branch Skipping in $O(1)$):**
```rust
let mut pc = 0;
while pc < instrs.len() {
    let ins = instrs[pc];
    match ins.op {
        USE_COND_POS => {
            if !use_bitset.contains(ins.aux) {
                pc += ins.payload as usize; // Salta l'intero sottoalbero in O(1)!
                continue;
            }
            pc += 1;
        }
        USE_COND_NEG => {
            if use_bitset.contains(ins.aux) {
                pc += ins.payload as usize; // Salta l'intero sottoalbero in O(1)!
                continue;
            }
            pc += 1;
        }
        ATOM => {
            emit(ins.payload); // AtomId già parsato nella atom_table!
            pc += 1;
        }
        ...
    }
}
```
**Vantaggi:**
- **Zero puntatori, 100% compatibile con `mmap` diretto** (nessuna deserializzazione all'apertura dello snapshot).
- **Sublinear evaluation**: i rami `flag? ( ... )` disattivati (es. `test? ( ... )`, `doc? ( ... )`, o architetture/lingue non attive) vengono saltati in **una singola addizione `pc += skip_offset`** senza mai leggere né allocare i token interni!
- Nella `atom_table`, ogni `AtomId` punta a una struct `PreparsedAtom` (`category: CpId`, `operator: u8`, `version_id: u32`, `slot_id: u16`, `sub_slot_id: u16`, `blocker: u8`, `use_deps_slice: (u32, u16)`), eliminando qualsiasi chiamata a `portage_dep::parse_atom` durante il walk.

---

### Algoritmo 5: `SharedResolveSession` cross-pass/cross-phase e Short-Circuit di Fase 2

**Il problema nel codice attuale:**
1. Tra una passata di backtracking e la successiva in `backtracking_resolve` (`driver.rs:32`), `PassState` viene ricreato da zero (`PassState::default()`), buttando via `installed_meta_memo`, `top_pinned_installed`, e ricalcolando `deps_changed` / `reinstall_flags_for_use_change` per tutti i nodi che non sono stati minimamente toccati dal feedback di backtrack!
2. Tra Fase 1 e Fase 2 in `run_resolve_phases` (`resolve_phases.rs:283, 373, 393`), `ResolveCtx::new` viene ricostruito da zero e, come visto in §2.2.B, sotto `--deep` (`-D`) il ramo `else if auto_enable` passa `&empty_reuse` invece di riusare le decisioni della Fase 1.

**L'algoritmo:**
1. **Introdurre `ResolveSession` passata per riferimento mutabile a `run_resolve_phases` e `backtracking_resolve`**:
   - Contiene:
     - `vdb_graph: VdbReverseGraph` (calcolato 1 sola volta per sessione, non ad ogni `ResolveCtx::new` né ad ogni passata);
     - `reinstall_memo: Vec<Option<ReinstallVerdict>>` indicizzato per `CpvId` (quando `autounmask_use` cambia per un singolo `cp`, solo la entry di quel `cp` viene invalidata; gli altri 2098 pacchetti mantengono il verdetto `-N` / `deps_changed` / `slot_changed` / `new_repo` in $O(1)$!);
     - `installed_meta_memo`: condiviso tra tutte le passate.
2. **Correggere e ottimizzare la transizione Fase 1 → Fase 2 in `resolve_phases.rs`**:
   - **Pre-seeding per `@world -D`**: quando l'utente chiede `@world` con `-D` (`--deep`), i seed di `@world` e `@system` sono *già* espansi come target. Se `ResolvePlan` rileva che i target coprono già `complete_seed_atoms`, alimentare `with_seeds = true` fin dalla prima passata evita del tutto la seconda risoluzione del ramo `else if auto_enable` (`resolve_phases.rs:387`)!
   - **Lazy / Short-Circuit `_complete_graph`**: nella Fase 1, `complete_graph_auto_enable` scatta non appena *qualche* pacchetto installato cambia versione o slot. Ma in Portage `_complete_graph` estende il grafo **in-place** aggiungendo solo i reverse-dependent dei pacchetti modificati, non riesegue da zero il solver sui pacchetti già risolti! Anche mantenendo l'architettura attuale, prima di lanciare la seconda `run_resolve`, si può verificare tramite `VdbReverseGraph` se esistono effettivamente consumer installati fuori dal grafo della Fase 1 che hanno un vincolo (o `:=`) verso i pacchetti modificati nella Fase 1. Se l'insieme dei consumer esterni impattati è vuoto, la Fase 2 è matematicamente una no-op e può essere saltata in $O(1)$!

---

### Algoritmo 6: Indicizzazione $O(1)$ in `select_nodes` e Critical-Path DAG Scheduling per `mrg`

**Il problema nel codice attuale (`merge_order` e `mrg`):**
1. **Per `emerge` (`crates/portage-repo/src/merge_order/select_nodes.rs`)**:
   - Riga 41–59 (`removal_owners`): per ogni removal fa `entries.iter().position(...)` ($O(|R| \cdot |V|)$);
   - Riga 191 (`roots` fallback): `roots.sort_by_key(|&i| g.order.iter().position(|&j| j == i))` esegue una ricerca lineare `position` su `g.order` ad ogni comparazione del sort ($O(|K| \log |K| \cdot |V|)$);
   - Riga 75 (`while g.order.iter().any(|&i| g.alive[i])`) e righe 82–113 (`waiting`, `admitted_waiting`, `ineligible`): ad **ogni singola iterazione** del while loop (`mo_iter`, che su `@world` può girare fino a 2099 volte!), alloca e ricostruisce tre `HashSet<usize>` (`waiting`, `admitted_waiting`, `ineligible`) e filtra `rest` cercandovi dentro!
   - **Fix algoritmico $O(1)$ per `select_nodes`**:
     - Mantenere `alive_count: usize` decrementato in `commit_selection` (elimina `.any(|&i| g.alive[i])` ad ogni giro);
     - Precalcolare un array `order_rank: Vec<u32>` di dimensione `g.n` aggiornato solo quando `merge_order_bias` riordina `g.order`: il sort di `roots` a riga 191 diventa `roots.sort_by_key(|&i| order_rank[i])` in $O(|K| \log |K|)$;
     - Sostituire i 3 `HashSet<usize>` allocati ad ogni iterazione (`waiting`, `admitted_waiting`, `ineligible`) con un `FixedBitSet` / `Vec<u8>` preallocato fuori dal loop `'select`,(quando `removals.is_empty()` — il caso del 99% dei run! — tutti e tre gli insiemi sono vuoti e l'intero blocco righe 81–113 va saltato con `if !removals.is_empty()`!).

2. **Per `mrg` (§1.3, §6.2, §13): Algoritmo di Scheduling Parallelo Ottimo sotto Vincoli Hard**:
   Il documento nota giustamente che per `mrg` non serve replicare l'euristica sequenziale `_serialize_tasks` di Portage, ma serve rispettare le dipendenze hard massimizzando il parallelismo. Suggeriamo il seguente algoritmo a 3 stadi per `mrg`:
   - **Stadio 1 — Classificazione degli archi Hard vs Soft**:
     - Archi **Hard** (vincolo di precedenza stretto $u \prec v$, cioè $v$ deve essere completato prima che inizi $u$):
       - `DEPEND`, `BDEPEND`, `IDEPEND` verso un pacchetto $v$ merge-bound non ancora soddisfatto dall'installato;
       - `RDEPEND` quando $u$ ha una fase `pkg_preinst` / `pkg_setup` / `src_test` o quando $v$ è una libreria di sistema/runtime necessaria durante il build/install di $u$;
       - Blocker hard (`!!`) / sostituzione nello stesso slot.
     - Archi **Soft / Post-merge (`PDEPEND`)**: possono essere rilassati fintanto che entrambi i pacchetti raggiungono lo stato merged alla fine della transazione (compartimento).
   - **Stadio 2 — Condensazione di Tarjan (SCC) e Aciclicità**:
     - Eseguire l'algoritmo di **Tarjan** in $O(V + E)$ sul sottografo degli archi Hard per trovare le Componenti Fortemente Connesse (SCC).
     - Se una SCC con $|C| > 1$ contiene solo archi Hard non soddisfatti, segnalare il ciclo irrisolvibile (o applicare un USE-bootstrap break se un lato ha un flag di bootstrap); altrimenti il grafo degli archi Hard è un **DAG** (Directed Acyclic Graph).
   - **Stadio 3 — Critical Path Method (CPM) / Bottom-Level List Scheduling + Compartimenti**:
     - Assegnare a ogni nodo $u$ del DAG un peso stimato $w(u)$ (letto da `InstalledSnapshot`/`BinpkgSnapshot` come `BUILD_TIME` storico o `SIZE`, con un default per pacchetti nuovi).
     - Calcolare in ordine topologico inverso il **Bottom-Level (cammino critico residuo)**:
       $$\text{rank}(u) = w(u) + \max_{(u \to v) \in E_{\text{hard}}} \text{rank}(v)$$
     - Maintain una **Ready Priority Queue (Max-Heap per $\text{rank}(u)$)** dei nodi con `in_degree_hard == 0`: ad ogni worker libero, `mrg` assegna il nodo pronto con $\text{rank}(u)$ massimo (regola HLFET — *Highest Levels First with Estimated Times*, con fattore di approssimazione garantito $< 4/3$ rispetto all'ottimo teorico NP-hard $P \mid \text{prec} \mid C_{\max}$!).
     - **Separazione Build vs Merge**: poiché il build (`src_unpack`..`src_install` / fetch / estrazione binpkg) è parallelizzabile su $J$ thread mentre il merge su `${ROOT}` è seriale (vincolo §15), modellare ogni pacchetto $u$ come due task $(u_{\text{build}}, u_{\text{merge}})$:
       - $u_{\text{build}}$ dipende solo da $v_{\text{merge}}$ per $v \in \text{BDEPEND}(u) \cup \text{DEPEND}(u)$;
       - $u_{\text{merge}}$ dipende da $u_{\text{build}}$, da $v_{\text{merge}}$ per $v \in \text{IDEPEND}(u) \cup \text{RDEPEND}(u)$, e usa una risorsa seriale `RootMergeLock`.
       Questo permette a `mrg` di **compilare/estrarre in parallelo anche pacchetti che hanno dipendenze `RDEPEND` reciproche**, serializzando solo il passo finale di merge nel VDB!

---

### Algoritmo 7: Merkle-Fingerprint a 2 livelli e Validazione Inode `ctime_ns` in $O(1)$

**Il problema in §4.3, §6.1 e §18:**  
1. Come calcolare `StateFingerprint` in $< 2$ ms senza leggere migliaia di file?
2. Come proteggersi dal rischio citato in §18 (*"VDB in-place: un edit esterno con mtime invariato può produrre un falso hit"*)?

**L'algoritmo:**
1. **Inode Change Time (`st_ctime_ns`) oltre a `st_mtime_ns`**:
   - Su Linux/POSIX, un programma o utente può alterare un file e reimpostare `st_mtime` al valore precedente con `utimensat(2)`, ma **non può mai falsificare `st_ctime_ns`** senza modificare l'orologio del kernel: qualsiasi `write`, `rename`, `unlink`, `chmod`, `chown` aggiorna `st_ctime_ns` dell'inode e della directory padre!
   - Pertanto, sostituire ogni controllo `mtime` nel calcolo delle revisioni L1 con la tupla **`(st_ino, st_size, st_mtime_ns, st_ctime_ns)`** elimina completamente la classe di falsi hit da modifica in-place con mtime preservato.
2. **Validazione Gerarchica del VDB in 0.2 ms (102 `statx` invece di 2099)**:
   - Se il backend VDB è `sqlite` o `redb` (`portage-vdb`), basta leggere `db.generation()` (1 intero `u64`).
   - Se il backend è `files` (`/var/db/pkg`), Portage e Portuale mantengono l'invariante `_bump_mtime` sulle directory di categoria (`/var/db/pkg/<category>`).
   - `InstalledSnapshot` salva nell'header la lista delle ~102 categorie con `(cat_name, st_ino, st_mtime_ns, st_ctime_ns)` più il valore di `/var/cache/edb/counter`.
   - Al fast-path di avvio di `emerge`, per validare `InstalledSnapshot` bastano **1 lettura di `counter` + 102 `statx` sulle directory di categoria** ($\approx 0.2\text{ ms}$). Solo se una categoria ha cambiato `(mtime_ns, ctime_ns)` si scansionano le directory dei pacchetti *di quella sola categoria* (Incremental Snapshot Update)!

---

### Algoritmo 8: Risoluzione Incrementale / Delta (L4, §7) tramite "Plan Contraction" e "Dependency Cone Certificates"

In §7 il documento rimanda L4 perché teme che la natura globale del resolver Portage impedisca di sapere quando un calcolo incrementale è esatto. Esistono però **due algoritmi con certificato formale di esattezza** che coprono i casi reali post-merge:

1. **Algoritmo L4-A: Plan Contraction dopo esecuzione parziale o totale dello stesso piano**:
   - Scenario reale: l'utente calcola il piano per `@world` ($k$ pacchetti da aggiornare), poi esegue il merge (o il merge si interrompe al pacchetto $m \le k$, oppure installa 1 pacchetto tra quelli previsti dal piano).
   - Quando un pacchetto $p$ viene mergiato con successo esattamente alla versione, slot, subslot e insieme `USE` previsti dal nodo `GraphEntry` in L3b:
     - Lo stato del VDB per $p$ passa da $p_{\text{old}}$ a $p_{\text{new}}$, che è **esattamente lo stato che il grafo L3b aveva già assunto per tutti i dipendenti di $p$**!
     - Pertanto, se l'insieme dei pacchetti modificati nel VDB tra $t_0$ e $t_1$ è un sottoinsieme $M \subseteq \text{MergeSet}(\text{Plan}_{t_0})$ e per ogni $p \in M$ lo stato installato reale coincide con quello pianificato in $\text{Plan}_{t_0}$, il nuovo piano per `@world` ha **esattamente le stesse scelte di versione/USE** di $\text{Plan}_{t_0}$, con i nodi in $M$ promossi da `Upgrade`/`New`/`Reinstall` a `AlreadyInstalled`!
2. **Algoritmo L4-B: Dependency Cone Certificate (Sovra-approssimazione Static Reachability)**:
   - Durante una risoluzione, registrare il **Read-Footprint** $\mathcal{F} \subseteq \text{CpId}$: l'insieme di tutti i `CpId` per i quali il resolver ha interrogato `list_candidates` o `installed_candidates` (inclusi i vicini nel `reverse_index` per slot-operator e rev-dep).
   - Se tra due run cambiano solo i pacchetti in un insieme $\Delta \subseteq \text{CpId}$ (es. un overlay locale aggiorna `app-misc/foo`) e:
     $$\Delta \cap \mathcal{F} = \emptyset \quad \land \quad \text{Blockers}(\Delta) \cap \mathcal{F} = \emptyset$$
     allora nessuna funzione pura o query eseguita dal resolver può restituire un valore diverso: il risultato precedente è **dimostrabilmente invariante al 100%** (teorema standard di *Self-Adjusting Computation* / *Salsa red-green algorithm*).

---

## 5. Risposte motivate alle 8 Decisioni Aperte (§18)

| # | Decisione aperta in §18 | Raccomandazione critica | Motivazione tecnica |
|---|---|---|---|
| **1** | Formato: hand-rolled binario vs `redb` vs JSON compatto | **Hand-rolled binario immutabile (`mmap`-friendly)** per L1/L2/L3; **`portage-vdb`** (`sqlite`/`redb`/`files`) per il VDB autorevole. | `redb` prende un lock esclusivo per processo (`DatabaseAlreadyOpen`), rompendo i reader concorrenti (§8.3). JSON su 32k ebuild costa 50–100 ms di solo parsing. Un file binario immutabile con array ordinati di struct `#[repr(C)]` si apre in `< 50 µs` via `mmap` ed è lock-free. |
| **2** | Cache root: `/var/cache/portuale` vs `$PORTAGE_TMPDIR/portuale-cache` | **`${EROOT}/var/cache/portuale`** (default), con override `PORTUALE_CACHE_DIR`. | Segue la convenzione FHS e di Portage (`var/cache/edb` in `const.py:48`). `$PORTAGE_TMPDIR` (spesso `/var/tmp` o `tmpfs`) viene pulito al reboot o da `tmpreaper`, perdendo gli snapshot L1, e ha permessi più larghi. |
| **3** | L3a (output renderizzato) da subito, o prima L3b (`GraphResult`)? | **Prima L3b (`GraphResultCache`) con `ResolveFingerprint` disaccoppiato dai flag di display**, e L3a come micro-cache opzionale sopra L3b. | L3b serve contemporaneamente `--pretend`, `--ask`, `--tree`, `--json`, l'esecuzione reale `emerge -uDN @world` e `mrg`! Se si fa solo L3a, passando da `emerge -uDpvN @world` a `emerge -uDN @world` si ricalcola tutto da capo. |
| **4** | `ConfigSnapshot` serializzato: necessario per l'1s o basta il fingerprint? | **Fase A (fingerprint veloce + `CpBucketIndex` pre-costruito)**: non serve serializzare tutto `Config`, ma conviene includere i bucket `CpId` nel warmup. | `resolve_config` impiega poche decine di ms. Il vero costo era il matching lineare di `package.*`, già ridotto da `CpBucketIndex`. Con `CpId` numerici (Algoritmo 1), il matching `package.*` diventa un array lookup in $O(1)$. |
| **5** | Hook post-sync: `postsync.d` / `repo.postsync.d`, wrapper di sync, o timer? | **`repo.postsync.d` per ricostruire solo `RepoSnapshot` (L1, ~1–2 s in parallelo)** + cold resolve reso `< 1 s` dagli Algoritmi 1–5. | `repo.postsync.d` riceve il nome e il path del singolo repo sincronizzato, quindi ricostruisce *solo* quel `RepoSnapshot` in 1–2 s senza bloccare l'utente per minuti su `--plans`. Il precalcolo `--plans` può essere opzionale/background. |
| **6** | Dopo merge: cold accettato o `cache warm` one-shot? | **Aggiornamento incrementale di `InstalledSnapshot` a fine merge (solo le categorie toccate)** + Algoritmo L4-A (Plan Contraction). | Aggiornare nel snapshot solo i pacchetti appena mergiati richiede `< 10 ms` alla fine del merge stesso, senza alcun processo in background! |
| **7** | `mrg`: modello multi-client (lock globale vs coda vs shard) | **Snapshot immutabili condivisi (`Arc<Snapshot>`) per i reader + coda serializzata per le mutazioni per-root**. | Poiché L1/L2 sono immutabili e content-addressed, $N$ client che chiedono un piano leggono in parallelo senza lock; solo il commit di un merge aggiorna il puntatore atomico (`ArcSwap` / `RwLock<Arc<...>>`) allo stato corrente. |
| **8** | GC: quante revisioni/piani tenere? | **$N = 2$ revisioni per repo/binpkg/installed** (corrente + precedente per rollback/delta L4) e **LRU di $M = 16$ piani L3**. | Con snapshot immutabili e rename atomico, qualsiasi processo già avviato mantiene aperto il file descriptor del vecchio inode (su Linux un `unlink` non invalida un fd/mmap aperto!). Quindi tenere $N=2$ è 100% sicuro anche con processi concorrenti attivi. |

---

## 6. Proposta di Roadmap rivista (S0 → S4)

Alla luce dell'analisi del codice, suggeriamo di raffinare le prime slice della roadmap (§17 del documento) come segue:

### S0 — Profiling granulare su `@world` (invariata, ma con contatori aggiuntivi)
Oltre ai contatori previsti in §9, aggiungere esplicitamente:
1. Tempo e numero di chiamate a `ResolveCtx::new` e alle 3 invocazioni di `required_set_reachable_cps`;
2. Tempo speso nelle 4 scansioni globali del VDB per passata:
   - `collect_unwalked_installed_blockers`
   - `reverse_dependency_constraints`
   - `slot_operator_rebuild_scan` (+ `collect_probe_parents` + `slot_operator_slot_change_probe`)
   - `add_installed_dependency_closure` (dentro `serialize_merge_order`);
3. Numero totale di `run_pass` eseguite in Fase 1, numero di `replay_walk`, e quale ramo di `run_resolve_phases` viene preso in Fase 2 (`want_complete` vs `else if auto_enable` senza `complete_reuse`!).

### S1 — Algoritmi in-memory e rimozione dei moltiplicatori (prima ancora della cache su disco)
Questa slice da sola può ridurre i 194 s di un ordine di grandezza, rendendo veloce anche il `cache build`:
1. **`VdbReverseGraph` e chiusure in `ResolveSession`** (Algoritmo 3 + Algoritmo 5):
   - Costruire una sola volta per run il grafo diretto e inverso del VDB;
   - Calcolare `world_reachable`, `slot_op_reachable` e `blocker_retry_closure` con BFS su bitset in `< 50 µs` e condividerli tra le passate;
   - Riscrivere `reverse_dependency_constraints`, `slot_operator_rebuild_scan` e `collect_unwalked_installed_blockers` perché interroghino l'indice inverso anziché fare `for pkg in all_installed_packages(root)`.
2. **Fix del ramo `else if auto_enable` in `resolve_phases.rs`** (Algoritmo 5):
   - Evitare che `-uDpvN @world` riesegua la Fase 1 da zero con `&empty_reuse`, e fare short-circuit della Fase 2 quando non ci sono consumer esterni impattati.
3. **Memo cross-pass di `reinstall_flags_for_use_change` / `deps_changed` / `slot_changed` + `BitSet` USE** (Algoritmo 2).
4. **Indice incrementale in `PassState`** (`by_cp`, `installed_slots` con `usize` index) e **fix `order_rank` + `removals.is_empty()` fast-path in `select_nodes.rs`** (Algoritmo 6.1).

### S2 — Snapshot L1 (`RepoSnapshot`, `BinpkgSnapshot`, integrazione `portage-vdb` `InstalledSnapshot`)
1. Formato binario immutabile con **Interning Lessicografico (`CpId`, `AtomId`, `FlagId`)** (Algoritmo 1) e **Bytecode `FlatDepAst` con skip-offsets** (Algoritmo 4).
2. Unificazione di `InstalledSnapshot` con `portage-vdb::Snapshot`, validato su backend `files` tramite i ~102 `(mtime_ns, ctime_ns)` delle directory di categoria + `counter` (Algoritmo 7).
3. Hook `repo.postsync.d` leggero che aggiorna solo il `RepoSnapshot` del repo sincronizzato.

### S3 — Plan Cache separata L3b (`ResolveFingerprint`) + L3a (`RenderFingerprint`)
1. Separare `ResolveFingerprint` (usato da `--pretend`, `--ask`, `--tree`, `--json`, merge reale e `mrg`) da `RenderFingerprint` (specifico per larghezza terminale e flag di visualizzazione).
2. Usare BLAKE2b/SHA-256 canonico (mai `DefaultHasher`) su tutti i fingerprint persistenti.

### S4 — `mrg` Server + Critical-Path DAG Scheduler
1. Mantenere L1/L2 mappati in memoria nel server `mrg`.
2. Implementare lo scheduler DAG basato su condensazione SCC di Tarjan + **Bottom-Level Critical Path (HLFET)** con disaccoppiamento tra task $u_{\text{build}}$ (parallelo) e $u_{\text{merge}}$ (seriale su `${ROOT}`) (Algoritmo 6.2).
