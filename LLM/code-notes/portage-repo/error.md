# crates/portage-repo/src/error.rs
The crate's error type.

## enum Error
- `Detail` carries an already-fully-formatted message whose assembly spans multiple conditional appends upstream (Portage resolver conflict/`REQUIRED_USE`-violation reporting).
- `StaleMd5Cache`: the entry no provider could replace -- a nested read from inside the depend phase, or a provider whose phase failed.
- `PregenCacheUnavailable`: the repo's first known `cache-formats` entry is not `md5-dict` (#55 L1: no `pms` reader) or `FEATURES` names `metadata-transfer` (#55 L2), and no depend-phase provider could supply the metadata the pregen cache would have. The pregen rung is skipped exactly like Portage `_pull_valid_cache` falling through its rungs.
- `InvalidRequiredUse`: `package` is the `category/package-version` prefix, `source` the `portage_required_use::Error` `Display`.
- The CLI-visible message shapes for the plain variants live on the `Display` impl below, so the per-variant doc lines duplicating them were deleted as restatements.
