// Real `NeededEntry` (`lib/portage/util/_dyn_libs/NeededEntry.py`): the
// data model for one parsed line of a real `NEEDED.ELF.2` file -- the
// aux vdb metadata real, unmodified `bin/misc-functions.sh
// install_qa_check`'s own real `scanelf`-driven step generates, and
// which `ebuild_merge::write_vdb_entry` now copies into every real vdb
// entry that has one (see that function's own doc comment).
//
// Deliberately just the data primitive plus one narrow read step,
// confirmed with the user before implementing each time: real parsing
// (`NeededEntry`) and real `LinkageMap.rebuild()`'s own initial data-
// gathering loop (`read_all_needed_entries`, every installed package's
// own vdb-stored `NEEDED.ELF.2`) -- no soname map, no multilib/runpath
// resolution, no `findConsumers`, no preserve-libs decision. Those
// remain real, separately-scoped future work (`rebuild()`'s own
// indexing alone is ~280 lines of multilib categorization, `$ORIGIN`
// runpath expansion, and implicit-runpath inference for bundled
// libraries; `findConsumers()` is ~140 more; `_find_libs_to_preserve()`'s
// own graph-reachability decision is another ~80 -- see
// `PORTING/PROMPT-next.md`'s own "preserve-libs registration" backlog
// entry). `#[allow(dead_code)]` below is deliberate: this module has no
// real caller yet, the same "narrow, additive, no wiring until the next
// slice needs it" shape this pilot has used before (e.g. `masters =`
// parsing landing before eclass masters-chain search consumed it).
#![allow(dead_code)]

use std::path::Path;

/// Real `NeededEntry.__slots__`: one parsed `NEEDED.ELF.2` line.
/// `soname` is a plain (possibly empty) string, not `Option`, matching
/// real Python exactly -- real `scanelf` genuinely reports an empty
/// soname for some real libraries (e.g. musl's own `libc.so`, which has
/// no `DT_SONAME` at all; this is precisely why real `misc-functions.sh`
/// deliberately never uses `scanelf -q`, see that script's own comment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeededEntry {
    pub arch: String,
    pub filename: String,
    pub soname: String,
    pub runpaths: Vec<String>,
    pub needed: Vec<String>,
    /// Real `NeededEntry._MULTILIB_CAT_INDEX`: the optional 6th field,
    /// `None` when the field is either absent (real, pre-multilib-
    /// category `NEEDED.ELF.2` data) or present-but-empty.
    pub multilib_category: Option<String>,
}

impl NeededEntry {
    /// Real `NeededEntry.parse()`: `arch;filename;soname;rpaths;needed`,
    /// semicolon-delimited, an optional 6th `multilib_category` field,
    /// any further fields silently ignored (real "extra fields may exist
    /// for future extensions"). `None` for a malformed line (real
    /// `InvalidData`, fewer than 5 fields) -- callers skip it and keep
    /// going, the same tolerance real `LinkageMap.rebuild()` itself
    /// already has for a bad line (`writemsg_level` + `continue`, never
    /// aborting the whole read over one bad entry).
    ///
    /// `rpaths`'s own real `"  -  "` sentinel (two spaces, a dash, two
    /// spaces) means "no rpath at all" -- real `scanelf`'s own `%r`
    /// output for an object with none, since `bin/misc-functions.sh`
    /// deliberately never passes `scanelf -q` (which would otherwise
    /// omit rpath-less-and-soname-less libraries like musl's `libc.so`
    /// entirely) and so must handle this literal placeholder itself.
    pub fn parse(line: &str) -> Option<Self> {
        let fields: Vec<&str> = line.split(';').collect();
        if fields.len() < 5 {
            return None;
        }
        let multilib_category = fields
            .get(5)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let rpaths = if fields[3] == "  -  " { "" } else { fields[3] };
        let runpaths = rpaths
            .split(':')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        let needed = fields[4]
            .split(',')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();

        Some(Self {
            arch: fields[0].to_string(),
            filename: fields[1].to_string(),
            soname: fields[2].to_string(),
            runpaths,
            needed,
            multilib_category,
        })
    }

    /// Parses every line of a real `NEEDED.ELF.2` file's own text,
    /// silently skipping any malformed line (see `parse`'s own doc
    /// comment for why that matches real tolerance).
    pub fn parse_file(text: &str) -> Vec<Self> {
        text.lines().filter_map(Self::parse).collect()
    }
}

