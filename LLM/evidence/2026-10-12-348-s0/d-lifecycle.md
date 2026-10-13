# S0.7 Bordi di metadata e ciclo di vita — catture e inventario (worker `d`)

Task S0.7 del piano `LLM/02.348-eapi-phase-audit.opus.md` (§0 fatti 1–5, §1
metodo letti). **Solo catture e inventario**: nessun codice di prodotto, nessuna
fixture esistente toccata. Oracolo: Portage 3.0.82.2 dell'host. Perimetro EAPI:
7 e 8 + valori non supportati.

## 0. Convenzioni di lettura

- Celle in `LLM/evidence/2026-10-12-348-s0/d/`: per cella
  `<cella>.argv/.rc/.stdout/.stderr` (+ `.lifecycle` dove indicato).
  Runner: `../scripts/d-run.sh` (prefisso `d`).
- PM: `portage` = `ebuild`/`emerge` host; `portuale` =
  `target/release/{ebuild,emerge}` (symlink al multicall, **mai ricompilato**).
- Configroot ermetico: `/var/tmp/t348-d/cfg` (`PORTAGE_CONFIGROOT`), `ROOT` =
  cfg oppure root dedicati (`--root`: `rootP/Q` merge IDEPEND, `rootBP/BQ`
  binpkg, `rootU1/U2` unmerge). Repos: testrepo+overlay+independentoverlay+
  layoutmasteroverlay+repnamerepo (come `_t341_configroot`) + `t348`
  (`pmtest/fixtures/repo-348`, priority 50). `make.profile` → fixture default
  profile. `FEATURES="-sandbox -usersandbox -network-sandbox"`,
  `PORTAGE_TMPDIR/LOGDIR/DEPCACHEDIR` dentro cfg (convenzione pmtest),
  `BROOT=$ROOT` esportato (ignorato da Portage, vedi sotto).
