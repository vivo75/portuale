# Evidence: `emerge -uDpvN @world`, real Portage vs portuale (2026-09-16)

Frozen copies of the comparison outputs that ground backlog #63–#68. The
originals lived in a scratch directory (`helpers/real_emerge/`, not part of
the tracked tree); these copies are committed so plan documents can cite
stable paths.

Capture facts:

- Host: the developer workstation (`vivo`), a real Gentoo systemd/plasma
  amd64 tree; date 2026-09-16, ~12:47–12:54 local time.
- real Portage 3.0.82.2 (`/usr/bin/emerge`).
- portuale: workspace `main` @ `e54bd098659d970fabc6a31339f40fcd30b6f0ef`,
  binary `rust/target/release/portuale`, sha256 prefix `78026c3a120849da`.
- The host `/etc/portage/make.conf` contains `source /etc/make.local`; the
  sourced file sets
  `EMERGE_DEFAULT_OPTS="... --binpkg-respect-use=y --load-average=11 --autounmask=y --ask-enter-invalid --usepkg=n --getbinpkg=y"`.
- Active profile `/.gentoo/repos/buildovl/profiles/vivo/desktop`; its repo
  (`buildovl`) declares `profile-formats = portage-2 profile-set profile-bashrcs`
  in `metadata/layout.conf`, and the profile's `packages` file is 100
  **unstarred** lines (the `@profile` set).

Commands captured (both binaries, same argv):

```
emerge -uDpvN @world
emerge -uDpvN --tree @world
emerge -uDpvN --tree --unordered-display @world
emerge -uDpvN --columns @world
emerge -uDpvN --debug @world              # not copied: ~12 MB per side
portuale emerge -uDpvN --json @world      # only the abi_rebuilds extract is kept
```

Files:

| file | what it shows |
|---|---|
| `portage-emerge--uDpvN-world.txt`, `portuale-emerge--uDpvN-world.txt` | the merge lists compared package by package: 27 vs 68 packages |
| `portage-emerge--uDpvN--tree.txt`, `portuale-emerge--uDpvN--tree.txt` | dependency paths; the portuale tree exposes the spurious `rR`/`N` chains |
| `portage-emerge--uDpvN--columns.txt`, `portuale-emerge--uDpvN--columns.txt` | the same two lists in `--columns` form |
| `portage-emerge--uDpvN--tree--unordered.txt`, `portuale-emerge--uDpvN--tree--unordered.txt` | unordered variants |
| `portuale-abi_rebuilds.json` | the 22 `(provider, consumer)` pairs portuale scheduled as slot-operator ABI rebuilds (all `dev-lang/go-1.27.1` consumers plus the `media-libs/libdisplay-info-0.4.0` cascade) |

Observed deltas (the six causes; details in the plan files
[`01.063`](../../01.063-emerge_default_opts.md) …
[`02.066`](../../02.066-installed_buildtime_optional.md)):

1. portuale lists `dev-lang/go`/`dev-build/cmake` as `[ebuild]`; real lists
   them as `[binary gU]` (getbinpkg ignored — #63).
2. portuale is missing `net-misc/rclone`, `sys-kernel/linux-firmware`,
   `x11-libs/vte`, `gui-libs/vte-common` (the `@profile` set is not part of
   `@world`; `dev-libs/weston` is likewise absent, which is why the
   `media-libs/libdisplay-info` conflict is not seen — #64).
3. portuale rebuilds 20 Go consumers (`rR` bracket, `portuale-abi_rebuilds.json`)
   because their *build-time* `:=` dep on go is treated as rebuild-triggering
   (#65).
4. portuale updates/installs build-time-only deps of installed packages
   (`dev-python/flit-core` downgrade, `dev-util/source-highlight` new,
   `dev-python/pythran`, `dev-util/cargo-c`, `dev-lang/rust-1.94.0` new slot, … — #66).
5. portuale reinstalls `x11-base/xorg-drivers` (+ pulls the three
   `xf86-video-{dummy,fbdev,vesa}`) and `media-gfx/sane-backends` because
   `VIDEO_CARDS="-* …"` / `SANE_BACKENDS="-* …"` don't clear the parent
   profile's flags (#67).
6. portuale reports `[blocks B]` / `1 unsatisfied`, prints the
   "cannot be installed at the same time" error and exits 1; real reports
   `[blocks b]` / `all satisfied` and exits 0 (#68).
