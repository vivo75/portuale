// Real `NeededEntry` (`lib/portage/util/_dyn_libs/NeededEntry.py`): the
// data model for one parsed line of a real `NEEDED.ELF.2` file -- the
// aux vdb metadata real, unmodified `bin/misc-functions.sh
// install_qa_check`'s own real `scanelf`-driven step generates, and
// which `ebuild_merge::write_vdb_entry` now copies into every real vdb
// entry that has one (see that function's own doc comment).
//
// Each step confirmed with the user before implementing: real parsing
// (`NeededEntry`), real `LinkageMap.rebuild()`'s own initial data-
// gathering loop (`read_all_needed_entries`), and now `rebuild()`'s own
// remaining indexing logic (`rebuild`, `ObjKey`/`ObjProperties`/
// `SonameMap`/`LinkageMap` -- multilib categorization, `$ORIGIN` runpath
// expansion, implicit-runpath inference for bundled libraries, and the
// per-architecture providers/consumers soname map). Deliberately still
// missing: the live-`scanelf`-for-orphaned-preserved-libs branch inside
// real `rebuild()` itself (`LinkageMapELF.py:233-324` -- the one place
// real portage falls back to a raw ELF header read rather than
// `NEEDED.ELF.2`, out of scope until preserve-libs actually needs it),
// `findConsumers()` (~140 lines), and `_find_libs_to_preserve()`'s own
// graph-reachability decision (~80 lines) -- both real, separately-
// scoped future work (see `PORTING/PROMPT-next.md`'s own "preserve-libs
// registration" backlog entry). `#[allow(dead_code)]` below is
// deliberate: this module has no real caller yet, the same "narrow,
// additive, no wiring until the next slice needs it" shape this pilot
// has used before (e.g. `masters =` parsing landing before eclass
// masters-chain search consumed it).
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
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

/// Real `_approx_multilib_categories` (`LinkageMapELF.py:29-46`): maps a
/// real ELF `e_machine` value (real `NEEDED.ELF.2`'s own `arch` field,
/// already stripped of its real `EM_` prefix by real, unmodified
/// `bin/misc-functions.sh` before it's ever written) to an approximate
/// multilib category -- only ever consulted when a `NeededEntry` has no
/// `multilib_category` field of its own (real, pre-multilib-category
/// `NEEDED.ELF.2` data, or this pilot's own fixtures, which never emit
/// that optional 6th field). Falls back to the raw `arch` string
/// unchanged for anything not in the table, exactly like real portage.
fn approx_multilib_category(arch: &str) -> String {
    match arch {
        "386" => "x86_32",
        "68K" => "m68k_32",
        "AARCH64" => "arm_64",
        "ALPHA" => "alpha_64",
        "ARM" => "arm_32",
        "IA_64" => "ia64_64",
        "MIPS" => "mips_o32",
        "PARISC" => "hppa_64",
        "PPC" => "ppc_32",
        "PPC64" => "ppc_64",
        "S390" => "s390_64",
        "SH" => "sh_32",
        "SPARC" => "sparc_32",
        "SPARC32PLUS" => "sparc_32",
        "SPARCV9" => "sparc_64",
        "X86_64" => "x86_64",
        other => other,
    }
    .to_string()
}

/// Real `portage.util.normalize_path` (`lib/portage/util/__init__.py:
/// 139-153`): a lexical, non-symlink-resolving path normalizer (real
/// `os.path.normpath`, with real portage's own leading-`//`-vs-`/` fix)
/// -- collapses `.`/empty segments, resolves `..` segments against
/// already-collapsed real segments, never touches the filesystem.
fn normalize_path(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => continue,
            ".." => {
                if matches!(out.last(), Some(&last) if last != "..") {
                    out.pop();
                } else if !absolute {
                    out.push("..");
                }
            }
            _ => out.push(seg),
        }
    }
    let joined = out.join("/");
    if absolute {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

/// Real dynamic-linker `$ORIGIN`/`${ORIGIN}` runpath expansion (real
/// `rebuild()`'s own `varexpand(x, {"ORIGIN": os.path.dirname(entry.
/// filename)}, ...)`) -- narrowed to just this one real substitution
/// (portage's own general `varexpand` handles arbitrary `${VAR}`/`$VAR`
/// references; `ORIGIN` is the only one `rebuild()` itself ever
/// supplies a value for).
fn expand_origin(rpath: &str, origin: &str) -> String {
    rpath
        .replace("${ORIGIN}", origin)
        .replace("$ORIGIN", origin)
}

/// Real `LinkageMap._ObjectKey`'s own generated key (`LinkageMapELF.py:
/// 98-148`): a real `(dev, ino)` pair when the object still exists on
/// disk (real `os.stat`, follows symlinks, matching real `_obj_key`
/// exactly) -- the same file reachable via multiple filesystem paths
/// (symlinks, hardlinks) collapses to one entry, every path kept as an
/// `alt_paths` entry (see `ObjProperties`). Falls back to the object's
/// own literal path string when it doesn't exist: real `_obj_key`
/// instead falls back to `os.path.realpath(...)` (symlink-resolved, but
/// tolerant of a nonexistent target) -- a deliberate, narrower
/// simplification for a case that should be rare in practice (a real
/// `NEEDED.ELF.2` entry, read moments after real `scanelf` itself
/// confirmed the object's existence, no longer existing by the time this
/// runs).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObjKey {
    Inode(u64, u64),
    Path(String),
}

