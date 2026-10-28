# #558 S0 inventory — baseline l0-20261025T145359Z

Baseline: `../pmtest/differential-test-bed/logs/l0-20261025T145359Z/`
`pm.json`: `repo_pins_match_file: true`, gentoo `350c0882`.
Report: 120 probe, 88 clean, parity 0.733, 27 explained, **44 UNEXPLAINED** (order 22, advice 9, error 6, exit 2, flags 1, totals 2, truncated 2).

| n | probe | classe | yaml-id | dettaglio |
|---|-------|--------|---------|-----------|
| 1 | sys-apps_systemd | C2 puro-ordine | nessuno | order #2: real dev-lang/nasm vs portuale virtual/libelf-3-r1:0/1 |
| 2 | dev-lang_ruby | C2 puro-ordine | nessuno | order #23: real dev-ruby/did_you_mean vs portuale dev-ruby/matrix |
| 3 | dev-ruby_rake | C2 puro-ordine | nessuno | order #23: real dev-ruby/did_you_mean vs portuale dev-ruby/matrix |
| 4 | kde-frameworks_kcoreaddons | C2 puro-ordine | nessuno | order #8: real media-libs/freetype vs portuale virtual/libelf-3-r1:0/1 |
| 5 | kde-apps_kdecore-meta | C2 puro-ordine | nessuno | order #9: real media-gfx/qrencode-4.1.1-r1:0/4 vs portuale sys-libs/libutempter |
| 6 | gui-libs_gtk_4 | C2 puro-ordine | nessuno | order #9: real llvm-core/clang-common vs portuale virtual/libelf-3-r1:0/1 |
| 7 | x11-libs_gtk__3 | C2 puro-ordine | nessuno | order #9: real llvm-core/clang-common vs portuale virtual/libelf-3-r1:0/1 |
| 8 | net-misc_networkmanager | C2 puro-ordine | nessuno | order #7: real media-libs/freetype vs portuale virtual/libelf-3-r1:0/1 |
| 9 | sys-auth_polkit | C2 puro-ordine | nessuno | order #2: real dev-lang/nasm vs portuale virtual/libelf-3-r1:0/1 |
| 10 | app-text_texlive | C2 puro-ordine | nessuno | order #10: real dev-perl/Module-Runtime vs portuale virtual/libelf-3-r1:0/1 |
| 11 | app-text_texlive-core | C2 puro-ordine | nessuno | order #6: real dev-perl/Module-Runtime vs portuale virtual/libelf-3-r1:0/1 |
| 12 | dev-texlive_texlive-latex | C2 puro-ordine | nessuno | order #6: real dev-perl/Module-Runtime vs portuale virtual/libelf-3-r1:0/1 |
| 13 | www-client_firefox | C2 puro-ordine | nessuno | order #17: real llvm-core/clang-common vs portuale virtual/libelf-3-r1:0/1 |
| 14 | mail-client_thunderbird | C2 puro-ordine | nessuno | order #17: real llvm-core/clang-common vs portuale virtual/libelf-3-r1:0/1 |
| 15 | app-office_libreoffice | C2 puro-ordine | nessuno | order #18: real sys-libs/binutils-libs vs portuale dev-libs/boehm-gc |
| 16 | media-gfx_gimp | C2 puro-ordine | nessuno | order #15: real llvm-core/clang-common vs portuale virtual/libelf-3-r1:0/1 |
| 17 | net-analyzer_wireshark | C2 puro-ordine | nessuno | order #42: real acct-group/pcap vs portuale net-libs/libpcap |
| 18 | media-video_vlc | C2 puro-ordine | nessuno | order #7: real sys-libs/mtdev vs portuale x11-libs/xtrans |
| 19 | x11-base_xorg-server | C2 puro-ordine | nessuno | order #74: real xf86-video-vesa vs portuale xf86-video-amdgpu |
| 20 | _system | C2 puro-ordine | nessuno | order #9: real app-arch/xz-utils vs portuale virtual/editor |
| 21 | _world | C2 puro-ordine | nessuno | order #11: real app-arch/xz-utils vs portuale virtual/editor |
| 22 | MULTI_emptytree-system | C2 puro-ordine | nessuno | order #144: real virtual/perl-Test-Harness vs portuale sys-libs/libxcrypt |
| 23 | app-containers_podman | C4/truncated | nessuno | truncated: real Total 8/8 vs portuale 44/44 |
| 24 | kde-plasma_plasma-meta | C4/truncated | nessuno | truncated: real Total 4/4 vs portuale 470/470 |
| 25 | MULTI_deep-update-world | C4/exit | nessuno | exit: real rc=0 vs portuale rc=1 |
| 26 | MULTI_deep-update-world | C4/exit | nessuno | exit: merge-list soppressa 141 vs 107 righe |
| 27 | app-office_libreoffice | legata #567 | version-selection | totals: real 230 vs portuale 228 (= 2 missing Encode/Sub-Name) |
| 28 | media-gfx_gimp | legata #567 | version-selection | totals: real 183 vs portuale 181 (= 2 missing) |
| 29 | kde-plasma_plasma-meta | pre-esistente | nessuno | advice real-only masked-atom: net-print/cups |
| 30 | kde-plasma_plasma-meta | pre-esistente | nessuno | advice real-only masked: All ebuilds that could satisfy cups have been masked |
| 31 | kde-plasma_plasma-meta | pre-esistente | nessuno | advice real-only masked: One of the following masked packages (cups) |
| 32 | kde-plasma_plasma-meta | pre-esistente | nessuno | error real-only: All ebuilds that could satisfy cups |
| 33 | kde-plasma_plasma-meta | pre-esistente | nessuno | error real-only: One of the following masked packages (cups) |
| 34 | gnome-base_nautilus | pre-esistente | nessuno | advice real-only masked-atom: >=app-crypt/gcr-3.27.90:0=[gtk] |
| 35 | gnome-base_nautilus | pre-esistente | nessuno | advice real-only masked: All ebuilds gcr |
| 36 | gnome-base_nautilus | pre-esistente | nessuno | advice real-only masked: One of the following (gcr) |
| 37 | gnome-base_nautilus | pre-esistente | nessuno | error real-only: All ebuilds gcr |
| 38 | gnome-base_nautilus | pre-esistente | nessuno | error real-only: One of the following (gcr) |
| 39 | app-editors_gedit | pre-esistente | nessuno | advice real-only masked-atom: >=app-crypt/gcr-3.27.90:0=[gtk] |
| 40 | app-editors_gedit | pre-esistente | nessuno | advice real-only masked: All ebuilds gcr |
| 41 | app-editors_gedit | pre-esistente | nessuno | advice real-only masked: One of the following (gcr) |
| 42 | app-editors_gedit | pre-esistente | nessuno | error real-only: All ebuilds gcr |
| 43 | app-editors_gedit | pre-esistente | nessuno | error real-only: One of the following (gcr) |
| 44 | MULTI_emptytree-system | da-verificare | nessuno | flags: dev-build/autoconf real [NS] vs portuale [R] |

Regola #558: nessuna riga si sposta senza causa nominata. #573 resta di #573. Zero move yaml in questo task.
