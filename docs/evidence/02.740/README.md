# Evidence for 02.740 — the fast plan takes the `||` branch that keeps a package newest

Host captures, fresh at HEAD `2a046183` (2026-11-14). Commands:

- `target/release/portuale emerge -pv --json --update --deep --newuse --getbinpkg=n @world > resolver.json`
- `target/release/portuale mrg --plan-world --json > fast.json`
- `target/release/portuale mrg --check-world --json > check.jsonl` (exit 1: one
  `world-no-match`, `app-misc/mime-types[nginx]`, also present as `[judge]` rows)
- `python3 ../pmtest/differential-test-bed/compare/world-crosscheck.py resolver.json fast.json --check check.jsonl > cross-before.txt`

## S0 mechanism (temporary `eprintln!`, removed after capture; full output in `requesters.stderr`)

- `branch_preference`: arm `[dev-libs/openssl-compat:3]` -> `Available`,
  arm `[dev-libs/openssl:0/3]` -> `Installed`, so the fast walk takes arm 2.
- `dev-libs/openssl:0` reorder: `chosen=3.6.5`; atoms aimed include
  `dev-libs/openssl:0/3` requested by exactly `app-editors/vscode-1.145.0`,
  `app-shells/pwsh-bin-7.6.5-r1`, `dev-dotnet/dotnet-sdk-bin-10.0.113-r1`,
  `net-vpn/microsoft-azurevpnclient-3.0.0-r1`, plus `dev-libs/openssl:=`
  requested by kept `app-admin/sudo-1.9.17_p2-r1` (whose best candidate is 4.1.0).
  The `:0/3` requester is why `chosen()` keeps 3.6.5: the arm *and* the reorder
  pin, as plan §1 requires.

## S0 reinstall split (from `resolver.json` entry flags)

- 112 reinstalls = 87 `slot_operator_rebuild` + 25 `changed_use` + 0 other.
- Resolver-only reinstalls: 101 = 76 slot-op + 25 changed-use (slot-keyed; the
  101st is `llvm-core/clang:22`, whose cp also has a fast row in slot 23).
- Of the 76 slot-op rows, 71 name `dev-libs/openssl` in `dependencies`;
  5 do not: `media-plugins/gst-plugins-libav`, `kde-frameworks/kfilemetadata`,
  `dev-qt/qtmultimedia`, `kde-plasma/kpipewire`, `media-libs/gegl`.
- The 5 record (installed VDB `RDEPEND`/`DEPEND`) `media-video/ffmpeg:0/60.62.62=`
  and, variously, `exiv2:0/0.28=`, `taglib:0/2=`, `pipewire:0/0.4=`, `libva:0/2=`,
  `libjpeg-turbo:0/0.2=`, `libnsgif:0/1.0.0=`, `libpng:0/16=`, `zlib:0/1=`,
  `tiff:0/6=`, `libwebp:0/7=`, `gobject-introspection:0/0=`, `libraw:0=`.
  Installed provider SLOTs all match the recorded bindings; no provider of the 5
  moves in either plan (ffmpeg kept 8.1.3 both; pipewire 1.6.9-r1 -> 1.6.11,
  sub-slot 0.4 -> 0.4, both; the rest kept both). Per the plan, no guessing:
  filed as a residue number at close-out.

## Drift vs the 2026-11-13 snapshot (`/tmp/opencode/p740/`)

- 102 -> 101 resolver-only reinstalls: `app-crypt/gocryptfs` is now a fast-plan
  `reinstall` row too (its `>=dev-lang/go-1.24.11:=` BDEPEND; #741's stale-pin
  arm landed since: fast reinstalls 10 -> 30, the 20 go consumers).
- New `resolver-only dev-util/cargo-c:0` upgrade row: tree gained
  `cargo-c-0.10.26.ebuild` after 11-13; nothing installed or in `@world`
  requires it and no plan entry's `dependencies` mentions it, so the puller is
  unidentified — recorded, not guessed; re-judged in S2.
- `llvm-core/clang` has two slots in play (`:22` reinstall with changed USE,
  `:23` upgrade in both plans); unchanged since 11-13.

## S2 after-state (`*-after.*`, same commands with the plan `02.740` binary)

