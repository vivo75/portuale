# crates/portage-vdb/src/dep_cp.rs
Conservative `category/package` extraction behind the `dep_atom` prefilter index.

## module
- Moved out of `sqlite/write.rs` unchanged (S5.3).
- `dep_atom` holds a superset of the dependency atoms, found without `portage-dep` (vdb lib module doc, item 8). The caller of `reverse_dependents` still does the USE reduction and atom match.

## const UNSURE_CP
- S8.1: every token that is neither structure nor read by `dep_cp` is stored with `cp = ""`, and `reverse_dependents(cp)` returns entries with a row for `cp` or for `""`, so an entry whose deps could not be fully indexed is never lost. A database written before S8.1 has no markers and needs its index rebuilt (S8.3).

## fn dep_index_key
- `Some(UNSURE_CP)` is for a token that might still be an atom the matcher accepts (a wildcard, a bare versioned name, an odd version).
- The index key of one token of a `*DEPEND` string (S8.1): `None` for a structure token that can never hold an atom (`(`, `)`, `||`, `^^`, `??`, `flag?`), `Some(cp)` when [`dep_cp`] reads a package out of it and `Some(`[`UNSURE_CP`]`)` for anything else, i.e. a token that might still be an atom the matcher accepts (a wildcard, a bare versioned name, an odd version). The marker keeps the prefilter a superset: an entry with such a token is returned for every `cp`.

## fn dep_cp
- The conservative string routine; a token is read as follows:
  - strip a leading `!!` or `!`, then one version operator (`<=`, `>=`, `<`, `>`, `=`, `~`);
  - cut at the first `[` (`USE` dependency) or `:` (slot, `::repo`);
  - the rest must be `category/name[-version]`: a category of `[A-Za-z0-9+_.-]`, a name of `[A-Za-z0-9+_-]` that does not start with `-`, exactly one `/`;
  - with an operator, the name must end in `-<version>[-r<N>][*]` (digits and dots, an optional lowercase letter, `_alpha|beta|pre|rc|p` suffixes), which is stripped; without one, a name that looks like it ends in a version is skipped.
