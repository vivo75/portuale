//! Value types of the interface.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use crate::Error;

/// State of the `#dir_mtime=` stamp of an entry's consolidated
/// `metadata` file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataStamp {
    /// The entry has no `metadata` file.
    Absent,
    /// The stamp equals the entry directory's `st_mtime_ns`; a writer
    /// stamps the new directory's own value.
    Valid,
    /// The stamp does not match (or is missing); a writer keeps the
    /// stored bytes, so it still does not match.
    Stale,
}
