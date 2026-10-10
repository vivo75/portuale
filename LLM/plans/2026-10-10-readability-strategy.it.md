# Strategie per leggibilità e struttura dell'applicazione Rust

Contesto: ~200.000 righe (commenti inclusi), CLI Linux/Unix, repo Git con ~1.500 commit, nessun vincolo di produzione. Obiettivo: **manutenibilità umana** — codice leggibile, struttura chiara, history lineare, lezioni degli LLM condensate per tipo di attività, documentazione viva.

---

## 1. Principi guida

1. **Ottimizza per il lettore, non per lo scrittore.** Il codice si scrive una volta e si legge decine di volte. Ogni decisione (naming, struttura, commit) privilegia chi arriva dopo.
2. **La history è documentazione.** Un commit deve raccontare *una* modifica coerente; la sequenza dei commit deve raccontare *perché* l'applicazione è cresciuta in quel modo.
3. **Automatizza ciò che è meccanico** (formatting, lint, commit hygiene): libera energia umana per le decisioni architetturali.
4. **Refactoring e funzionalità mai mescolati** nello stesso commit o PR.
5. **Zero vincoli legacy**: sfrutta la fase pre-produzione per fare adesso le ristrutturazioni che dopo costerebbero 10 volte tanto (workspace, architettura, history).

---

## 2. Leggibilità del codice Rust

### 2.1 Igiene automatica (implementa subito, costa poco)

| Strumento | Azione | Effetto |
|---|---|---|
| `rustfmt.toml` | Configurazione condivisa + `cargo fmt --check` in CI | Zero dibattiti di stile |
| Clippy | `#![deny(clippy::all)]`, poi alzare gradualmente a `clippy::pedantic` | Elimina anti-pattern idiomatici |
| `#![deny(missing_docs)]` sui crate pubblici | Obbliga doc-comment sulle API | Documentazione intergrata al codice |
| `cargo-deny` | Licenze e duplicazioni di dipendenze | Grafo dipendenze pulito |
| pre-commit / hooks | fmt + clippy + test veloci locali | Errori fermati prima del commit |

### 2.2 Regole di leggibilità (da far rispettare in code review)

- **Funzioni brevi e a singolo scopo**: target < 40–50 righe; se serve un commento per spiegare *cosa* fa, estrai una funzione; se serve per *perché*, scrivilo bene e tienilo.
- **Nomi che documentano**: nessuna abbreviazione non evidente; i tipi Rust rendono i nomi corti sicuri, non serve il prefisso ungherese.
- **Newtype pattern** per tipi primitivi con significato (`struct UserId(u64)`): previene errori e auto-documenta le firme.
- **Gestione errori rigorosa**: `thiserror` per errori di libreria, `anyhow` solo nel `main`/CLI; **mai** `unwrap()`/`expect()` fuori da test e invarianti provati; usare `?` e contesto.
- **Idiomi iterator** invece di loop indicizzati; `match` esauriente con enum invece di booleani sparsi.
- **Limiti soft di dimensione**: file < 400–500 righe, modulo < ~10 item pubblici. Oltre, si splitta.
- **`clone()` visibile = campanello d'allarme**: in revisione chiedersi sempre se è necessario o se è un odore di proprietà dei dati mal progettata.

### 2.3 Metriche di leggibilità da monitorare

- Complessità cognitiva/ciclomatica per funzione (tool: `cargo clippy` + linter esterni tipo `cargo-crev`/`rust-code-analysis`).
- Rapporto commenti/codice: puntare a doc-comment (**perché**) più che a commenti di codice (**come**).
- Lunghezza media delle firme e numero di parametri (> 4–5 → introduci una struct di configurazione).

---

## 3. Struttura del progetto

### 3.1 Passaggio a Cargo workspace

Un monolite da 200k righe in un solo crate è il maggior ostacolo alla navigabilità. Struttura target:

