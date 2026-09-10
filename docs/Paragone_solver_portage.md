Ci sono discordanze tra l'ordinamento utilizzato da Portuale e l'ordinamento usato da Portage, di cui Portuale è un clone.
Sai dirmi il perchè?

Sì. Ho guardato **il codice attuale di Portuale** e l'implementazione corrente di **Portage**, e la discrepanza ha una causa abbastanza precisa: **l'ordinamento di Portage non è un semplice topological sort del dependency graph**.

Anzi, nel repository di Portuale c'è una traccia molto esplicita di questo problema: `merge_order.rs` dice che l'implementazione precedente usava un approccio Kahn/topological-sort con `discovery-rank`, e che questo riproduceva il *set* dei package ma **non il loro ordine**.  GitHub 

## Il punto fondamentale

Portage fa sostanzialmente questo:

1. costruisce un `digraph` molto particolare;
2. assegna ad ogni edge una **priorità di dipendenza**;
3. determina quali nodi sono "leaf" considerando progressivamente meno vincolanti alcuni tipi di dipendenza;
4. applica un **bias sull'ordine dei nodi**;
5. ha un percorso speciale `asap_nodes`;
6. quando ci sono cicli, non fa semplicemente una topological sort: cerca e seleziona cicli con una logica specifica.

Questo è esattamente ciò che fa `_serialize_tasks()` di Portage.  GitHub+1 

Quindi due implementazioni possono avere **lo stesso identico dependency graph** e produrre comunque:

```
Portage:
A
B
C
D
```

e:

```
Portuale:
A
C
B
D
```

senza che nessuna delle due violi una dipendenza.

La differenza è nella **politica di scheduling**, non necessariamente nella risoluzione delle dipendenze.

## C'è una cosa interessante nel Portuale attuale

Il repository che mi hai indicato sembra avere già fatto un lavoro importante proprio su questo problema.

`rust/portage-repo/src/merge_order.rs` oggi implementa esplicitamente:

> ```
> Real _emerge/depgraph.py::_serialize_tasks
> ```

e documenta che l'implementazione precedente era:

> "a batched Kahn's walk over cp-level `required_by` edges plus a discovery-rank tie-break"

e che questo **non riproduceva l'ordine di Portage**.  GitHub 

Il nuovo codice ha quindi introdotto:

- `DepPriority`;
- `DepPriorityNormalRange`;
- `DepPrioritySatisfiedRange`;
- priority relaxation;
- `_merge_order_bias`;
- `asap_nodes`;
- gestione dei cicli;
- gestione delle dipendenze già soddisfatte;
- gestione della closure delle dipendenze dei pacchetti già installati.

Sono tutti elementi che corrispondono abbastanza fedelmente a Portage.  GitHub+1 

### 1. `DepPriority` è fondamentale

In Portage un edge non è semplicemente:

```
A -> B
```

ma qualcosa del tipo:

```
A -> B [RDEPEND]
A -> B [DEPEND]
A -> B [PDEPEND]
A -> B [:=]
```

e queste relazioni hanno **forza diversa**.

Portuale le rappresenta esplicitamente con:

```
pub struct DepPriority {
    pub buildtime: bool,
    pub runtime: bool,
    pub runtime_post: bool,
    pub buildtime_slot_op: bool,
    pub runtime_slot_op: bool,
    pub optional: bool,
    pub satisfied: bool,
}
```

 GitHub 

Questo è importante soprattutto quando il grafo contiene cicli.

------

## 2. Portage non usa sempre "tutti i leaf"

Questa probabilmente è una delle fonti più comuni delle differenze che state vedendo.

Portage cerca i leaf attraverso una serie di livelli di rilassamento delle priorità.

Nel codice attuale di Portage si vede:

```
NONE
...
MEDIUM_SOFT
```

e il scheduler prova progressivamente `ignore_priority`.  GitHub 

Per esempio, concettualmente:

```
A --DEPEND--> B
A --RDEPEND-> C
A --PDEPEND-> D
```

non significa che B, C e D debbano necessariamente essere trattati allo stesso modo dall'algoritmo di scheduling.

Una topological sort classica direbbe semplicemente:

```
B, C, D prima di A
```

Portage invece può dire:

> C è una dipendenza runtime forte, D è una PDEPEND e può essere posticipata, B è build-time, ecc.

Quindi la scelta del **prossimo package** dipende dalla categoria dell'edge.

------

## 3. C'è poi `_merge_order_bias`

Questo è un altro punto che può spiegare ordinamenti apparentemente "strani".

Portage, prima dello scheduling, riordina `mygraph.order` usando:

- le dipendenze runtime profonde di `@system`;
- il numero di parent di ciascun nodo.

La funzione è esplicitamente `_merge_order_bias()`.  GitHub 

Il criterio principale è:

```
deep system runtime dependency
        ↓
numero di riferimenti
        ↓
ordine originale
```

Portuale lo replica in `merge_order_bias()`:

```
g.order
    .sort_by_key(|&i| (!deep[i], Reverse(parent_count[i])));
```

 GitHub 

Quindi **l'ordine di scoperta dei package non è necessariamente l'ordine di merge**.

------

## 4. E soprattutto c'è `asap_nodes`

Questo è un'altra differenza molto facile da perdere se si implementa il comportamento guardando solo il grafo.

Portage mantiene una lista speciale di nodi che devono essere portati avanti "ASAP".

Un caso importante sono le `PDEPEND`.

Quando un nodo viene selezionato dopo aver rilassato una certa priorità, Portage può mettere alcuni figli in `asap_nodes`, e questi avranno precedenza rispetto al normale ordinamento.  GitHub 

