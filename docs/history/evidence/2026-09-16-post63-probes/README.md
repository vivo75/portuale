# Evidence: post-#63 host probes, real Portage vs portuale (2026-09-16)

Captured during the plan review of #14/#59/#65–#69 on branch
`explore/real_emerge` @ `0de42ac` (so #63/#64 `d24d684` applied: portuale
now honours the host `EMERGE_DEFAULT_OPTS`, which carries
`--usepkg=n --getbinpkg=y --binpkg-respect-use=y`). The earlier set
[`2026-09-16-world-uDpvN/`](../2026-09-16-world-uDpvN/README.md) was
captured at `e54bd09`, **before** #63: real ran with `--getbinpkg`
(⇒ `--usepkg` ⇒ `bdeps` unset ⇒ built packages' DEPEND/BDEPEND dropped)
while portuale ran without it (build-time deps walked). Several
divergences filed from that set were therefore option mismatches.

Real Portage 3.0.82.2; portuale `rust/target/release/portuale`.

## Files

| file | what |
|---|---|
| `portage-emerge--uDpvN-world.txt` / `portuale-emerge--uDpvN-world.txt` | `emerge -uDpvN @world`, host defaults; 27 vs 50 packages; real rc 0, portuale rc 1 |
| `package-list.diff` | `diff real portuale` of the sorted package names (name munging of `rR` lines is cosmetic) |

`@world` divergences still present post-#63:

- #65: 20 Go consumers `rR` + `dev-lang/go` carries `r` in portuale; real
  `[binary gU] dev-lang/go-1.27.1-1` with no `r`, no consumer rebuilds.
- #67: `x11-base/xorg-drivers`, `xf86-video-{dummy,fbdev,vesa}`,
  `media-gfx/sane-backends` extra in portuale.
- #68: portuale `[blocks B] <dev-util/gtk-doc-1.36.1` / `Conflict: 1 block
  (1 unsatisfied)` / rc 1; real `b` / `all satisfied` / rc 0 (gtk-doc
  upgraded to 1.36.1 in the same run on both sides).
- #69: `dev-qt/qttools` `[ebuild R]` in real, missing in portuale.
- #66's packages (flit-core, pythran, source-highlight, uv, cargo-c, …)
  are **gone**: fixed by #63.

## Single-atom probes (not copied, key lines recorded here)

`emerge -puDvN --oneshot <atom>`, host defaults (build-time deps off):

| atom | real | portuale |
|---|---|---|
| `app-containers/runc` | 0 packages | 0 packages |
| `dev-libs/libtracefs` | 0 packages | 0 packages |
| `dev-python/scipy` | 13 packages | same 13 (no pythran) |
| `dev-python/cloudpickle` | 13 packages | same 13 (no flit-core downgrade) |

`emerge -puDvN --ignore-default-opts --oneshot <atom>` (`bdeps=auto`,
build-time deps of built packages walked):

| atom | real | portuale |
|---|---|---|
| `app-containers/runc` | 26 pkgs: `[ebuild r U] dev-lang/go-1.27.1`, **`rR` only `dev-go/go-md2man` and `app-containers/runc`**, `U dev-build/gtk-doc-am-1.36.1`, `N kdoctools`, `N mdit-py-plugins`, `N myst-parser`, `U cargo-c`, `U libgit2`, `[blocks B] <dev-util/gtk-doc-1.36.1` | 42 pkgs: go `r U` + **18 `rR` Go consumers** (mongo-tools, helm, k3d, cfssl, gocryptfs, mc, cilium-cli, hubble, kubeadm, kubectl, kubelogin, skopeo, flannel, buildah, podman, snapd, lab, runc, go-md2man), `NS dev-lang/rust-1.94.0`, no gtk-doc-am, no blocker |
| `dev-python/cloudpickle` | 24 pkgs incl. **`UD dev-python/flit-core-3.12.0`**, `U cargo-c`, `N kdoctools`, `U gtk-doc-am` + `B` blocker | 22 pkgs incl. `UD flit-core`, `NS rust-1.94.0`, no gtk-doc-am, no blocker, no mdit-py-plugins/myst-parser |
| `dev-libs/libtracefs` | 24 pkgs incl. **`N dev-util/source-highlight`** | 22 pkgs incl. `N source-highlight`, `NS rust-1.94.0` |

Conclusions used by the plans:

- Real **does** install/update/downgrade installed packages' build-time
  deps when `bdeps` is `auto`/`y` (#66 withdrawn).
- Real **does** slot-operator-rebuild an installed consumer through a
  BDEPEND `:=` when build-time deps are walked (runc), but only for the
  consumers reached from the argument walk, not the whole world set
  (#65 reframed).
- The `--ignore-default-opts` runc probe is a free #68 oracle cell for an
  *unsatisfied* soft block (installed `gtk-doc` not replaced).
- Remaining portuale-only divergences in the bdeps-on probes
  (`rust-1.94.0` new slot, missing gtk-doc-am / mdit-py-plugins /
  myst-parser) are not owned by any of these plans; see #65 §7.
