# ideas

- project website
- rpm and dep package manager
- web interface
  https://github.com/Devolutions/UniGetUI
- new DBs /var/...
- infrastructure for Profile-Guided Optimization (PGO)
  AutoFDO (Automatic Feedback-Directed Optimization)
  LLVM BOLT (Binary Optimization and Layout Tool)
  propeller https://github.com/google/autofdo/blob/master/docs/OptimizeClangO3WithPropeller.md
  Static Heuristics and Machine Learning Inference
  https://github.com/zamazan4ik/awesome-pgo
  https://bugs.gentoo.org/907931
  https://neurips.cc/virtual/2025/loc/san-diego/poster/119293
  https://github.com/Kobzol/cargo-pgo
- auto-benchmark
- parallelize?

- index di codice
  https://github.com/DeusData/codebase-memory-mcp
  curl -fsSL https://raw.githubusercontent.com/DeusData/codebase-memory-mcp/main/install.sh > install.sh
  https://github.com/trailhq/Graft
  
- Babeltele (linguaggio specifico tre LLM) ((approved))

- Solver per via insiemistica. 
    Prima si definiscono tutti i pacchetti coinvolti
    Si stabilisce quindi un insieme ordinato per ogni pacchetto.
        L'insieme contiene "v" versioni, ogni versione v può essere visibile o meno, in caso non lo fosse il motivo viene memorizato
        La funzione di ordinamento tiene conto di molti parametri, in primis la versione e la visibilità
    Per ogni pacchetto dipendente si aggiunge un insieme con un tag che rimanda alla dipendenza.
    Durante l'attraversamento si stabilisce anche l'ordine di esecuzione
    Ad attraversamento terminato per ogni pacchetto si cerca l'intersezione di tutti gli insiemi.
        se vuota si stampa errore e albero di dipendenze
        se piena si sceglie la "v" con priorità più alta
    Pensare bene al problema "slot" e come forzare un upgrade
    Ogni "v" avrà anche un insieme di USE
  also read https://nex3.medium.com/pubgrub-2fb6470504f  https://pubgrub-rs-guide.pages.dev/internals/intro

- LOGO:
    L'idea parte dal nome: un portuale è chi carica le navi, e un package manager in fondo fa la stessa cosa. Nel logo c'è una gru da porto che sta imbarcando un container con una faccina, e la bocca è un cursore da terminale che lampeggia. Il container è color ruggine, perché i container arrugginiscono e il progetto è in Rust. Sotto il nome c'è $ emerge --in-rust_, come prompt nerd.

    Qualche dettaglio pratico:

    Animazione: negli SVG il container oscilla appena sotto la gru e il cursore lampeggia. Funziona anche nel README di GitHub. I PNG invece sono statici.
    Font: il testo è convertito in tracciati (JetBrains Mono), quindi il logo si vede uguale ovunque, anche su macchine senza il font installato.
    Tema scuro: c'è una variante -dark con il testo chiaro. Nel README puoi far scegliere a GitHub quella giusta in base al tema:
    <picture>
        <source media="(prefers-color-scheme: dark)" srcset="docs/portuale-logo-dark.svg">
        <img src="docs/portuale-logo.svg" alt="portuale" width="480">
    </picture>

- Diagrammi esplicativi
  https://github.com/cathrynlavery/diagram-design
  https://github.com/tt-a1i/archify

- test suite:
  + deve poter essere eseguita per due package manager contemporaneamente sullo stesso sistema
  + non deve utilizzare path hard-coded, in quanto la sua posizione nel file system non sarà sempre la stessa
