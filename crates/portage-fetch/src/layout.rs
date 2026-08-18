// Portage's distfiles mirror layouts (`lib/portage/package/ebuild/fetch.py`:
// `FlatLayout`, `FilenameHashLayout`, `ContentHashLayout`,
// `MirrorLayoutConfig`): the path a file lives at under a mirror's
// `distfiles/` directory, as described by that mirror's own
// `distfiles/layout.conf`. This matters for correctness: the Gentoo
// mirrors publish only the hashed layout path and 404 the flat one (see notes).
//
// Scope: the pure path math and `layout.conf` parsing. Fetching a
// mirror's `layout.conf` and the `.mirror-cache.json` cache live with
// the fetch loop (`portuale/src/fetch.rs`).
//
// Documented divergences (see notes for detail):
//   - `filename-hash` with `RMD160`/`SHA3_256`/`SHA3_512`/`WHIRLPOOL`
//     is unsupported here, so the next structure entry is used instead.
//   - `layout.conf` is parsed by a small `configparser` subset.

use std::collections::HashMap;

use blake2::Digest;

/// Portage `MirrorLayoutConfig`: the ordered `[structure]` entries of a
/// mirror's `layout.conf`, each already whitespace-split.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MirrorLayoutConfig {
    pub structure: Vec<Vec<String>>,
}
