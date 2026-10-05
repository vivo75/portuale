//! Test helpers shared by the backend test modules.

use crate::{EntryKey, Error, FilesDb, InstalledDb, METADATA_FILE_FIELDS};

/// Read `name` as `files` shows the live and the pending file.
pub(crate) const PROBE_FILES: &[&str] = &[
    "CONTENTS",
    "SLOT",
    "EMPTY",
    "BIN",
    "NUL",
    "metadata",
    "NEEDED.ELF.2",
    "nope",
    "extra",
    "DESCRIPTION",
    "RDEPEND",
];

/// Asserts that `other` answers every read exactly as the files backend
/// does, for `keys` and [`PROBE_FILES`].
pub(crate) fn assert_same_reads(fdb: &FilesDb, other: &dyn InstalledDb, keys: &[EntryKey]) {
    assert_eq!(other.entries().unwrap(), fdb.entries().unwrap(), "entries");
    assert_eq!(
        other.categories().unwrap(),
        fdb.categories().unwrap(),
        "categories"
    );
    let mut cats: Vec<String> = keys.iter().map(|k| k.category.clone()).collect();
    cats.push("nope-cat".into());
    for c in &cats {
        assert_eq!(
            other.category_entries(c).unwrap(),
            fdb.category_entries(c).unwrap(),
            "category_entries {c}"
        );
    }
    for k in keys {
        assert_eq!(
            other.has_entry(k).unwrap(),
            fdb.has_entry(k).unwrap(),
            "has_entry {k}"
        );
        assert_eq!(
            other.list_files(k).unwrap(),
            fdb.list_files(k).unwrap(),
            "list_files {k}"
        );
        for f in METADATA_FILE_FIELDS {
            assert_eq!(
                other.aux_get(k, f).unwrap(),
                fdb.aux_get(k, f).unwrap(),
                "aux_get {k} {f}"
            );
        }
        for n in PROBE_FILES {
            assert_eq!(
                other.read_file(k, n).unwrap(),
                fdb.read_file(k, n).unwrap(),
                "read_file {k} {n}"
            );
            assert_eq!(
                other.file_meta(k, n).unwrap(),
                fdb.file_meta(k, n).unwrap(),
                "file_meta {k} {n}"
            );
            assert_eq!(
                other.read_pending_file(k, n).unwrap(),
                fdb.read_pending_file(k, n).unwrap(),
                "read_pending_file {k} {n}"
            );
        }
    }
    for n in ["CONTENTS", "NEEDED.ELF.2", "nope"] {
        assert_eq!(
            other.read_file_all(n).unwrap(),
            fdb.read_file_all(n).unwrap(),
            "read_file_all {n}"
        );
    }
    let q: &[&[u8]] = &[
        b"/usr/bin/x",
        b"usr/bin/y",
        b"/usr",
        b"/usr/z",
        b"/nope",
        b"/usr/bin/x",
    ];
    assert_eq!(other.owners(q).unwrap(), fdb.owners(q).unwrap(), "owners");
    assert_eq!(other.owners(&[]).unwrap(), fdb.owners(&[]).unwrap());
    assert_eq!(other.world().unwrap(), fdb.world().unwrap(), "world");
    assert_eq!(
        other.world_sets().unwrap(),
        fdb.world_sets().unwrap(),
        "world_sets"
    );
    assert_eq!(
        other.preserved_libs().unwrap(),
        fdb.preserved_libs().unwrap(),
        "preserved_libs"
    );
    assert_eq!(
        other.config_memory().unwrap(),
        fdb.config_memory().unwrap(),
        "config_memory"
    );
    assert_eq!(other.counter().unwrap(), fdb.counter().unwrap(), "counter");
    for bad in ["CONTENTS", "NEEDED.ELF.2"] {
        let k = &keys[0];
        assert!(matches!(other.aux_get(k, bad), Err(Error::Invalid(_))));
        assert!(matches!(fdb.aux_get(k, bad), Err(Error::Invalid(_))));
    }
}
