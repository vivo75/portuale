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
| `portage/` | the `.py` ebuild helpers that `import portage` (`doins.py`, `xpak-helper.py`, …), their `lib/portage` import path, `cnf/sets/portage.conf`, and the real `portage.*` modules the Python reference (`python/emerge_pretend_reference.py`) mirrors | `repos.toml` `[portage]` |
| `brush/` | reference/hacking checkout of the bash interpreter `portuale` embeds; `rust/portuale/Cargo.toml` fetches it via git independently | `repos.toml` `[brush]` |

The **bash phase runtime** (`ebuild.sh` and friends) is *not* here — it's
vendored into the tree at `bin/` so `emerge` runs with no Portage
installed. See `bin/README.md`.

Override the portage checkout location at runtime with
`$PORTUALE_PORTAGE_CHECKOUT` (both the Rust and Python sides honour it).
