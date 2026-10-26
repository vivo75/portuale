# crates/portage-repo/src/resolve/autounmask.rs
Autounmask dependency chains and suggestions.

## fn autounmask_dep_chain
- Portage `_get_dep_chain_as_comment` (`depgraph.py:6457`). A dependency yields `required by <parent cpv>::<repo>` and, when that parent is itself a command-line argument, a trailing `required by <parent atom> (argument)`. The top-level example: `dev-libs/useflagpkg[-foo]`.
- Portage walks the whole parent chain to the argument, not just the immediate owner (backlog #135 (e); bed `l0-fx-20260922T192350Z` arm A prints three rows where portuale printed one). Vdb repo for an installed parent is Portage's `installed_use_str` counterpart.
- Backlog #341 S8: when several top-level atoms name the same package (e.g. two valid lines of one `@set`), the argument line is the sorted-first match, and multi-match chain tails print sorted. Rationale: the candidates come from a `HashSet<&str>`, so a plain `find`/collect printed either atom from run to run (S8's `@s341` pin flipped between `dev-libs/t341-dep::t341` and `dev-libs/t341-dep[x]`). Single-match output is unchanged. Same sorted-first rule in `required_use_dep_chain`.
- Single-branch rule (deliberate, unverified against Portage on a multi-parent shape): the sibling `masked_dep_chain` discloses every failing branch. All pinned chains are linear; Portage's branch choice on a multi-parent autounmask cell has no oracle.

## fn render_use_display_pairs
- Portage `pkg_use_display(pkg, opts)`'s own return string. Mirrors `portuale::pretend::render_pkg_use_display`.

## fn required_use_dep_chain
- Portage `_get_dep_chain` with `target_atom` set: first the failing package's direct parent, then that parent's own command-line argument when it is one. Portage skips the whole block for a top-level atom (`if not isinstance(myparent, AtomArg)`).

## fn render_required_use_block
- Portage `depgraph.py::_show_unsatisfied_dep` (`:6763-6799` + the `:6899-6912` dep-chain tail). `reduced_human` / `full_human` are already `portage_required_use::human_readable`-rewritten.
- The `!!! The ebuild selected to satisfy "<atom>" has unmet requirements.` header, the `- <cpv>::<repo> USE="…"` line, the minimal still-unsatisfied sub-expression, the complete expression (only when it differs), then the `(dependency required by "…" [ebuild])` chain.

## fn check_if_latest_atom_form
- Portage `_display_autounmask`'s `check_if_latest(pkg, check_visibility=...)` (`depgraph.py:10649`). Keywords always use `=` (bug #536392, "don't unmask undesired versions"). A use-masked-only candidate is still `is_visible`. Portage calls `check_if_latest(pkg)` for license changes.

## fn render_required_use_block
- No trailing newline: the print site terminates the report with one (`eprintln!`), like Portage's final `writemsg`, so the block ends exactly after the last chain line (#517 (c)).

## fn autounmask_use_tier
- Fed to `effective_use_flags` as the top USE tier on the next attempt.
