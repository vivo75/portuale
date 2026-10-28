# Task 1 S0 — C2 puro-ordine (baseline l0-20261025T145359Z)

Metodo: per ognuna delle 22 righe C2, confronto multiset delle merge-row
(`[ebuild|binary|blocks`) tra `real/<probe>.txt` e `portuale/<probe>.txt`
stesso log (script in sessione, `collections.Counter`). Una riga è
puro-ordine solo se i due multiset sono identici.

## Esito

Puro-ordine, set identici (20 probe, counts real==ptl):

- sys-apps_systemd (24/24), dev-lang_ruby (62/62), dev-ruby_rake (62/62),
  kde-frameworks_kcoreaddons (83/83), kde-apps_kdecore-meta (256/256),
  gui-libs_gtk_4 (120/120), x11-libs_gtk__3 (109/109),
  net-misc_networkmanager (67/67), sys-auth_polkit (24/24),
  app-text_texlive (146/146), app-text_texlive-core (102/102),
  dev-texlive_texlive-latex (105/105), www-client_firefox (145/145),
  mail-client_thunderbird (148/148), net-analyzer_wireshark (157/157),
  media-video_vlc (118/118), x11-base_xorg-server (80/80),
  _system (70/70), _world (77/77), MULTI_emptytree-system (370/370).

NON puro-ordine (2 probe, set diversi — non spostare a EXPLAINED qui):

- app-office_libreoffice: real 230 vs ptl 228. Solo real:
  `dev-perl/Sub-Name-0.280.0-r1`, `perl-core/Encode-3.210.0`.
- media-gfx_gimp: real 183 vs ptl 181. Solo real: gli stessi 2 ebuild.
  Causa già nominata: #567 version-selection (Encode/Sub-Name), yaml
  `perl-544-stabilization-version-selection-{encode,sub-name}` già
  explained. Nessun nuovo follow-up.

## Ruling (deviazione dal Task 1 Step 2)

Lo Step 2 chiedeva di spostare le 20 righe puro-ordine a EXPLAINED come
`merge-order-only`. Non lo faccio.

Motivo: "solo ordine" non è una causa nominata (regola #558, Global
Constraints). Il pattern è sistematico, non rumore: in 10 probe la riga
portuale è `virtual/libelf-3-r1:0/1` dove Portage mette un pacchetto
reale (nasm, freetype, clang-common, Module-Runtime) — stesso virtual
anticipato in probe indipendenti. Sembra una differenza sistematica del
serializzatore, non traversal noise CPython (cfr. le entry esistenti
`l0-vm-system-virtual-order`, `gedit-nautilus-cluster-a-rewalk-abort`,
che hanno ciascuna una ragione meccanica). Spostarle in blocco
launderebbe un possibile bug d'ordine come drift.

Decisione: nessuna modifica a `known-divergences.yaml` in questo task.
Le 20 righe restano UNEXPLAINED. Il follow-up corretto è uno S0
d'ordine (confronto trace del serializzatore sul pattern libelf-early,
primo libero 617+), oppure trattenerle nel residuo #558 come
"C2 puro-ordine, causa non nominata". La scelta del numero/fix sta al
Task 3 con l'owner.
