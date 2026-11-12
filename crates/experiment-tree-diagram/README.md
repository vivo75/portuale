# experiment-tree-diagram

Draws the dependency tree of one or more packages (or sets) as a
[Graphviz](https://graphviz.org/) DOT text. `dot` turns that text into an SVG.

An experiment: it is not a default workspace member and nothing else depends
on it.

## Build

```
cargo build --release -p experiment-tree-diagram
```

The binary is `target/release/experiment-tree-diagram`. A bare `cargo build`
skips it.

You also need Graphviz's `dot` (Gentoo: `emerge media-gfx/graphviz`). The
program only prints text; it never runs `dot` itself.

## Run

The program prints DOT on stdout and a one-line summary (`68 packages, 18
levels`) on stderr. Pipe stdout into `dot`:

```
target/release/experiment-tree-diagram --depth 2 --reduce app-editors/nano dev-vcs/git \
  | dot -Tsvg -o tree.svg
```

Or in two steps, to keep the text and try several renderings:

```
target/release/experiment-tree-diagram --depth 2 app-editors/nano > tree.dot
dot -Tsvg tree.dot -o tree.svg        # vector, opens in a browser
dot -Tpng -Gdpi=80 tree.dot -o tree.png
```

The program reads the installation at `--root` and `--config-root` (default
`/`); `dot` only sees the text.

### Arguments

Options also take the `--name=value` form (`--jobs=2`). Atoms (`dev-libs/openssl`, `=app-editors/nano-9.3`) and sets (`@world`,
`@system`, `@profile`, a user set) can be mixed. At least one is needed.

| Option | Meaning |
|---|---|
| `--depth N` | Keep only the packages at most N dependencies away from a target. Without it `app-editors/nano` alone reaches 535 packages and `dot` needs about 25 s; `--depth 2` is 0.2 s. |
| `--kinds all\|KIND[,KIND]...` | Draw only these dependency kinds: `DEPEND`, `RDEPEND`, `PDEPEND`, `BDEPEND`, `IDEPEND` (any case). Default `all` (also `tutte`). |
| `--reduce` | Hide an arrow when a longer path between the same two packages already implies it (`a -> b -> c` makes `a -> c` redundant). The biggest readability win. |
| `--bundle` | Join arrows of one kind that reach the same package into a small dot just above it, and go on as one arrow. |
| `--jobs N` | Number the packages with their build step for N parallel builds: `[3] cat/pkg-1.0`. With `--jobs 1` every number differs (the exact build order); with `--jobs 2` or more, packages sharing a number are built at the same time. See "Build steps" below. |
| `--without-bdeps` | Do not follow `DEPEND`/`BDEPEND`/`IDEPEND` of installed packages that stay as they are (Portage's default). Default here: follow them. |
| `--root DIR`, `--config-root DIR` | Installation to read. |
| `--threads N` | Worker threads of the walk. |

## What the diagram means

- **Rows are build levels.** Dependents are above their dependencies. A
  package sits one row above the highest package it waits for, so everything
  in one row can be built at the same time. `PDEPEND` never makes a package
  wait; it is drawn without constraining the rows, and so is the one arrow of
  a cycle that has to be cut (a run-time arrow first).
- **Arrows by kind** (the legend is part of the diagram):

  | Kind | Look |
  |---|---|
  | `DEPEND` | blue, solid |
  | `RDEPEND` | green, open head |
  | `DEPEND` + `RDEPEND` on the same pair | one arrow of two parallel lines, blue and green |
  | `BDEPEND` | red, dashed |
  | `PDEPEND` | purple, dotted |
  | `IDEPEND` | orange, dashed, circle head |

  `DEPEND` with `BDEPEND` on one pair stays two arrows in different colours.
- **Packages by set**, the most important one wins: `@system` red with a
  double border, `@profile` orange with rounded corners, `@world` green with a
  thick border, everything else white. A set claims a package by name and slot;
  the version in the set's atom is not compared.
- The rows come from every dependency even when `--kinds` shows only some, so a
  row always means "buildable together". After `--kinds`, a package left with no
  drawn arrow is dropped unless you asked for it.

## Build steps (`--jobs`)

`--jobs N` simulates N builders. Step 1 is the first round; a package starts
in the first round after everything it waits for has finished (same rules as
the rows: `DEPEND`, `BDEPEND`, `RDEPEND` and `IDEPEND` make it wait, `PDEPEND`
and cut cycle arrows do not). When more than N packages are ready, the ones
with the longest chain of dependents above them go first, then by name, so
the numbers are the same on every run. With enough jobs the step of a package
is its row; with fewer, rows spread over more steps (68 packages: 68 steps with
`--jobs 1`, 34 with `--jobs 2`, 18 with `--jobs 4`).

Two packages get a letter instead of a number and are left out of the
numbering, so others do not wait for them: `[x]` is already installed at the
version that would be chosen, `[!]` has no visible version to choose (an
invalid slot, worth a look).

Read the numbers with these limits in mind:

- Every package is taken to need the same time, so a step is a round, not a
  duration. A real build of `gcc` is not the length of a `zlib` one.
- With `--depth` only the packages drawn are numbered.
- The numbers come from every dependency, so `--kinds` and `--reduce` change
  what is drawn, not the numbers.

## How it works

1. `portage-world` follows the targets and their dependencies (the first pass
   of its `@world` analysis) and returns the package slots reached, the version
   each would install, and, for each edge, which `*DEPEND` variables stand
   behind it (`World::edge_kinds`).
2. `levels.rs` gives every package its row (longest path down, cycles cut) and
   `reduce` removes redundant arrows.
3. `dot.rs` writes the text: one node per package, one `rank=same` group per
   row, one arrow per kind, the legend. `nslimit=4` is set because `dot`'s
   horizontal placement otherwise takes minutes on a few hundred packages.

Known cuts: a repository without `metadata/md5-cache` adds no dependencies
(no aux-metadata provider is registered); with `--reduce` a direct dependency
that a path implies is no longer drawn.

## Tests

```
cargo test --release -p experiment-tree-diagram
cargo test --release -p portage-world a_target_is_followed_and_each_edge_knows_its_dependency_kind
```