```text
nome-app/
├── Cargo.toml                # [workspace]
├── crates/
│   ├── nome-app-cli/         # solo parsing argomenti (clap), orchestrazione, exit code
│   ├── nome-app-core/        # logica di dominio pura, senza I/O
│   ├── nome-app-storage/     # adattatori I/O: file, DB, rete
│   ├── nome-app-commands/    # un modulo per sottocomando CLI
│   └── nome-app-test-utils/  # fixture e helper condivisi
└── docs/
```

Vantaggi: tempi di compilazione parallela, confini architetturali *enforced dal compilatore* (dipendenze unidirezionali CLI → commands → core), test più veloci per crate.

### 3.2 Architettura: dipendenze unidirezionali

- **Core puro**: nessun `std::fs`, `std::net`, no `println!`. Solo logica e tipi. Testabile senza mock dell'I/O.
- **Adattatori** implementano trait definiti nel core (ports/adapters, "hexagonale lite"): `trait ConfigStore`, `trait Output`.
- **CLI è un sottile guscio**: clap definisce argomenti → mappa a comandi del core → formatta output. Nessuna logica di business in `main`.
- Regola pratica: **`cargo modules` / `cargo depgraph`** devono mostrare frecce che vanno sempre nella stessa direzione; un ciclo è un difetto.

### 3.3 Feature flags al posto di `if cfg` sparsi

Varianti sperimentali o opzionali come feature Cargo dichiarate nel workspace, con feature-unione esplicita; semplifica la rimozione del codice morto.

---

## 4. Strategia di refactoring incrementale

1. **Prima di toccare: test di caratterizzazione.** Avvolgi il comportamento attuale in test (anche snapshot-based con `insta`) così il refactoring ha una rete di sicurezza.
2. **Strangler fig, non big bang.** Si estrae un modulo/crate alla volta, si lascia il vecchio codice reindirizzato al nuovo, poi si rimuove il vecchio *nello stesso ciclo* di lavoro, non mesi dopo.
3. **Ogni estrazione = un commit/PR dedicato**, compilante e verde a ogni passo. Mai refactoring e feature insieme.
4. **Budget di pulizia**: ogni iterazione riserva il 15–20% del tempo a estinguere il "debito" più fastidioso (non tutto il debito, il più fastidioso).
5. **Ordine di attacco consigliato**: (a) split workspace/crate → (b) separazione core/I/O → (c) gestione errori unificata → (d) rimozione codice morto e rami sperimentali abbandonati → (e) semplificazione dei moduli più grandi.
6. **Non rifattorizzare per gusto estetico**: ogni intervento deve ridurre il costo di *una* attività futura concreta (aggiungere un comando, cambiare formato di output, ecc.).

---

## 5. Riscrittura della history Git

Con ~1.500 commit di sviluppo pre-produzione, la history è un bene *riscrivibile*: l'obiettivo è una narrazione lineare con un ordine di grandezza in meno commit.

### 5.1 Strategia complessiva

1. **Backup totale prima di tutto**: `git clone --mirror` su storage esterno + tag `pre-squash`. La history riscrituta è irreversibile.
2. **Analisi preliminare**: `git log --oneline | wc -l`, identificare i revert (`git log --merges`, sequenze `revert of <hash>`), i fixup, i "wip", i commit di stile.
3. **Ricostruzione per funzionalità, non per tempo**: la nuova history racconta le *funzionalità* nell'ordine logico dell'architettura finale. Alternative:
   - **(a) History compressa**: squash di tutti i commit per feature in un commit per feature (~50–150 commit finali). Semplice, perde granularità intermedia.
   - **(b) Ricostruzione "as-if-new"** (consigliata, più ambiziosa): partendo dal codice finale, si ricrea una history di snapshot significativi ("aggiunge crate core", "aggiunge storage", "aggiunge comando X", "rimuove esperimento Y"). Ogni commit è *chiuso*, compila, i test passano. La granularità intermedia rumorosa sparisce.
