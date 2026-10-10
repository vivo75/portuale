# crates/portage-repo/src/dep_chain.rs
The "required by" chain printed under a masked dependency.

## fn chain_node_usedep_suffix
- Portage `_pkg_use_enabled` renders `[flag]`/`[-flag]`. Sources mirror the node's kind: merge-bound reads the same-version repo candidate's metadata (`BDEPEND`, `DEPEND`, `IDEPEND`, `PDEPEND`, `RDEPEND` -- Portage `Package._dep_keys`) and its effective USE; installed reads the vdb (`RDEPEND`, `PDEPEND`, `DEPEND` files; the recorded `USE`).
- Portage filters dep strings by the edge's buildtime/runtime priority; portuale doesn't track it, so every available key is scanned (union -- wider only when one cp is gated differently under two keys).
- Portage matches the linking atom by exact token text (evaluated for the first row, unevaluated parent atoms after); portuale re-derives the first token with the child's cp, agreeing everywhere except a doubly-conditional edge (`flag? ( pkg[other=] )`), where Portage's two forms can disagree with each other too.
- Multi-flag sort: Portage iterates a `set` (hash order under the bed's `PYTHONHASHSEED=0`; every fixture and live row so far carries one flag). A malformed dep string is skipped (Portage raises for ebuilds).
- Installed-node mask/force falls back to no exclusion when no same-version repo candidate exists.

## fn masked_dep_chain
- `entries` must be final (post `required_by` fill). A merge-bound parent renders `cat/pkg-ver::repo` as `ebuild` (`binary` for a binpkg parent); an installed one renders its vdb repo (`__unknown__` fallback, like Portage) as `installed`; a top-level atom targeting the chain top renders as `argument`. Cycles guarded by a visited set; ascent stops at the first arg-targeted node (Portage prefers arguments since they are root nodes). Portuale never abandons, so all failing branches disclose. A node whose metadata can't be read keeps the bare row.
- Backlog #286: a built `:S/SS=` atom and a live `:=` atom on one package have different pullers, and the aggregated chain would blame both. A parent qualifies on an evaluated-or-raw edge-atom match; with conditional-evaluated edges or missing dep lists the restriction is vacuous and every direct parent stays, exactly as before.