Portuale ora lo implementa esplicitamente:

> "`asap_nodes` -- real's `PDEPEND`-promotion path"

 GitHub 

Questa è una delle ragioni per cui una soluzione del tipo:

```
while let Some(leaf) = find_leaf() {
    emit(leaf);
}
```

non basta.

------

## 5. I cicli sono un'altra grossa differenza

Portage non considera un ciclo semplicemente come:

> "non esiste più un leaf → fai qualcosa di arbitrario".

Quando non riesce a selezionare un nodo, entra nella logica `find_smallest_cycle()`.

Cerca cioè un ciclo adatto da rompere, cercando di minimizzare il numero di nodi coinvolti e utilizzando nuovamente le priorità.  GitHub 

Inoltre usa `DepPrioritySatisfiedRange` per poter rilassare edge che sono già soddisfatti dall'installazione corrente.  GitHub 

Quindi:

```
A -> B
B -> C
C -> A
```

non determina da solo l'ordine.

L'ordine dipende anche da:

- tipo delle dipendenze;
- quali package sono già installati;
- slot operator;
- priorità;
- numero di riferimenti;
- ordine nel grafo;
- eventuali `PDEPEND`.

------

# Ma c'è un'altra cosa che controllerei nel vostro caso

Se **l'attuale Portuale continua a dare un ordine diverso da Portage**, io non partirei più dal scheduler.

Il codice attuale del scheduler è ormai molto vicino a `_serialize_tasks()` di Portage.  GitHub+1 

A questo punto il sospetto principale diventa:

> **il grafo che arriva allo scheduler non è esattamente lo stesso grafo che Portage costruisce.**

Ed è qui che vedo alcuni possibili punti di divergenza.

### Disjunctive dependencies

Portage tratta in modo particolare:

```
|| ( A B C )
```

e non mette semplicemente A, B e C nel grafo nello stesso momento.

Portuale ha infatti dovuto introdurre una `disjunctive_stack` e ritardare l'espansione di questi nodi. Il commento nel codice lo dichiara esplicitamente.  GitHub 

Questo può cambiare il **discovery order**, che a sua volta può cambiare il merge order quando il bias trova dei pareggi.

### Multi-slot

Portuale ha anche una situazione delicata perché `required_by` è modellato a livello:

```
cat/pkg
```

mentre Portage lavora con oggetti `Package` distinti.

Il codice stesso segnala il problema:

> "`required_by` is keyed by `cat/pkg` only"

e deve evitare di creare fallback edge verso slot diversi quando esiste già un edge reale verso lo slot corretto.  GitHub 

Questo è **un candidato molto forte** se le vostre discrepanze coinvolgono pacchetti con più slot.

------

# La mia diagnosi, quindi

Direi che ci sono **due generazioni di problema**:

### Problema storico

Portuale usava un algoritmo sostanzialmente equivalente a:

```
dependency graph
      ↓
Kahn/topological sort
      ↓
discovery rank come tie-break
```

Questo **non poteva** riprodurre fedelmente Portage.

Il repository stesso documenta che questa implementazione è stata sostituita il 6 settembre 2026.  GitHub 

### Problema eventualmente ancora presente

Se state confrontando una versione di Portuale **successiva a quella modifica** e vedete ancora differenze, allora bisogna confrontare:

```
Portage graph
      ↓
Portage _serialize_tasks
```

con:

```
Portuale GraphEntry
      ↓
Portuale build_digraph
      ↓
Portuale select_nodes
```

**nodo per nodo ed edge per edge**.

Non mi limiterei a confrontare la lista finale.

------

## In sostanza:

Il grafo delle dipendenze e il modello dello Scheduler rispondono a domande diverse.

Il grafo (`depgraph`) chiede cosa deve esistere e perché. I suoi nodi sono `Package` identificati da cpv, root, tipo (ebuild, binario, installato) e operazione (merge o nomerge). Gli archi portano una priorità: buildtime, runtime o runtime_post, con varianti *slot_op* per i `:=`. Il grafo può contenere cicli. Sono loro a determinare i rebuild per slot operator, come qemu e libvirt, e contengono anche i pacchetti già soddisfatti, che non producono lavoro.

Lo Scheduler chiede invece in che ordine eseguire il lavoro. Il depgraph serializza il grafo con un ordinamento topologico. Quando incontra un ciclo rilassa prima gli archi soft: una dipendenza runtime può finire dopo il pacchetto, una buildtime no. Lo Scheduler riceve quindi una merge list ordinata di task. Senza `--jobs` la esegue in sequenza. Con `--jobs`, in sostanza, avvia la build di un pacchetto solo quando le sue dipendenze nel grafo che lo precedono nella lista sono già state installate. Le build possono girare in parallelo, ma l'installazione nel filesystem vivo avviene una alla volta.

La differenza pratica è che lo Scheduler non ragiona più su DEPEND, RDEPEND o BDEPEND. Per questo, nell'esempio, xen-tools aspetta anche la merge di bridge-utils, che è solo una RDEPEND: il tipo ha già svolto il suo ruolo durante la serializzazione.

![xen_tools_4193_dipendenze_per_tipo](/home/vivo/repo/PORTUALE/portuale/docs/xen_tools_4193_dipendenze_per_tipo.svg)


![xen_tools_valutazione_use_e_slot_operator](/home/vivo/repo/PORTUALE/portuale/docs/xen_tools_valutazione_use_e_slot_operator.svg)

![xen_tools_grafo_vs_modello_scheduler](/home/vivo/repo/PORTUALE/portuale/docs/xen_tools_grafo_vs_modello_scheduler.svg)
