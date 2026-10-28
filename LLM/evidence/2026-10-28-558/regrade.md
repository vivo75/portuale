# Task 3 — re-grade finale (senza nuovo run L0, vedi Ruling)

## Ruling (deviazione dal Task 3 Step 1)

Lo Step 1 chiedeva un nuovo run `l0-resolver.sh` congelato. Non lo lancio.

Motivi:
1. Zero modifiche yaml/codice nei Task 1–2: un re-run sul baseline
   riprodurrebbe identico 44 UNEXPLAINED (a meno di drift del tree).
2. Il bed è avanzato sotto altri slice paralleli: esistono già log più
   nuovi dello stesso letto (`l0-20261027T141949Z`,
   `l0-20261028T083834Z`: clean 91, parity 0.758, explained 16,
   UNEXPLAINED 44 ma composizione diversa — order 23, flags 2, exit 0).
   Un mio run oggi misurerebbe il lavoro altrui (#589-10a MULTI heal,
   ecc.), non questo piano.
3. Run L0 = container bed pesante; lanciarlo per confermare un nulla
   spreca la risorsa condivisa (`target/`, corpus, mount namespace).

Al suo posto: confronto baseline vs log più recente disponibile.

## Baseline (questo piano): l0-20261025T145359Z

120 probe, 88 clean, 0.733, 27 explained, 44 UNEXPLAINED
(order 22, advice 9, error 6, exit 2, flags 1, totals 2, truncated 2).
Inventario: `inventory.md` (44 righe). Nessun move yaml (Task 1 Ruling:
20 puro-ordine senza causa nominata, non allowlistati; Task 2: zero move
C4/C5, #573 intatto).

## Log più recente ispezionato: l0-20261028T083834Z

120 probe, 91 clean, 0.758, 16 explained, 44 UNEXPLAINED
(order 23, advice 9, error 6, totals 2, truncated 2, flags 2, exit 0).
Letto da `l0-report.json` summary, non da re-run mio. La composizione è
cambiata (exit guarite via #589-10a MULTI heal, un order e un flags in
più): il residuo #558 va re-triaged contro QUESTO log, non contro il mio
baseline, quando gli slice paralleli atterrano.

## Stato #558 proposto (voce NON modificata qui — vedi sotto)

Resta OPEN. Residuo attuale, per nome/classe sul baseline di questo piano:
20 order puro-ordine senza causa (c2-order.md, pattern libelf-early
sistematico → S0 d'ordine dedicata, primo libero 617+); 2 totals legati
a #567; 2 truncated + 2 exit C4/#276; 15 masked cups/gcr pre-esistenti;
1 flags da verificare; #573 di proprietà di #573. Contro il log 10-28 il
conteggio va rifatto (44 ma ricomposti).

Ruling voce: non modifico `LLM/backlog-tasks.md` in questo run.
Motivo: niente commit chiesto (AGENTS.md regola 9) + slice paralleli
stanno producendo log più nuovi che cambiano il residuo; scrivere la
voce ora rischierebbe un testo stale in giornata. Il testo proposto sta
nel paragrafo sopra; l'edit va fatto al momento del commit, contro il
log più recente di allora.
