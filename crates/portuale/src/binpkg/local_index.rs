//! The local PKGDIR index (`Packages`).

use crate::binpkg::*;

/// The fast-path key set: the `_aux_cache_keys` plus `CPV`/`SIZE`, minus
/// the digest keys (digests are never computed at scan, so a stanza
/// missing only digests must still hit the fast path). `CPV`/`SIZE`/`SLOT`/
/// `_mtime_` ride along; only `_mtime_`/`SIZE` compare by value.
// Portage: `bintree._pkgindex_keys`, `bintree._pkgindex_hashes`.
pub(crate) const PKGINDEX_MINIMUM_KEYS: &[&str] = &[
    "BDEPEND",
    "BUILD_ID",
    "BUILD_TIME",
    "CHOST",
    "DEFINED_PHASES",
    "DEPEND",
    "EAPI",
    "IDEPEND",
    "IUSE",
    "KEYWORDS",
    "LICENSE",
    "PDEPEND",
    "PROPERTIES",
    "PROVIDES",
    "RDEPEND",
    "repository",
    "REQUIRES",
    "RESTRICT",
    "SIZE",
    "SLOT",
    "USE",
    "_mtime_",
    "CPV",
];
