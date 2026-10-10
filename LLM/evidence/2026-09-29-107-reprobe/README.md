# #107 re-probe and fix, 2026-09-29 (batch-2026-09-28_236_107 Slice D)

Host: this server (see `host-facts.txt`: 32 cores, 2128 installed packages,
Portage 3.0.82.2, installed `dev-libs/weston-16.0.0` and
`media-libs/libdisplay-info-0.3.0`). Command on both sides, as a non-root
user: `emerge -puD --getbinpkg --color=n net-libs/rest` (portuale via its
`emerge` applet symlink). Same shape as `../2026-09-27-107-reprobe/`.

**D1 (after #236 Slices A-C, portuale `main` @ `3a4e4ffd`):**
`real-full.log` — real merges `U md4c-0.6.0` + `N rest-0.10.2`
(`backtrack: 0/20`, no skipped-update block; both sides now build `rest`
from the ebuild, so #192's `[binary]`/`[ebuild]` row delta is gone).
`ptl-full-d1.log` / `ptl-json-summary-d1.txt` — portuale prints the same
two rows, still withholds `libdisplay-info-0.4.0`, but prints the
skipped-update warning and reports `restarts: 1`. So D2 ran.

**D2 S0 (`real-debug-libdisplay-info.txt`, an excerpt of real's
`--debug`):** real binds *both* of installed `media-libs/mesa`'s
dynamic-deps atoms — live `media-libs/libdisplay-info:=[abi_x86_32(-),abi_x86_64(-)]`
and built `media-libs/libdisplay-info:0/3=[...]` — to the installed
`0.3.0` (`_minimize_children`), and the update probe's `0.4.0` is refused
by weston's `<media-libs/libdisplay-info-0.4.0:=`. Portuale had the atom
and did not weigh it: Slice B's collapse counted a use-dep atom as
matching only its own pick, so mesa's `:=[...]` kept `0.4.0`. The fix
checks an installed package's use-deps against its vdb USE/IUSE (real's
`findAtomForPackage(pkg, modified_use=_pkg_use_enabled(pkg))`), the
narrowest point — no complete-graph re-walk (owner B7 not triggered).

**After D2:** `ptl-full-d2.log` / `ptl-json-summary-d2.txt` — the same two
rows, no warning, `restarts: 0`: real's one silent pass.

`playground/r107_pg.py` → `r107-debug.log`: real's ResolverPlayground on
the hermetic form (pmtest fixtures `dev-libs/r107{lib,mid,pin,up,target}`):
merge list `r107up-2.0`, `r107target-1.0`, the probe refused by
`r107pin`'s `<r107lib-2.0:=`, no backtrack. The plain `r107up` upgrade is
load-bearing: without a version change real's complete mode stays off,
`r107pin` is never walked, and real itself backtracks and warns (the first
draft of the playground script showed exactly that).
