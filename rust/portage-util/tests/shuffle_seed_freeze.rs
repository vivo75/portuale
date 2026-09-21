//! #113: `shuffle_seed` reads `PORTUALE_SHUFFLE_DIRS` once and freezes it
//! for the life of the process.
//!
//! This file is its own test binary, so it owns the variable from the
//! first instruction -- the one state the in-process unit tests cannot
//! reach once the global freeze exists (any earlier directory read pins
//! the value, and cargo runs a binary's tests in parallel). It also keeps
//! the seeded half of the old `read_dir_entries` unit test: with the
//! variable set, the shuffle still executes and reproduces per
//! (seed, directory).

use std::path::Path;

use portage_util::{read_dir_entries, shuffle_seed};

fn names(dir: &Path) -> Vec<String> {
    read_dir_entries(dir)
        .unwrap()
        .into_iter()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn shuffle_seed_is_frozen_at_first_use_and_still_shuffles() {
    // SAFETY: the only test in this binary; no other thread reads or
    // writes the environment.
    unsafe { std::env::set_var("PORTUALE_SHUFFLE_DIRS", "7") };

    assert_eq!(shuffle_seed(), Some(7), "first use reads the variable");

    let dir = std::env::temp_dir().join(format!("portage-util-freeze-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["g", "a", "h", "c", "b", "f", "d", "e"] {
        std::fs::write(dir.join(name), b"").unwrap();
    }

    let first = names(&dir);
    let sorted = ["a", "b", "c", "d", "e", "f", "g", "h"];
    assert_ne!(first, sorted, "an 8-entry shuffle must not stay sorted");
    assert_eq!(
        first,
        names(&dir),
        "same seed must reproduce the same order"
    );

    unsafe { std::env::set_var("PORTUALE_SHUFFLE_DIRS", "8") };
    assert_eq!(shuffle_seed(), Some(7), "the first read is frozen");
    assert_eq!(names(&dir), first, "a later value is ignored");

    std::fs::remove_dir_all(&dir).unwrap();
}