4. **I revert spariscono**: la coppia commit+revert (e le rielaborazioni successive) collassa in un solo commit con il risultato finale. La logica tentata e scartata può vivere come nota ADR o lezione appresa, non come rumore nella history.
5. **Dopo la riscrittura**: branch `main` lineare, nessun merge commit, `git push --force` coordinato con il team, tutti i cloni vanno riclonati.

### 5.2 Policy futura (per non ritrovarsi il problema)

- **Solo squash-merge** o **rebase**: la main resta lineare.
- **Conventional Commits** (`feat:`, `fix:`, `refactor:`, `docs:`, `test:`): abilita changelog automatico e rende gli squash naturali.
- Un PR = una funzionalità/refactoring = un commit finale ben scritto (corpo esteso col *perché*).
- `git rerere` attivato per riusare le risoluzioni di conflitti.

### 5.3 Strumenti

| Strumento | Uso |
|---|---|
| `git rebase -i` | Riscrittura di segmenti recenti, squash, riordino |
| `git filter-repo` (successore di filter-branch) | Rimozione massiva di file/percorsi, riscritture su tutta la history |
| `git rerere` | Riuso risoluzioni conflitti durante i rebase |
| `git merge --squash` | Politica futura di squash-merge |
| `graft`/`replace` (temporanei) | Sperimentare la nuova history prima del filtro definitivo |

---

## 6. Lezioni apprese dagli LLM: condensazione per tipo di operatività

I vari LLM che hanno lavorato al progetto hanno accumulato conoscenza implicita nei messaggi e nei prompt. Va resa **esplicita, deduplicata e organizzata per attività**, così umani e LLM futuri la consultano come manuale operativo.

### 6.1 Dove vive

- `docs/lessons/` nel repo: `development.md`, `testing.md`, `refactoring.md`, `documentation.md`, `ci-tooling.md`, `llm-collaboration.md`.
- Ogni lezione: una **regola azionabile** (non un aneddoto), con un contro-esempio preso dal progetto quando esiste.
- Riferimenti incrociati da `CONTRIBUTING.md`; le lezioni si citano in code review come si cita una regola di stile.

### 6.2 Contenuto tipico per categoria

- **Sviluppo**: errori API ricorrenti degli LLM nel progetto (es. uso di crate deprecati, allucinazioni su firme), pattern di dominio scoperti, convenzioni di naming scelte e perché, aree del codice "trappole" dove il compilatore non aiuta.
- **Testing**: cosa ha reso i test fragili, dove servono fixture condivise, convenzioni sui nomi dei test, come testare l'I/O con trait.
- **Refactoring**: sequenze di interventi che hanno funzionato, test di caratterizzazione necessari prima di X, tranelli di borrow-checker tipici del progetto.
- **Documentazione**: convenzioni rustdoc, cosa gli LLM tendevano a documentare male o a far divergere dal codice.
- **CI/tooling**: problemi di CI incontrati e fix definitivi.
- **LLM-collaboration**: come si lavora bene con gli LLM su *questo* repo: contesto minimo da dare, punti del codice dove farli partire, tipi di output da pretendere (commit compilanti, test inclusi).

### 6.3 Processo di manutenzione

- **Revisione trimestrale**: si elimina ciò che non ha cambiato una decisione negli ultimi mesi; le lezioni non vissute si rimuovono.
- Massimo ~1 pagina per categoria: se cresce, si trasforma in guida strutturata.
- Ogni lezione nuova nasce da un incidente reale (bug, retro debug lungo, allucinazione LLM costata tempo).

---

## 7. Documentazione

### 7.1 Principio: documentation-as-code

La documentazione vive nel repo, è in markdown, si rivede nei PR come il codice, e ha un owner. Tutto ciò che non è nel repo muore.

### 7.2 Struttura target

