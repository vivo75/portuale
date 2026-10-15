# G6c — `-pve app-editors/emacs` `R app-doc/info-manual-30.2`

Verdict: **engine** (the G5 `||`-choice limitation in its strongest form:
the in-graph alternative is the ROOT TARGET and still loses). No bridge
patch, no fixture/test/fix — allowlist + crate-gap text at the bottom.

## The row (bed `-pve l0-20261015T094939Z`; real 341 rows / portuale 332)

- real `app-editors_emacs.txt`: no info-manual row.
- portuale `app-editors_emacs.txt`, line 60:
  `[ebuild  R    ] app-doc/info-manual-30.2::gentoo`
- VDB holds info-manual-30.2 (installed) AND texinfo-7.3 (installed);
  emacs-30.2-r6 is NOT installed.
- Both sides show `[ebuild R] sys-apps/texinfo-7.3` with IDENTICAL USE
  (`nls standalone -static -test -verify-sig`); both show
  `[ebuild N] app-editors/emacs-30.2-r6:30` with identical USE.

## Chain (established from the `PORTUALE_PG_TRACE=1 -pve` run, `../cmd.txt`)

1. `sys-apps/texinfo-7.3` is in the solution (R, both sides).
   Container-tree RDEPEND (`texinfo-7.3.ebuild:50-51`), unconditional:
   `|| ( app-doc/info-manual >=app-editors/emacs-26:* )`.
2. Trace (`pg-pve-emacs-trace.txt`):
   `pg-edge sys-apps/texinfo-7.3 -> app-doc/info-manual (DEPEND)` and
   `(RDEPEND)` — the texinfo `||` is the ONLY referrer of info-manual in
   the whole trace (no other parent).
3. Solution (`pg-plan` set, 330 entries) contains BOTH
   `app-doc/info-manual-30.2` AND `app-editors/emacs-30.2-r6` (the probe's
   root target — trivially in-graph).
4. Portage takes the emacs branch (`>=emacs-26:*` satisfied by in-graph
   emacs-30.2:30) → no info-manual row. The engine takes the FIRST-LISTED
   info-manual branch → `R` row (info-manual is installed; under `-e` every
   installed cpv is Rebuild, hence R rather than hidden).

## Ruling out bridge mishandling of `>=emacs-26:*` (why this is engine,
## not bridge)

- `SlotOperator::Star` converts to an any-slot Choice requirement
  (`portage-atom-pubgrub-0.8.0/src/convert.rs:325-328`,
  `push_unslotted_or_choice`), covered by the crate's own
  `convert_slot_operator_any_slot` unit test (`convert.rs:1242`): the emacs
  branch IS a live candidate (emacs-30.2 satisfies `>=26`), so the engine
  is choosing between two satisfiers — not defaulting for lack of one.
- Engine `choose_version` (`provider/solve.rs:140-259`): for the `Choice`
  node, 1 of 2 branches is directly installed (info-manual) →
  `directly_installed_count (1) < candidates.len() (2)` → returns the max
  (first-listed) INSTALLED branch = info-manual (`solve.rs:200-208`). Had
  neither been installed, the `:258` fallback (`max()` = first-listed)
  picks info-manual anyway. In NO installed-state does the in-graph-but-
  uninstalled root target win — the engine has no in-graph input.
- Portage: `depgraph.py:778-793` (in-graph atoms preferred) — emacs is not
  merely in-graph, it is the argument itself.

## Disposition

`owner: engine`, same crate gap as G5/G6a/G6b (proposed backlog #366).
Proposed allowlist text (second slug on the same entry — same mechanism,
same match string; S5-step-2 files it):

`… / slugs: [kde-apps_kdecore-meta, app-editors_emacs] / match: "present
for portuale, absent for real" / reason: … (as in sibling file a/) …
Covers additionally: -pve app-editors_emacs R app-doc/info-manual-30.2
(texinfo RDEPEND || ( info-manual emacs-26:* ) unconditional; the in-graph
alternative is the probe target itself, uninstalled — the engine's
installed-only Choice preference (solve.rs:200-208, fallback :258) can
never prefer it). Evidence: …/g6/c/.`