- Regola di verdetto: `same` = stesso rc + stesso testo a meno dei delta
  sistematici noti sotto (non EAPI-specifici, per S0.8): (a) stream
  stdout-vs-stderr dell'output fasi; (b) suffisso `::t348` vs `::` nei die;
  (c) numeri di linea di `phase-functions.sh` vendored (444 vs 514);
  (d) suffisso `for <root>` assente in portuale (convenzione già vista in #341);
  (e) righe instabili (`Dependency resolution took N s`, news, updates).
  `diverges (fix)` = gap di semantica EAPI. `diverges (cut, owner decides)` =
  differenza deliberata o da decidere. `not reproducible` = oracolo instabile.
- Rumore stabile escluso dal confronto (identico a ogni run): `masters
  attribute` warnings (fixture altrui, non toccati), `Invalid news item`
  (news fixture volutamente malformate), `Update type not recognized
  '# Quarter 2 2024 package moves (fixture)'`, `missing repo_name entries`,
  `Performing Global Updates` + progress. Eccezione: due run (`d-noeapi-pret`,
  `d-eapisp-nc-pret` prima versione) hanno listato anche `/.gentoo/...`
  updates host — one-off non riprodotti, run successive pulite; nessun effetto
  sulla risoluzione né scritture host (verificato via mtime, vedi §6).

## 1. Fixture nuove (solo `repo-348`, nomi `t348-d-*`, cat. `app-misc`)

13 package dir, 21 ebuild, 1 eclass `repo-348/eclass/t348-d.eclass`
(`RESTRICT="t348ec-restrict"`, `PROPERTIES="t348ec-prop"`). Varianti EAPI 7/8
come versioni `-7`/`-8` dello stesso PN:

| PN | versioni | contenuto |
|---|---|---|
| `t348-d-good` | `-1` (EAPI 8) | controllo valido, per comandi a due target |
| `t348-d-eapi99` | `-1` (`EAPI=99`) | EAPI non supportata |
| `t348-d-eapi7a` | `-1` (`EAPI=7_alpha`) | EAPI non supportata (suffisso PMS-valido) |
| `t348-d-eapisp` | `-1` (`EAPI=" 8"`) | spazi dentro le virgolette |
| `t348-d-noeapi` | `-1` (senza riga EAPI) | + `IDEPEND` (vedi §4) |
| `t348-d-eapimm` | `-1` (EAPI 8) | + `IDEPEND`; cache ritoccata EAPI→7 |
| `t348-d-accum` | `-7`/`-8` | `inherit t348-d` + `RESTRICT`/`PROPERTIES` propri |
| `t348-d-idep` | `-7`/`-8` | `IDEPEND="app-misc/t348-d-good"` |
| `t348-d-sfall` | `-7`/`-8` | `S=${WORKDIR}/t348-d-missing` + `src_prepare(){ :; }` |
| `t348-d-sfallb` | `-7`/`-8` | S mancante, senza `src_prepare` |
| `t348-d-live` | `-7`/`-8` | `PROPERTIES="live"` |
| `t348-d-requse` | `-7`/`-8` | `IUSE="t348dfoo t348dbar"`, `REQUIRED_USE="t348dfoo? ( t348dbar )"` (violata via `package.use/t348-d`: `t348dfoo`) |
| `t348-d-pret` | `-7`/`-8` | `pkg_pretend()` che `die` |
| `t348-d-unmerge` | `-1` (EAPI 8) | installa un file; `pkg_prerm/postrm` loggano in `${EROOT}/tmp/t348d-lifecycle.log` |

md5-cache: generate con `egencache --config-root $CFG --repo t348 --update
app-misc/<pn>` (solo i miei pacchetti). `_eclasses_` di `t348-d-accum-*`
riporta md5 reale dell'eclass (`08308972…`, verificato con md5sum).
Hand-edit (solo miei file, `_md5_` = md5 dell'ebuild quindi la validazione
`template.py:245` resta verde; sopravvivono ai rerun egencache che saltano le
entry up-to-date):
- `t348-d-eapimm-1`: `EAPI=8→7` (file dice 8);
- `t348-d-noeapi-1`: `EAPI=0→8` + riga `IDEPEND` aggiunta (egencache l'aveva
  droppata con EAPI 0, come per `idep-7`).

## 2. Tabella celle

`P:` = Portage `portage/__init__.py`, `E:` = `portage/eapi.py`,
`PT:` = `portage/dbapi/porttree.py`, `GMS:` =
`portage/package/ebuild/getmaskingstatus.py`, `DD:` = `doebuild.py`,
`DG:` = `_emerge/depgraph.py`, `EX:` = `portage/exception.py`,
`pdep:` = `crates/portuale/src/ebuild_phases/depend_phase.rs`,
`pmod:` = `crates/portuale/src/ebuild_phases/mod.rs`,
`peapi:` = `crates/portage-dep/src/eapi.rs`,
`pout:` = `crates/portuale/src/pretend/`.

### 2a. EAPI non supportata agli ingressi

| cella | P rc / Q rc | verdetto | oracolo → portuale |
|---|---|---|---|
| `d-eapi99-pret`, `d-eapi7a-pret` (`emerge -p`) | 1 / 1 | **diverges (fix)** | `!!! … have been masked.` `- …::t348 (masked by: EAPI 99)` + `The current version of portage supports EAPI '9'. You must upgrade…` (`GMS:89`, `DG:7010`) → `emerge: there are no ebuilds to satisfy "=…".` + `searching for similar names…` (che non lista nemmeno il pacchetto) (`pout:entry_line.rs:527`). portuale droppa invece di mascherare-per-EAPI, pur conoscendo il supporto (`peapi.rs:8` lista 0–9). |
| `d-eapi99-pret2`, `d-eapi7a-pret2` (`-p` bad + good) | 1 / 1 | **diverges (fix)** | Stesso pattern; il target valido non viene mergiato da nessuno (tutto rc 1), ma Portage nomina il pacchetto mascherato, portuale solo l'atomo mancante. |
| `d-eapi99-oneshot`, `d-eapi7a-oneshot`, `d-eapi99-buildpkg`, `d-eapi7a-buildpkg` | 1 / 1 | **diverges (fix)** | Stesso mask-testo vs `no ebuilds` (file: `d-eapi99-oneshot.*`). |
| `d-eapi99-clean`, `d-eapi7a-clean` (`ebuild <f> clean`) | 1 / 0 | **diverges (fix)** | `!!! Unable to do any operations on '…', since its EAPI is higher than this portage version's. Please upgrade to a portage version that supports EAPI '99'.` (`EX:228-230`, misma `UnsupportedAPIException`, `DD:571-572`), stdout vuoto → `ebuild: dry run -- … not yet implemented … commands: ["clean"]`, rc 0. |
| `d-eapi99-pretend`, `d-eapi7a-pretend` | 1 / 0 | **diverges (fix)** | Stesso rifiuto `EX:228-230` → silenzio rc 0 (fase mai eseguita né rifiutata). |
| `d-eapi99-install`, `d-eapi7a-install` | 1 / 0 | **diverges (fix)** | Stesso rifiuto → portuale ESEGUE l'intera catena fasi (marker `.setuped/.unpacked/.prepared/.configured/.compiled`, `Final size of build directory/installed tree: 1 KiB`) su ebuild di EAPI ignota. Nessuna scrittura VDB (`install` ≠ merge) ma esecuzione fasi piena. |
| `d-eapimm-pret` (file 8 + cache 7 + IDEPEND) | 0 / 0 | **diverges (fix)** | Portage tira `t348-d-good-1` (usa la stringa IDEPEND della cache **senza** gate EAPI: `edepend[k] = metadata[k]`, `DG:4155-4157`; il gate IDEPEND vive solo a generazione, provato da `idep-7` senza riga IDEPEND). portuale NON tira (gate `idepend` su EAPI cache a risoluzione, `peapi.rs:165`). A stati consistenti i due concordano (vedi `idep7/8-pret`): diverge solo su skew cache/file — esattamente il caso del task. |
| `d-noeapi-pret` (file senza EAPI + cache 8 + IDEPEND) | 0 / 0 | same | Entrambi tirano `good-1`: entrambi fidano la cache (file sarebbe EAPI 0 → niente IDEPEND). |
| `d-eapimm99-pret` (solo Portage; cache EAPI→99 transiente poi ripristinata a 7) | 0 / – | riferimento oracolo | rc 0 + pull: cache con EAPI non supportata **disregardata**, file risorgentato (`PT:647-652`). Conferma la regola citata; stato persistito = EAPI 7 (repro: `sed -i s/^EAPI=7$/EAPI=99/`). |
| `d-eapimm-clean`, `d-noeapi-clean`, `d-good-clean` | 0 / 0 | **diverges (cut, owner decides)** | Portage rc 0 silenzioso; portuale avviso dry-run `clean` non implementato. Proposta default: tenere l'avviso (comandi volutamente no-op; vedi `portuale ebuild --help`). |
| `d-binpkg-good` (`-K --usepkgonly`, binpkg valido) | 0 / 0 | same | Merge binario ok entrambi; transcript quasi identico (Portage aggiunge `Extracting…`). |
| `d-binpkg-merge` (`-K` idep-8 senza binpkg del dep) | 1 / 1 | **diverges (fix)** | Portage: `emerge: there are no binary packages to satisfy "app-misc/t348-d-good". (dependency required by … [binary])`. portuale: rc 1 con stdout `Calculating dependencies … done!` e **stderr vuoto** (fallimento muto). |
| `d-binpkg-eapi99` (`-K` binpkg con EAPI=99 craftato: `metadata/EAPI`, environment, ebuild interno a 99 + `Packages` con `EAPI: 99` e hash aggiornati; originale ripristinato e verificato md5 dopo) | 1 / 0 | **diverges (fix)** | Portage: `- app-misc/t348-d-good-1 (masked by: EAPI 99)` + upgrade-text, rc 1 (nota: senza suffisso `::repo` per i binari). portuale: **mergia** rc 0 in target-ROOT, VDB con `EAPI=99`. Safety-relevant. Nota craft: un primo repack con dir-entry `metadata/` + ordine alfabetico + zstd -19 faceva fallire il reader portuale (`Invalid binary package … inner metadata member "metadata" is outside metadata/`); rebuild byte-fedele (ordine originale, no dir-entry, zstd default) accettato — il reader portuale è più stretto di Portage sul layout (osservazione, non verdetto). |
| `d-unmerge-99`, `d-unmerge-99b` (`emerge -C` con VDB EAPI=99, env intatto con `pkg_prerm/postrm`; bystander `accum-8`) | 0 / 0 | same | Entrambi rimuovono, bystander intatto, **prerm+postrm eseguiti** (`…99b.portuale.lifecycle`: `prerm/postrm EAPI=8` dall'environment salvato — `DD:568-572` inizializza PATH/BUILDDIR anche con EAPI corrotta proprio per l'uninstall). Delta: in `d-unmerge-99` (senza `${EROOT}/tmp` pre-creata) i redirect log falliscono su portuale; Portage garantisce `$EROOT/tmp` 1777, portuale no (755 ereditata da mkdir). |
| `d-unmerge-setup`, `d-unmerge-setup2` | 0 / 0 | same | Install sorgente ok entrambi (setup iniziale fallì identico su entrambi per S mancante — stesso die `phase-functions.sh`, poi probe fixata con `S="${WORKDIR}"`; il fallimento non è in file, ma il die è coperto dalle celle sfall). |

### 2b. RESTRICT/PROPERTIES accumulati, IDEPEND al merge, S/WORKDIR, live, gate

| cella | P rc / Q rc | verdetto | oracolo → portuale |
|---|---|---|---|
| `d-accum7-pret`, `d-accum8-pret` | 0 / 0 | same | `-p` identico. La semantica è negli artefatti egencache (stato persistito): EAPI 7 `RESTRICT=t348eb-restrict` (overwrite), EAPI 8 `RESTRICT=t348eb-restrict t348ec-restrict` (accumulo) — `bin/ebuild.sh:304-368,713-717`, `___eapi_has_accumulated_*` (`bin/eapi.sh:21-22`, veri solo per EAPI 8+). |
| `d-idep7-pret` | 0 / 0 | same | Niente pull entrambi (IDEPEND droppato a generazione per EAPI 7). |
| `d-idep8-pret` | 0 / 0 | same | Ordine `good-1` poi `idep-8`; entrambi omettono `to <root>` sulla riga IDEPEND (merge in running-root, `DG:4252`). |
| `d-idep8-merge` (`--oneshot` in ROOT ermetica) | 1 / 0 | **diverges (fix)** | Portage: `!!! Read-only file system: /var/db/pkg` — il dep IDEPEND va nella **running root** (`_running_root`, `DG:4252`), non redirigibile via env/make.conf (`broot = EPREFIX`, `LocationsManager.py:99`; nessuna opzione `--broot`). portuale: mergia entrambi in TARGET root rc 0. Merge-path, safety-relevant. |
| `d-idep8-buildpkgonly` | 0 / 0 | same | Entrambi compilano solo il target (Portage scarta IDEPEND/DEPEND sotto `--buildpkgonly`, `DG:4183`); binpkg `.gpkg.tar` prodotti da entrambi (nomi diversi: `t348-d-idep/t348-d-idep-8-1.gpkg.tar` vs `t348-d-idep-8.gpkg.tar` — osservazione formato, fuori perimetro EAPI). |
| `d-sfall7-prepare`, `d-sfall8-prepare` (S mancante, `src_prepare` definita) | 1 / 1 | same | Stesso die `The source directory '…' doesn't exist` (no fallback al floor 7: `___eapi_has_S_WORKDIR_fallback` = EAPI 0–3, `bin/eapi.sh:8`; cfr `phase-functions.sh:509`). Delta noti: stream, `::t348`, linea 444 vs 514. |
| `d-sfallb7-prepare`, `d-sfallb8-prepare` (S mancante, nessuna `src_prepare`) | 0 / 0 | same | `cd WORKDIR` silenzioso entrambi (ramo `[[ -z ${A} ]] && ! __has_phase_defined_up_to`, `phase-functions.sh:511`). portuale non stampa i `>>>` ma i marker `.setuped/.unpacked/.prepared` ci sono (riverificato ad hoc). |
| `d-live7-pret`, `d-live8-pret` | 0 / 0 | same | `PROPERTIES=live` nessuna differenza osservabile entrambi. |
| `d-requse7-setup`, `d-requse8-setup` (`ebuild setup`, REQUIRED_USE violata) | 1 / 0 | **diverges (fix)** | Portage: `\n  The following REQUIRED_USE flag constraints are unsatisfied:\n    t348dfoo? ( t348dbar )` (`DD:~1943` `eapi_has_required_use`). portuale: rc 0 muto, nessun gate. |
| `d-requse7-pret`, `d-requse8-pret` | 1 / 1 | same | Blocco use identico (`- …::t348 USE="t348dfoo -t348dbar" ELIBC="glibc"` + constraints), a meno del suffisso `for <root>` (convenzione d). |
| `d-pret7-pretend`, `d-pret8-pretend`, `d-pret7-oneshot`, `d-pret8-oneshot` | 1 / 1 | same | `pkg_pretend` eseguita e `die` propagato identico entrambi (EAPI 7 e 8 hanno `pkg_pretend`); delta noti: stream, `::t348`. |
| `d-*-depend` (`ebuild <f> depend`: accum7/8, idep7/8, noeapi, eapimm, good) | 1 / 0 | **not reproducible (perché)** | L'oracolo stesso fa traceback: `TypeError: returnproc or returnpid must be True for depend phase` (`DD:1116`) — nessun testo atteso pinnabile. Confronto metadata via artefatti egencache (§1). portuale: dry-run notice. |
| `d-egencache-99`, `d-egencache-7a` (solo Portage) | 0 / – | riferimento oracolo | rc 0, **nessuna cache scritta**, nessun errore: ebuild EAPI non supportata saltati in silenzio. |
| `d-egencache-sp` (solo Portage) | 0 / – | riferimento oracolo | `EAPI=" 8"` genera cache con `EAPI=8` (stato persistito). |
| `d-good-buildpkg`, `d-good-pret` | 0 / 0 | same (pret) / riferimento (buildpkg: produttore del binpkg) | — |

Conteggio (69 celle base; 64 a due PM + 5 solo-oracolo):
**same 29, diverges (fix) 23, diverges (cut, owner decides) 5, not reproducible
7, solo-oracolo 5** — vedi conteggio per-riga sotto.
Fix: 14 ingressi EAPI-unsupported (7 probe × {pret,pret2,oneshot,buildpkg,clean,
pretend,install} contano 7+7) + eapimm-pret + idep8-merge + binpkg-merge +
binpkg-eapi99 + requse-setup ×2 + eapisp build-path ×3 (install,oneshot,
buildpkg — gap `eapply_user`, vedi sotto) = 7+7+1+1+1+1+2+3 = **23**.
Cut: clean-a-EAPI-valida ×5 (eapimm/noeapi/good/eapisp/eapisp-nc). notrepro:
depend ×7. Oracolo: egencache ×3 + eapimm99 + good-buildpkg. Same: gli altri
29 (inclusi `idep8-buildpkgonly`, `noeapi-pret`, `eapisp-nc-*`, tutti gli
`sfall*`, `live*`, `requse-pret*`, `pret*`, `unmerge*`, `accum*-pret`,
`binpkg-good`, `good-pret`, `eapisp-pret/pret2/pretend`).

## 3. Divergenze principali (una riga ciascuna)

1. `emerge` non maschera per EAPI: unsupported → `no ebuilds to satisfy` invece di `(masked by: EAPI …)` (14 celle ingressi; `GMS:89`, `pout:entry_line.rs:527`).
2. `ebuild <f> <fase>` non rifiuta EAPI ignota: `install` esegue tutte le fasi rc 0, `pretend` rc 0 muto, `clean` dry-run rc 0 — atteso `EX:228-230` rc 1 (`DD:571-572`).
3. Merge binpkg EAPI-99: Portage maschera, portuale installa (VDB `EAPI=99`).
4. Merge IDEPEND in target-ROOT invece che running-root (`DG:4252`).
5. `ebuild setup` non applica REQUIRED_USE (emerge invece sì, identico).
6. Risoluzione IDEPEND con skew cache/file: Portage presence-based (`DG:4155`), portuale gated su EAPI cache.
7. `eapisp` (`EAPI=" 8"`, EAPI 8 effettiva): il build path portuale muore in `default_src_prepare` — `eapply_user: command not found` (`phase-helpers.sh:849`) — mentre Portage compila/mergia rc 0; gap env fasi, non grammatica EAPI.
8. `-K` fallito muto (stderr vuoto) vs `there are no binary packages…`.

## 4. Non riproducibile / instabile (con ragione)

- **`EAPI=" 8"` senza cache (stato transiente, celle `d-eapisp-nc-*`):**
  le prime run storiche (senza cache, file appena creati) mostravano
  `masked by: corruption` rc 1 ovunque (`GMS:60`) e un run egencache falliva
  con l'errore PMS 7.3.1 — **mai più riprodotti** (3+ rerun egencache ok;
  `-p`/`ebuild` odierni senza cache: rc 0 visibile, anche con tmp isolata,
  pacchetto disinstallato e cache assente). Causa non isolata (possibile race
  egencache parallelo o memoization opaca; i file originali sono stati
  sovrascritti dai recapture, quindi resta solo questa descrizione, non celle).
  Le celle `nc` persistite documentano lo stato no-cache **stabile odierno**
  (rc 0 entrambi; `R` in pret = eapisp installato in $CFG dal merge with-cache,
  non un effetto cache).