- **README** snello (≤ 1 schermata): cosa fa, installazione, uso base, link al resto.
- **`docs/adr/`** — *Architecture Decision Records*: la decisione più importante da aggiungere subito. Ogni scelta strutturale (workspace, gestione errori, storage, policy git) come ADR breve: contesto → opzioni → decisione → conseguenze. È qui che si condensa il "perché" dei percorsi tentati e scartati, liberando la history di quell'onere.
- **`CONTRIBUTING.md`**: setup, comandi, policy di commit/PR, come citare le lezioni.
- **Rustdoc come prima fonte per le API**: doc-comment con esempi eseguibili (`///` + doctest); `cargo doc --open` come esperienza di consultazione; pagine di documentazione per modulo con panoramica (`//!`).
- **Documentazione utente CLI**: help di `clap` come fonte di verità, generare man page/completamenti da lì (`clap_mangen`/`clap_complete`), guida utente separata in `docs/user/`.
- **Diagrammi**: uno o due diagrammi mermaid/C4 della dipendenza tra crate e del flusso di un comando; tenuti in `docs/` e verificati quando cambia l'architettura.

### 7.3 Regole anti-decadenza

- Doc-check in CI (`cargo doc` senza warning, doctest eseguiti).
- Ogni ADR è datato e può essere "superseded", mai rimosso.
- Commenti nel codice solo per il *perché* non ovvio o per invarianti delicate; tutto il resto appartiene al doc-comment o all'ADR.

---

## 8. Testing e guardie di qualità

- **Piramide**: la maggior parte dei test nel `core` (puro, veloce, senza I/O), pochi test di integrazione end-to-end sulla CLI (invocazione reale del binario con file temporanei).
- **Snapshot testing** (`insta`) per output CLI e per i test di caratterizzazione durante il refactoring: leggibili, aggiornabili con `cargo insta review`.
- **Copertura con `cargo llvm-cov`** sul crate core; target di copertura come indicatore, non come culto.
- **Test deterministici**: niente dipendenze da timing/filesystem globale; fixture e `tempfile`.
- **CI obbligatoria**: fmt + clippy + test + doc + llvm-cov + cargo-deny + MSRV; la main non accetta rosso.

---

## 9. Piano d'azione consigliato

| Fase | Attività | Esito |
|---|---|---|
| 1. Igiene (settimana 1–2) | rustfmt, clippy pedant progressivo, CI, pre-commit hooks | Base automatica |
| 2. Lezioni (settimana 2–3) | Condensare le lezioni LLM in `docs/lessons/` per categoria; primo giro di ADR per le scelte esistenti | Conoscenza esplicita |
| 3. Sicurezza (settimana 3–4) | Test di caratterizzazione/snapshot sui comportamenti da preservare | Rete di protezione |
| 4. Struttura (mese 2) | Split in workspace/crate, separazione core–I/O, gestione errori unificata | Architettura target |
| 5. History (mese 2–3) | Backup, analisi revert/fixup, riscrittura (approccio b), policy squash-merge futura | History lineare ridotta |
| 6. Documentazione (in parallelo, mese 3) | README, ADR completi, rustdoc, guida utente | Documentazione viva |

**Regola d'ordine**: la riscrittura della history viene *dopo* il grosso del refactoring strutturale, così i commit ricostruiti raccontano l'architettura finale e non quella intermedia.

---

## 10. Metriche di successo

- Il tempo medio per **onboarding** di un nuovo sviluppatore umano (o LLM con contesto limitato) scende da "giorni" a "ore": con questa metrica si giudica ogni intervento.
- Un newcomer sa da solo: dove sta la logica di dominio, come si aggiunge un sottocomando, dove si legge il perché di una decisione.
- La history risponde alla domanda "perché esiste X?" in un solo posto, sempre l'ADR o la lezione, mai un'archeologia di revert.
- Le **lezioni** sono abbastanza brevi da essere lette interamente, e abbastanza dense da avere fermato almeno una volta un errore reale.