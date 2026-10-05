//! The per-entry read rule the database backends share: the files an
//! `aux_get` of some fields needs, the stored `metadata` stamp, and the
//! `FilesDb::aux_get_field` normalisation applied to them (used by
//! `sqlite.rs` and `redb_db.rs`, so the two cannot drift).

use std::collections::HashMap;

use crate::files::{normalise_aux_bytes, parse_metadata_text, translate_aux_slot};
use crate::{EntryKey, MetadataStamp};

/// One live entry with the files an `aux_get` of some fields needs.
pub(crate) struct Loaded {
    pub(crate) key: EntryKey,
    pub(crate) stamp: MetadataStamp,
    pub(crate) files: HashMap<String, Vec<u8>>,
}

impl Loaded {
    /// The validated snapshot, as `files` reads it: only when the stored
    /// stamp is `valid`, a `metadata` file is stored, it is UTF-8 and its
    /// `#format=` is the supported one.
    pub(crate) fn snapshot(&self) -> Option<HashMap<String, String>> {
        if self.stamp != MetadataStamp::Valid {
            return None;
        }
        let text = std::str::from_utf8(self.files.get("metadata")?).ok()?;
        parse_metadata_text(text).map(|(m, _)| m)
    }

    /// `FilesDb::aux_get_field` for one in-set field: a validated snapshot
    /// is complete (a missing field is `""`, the stored field file is not
    /// consulted); otherwise the field file, whitespace-joined, lossy
    /// UTF-8, absent as `""`; then the invalid-`SLOT` translation.
    pub(crate) fn field(&self, snap: Option<&HashMap<String, String>>, field: &str) -> String {
        let v = match snap {
            Some(m) => m.get(field).cloned().unwrap_or_default(),
            None => self
                .files
                .get(field)
                .map(|b| normalise_aux_bytes(b))
                .unwrap_or_default(),
        };
        translate_aux_slot(field, v)
    }
}
