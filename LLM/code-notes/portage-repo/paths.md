# crates/portage-repo/src/paths.rs
Display roots: Portage normalises every root to exactly one trailing slash.

## fn root_with_trailing_slash
- Exactly one trailing slash (`/` stays `/`); moved here from `portuale`'s `emerge_build::status`, which called its own copy — the categories `!!!` line is printed from `portage-repo`, which cannot depend on `portuale`. Shared by the merge status lines (`for`/`to <root>/`) and the unlisted-category path. Portage: `package/ebuild/_config/LocationsManager.py`, `set_root_override`.
