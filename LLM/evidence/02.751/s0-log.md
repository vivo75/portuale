# S0 probe log for plan 02.751 (kept-virtual cut hole)
# Scratch only. No product code changes in S0.

## Step 0 — fixture virtual modelling
- `../pmtest/fixtures/repo/virtual/texteditor/texteditor-0.ebuild`: ordinary
  ebuild, `RDEPEND="|| ( dev-libs/newpkg dev-libs/samepkg )"`, no PROVIDE.
- Probe mirrors that: `virtual/w751virt-1` with plain `RDEPEND
  dev-libs/w751prov` (single provider, no `||` — the kept-middle shape).

## Fill-in choices (plan under-specifies two points; both recorded here)
- (a) prov-2 ebuild RDEPEND: plan says prov `RDEPEND dev-libs/w751user`
  (cycle close). The w746 precedent (`kept_middle_tree`) gives the
  rebuilt consumer BOTH the plain dep AND the stale slot-op atom, so the
  ebuild reads `RDEPEND "dev-libs/w751base dev-libs/w751user"` and the
  installed prov-1 records `RDEPEND "dev-libs/w751user
  dev-libs/w751base:0/1="` (plain + `:0/1=`, w746loc idiom verbatim).
- (b) installed base: plan lists visible base-2 but no installed base.
  w746 installs the stale-binding TARGET at the new subslot (prov-2 at
  0/2), so variant A installs base-2 (kept). Variant C (plan-literal,
  no installed base) is run as well to show it does not matter.

## Prediction (traced before running, order.rs + health_checker.py)
- Cycle `[prov,user,user->virt,virt->prov]`; `cycle_cut` takes the FIRST
  cuttable edge in cycle order from the ancestor = `(prov,user)`
  (runtime-only, prov not build-time-used). The `virt->prov` edge is never
  the cut, so the `virtual/*` exclusion is never consulted by this shape.
- Expected: cut `Edge(prov,user)`, rows `[prov,user]` (+`[base,...]` when
  base is a row), planner duplicates user (`close_usability_gaps`), checker
  `cycle-noted` on user-first, zero failures. Control identical.
- If so: NOT verdict A (excuse present), NOT verdict B (no hard failure)
  -> NEEDS_CONTEXT per plan STOP rules (probe cannot fail either way).