- **`ebuild <f> depend` (7 celle):** traceback dell'oracolo stesso, nessun
  atteso definibile (`DD:1116`).
- **Condivisione `PORTAGE_TMPDIR` tra i due PM:** i build lasciano marker
  condivisi (es. `d-eapisp-nc-install.portuale` rc 0 muto = probabile riuso
  marker del build Portage precedente). I verdetti rc restano validi; i testi
  di dettaglio dei build successivi al primo per pacchetto vanno letti con
  questa cautela. Raccomandazione S0.8: tmp separati per PM nei recapture.

## 5. Decisioni per il proprietario (non mi fermo; default proposti)

1. `clean`/`depend` dry-run (5 celle cut): default = tenere l'avviso (comandi
   volutamente non implementati); il gate EAPI prima dell'avviso solo per
   `install/merge/pretend/setup/unmerge` (safety).
2. `eapply_user: command not found` in env fasi portuale (celle eapisp
   build-path): è il gap IPC/PATH noto (fatto 3 del piano) o bug PATH a sé?
   Default: trattarlo come slice S4 (helpers), non S5.
3. Nomi file gpkg diversi tra i PM (`…-8-1.gpkg.tar` vs `…-8.gpkg.tar`) e
   reader portuale più stretto sul layout `metadata.tar.zst`: fuori perimetro
   EAPI; default = nota per il proprietario del formato binpkg.
