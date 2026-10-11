# Contributing

Build and test from the repo root. Agent working material lives in `LLM/`.

## Build and test

```sh
cargo build --release
cargo fmt --check
cargo clippy --release --all-targets
cargo test --release
```

Clippy must report zero warnings.
Build before testing: the unit tests in `crates/portuale` run
`target/release/portuale`, which `cargo test` does not rebuild.
`LLM/tools/gate.sh` runs all of the above plus the contract suite.

## The pmtest sibling checkout

The black-box contract suite, the fixtures, and the differential test bed
live in the sibling `pmtest` repository, next to this checkout.
`fixtures/` here is a symlink into its fixture tree, so the Rust tests and
the contract suite read the same files. A checkout without the sibling
beside it cannot run the tests. Never edit the suite to make a package
manager pass; fix the package manager instead.

## Comment policy

- Public items get short `///` summaries, plus `# Errors` and `# Panics`
  where they apply.
- `//` comments explain only a non-obvious why, in one or two lines.
- Where behaviour is surprising without it, add one short
  `// Portage: <file>, <function>` reference.
- Long context goes to the notes file
  `LLM/code-notes/<crate>/<path without src/ and .rs>.md`,
  under a `## <kind> <symbol>` heading.
- After adding a notes file, run `python3 LLM/tools/code_notes_index.py`.

## Commit policy

- Title: `<what changed>: <short description>`.
- Body: a wrapped paragraph explaining the why and the Portage grounding.
- Commits are paired across the two repos (see `AGENTS.md`,
  "Two repos, one project"): the product change lands here, fixtures and
  contract pins land in `pmtest` on a same-named branch, and the two
  messages quote each other.

## Function size and argument structs

Keep functions short and calls readable.
A function taking more than five parameters takes one `<Fname>Args` struct
built with named fields (see `clippy.toml`).