pub fn obj_key(root: &Path, obj: &str) -> ObjKey {
    let abs = root.join(obj.trim_start_matches('/'));
    match std::fs::metadata(&abs) {
        Ok(meta) => {
            use std::os::unix::fs::MetadataExt;
            ObjKey::Inode(meta.dev(), meta.ino())
        }
        Err(_) => ObjKey::Path(obj.to_string()),
    }
}

/// Real `LinkageMap._obj_properties_class` (a real `slot_dict_class`):
/// everything real `rebuild()` records about one indexed object, keyed
/// by its own `ObjKey`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjProperties {
    /// Real `arch` (the multilib category, real or approximated).
    pub category: String,
    pub needed: BTreeSet<String>,
    pub runpaths: Vec<String>,
    pub soname: String,
    /// Every real filename this exact object was indexed under -- always
    /// has at least one entry (the first-seen one, real `myprops` is
    /// only ever created once per `ObjKey`).
    pub alt_paths: Vec<String>,
    /// The real owning `cpv` (`category/pf`).
    pub owner: String,
}

/// Real `LinkageMap._soname_map_class` (a real `slot_dict_class`): every
/// real object that provides this soname (`DT_SONAME` matches it), and
/// every real object that needs it (this soname appears in its own
/// `DT_NEEDED` list) -- within one multilib category's own map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SonameMap {
    pub providers: BTreeSet<ObjKey>,
    pub consumers: BTreeSet<ObjKey>,
}

/// Real `LinkageMap._libs`/`_obj_properties`, populated by `rebuild`.
#[derive(Debug, Clone, Default)]
pub struct LinkageMap {
    /// Real `self._libs`: multilib category -> soname -> providers/
    /// consumers.
    pub libs: BTreeMap<String, BTreeMap<String, SonameMap>>,
    pub obj_properties: BTreeMap<ObjKey, ObjProperties>,
}