4. `::` vs `::t348`, stdout-vs-stderr fasi, `for <root>` omesso: sistematiche
   portuale; default = `same` con nota (come in tabella), triage a S0.8.
5. Stati VDB/tmp in $CFG dopo le celle (eapisp-1 mergiato da Portage,
   t348d-lifecycle.log in rootU1/U2, pkgdir con binpkg validi): lascio tutto
   in /var/tmp (scratch) — in repo solo fixture+cache volute.

## 6. Note ambientali e di sicurezza (verificate)

- Mai `ROOT=/`; nessun write host a config/VDB (`/etc/portage`,
  `/var/db/pkg`, `/var/log/portage` invariati per mtime; i `M fixtures/etc/
  profile.env`, `fixtures/var/lib/portage/world` in pmtest sono sporchi
  preesistenti, non miei — `git diff` invariato su quei path dalle mie run).
- Portage con ROOT ermetico appende comunque a `/.gentoo/log/emerge.log` e
  alla dep-cache host: da quando il runner esporta `PORTAGE_LOGDIR` e
  `PORTAGE_DEPCACHEDIR` ermetici (convenzione pmtest) non più.
- `fixtures/` è in pmtest `.git/info/exclude` (`:/fixtures`): le 34 nuove
  fixture esistono su disco ed escluse da git per design; niente commit (S0).
- portuale non ricompilato; niente pytest/cargo; niente pkill; scratch in
  /var/tmp; nessun delega/subagent.

## 7. Riepilogo file creati

- `LLM/evidence/2026-10-12-348-s0/d/`: 69×4 file cella (+2 `.lifecycle`)
  ≈ 280 file; `../scripts/d-run.sh` (+fix `--root`, +`BROOT`/log/depcache
  ermetici); questo file.
- `../pmtest/fixtures/repo-348/`: 14 dir `app-misc/t348-d-*` (21 ebuild),
  `eclass/t348-d.eclass`, 19 entry md5-cache (99/7a: nessuna, come l'oracolo).
