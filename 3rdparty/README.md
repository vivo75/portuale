# `3rdparty/`

Working checkouts of the repos portuale depends on but doesn't vendor,
pinned in **`repos.toml`** (the tracked source of truth). The checkouts
themselves are **gitignored** (`/3rdparty/*/`).

## Bootstrap

```sh
./3rdparty/setup.sh          # clone/update every repo to its pinned ref
./3rdparty/setup.sh portage  # just one
```

Run it after a fresh clone, and again whenever a `repos.toml` pin
changes.

## What lives here

| dir | why it's needed | pinned by |
|---|---|---|
| `portage/` | the `PORTUALE_PYTHON_HELPERS=real` oracle's `.py` helpers and `lib/portage` import path, the D4 fixture generators' source (`generate*.sh` read it), the `bin/` + `cnf/sets/portage.conf` re-sync source, and the real `portage.versions` / `portage.dep` modules the primitive harnesses (`pmtest/python-harness/`) wrap — nothing at runtime needs it (#326 S2–S7) | `repos.toml` `[portage]` |
| `brush/` | reference/hacking checkout of the bash interpreter `portuale` embeds; `crates/portuale/Cargo.toml` fetches it via git independently | `repos.toml` `[brush]` |
| `devmanual/` | the Gentoo developer manual, for grounding ebuild-helper and phase semantics | `repos.toml` `[devmanual]` |
| `portage-cli/` | reference checkout of upstream Rust Portage CLI (`portage-solver` / `portage-atom-pubgrub` / `portage-atom-resolvo`); analysis in `docs/history/solver-backends-analysis.md`. No Cargo dependency yet | `repos.toml` `[portage-cli]` |

The **bash phase runtime** (`ebuild.sh` and friends) is *not* here — it's
vendored into the tree at `bin/` so `emerge` runs with no Portage
installed. See `bin/README.md`.

Override the portage checkout location at runtime with
`$PORTUALE_PORTAGE_CHECKOUT` (both the Rust and Python sides honour it).