/// Real `LinkageMap.rebuild()`'s own remaining indexing logic
/// (`LinkageMapELF.py:325-469`, everything after the initial data-
/// gathering loop `read_all_needed_entries` already covers): consumes
/// `owner_entries` (real shape: every installed package's own real
/// `NEEDED.ELF.2` entries, exactly what `read_all_needed_entries`
/// returns) and builds the real soname providers/consumers map.
///
/// Per entry: the real multilib category (its own `multilib_category`
/// field, or `approx_multilib_category`'s own fallback), real
/// `normalize_path`'d filename, and real `$ORIGIN`-expanded (then also
/// `normalize_path`'d) runpaths. Then, real "implicit runpath" inference
/// for bundled libraries (`LinkageMapELF.py:380-410`): within the *same*
/// owner package's own entries, if a needed soname is provided by
/// another entry from that same owner, and that provider's own directory
/// isn't already in the runpaths, it's added -- accounting for internal
/// library resolution a package may implement itself (e.g. bundled
/// libraries), without requiring an explicit rpath for it.
///
/// Finally, real per-object indexing: an object already indexed under
/// the same `ObjKey` (a hardlink/symlink alias, or simply listed twice)
/// only ever contributes its own filename as an extra `alt_paths` entry
/// -- real "only one set of data can be correct... mixing data may
/// corrupt the index" -- never re-indexed into `libs`. A newly-seen
/// object's own soname (if non-empty) is added as a provider, and every
/// one of its own needed sonames as a consumer, both keyed by its real
/// multilib category.
pub fn rebuild(root: &Path, owner_entries: &[(String, Vec<NeededEntry>)]) -> LinkageMap {
    struct Resolved {
        owner: String,
        category: String,
        filename: String,
        soname: String,
        runpaths: Vec<String>,
        needed: Vec<String>,
    }

    let mut resolved_by_owner: Vec<Vec<Resolved>> = Vec::new();
    for (owner, entries) in owner_entries {
        let mut resolved: Vec<Resolved> = Vec::new();
        for entry in entries {
            let category = entry
                .multilib_category
                .clone()
                .unwrap_or_else(|| approx_multilib_category(&entry.arch));
            let filename = normalize_path(&entry.filename);
            let origin = Path::new(&filename)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let runpaths = entry
                .runpaths
                .iter()
                .map(|r| normalize_path(&expand_origin(r, &origin)))
                .collect();
            resolved.push(Resolved {
                owner: owner.clone(),
                category,
                filename,
                soname: entry.soname.clone(),
                runpaths,
                needed: entry.needed.clone(),
            });
        }
        resolved_by_owner.push(resolved);
    }

    for entries in &mut resolved_by_owner {
        let providers: BTreeMap<(String, String), String> = entries
            .iter()
            .filter(|e| !e.soname.is_empty())
            .map(|e| ((e.category.clone(), e.soname.clone()), e.filename.clone()))
            .collect();
        for entry in entries.iter_mut() {
            let mut implicit = Vec::new();
            for soname in &entry.needed {
                if let Some(provider_filename) =
                    providers.get(&(entry.category.clone(), soname.clone()))
                {
                    let provider_dir = Path::new(provider_filename)
                        .parent()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if !entry.runpaths.contains(&provider_dir) {
                        implicit.push(provider_dir);
                    }
                }
            }
            entry.runpaths.extend(implicit);
        }
    }

    let mut map = LinkageMap::default();
    for entries in &resolved_by_owner {
        for entry in entries {
            let key = obj_key(root, &entry.filename);
            if let Some(existing) = map.obj_properties.get_mut(&key) {
                existing.alt_paths.push(entry.filename.clone());
                continue;
            }
            map.obj_properties.insert(
                key.clone(),
                ObjProperties {
                    category: entry.category.clone(),
                    needed: entry.needed.iter().cloned().collect(),
                    runpaths: entry.runpaths.clone(),
                    soname: entry.soname.clone(),
                    alt_paths: vec![entry.filename.clone()],
                    owner: entry.owner.clone(),
                },
            );

            let arch_map = map.libs.entry(entry.category.clone()).or_default();
            if !entry.soname.is_empty() {
                arch_map
                    .entry(entry.soname.clone())
                    .or_default()
                    .providers
                    .insert(key.clone());
            }
            for needed_soname in &entry.needed {
                arch_map
                    .entry(needed_soname.clone())
                    .or_default()
                    .consumers
                    .insert(key.clone());
            }
        }
    }
    map
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

    fn make_object(root: &std::path::Path, relative: &str) {
        let abs = root.join(relative.trim_start_matches('/'));
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::write(&abs, b"fake elf content").unwrap();
    }

    #[test]
    fn rebuild_indexes_a_simple_provider_and_consumer() {
        let root = tempdir();
        make_object(&root, "/usr/lib/libfoo.so.1");
        make_object(&root, "/usr/bin/consumer");

        let owner_entries = vec![
            (
                "dev-libs/provider-1.0".to_string(),
                vec![NeededEntry::parse("X86_64;/usr/lib/libfoo.so.1;libfoo.so.1;;").unwrap()],
            ),
            (
                "dev-libs/consumer-1.0".to_string(),
                vec![NeededEntry::parse("X86_64;/usr/bin/consumer;;;libfoo.so.1").unwrap()],
            ),
        ];

        let map = rebuild(&root, &owner_entries);
        let soname_map = &map.libs["x86_64"]["libfoo.so.1"];
        let provider_key = obj_key(&root, "/usr/lib/libfoo.so.1");
        let consumer_key = obj_key(&root, "/usr/bin/consumer");
        assert!(soname_map.providers.contains(&provider_key));
        assert!(soname_map.consumers.contains(&consumer_key));
        assert_eq!(
            map.obj_properties[&provider_key].owner,
            "dev-libs/provider-1.0"
        );
    }

    #[test]
    fn rebuild_falls_back_to_the_approx_multilib_category_when_the_field_is_absent() {
        let root = tempdir();
        make_object(&root, "/usr/lib/libfoo.so.1");
        let owner_entries = vec![(
            "dev-libs/provider-1.0".to_string(),
            vec![NeededEntry::parse("AARCH64;/usr/lib/libfoo.so.1;libfoo.so.1;;").unwrap()],
        )];
        let map = rebuild(&root, &owner_entries);
        assert!(
            map.libs.contains_key("arm_64"),
            "{:?}",
            map.libs.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn rebuild_expands_origin_in_runpaths() {
        let root = tempdir();
        make_object(&root, "/usr/lib/foo/consumer");
        let owner_entries = vec![(
            "dev-libs/pkg-1.0".to_string(),
            vec![
                NeededEntry::parse("X86_64;/usr/lib/foo/consumer;;$ORIGIN/../bar;libfoo.so.1")
                    .unwrap(),
            ],
        )];
        let map = rebuild(&root, &owner_entries);
        let key = obj_key(&root, "/usr/lib/foo/consumer");
        assert_eq!(
            map.obj_properties[&key].runpaths,
            vec!["/usr/lib/bar".to_string()]
        );
    }

    #[test]
    fn rebuild_infers_an_implicit_runpath_for_a_same_owner_bundled_provider() {
        let root = tempdir();
        make_object(&root, "/opt/bundled/libbundled.so.1");
        make_object(&root, "/opt/bundled/consumer");
        // Same owner, no explicit rpath on the consumer -- real bundled-
        // library internal resolution (LinkageMapELF.py:380-410).
        let owner_entries = vec![(
            "dev-libs/pkg-1.0".to_string(),
            vec![
                NeededEntry::parse("X86_64;/opt/bundled/libbundled.so.1;libbundled.so.1;;")
                    .unwrap(),
                NeededEntry::parse("X86_64;/opt/bundled/consumer;;;libbundled.so.1").unwrap(),
            ],
        )];
        let map = rebuild(&root, &owner_entries);
        let key = obj_key(&root, "/opt/bundled/consumer");
        assert_eq!(
            map.obj_properties[&key].runpaths,
            vec!["/opt/bundled".to_string()]
        );
    }

    #[test]
    fn rebuild_does_not_infer_an_implicit_runpath_across_different_owners() {
        let root = tempdir();
        make_object(&root, "/opt/a/libshared.so.1");
        make_object(&root, "/opt/b/consumer");
        let owner_entries = vec![
            (
                "dev-libs/a-1.0".to_string(),
                vec![NeededEntry::parse("X86_64;/opt/a/libshared.so.1;libshared.so.1;;").unwrap()],
            ),
            (
                "dev-libs/b-1.0".to_string(),
                vec![NeededEntry::parse("X86_64;/opt/b/consumer;;;libshared.so.1").unwrap()],
            ),
        ];
        let map = rebuild(&root, &owner_entries);
        let key = obj_key(&root, "/opt/b/consumer");
        assert!(map.obj_properties[&key].runpaths.is_empty());
    }

    #[test]
    fn rebuild_dedups_the_same_real_object_reached_via_two_recorded_paths() {
        let root = tempdir();
        make_object(&root, "/usr/lib/real-lib.so.1");
        let real = root.join("usr/lib/real-lib.so.1");
        let hardlink = root.join("usr/lib/alt-name.so.1");
        std::fs::hard_link(&real, &hardlink).unwrap();

        let owner_entries = vec![(
            "dev-libs/pkg-1.0".to_string(),
            vec![
                NeededEntry::parse("X86_64;/usr/lib/real-lib.so.1;real-lib.so.1;;").unwrap(),
                NeededEntry::parse("X86_64;/usr/lib/alt-name.so.1;real-lib.so.1;;").unwrap(),
            ],
        )];
        let map = rebuild(&root, &owner_entries);
        assert_eq!(map.obj_properties.len(), 1, "{:?}", map.obj_properties);
        let (_, props) = map.obj_properties.iter().next().unwrap();
        assert_eq!(
            props.alt_paths,
            vec![
                "/usr/lib/real-lib.so.1".to_string(),
                "/usr/lib/alt-name.so.1".to_string()
            ]
        );
    }

    #[test]
    fn obj_key_falls_back_to_a_path_key_when_the_object_no_longer_exists() {
        let root = tempdir();
        assert_eq!(
            obj_key(&root, "/usr/lib/gone.so.1"),
            ObjKey::Path("/usr/lib/gone.so.1".to_string())
        );
    }

    #[test]
    fn obj_key_uses_the_real_inode_when_the_object_exists() {
        let root = tempdir();
        make_object(&root, "/usr/lib/real.so.1");
        assert!(matches!(
            obj_key(&root, "/usr/lib/real.so.1"),
            ObjKey::Inode(_, _)
        ));
    }
}