/// Real `LinkageMap.rebuild()`'s own initial data-gathering loop
/// (`LinkageMapELF.py:218-231`): for every real installed package (real
/// `dbapi.cpv_all()`, walked here the same way `ebuild_merge::
/// find_owners` already walks every installed package's own vdb
/// directory), its own real vdb-stored `NEEDED.ELF.2` (real `aux_get(cpv,
/// [self._needed_aux_key])`), parsed via `NeededEntry::parse_file`.
/// Degrades gracefully to an empty entry list for a package with no such
/// file at all (real `aux_get` itself already tolerates a missing aux
/// file the same way, returning `""`) -- a `cpv` is still included, with
/// an empty `Vec`, matching real `rebuild()`'s own unconditional per-cpv
/// walk (it never skips a `cpv` just because it happens to own no ELF
/// content). Returns `(cpv, entries)` pairs in sorted vdb directory-
/// listing order, for this pilot's own determinism -- real `cpv_all()`
/// has no particular real ordering guarantee, so this doesn't lose
/// anything real by sorting.
///
/// Still just the raw per-package data: no soname map, no multilib/
/// runpath resolution (real `rebuild()`'s own `libs`/`obj_properties`
/// indexing, `providers`/`consumers` bucketing, `$ORIGIN` expansion --
/// all real, separately-scoped future work), so nothing here yet answers
/// "what does path X provide" or "what needs path X" -- only "what did
/// each installed package's own real `NEEDED.ELF.2` say".
pub fn read_all_needed_entries(root: &Path) -> Vec<(String, Vec<NeededEntry>)> {
    let mut result = Vec::new();
    let pkg_root = root.join("var/db/pkg");
    let Ok(categories) = std::fs::read_dir(&pkg_root) else {
        return result;
    };
    let mut category_names: Vec<String> = categories
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    category_names.sort();

    for category in category_names {
        let category_path = pkg_root.join(&category);
        let Ok(packages) = std::fs::read_dir(&category_path) else {
            continue;
        };
        let mut pf_names: Vec<String> = packages
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        pf_names.sort();

        for pf in pf_names {
            let cpv = format!("{category}/{pf}");
            let needed_path = category_path.join(&pf).join("NEEDED.ELF.2");
            let entries = std::fs::read_to_string(&needed_path)
                .map(|text| NeededEntry::parse_file(&text))
                .unwrap_or_default();
            result.push((cpv, entries));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reads_a_real_minimal_five_field_line() {
        // Real, live-verified output for a real dynamically-linked ELF
        // binary with no DT_SONAME of its own (`/usr/bin/true`, see
        // `ebuild_merge.rs`'s own `real_merge_copies_a_real_needed_elf2_
        // into_the_vdb` test): the empty soname field is exactly why
        // real scanelf is never invoked with `-q`.
        let entry = NeededEntry::parse("X86_64;/usr/bin/true;;;libc.so.6").unwrap();
        assert_eq!(entry.arch, "X86_64");
        assert_eq!(entry.filename, "/usr/bin/true");
        assert_eq!(entry.soname, "");
        assert_eq!(entry.runpaths, Vec::<String>::new());
        assert_eq!(entry.needed, vec!["libc.so.6".to_string()]);
        assert_eq!(entry.multilib_category, None);
    }

    #[test]
    fn parse_reads_a_soname_multiple_rpaths_and_multiple_needed() {
        let entry = NeededEntry::parse(
            "X86_64;/usr/lib/libfoo.so.1;libfoo.so.1;/usr/lib/foo:/usr/lib/bar;libc.so.6,libm.so.6",
        )
        .unwrap();
        assert_eq!(entry.soname, "libfoo.so.1");
        assert_eq!(
            entry.runpaths,
            vec!["/usr/lib/foo".to_string(), "/usr/lib/bar".to_string()]
        );
        assert_eq!(
            entry.needed,
            vec!["libc.so.6".to_string(), "libm.so.6".to_string()]
        );
    }

    #[test]
    fn parse_treats_the_real_dash_sentinel_rpath_as_empty() {
        let entry = NeededEntry::parse("X86_64;/usr/bin/true;;  -  ;libc.so.6").unwrap();
        assert_eq!(entry.runpaths, Vec::<String>::new());
    }

    #[test]
    fn parse_reads_the_optional_sixth_multilib_category_field() {
        let entry = NeededEntry::parse("X86_64;/usr/bin/true;;;libc.so.6;x86_64").unwrap();
        assert_eq!(entry.multilib_category, Some("x86_64".to_string()));
    }

    #[test]
    fn parse_treats_an_empty_sixth_field_as_no_multilib_category() {
        let entry = NeededEntry::parse("X86_64;/usr/bin/true;;;libc.so.6;").unwrap();
        assert_eq!(entry.multilib_category, None);
    }

    #[test]
    fn parse_ignores_extra_fields_beyond_the_sixth() {
        let entry =
            NeededEntry::parse("X86_64;/usr/bin/true;;;libc.so.6;x86_64;future;fields").unwrap();
        assert_eq!(entry.multilib_category, Some("x86_64".to_string()));
    }

    #[test]
    fn parse_rejects_a_line_with_fewer_than_five_fields() {
        assert_eq!(NeededEntry::parse("X86_64;/usr/bin/true;;"), None);
        assert_eq!(NeededEntry::parse(""), None);
    }

    #[test]
    fn parse_file_skips_malformed_lines_and_keeps_going() {
        let text = "X86_64;/usr/bin/true;;;libc.so.6\n\
                     bad-line\n\
                     X86_64;/usr/lib/libfoo.so.1;libfoo.so.1;;\n";
        let entries = NeededEntry::parse_file(text);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].filename, "/usr/bin/true");
        assert_eq!(entries[1].filename, "/usr/lib/libfoo.so.1");
    }

    fn tempdir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "portuale-needed-elf-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn read_all_needed_entries_covers_every_installed_package_including_ones_without_one() {
        let root = tempdir();
        let with_needed = root.join("var/db/pkg/dev-libs/withneeded-1.0");
        let without_needed = root.join("var/db/pkg/dev-libs/withoutneeded-1.0");
        std::fs::create_dir_all(&with_needed).unwrap();
        std::fs::create_dir_all(&without_needed).unwrap();
        std::fs::write(
            with_needed.join("NEEDED.ELF.2"),
            "X86_64;/usr/bin/withneeded;;;libc.so.6\n",
        )
        .unwrap();

        let all = read_all_needed_entries(&root);
        assert_eq!(
            all,
            vec![
                (
                    "dev-libs/withneeded-1.0".to_string(),
                    vec![NeededEntry::parse("X86_64;/usr/bin/withneeded;;;libc.so.6").unwrap()]
                ),
                ("dev-libs/withoutneeded-1.0".to_string(), vec![]),
            ]
        );
    }

    #[test]
    fn read_all_needed_entries_degrades_gracefully_when_var_db_pkg_is_missing() {
        let root = tempdir();
        assert_eq!(read_all_needed_entries(&root), Vec::new());
    }
}