- `cross-after.txt`: the `resolver-only dev-libs/openssl:0` (upgrade) and
  `dev-libs/openssl-compat:3` (new) rows are gone — the fast plan now lists
  `[ebuild     U  ] dev-libs/openssl-4.1.0:0 [3.6.5]` and
  `[ebuild  N     ] dev-libs/openssl-compat-3.6.5:3`.
- `resolver-only-reinstall` 101 -> 26. The 75 that became fast rows: the 71
  slot-operator rows naming `dev-libs/openssl` in `dependencies` (all now fast
  `reinstall` rows via the #741 rebuild scan, property 1) plus 4
  `llvm_slot_18` changed-use rows that also record an openssl pin
  (`dev-db/postgresql:17`, `:18`, `dev-util/bpftool`, `dev-util/perf`). (A 76th
  added fast reinstall, `pkgcruft-git`, is covered below.) The 26
  that remain: the same 5 unmoved-provider slot-operator rows as S0 (filed as
  backlog #748) plus 21 changed-use rows (`plan-world-newuse` stays off by
  owner decision).
- `app-crypt/sbsigntools:0` and `sys-devel/bpf-toolchain:0` upgrades are now
  in both plans (before: resolver-only). Not arm churn: rebuilt `efitools`
  (now an openssl reinstall row) follows its `BDEPEND` on `sbsigntools`, and
  rebuilt `systemd` follows its `bpf? ( ... sys-devel/bpf-toolchain ) DEPEND`
  — the documented cascade of a built package contributing every dependency
  variable.
- New `fast-only` rows, each property-supported: `dev-vcs/pkgcruft-git:0`
  `reinstall` (its recorded build-time `openssl:0/3=` binding repaired; the
  #741 rule for a walked consumer on a source build — the resolver leaves it
  kept, a pre-existing scan-scope difference, out of scope) and
  `[note] fast-stale-binding-unrepaired dev-util/cargo-c
  (dev-libs/openssl:0/3=: no visible ebuild)` (property 5: cargo-c 0.10.25
  records a sub-slot-3 pin with nothing visible to rebuild to — its tree
  ebuild now says bare `:=` — so the plan keeps it and names the binding once;
  the resolver instead upgrades cargo-c to the new 0.10.26 ebuild, whose row
  stays resolver-only: explained tree drift plus known walk scoping, no number).
- `diff check.jsonl check-after.jsonl` is empty (plan-mode-only change; both
  runs exit 1 on the single pre-existing `mime-types[nginx]` world-no-match).
- The resolver side is unchanged as a set (same `(cp, slot, version, outcome)`
  rows before/after; only scheduling fields move).
- Unchanged from before: the two `fast-only` new-slot rows
  (`rust-bin:1.99.0`, `gexiv2:0.16`) and the `world-invalid-but-planned`
  `mime-types[nginx]` pair.

## S2 beds (2026-11-14 runs vs the 2026-11-13 baselines)

- `l0-health.sh` run `l0-health-20261114T011717Z` vs baseline
  `l0-health-20261113T214020Z`: the finding sets are identical (683 findings
  both sides, zero added, zero removed; `health-report.txt` differs only in
  timing lines). 122 plans, 113 checked, 0 with a failure portuale introduced;
  the world cross-check section is unchanged (584 atoms, 409 slots,
  0 unsatisfiable; 20 fast-only, 1 resolver-only-reinstall). The run used the
  slice binary (`pm.json` `binary_sha256` matches `target/release/portuale`).
- Default `l0-resolver.sh` run `l0-20261114T012111Z` vs baseline
  `l0-20261113T214838Z`: UNEXPLAINED 0 both sides (120 probes, clean 91,
  parity 0.758). The explained-finding sets are identical (same cells, same
  ebuild rows); every portuale-side cell capture is byte-identical across the
  two runs, every Portage-side capture differs only in its
  `Dependency resolution took N s` timing line, and the dev-lang_perl
  `[extra]` display order differs cosmetically with byte-identical plans
  behind it. No behavioral churn. The 3 allowlist entries matching nothing
  (incl. #747's `l0-737-openssl-world-late-reinstall-order`) match nothing in
  both runs — #747 stays open, untouched by this slice.
