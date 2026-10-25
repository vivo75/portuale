# #568/#569 S0 oracle: hermetic `p569pull` skip block

Fixture (`../pmtest/fixtures`, `p569`-prefixed, real md5-cache entries):
`dev-libs/p569prov-5.42` (`0/5.42`, installed) + `p569prov-5.44` (`0/5.44`,
in-repo only); installed `dev-libs/p569virt-1` recording
`|| ( =p569prov-5.44*... ` **without** the `-5.44*` arm (like the bed's old
virtual revisions whose menu lacks the new branch)
`+ built `dev-libs/p569prov:0/5.42=`; `dev-libs/p569core-1.0` in-repo only
(the `~perl-core` arm, not installed — as on the bed); merge-bound
`p569pull-2` (`=p569prov-5.44*`, pulls the new subslot in) + `p569pull-1`
(`=p569prov-5.42*`, the heal fallback).

Raw captures: `captures/portage-p569pull.*` vs `captures/portuale-p569pull.*`
(`probe-p569.sh`; both rc 0, both `backtrack: 2/20`, both merge
`p569pull-1`). First shape tried (`||` **with** the `-5.44*` arm) was
rejected by the oracle: Portage upgrades + rebuilds the consumer (`rR`)
with no skip block at all — the `||` stays satisfiable, so there is no
conflict to render. The menu must lack the new branch (then Portage heals
exactly like the bed: old puller + skip block).

## Portage skip block (stderr), exact lines in order

One conflicted provider (`dev-libs/p569prov:0`), two parent lines:

```
    dev-libs/p569prov:0/5.42= required by (dev-libs/p569virt-1:0/0::testrepo, installed in '<root>') USE=""
                     ^^^^^^^^
    =dev-libs/p569prov-5.42* required by (dev-libs/p569virt-1:0/0::testrepo, installed in '<root>') USE=""
    ^                  ^^^^^
```

- The built `:0/5.42=` form prints **first**, verbatim from the vdb record
  (Portage `_conflict_missed_update`: every `_parent_atoms` entry not
  matching the skipped package renders — the strip to `:=` is
  constraint-only, `depgraph.py:2509-2517`).
- Only the vdb-selected `||` branch prints (`=5.42*`, the arm matching
  installed 5.42). The non-selected arms (`=5.40*`, `=5.38*`) never become
  parent atoms (Portage `dep_zapdeps` keeps the installed-preferred arm,
  `dep_check.py:339`), and `~p569core-1.0` (not installed) neither.
- `=5.44*` cannot print here: it is not in the installed menu at all.

Order note: the two parent lines swap order between Portage runs (bare
first in two re-runs, built first in the first capture) — Portage collects
missed-update parents into a `set` (`depgraph.py:2116-2121`), so intra-block
order is hash order, not a stable rendering rule. portuale renders the
built form first deterministically (mask parents are insertion-ordered).

Marker note: for `=dev-libs/p569prov-5.42*` Portage prints five carets
(`^                  ^^^^^`, covering `5.42*`) while portuale prints four
plus a blank (`^                  ^^^^ `). That gap is pre-existing
(`skip_conflict_caret_line` spans the parsed version without the `=*`
star; Portage `output.py:947-949` folds the star into the version span)
and untouched by this slice — every other line and marker here agrees.

## portuale today (stdout), same cell

Three parent lines, no built form:

```
    =dev-libs/p569prov-5.38* required by (dev-libs/p569virt-1:0/0::testrepo, installed in '<root>') USE=""
    =dev-libs/p569prov-5.40* required by (dev-libs/p569virt-1:0/0::testrepo, installed in '<root>') USE=""
    =dev-libs/p569prov-5.42* required by (dev-libs/p569virt-1:0/0::testrepo, installed in '<root>') USE=""
```

## Bed cross-check (`l0-20261025T111515Z`, `dev-perl_XML-Parser`)

- Real side: 48 `:0/5.42=` lines + 21 `=perl-5.42*` lines; no `=5.38*`,
  `=5.40*`, `=5.44*` parent lines.
- portuale side: 0 `:0/5.42=` lines; 15 `=5.38*`/`=5.40*` lines;
  21 `=5.42*` lines (the selected branch already agrees).
