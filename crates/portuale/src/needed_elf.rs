//! `NEEDED.ELF.2` parsing and linkage-map computation: one parsed aux-file
//! line (`NeededEntry`), the per-package gathering loop
//! (`read_all_needed_entries`), the soname providers/consumers index
//! (`rebuild`), and the graph-reachability decisions (`find_consumers`,
//! `find_libs_to_preserve` plus `getlibpaths` and the minimal `LibGraph`).
//! Still missing: the live-`scanelf`-for-orphaned-preserved-libs branch
//! inside `rebuild()` itself, and wiring `find_libs_to_preserve` into a
//! merge/unmerge's control flow (a separate task, so `dead_code` stays
//! allowed until the next slice needs it).
// Portage: NeededEntry.py, NeededEntry; LinkageMapELF.py, rebuild/findConsumers; vartree.py, _find_libs_to_preserve
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// One parsed `NEEDED.ELF.2` line. `soname` is a plain (possibly empty)
/// string, not `Option`: `scanelf` genuinely reports an empty soname for
/// some libraries (e.g. musl's `libc.so`, which has no `DT_SONAME`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeededEntry {
    pub arch: String,
    pub filename: String,
    pub soname: String,
    pub runpaths: Vec<String>,
    pub needed: Vec<String>,
    /// The optional 6th field: `None` when absent or present-but-empty.
    pub multilib_category: Option<String>,
}

impl NeededEntry {
    /// `arch;filename;soname;rpaths;needed`, semicolon-delimited, with an
    /// optional 6th `multilib_category` field; further fields ignored.
    /// `None` for a malformed line (fewer than 5 fields); callers skip it
    /// and keep going.
    ///
    /// The `rpaths` `"  -  "` sentinel (two spaces, a dash, two spaces)
    /// means "no rpath at all".
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

    /// Parses every line of a `NEEDED.ELF.2` file's text, silently skipping
    /// malformed lines.
    pub fn parse_file(text: &str) -> Vec<Self> {
        text.lines().filter_map(Self::parse).collect()
    }

    /// Format this entry for writing back to a `NEEDED.ELF.2` file. Two
    /// intentional asymmetries from the read side: an empty `runpaths`
    /// serializes as `""`, never the `"  -  "` sentinel; the 6th field is
    /// always present, even when `None` (as `""`).
    pub fn to_needed_line(&self) -> String {
        format!(
            "{};{};{};{};{};{}\n",
            self.arch,
            self.filename,
            self.soname,
            self.runpaths.join(":"),
            self.needed.join(","),
            self.multilib_category.as_deref().unwrap_or("")
        )
    }
}

/// Read one ELF header's `EI_CLASS`, `e_machine` and `e_flags` and classify
/// it into the multilib category (`x86_64`, `x86_32`, `arm_64`, ...), or
/// `None` for a non-ELF/unreadable file or an unrecognized combination
/// (which leaves the entry's 6th `NEEDED.ELF.2` field empty).
pub fn compute_multilib_category(path: &Path) -> Option<String> {
    const ELFCLASS32: u8 = 1;
    const ELFCLASS64: u8 = 2;
    fn u16_at(data: &[u8], off: usize, le: bool) -> Option<u16> {
        let b = data.get(off..off + 2)?;
        Some(if le {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        })
    }
    fn u32_at(data: &[u8], off: usize, le: bool) -> Option<u32> {
        let b = data.get(off..off + 4)?;
        Some(if le {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        })
    }

    let data = std::fs::read(path).ok()?;
    if data.len() < 20 || &data[0..4] != b"\x7fELF" {
        return None;
    }
    let ei_class = data[4];
    let le = match data[5] {
        1 => true,
        2 => false,
        _ => return None,
    };
    let e_machine = u16_at(&data, 18, le)?;
    let flags_off = if ei_class == ELFCLASS64 { 48 } else { 36 };
    let e_flags = u32_at(&data, flags_off, le)?;

    // ELF machine/flag constants (Portage's portage/util/elf/constants.py).
    const EM_SPARC: u16 = 2;
    const EM_386: u16 = 3;
    const EM_68K: u16 = 4;
    const EM_MIPS: u16 = 8;
    const EM_PARISC: u16 = 15;
    const EM_SPARC32PLUS: u16 = 18;
    const EM_PPC: u16 = 20;
    const EM_PPC64: u16 = 21;
    const EM_S390: u16 = 22;
    const EM_ARM: u16 = 40;
    const EM_SH: u16 = 42;
    const EM_SPARCV9: u16 = 43;
    const EM_ARC: u16 = 45;
    const EM_IA_64: u16 = 50;
    const EM_X86_64: u16 = 62;
    const EM_ARC_COMPACT: u16 = 93;
    const EM_ALTERA_NIOS2: u16 = 113;
    const EM_MCST_ELBRUS: u16 = 175;
    const EM_AARCH64: u16 = 183;
    const EM_ARC_COMPACT2: u16 = 195;
    const EM_AMDGPU: u16 = 224;
    const EM_RISCV: u16 = 243;
    const EM_BPF: u16 = 247;
    const EM_ARC_COMPACT3_64: u16 = 253;
    const EM_ARC_COMPACT3: u16 = 255;
    const EM_LOONGARCH: u16 = 258;
    const EM_ALPHA: u16 = 0x9026;
    const EF_MIPS_ABI: u32 = 0x0000_F000;
    const EF_MIPS_ABI2: u32 = 0x0000_0020;
    const E_MIPS_ABI_O32: u32 = 0x0000_1000;
    const E_MIPS_ABI_O64: u32 = 0x0000_2000;
    const E_MIPS_ABI_EABI32: u32 = 0x0000_3000;
    const E_MIPS_ABI_EABI64: u32 = 0x0000_4000;
    const EF_RISCV_RVC: u32 = 0x0001;
    const EF_RISCV_FLOAT_ABI_DOUBLE: u32 = 0x0004;
    const EF_LOONGARCH_ABI_MASK: u32 = 0x07;

    let prefix = match e_machine {
        EM_386 => "x86",
        EM_68K => "m68k",
        EM_AARCH64 => "arm",
        EM_ALPHA => "alpha",
        EM_AMDGPU => "amdgpu",
        EM_ALTERA_NIOS2 => "nios2",
        EM_ARC | EM_ARC_COMPACT | EM_ARC_COMPACT2 | EM_ARC_COMPACT3 | EM_ARC_COMPACT3_64 => "arc",
        EM_ARM => "arm",
        EM_BPF => "bpf",
        EM_IA_64 => "ia64",
        EM_LOONGARCH => "loong",
        EM_MCST_ELBRUS => "e2k",
        EM_MIPS => "mips",
        EM_PARISC => "hppa",
        EM_PPC | EM_PPC64 => "ppc",
        EM_RISCV => "riscv",
        EM_S390 => "s390",
        EM_SH => "sh",
        EM_SPARC | EM_SPARC32PLUS | EM_SPARCV9 => "sparc",
        EM_X86_64 => "x86",
        _ => return None,
    };

    let suffix = match prefix {
        "loong" => match e_flags & EF_LOONGARCH_ABI_MASK {
            0b001 => "lp64s".to_string(),
            0b010 => "lp64f".to_string(),
            0b011 => "lp64d".to_string(),
            0b101 => "ilp32s".to_string(),
            0b110 => "ilp32f".to_string(),
            0b111 => "ilp32d".to_string(),
            _ => return None,
        },
        "mips" => {
            let abi = e_flags & EF_MIPS_ABI;
            if abi != 0 {
                match abi {
                    E_MIPS_ABI_EABI32 => "eabi32",
                    E_MIPS_ABI_EABI64 => "eabi64",
                    E_MIPS_ABI_O32 => "o32",
                    E_MIPS_ABI_O64 => "o64",
                    _ => return None,
                }
                .to_string()
            } else if e_flags & EF_MIPS_ABI2 != 0 {
                "n32".to_string()
            } else if ei_class == ELFCLASS64 {
                "n64".to_string()
            } else {
                return None;
            }
        }
        "riscv" => match (ei_class, e_flags) {
            (ELFCLASS64, EF_RISCV_RVC) => "lp64".to_string(),
            (ELFCLASS64, f) if f == EF_RISCV_RVC | EF_RISCV_FLOAT_ABI_DOUBLE => "lp64d".to_string(),
            (ELFCLASS32, EF_RISCV_RVC) => "ilp32".to_string(),
            (ELFCLASS32, f) if f == EF_RISCV_RVC | EF_RISCV_FLOAT_ABI_DOUBLE => {
                "ilp32d".to_string()
            }
            _ => return None,
        },
        _ => match ei_class {
            ELFCLASS64 => "64".to_string(),
            ELFCLASS32 if e_machine == EM_X86_64 => "x32".to_string(),
            ELFCLASS32 => "32".to_string(),
            _ => return None,
        },
    };
    Some(format!("{prefix}_{suffix}"))
}

/// One `fnmatch`-style exclusion pattern list: `shlex`-split the
/// `PROVIDES_EXCLUDE`/`REQUIRES_EXCLUDE` value and `lstrip("/")` each pattern.
fn exclude_patterns(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for c in value.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => current.push(c),
            None if c == '\'' || c == '"' => quote = Some(c),
            None if c.is_whitespace() => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            None => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out.into_iter()
        .map(|p| p.trim_start_matches('/').to_string())
        .collect()
}

/// Turn one package's `NEEDED.ELF.2` entries into the `PROVIDES` and
/// `REQUIRES` strings written into `build-info` (and the archive's
/// `metadata/PROVIDES`/`metadata/REQUIRES`). Each side is `None` when its
/// map is empty (no file is written then). Entries without a recognized
/// `multilib_category` must be filtered before calling.
/// Portage: soname_deps.py, SonameDepsProcessor
pub fn generate_soname_deps(
    entries: &[NeededEntry],
    provides_exclude: &str,
    requires_exclude: &str,
) -> (Option<String>, Option<String>) {
    use std::collections::{BTreeMap, BTreeSet};

    let provides_exclude = exclude_patterns(provides_exclude);
    let requires_exclude = exclude_patterns(requires_exclude);
    let excluded = |patterns: &[String], name: &str| {
        patterns
            .iter()
            .any(|p| crate::install_mask::fnmatch(name, p))
    };

    let mut basename_map: BTreeMap<String, Vec<&NeededEntry>> = BTreeMap::new();
    // cat -> soname -> set of expanded runpath sets.
    let mut requires_map: BTreeMap<String, BTreeMap<String, BTreeSet<Vec<String>>>> =
        BTreeMap::new();
    let mut provides_unfiltered: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut provides_map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for entry in entries {
        let Some(cat) = entry.multilib_category.as_deref() else {
            continue;
        };
        let basename = basename(&entry.filename).to_string();
        basename_map.entry(basename).or_default().push(entry);

        if !entry.needed.is_empty()
            && !excluded(&requires_exclude, entry.filename.trim_start_matches('/'))
        {
            let origin = dirname(&entry.filename);
            let runpaths: Vec<String> = entry
                .runpaths
                .iter()
                .map(|r| normalize_path(&expand_origin(r, &origin)))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            for needed in &entry.needed {
                if !excluded(&requires_exclude, needed) {
                    requires_map
                        .entry(cat.to_string())
                        .or_default()
                        .entry(needed.clone())
                        .or_default()
                        .insert(runpaths.clone());
                }
            }
        }

        if entry.soname.is_empty() {
            continue;
        }
        provides_unfiltered
            .entry(cat.to_string())
            .or_default()
            .insert(entry.soname.clone());
        if !excluded(&provides_exclude, entry.filename.trim_start_matches('/'))
            && !excluded(&provides_exclude, &entry.soname)
        {
            provides_map
                .entry(cat.to_string())
                .or_default()
                .insert(entry.soname.clone());
        }
    }

    // A provided soname is never a requirement; an internal library
    // without a soname is resolved through a matching DT_RUNPATH entry.
    let cats: BTreeSet<String> = requires_map
        .keys()
        .chain(provides_map.keys())
        .cloned()
        .collect();
    let mut drop_requires: Vec<(String, String)> = Vec::new();
    for cat in &cats {
        let Some(reqs) = requires_map.get_mut(cat) else {
            continue;
        };
        for (soname, consumers) in reqs.iter_mut() {
            if provides_unfiltered
                .get(cat)
                .is_some_and(|p| p.contains(soname))
            {
                drop_requires.push((cat.clone(), soname.clone()));
                continue;
            }
            if let Some(basename_entries) = basename_map.get(soname) {
                let mut resolved = false;
                for entry in basename_entries {
                    if entry.multilib_category.as_deref() != Some(cat.as_str()) {
                        continue;
                    }
                    let obj_dir = dirname(&entry.filename);
                    consumers.retain(|runpaths| !runpaths.contains(&obj_dir));
                    if consumers.is_empty() {
                        resolved = true;
                        break;
                    }
                }
                if resolved {
                    drop_requires.push((cat.clone(), soname.clone()));
                }
            }
        }
        for (cat, soname) in drop_requires.drain(..) {
            if let Some(reqs) = requires_map.get_mut(&cat) {
                reqs.remove(&soname);
            }
        }
    }

    let format = |map: &BTreeMap<String, BTreeSet<String>>| -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        for (cat, sonames) in map {
            if sonames.is_empty() {
                continue;
            }
            parts.push(format!("{cat}:"));
            parts.extend(sonames.iter().cloned());
        }
        (!parts.is_empty()).then(|| format!("{}\n", parts.join(" ")))
    };
    let requires = format(
        &requires_map
            .iter()
            .map(|(cat, reqs)| (cat.clone(), reqs.keys().cloned().collect()))
            .collect(),
    );
    let provides = format(&provides_map);
    (provides, requires)
}

/// For every installed package, its vdb-stored `NEEDED.ELF.2` parsed via
/// `NeededEntry::parse_file`. A package with no such file still yields a
/// `cpv`, with an empty `Vec`. Returns `(cpv, entries)` pairs in sorted vdb
/// directory-listing order, for determinism.
// Portage: LinkageMapELF.py, rebuild (initial data-gathering loop)
pub fn read_all_needed_entries(root: &Path) -> Vec<(String, Vec<NeededEntry>)> {
    // The old walk, moved into `FilesDb` unchanged (the `-MERGING-` skip
    // included); a missing, unreadable or non-UTF-8 file is an empty row.
    portage_vdb::for_root(root)
        .read_file_all("NEEDED.ELF.2")
        .unwrap_or_default()
        .into_iter()
        .map(|(key, data)| {
            let entries = data
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .map(|text| NeededEntry::parse_file(&text))
                .unwrap_or_default();
            (format!("{}/{}", key.category, key.pf), entries)
        })
        .collect()
}

/// Map an ELF `e_machine` value (the `NEEDED.ELF.2` `arch` field without its
/// `EM_` prefix) to an approximate multilib category, for entries with no
/// `multilib_category` field of their own. Unknown values pass through
/// unchanged.
// Portage: LinkageMapELF.py, _approx_multilib_categories
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

/// Lexical, non-symlink-resolving path normalizer: collapses `.`/empty
/// segments, resolves `..` against already-collapsed segments, never touches
/// the filesystem.
// Portage: util/__init__.py, normalize_path
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

/// Dynamic-linker `$ORIGIN`/`${ORIGIN}` runpath expansion: the only
/// substitution `rebuild()` supplies a value for.
fn expand_origin(rpath: &str, origin: &str) -> String {
    rpath
        .replace("${ORIGIN}", origin)
        .replace("$ORIGIN", origin)
}

/// Preserved libraries are in no `NEEDED.ELF.2` file, so run `scanelf -BF
/// '%a;%F;%S;%r;%n'` over them (without `-q`, so soname-less libraries are
/// not omitted) and index every reported line like `NEEDED.ELF.2` data,
/// owned by the preserving cpv.
///
/// `preserved` maps preserving cpv -> paths. Only still-existing regular
/// files and symlinks are scanned. Each line's `EM_`-prefixed arch is
/// stripped before parsing, and the multilib category comes from a live ELF
/// header read, falling back to the approximate table in `rebuild`.
///
/// Deliberate cuts (no fixture instance): no dummy line for a preserved
/// path `scanelf` reports nothing for (`find_consumers` returns `Err` there
/// and callers treat that as "no consumers"); no implicit-soname inference
/// for an empty-soname object; a missing `scanelf` degrades to no entries
/// with a loud stderr warning, while a `scanelf` that runs but fails
/// degrades silently.
///
/// Returns `(owner, entries)` groups ready to append to
/// `read_all_needed_entries`'s output before `rebuild`.
pub fn scan_preserved_lib_entries(
    root: &Path,
    preserved: &BTreeMap<String, Vec<String>>,
) -> Vec<(String, Vec<NeededEntry>)> {
    let mut targets: Vec<(String, String)> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (cpv, paths) in preserved {
        for p in paths {
            // First-wins on a duplicate path across cpvs.
            if !seen.insert(p.clone()) {
                continue;
            }
            let full = root.join(p.trim_start_matches('/'));
            let is_scannable = std::fs::symlink_metadata(&full)
                .map(|m| m.file_type().is_file() || m.file_type().is_symlink())
                .unwrap_or(false);
            if is_scannable {
                targets.push((cpv.clone(), full.to_string_lossy().into_owned()));
            }
        }
    }
    if targets.is_empty() {
        return Vec::new();
    }

    let mut args = vec!["-BF".to_string(), "%a;%F;%S;%r;%n".to_string()];
    args.extend(targets.iter().map(|(_, full)| full.clone()));
    // `/usr/bin/scanelf` first (empty `EPREFIX` assumed, like every other
    // live-execution path), then a `PATH` lookup.
    let scanelf_bin = if std::path::Path::new("/usr/bin/scanelf").is_file() {
        "/usr/bin/scanelf".to_string()
    } else {
        "scanelf".to_string()
    };
    let output = std::process::Command::new(&scanelf_bin)
        .args(&args)
        .output();
    let Ok(output) = output else {
        eprintln!(
            "!!! Command Not Found: {scanelf_bin} -- preserved-library orphan scan skipped, preserve-libs decisions may miss consumers"
        );
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let root_str = root.to_string_lossy();
    let root_prefix = format!("{}/", root_str.trim_end_matches('/'));
    // Which preserving cpv each scanned absolute path belongs to, so a
    // reported line is grouped under its owner.
    let mut path_owner: BTreeMap<String, String> = BTreeMap::new();
    for (cpv, full) in &targets {
        path_owner
            .entry(full.clone())
            .or_insert_with(|| cpv.clone());
    }
    let mut by_owner: BTreeMap<String, Vec<NeededEntry>> = BTreeMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        // Strip scanelf's `EM_`-prefixed arch (`EM_X86_64` -> `X86_64`).
        let Some(line) = line.get(3..) else {
            continue;
        };
        let Some(mut entry) = NeededEntry::parse(line) else {
            continue;
        };
        let full = entry.filename.clone();
        let Some(rel) = full.strip_prefix(&root_prefix) else {
            continue;
        };
        entry.filename = format!("/{rel}");
        entry.multilib_category = compute_multilib_category(Path::new(&full));
        if let Some(owner) = path_owner.get(&full) {
            by_owner.entry(owner.clone()).or_default().push(entry);
        }
    }
    by_owner.into_iter().collect()
}

/// Object identity for the index: a `(dev, ino)` pair when the object still
/// exists on disk (follows symlinks), so one file reachable via several
/// paths collapses to one entry with every path kept as `alt_paths`.
/// Falls back to the literal path string when it doesn't exist.
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

/// Everything `rebuild()` records about one indexed object, keyed by its
/// `ObjKey`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjProperties {
    /// The multilib category (indexed, or approximated).
    pub category: String,
    pub needed: BTreeSet<String>,
    pub runpaths: Vec<String>,
    pub soname: String,
    /// Every filename this exact object was indexed under -- always
    /// non-empty (the first-seen one; created once per `ObjKey`).
    pub alt_paths: Vec<String>,
    /// The owning `cpv` (`category/pf`).
    pub owner: String,
}

/// Every object providing a soname (`DT_SONAME` matches it), and every
/// object needing it -- within one multilib category's map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SonameMap {
    pub providers: BTreeSet<ObjKey>,
    pub consumers: BTreeSet<ObjKey>,
}

/// The `libs`/`obj_properties` index, populated by `rebuild`.
#[derive(Debug, Clone, Default)]
pub struct LinkageMap {
    /// Multilib category -> soname -> providers/consumers.
    pub libs: BTreeMap<String, BTreeMap<String, SonameMap>>,
    pub obj_properties: BTreeMap<ObjKey, ObjProperties>,
}

/// Consume `owner_entries` (every installed package's `NEEDED.ELF.2`
/// entries, as `read_all_needed_entries` returns) and build the soname
/// providers/consumers map.
///
/// Per entry: the multilib category (its own field, or the approximate
/// fallback), the normalized filename, and `$ORIGIN`-expanded normalized
/// runpaths. Then implicit-runpath inference for bundled libraries: within
/// the *same* owner's entries, if a needed soname is provided by another
/// entry from that owner and that provider's directory isn't in the
/// runpaths, it is added.
///
/// Finally, per-object indexing: an object already indexed under the same
/// `ObjKey` (a hardlink/symlink alias, or listed twice) only gains an extra
/// `alt_paths` entry and is never re-indexed. A newly-seen object's soname
/// (if non-empty) is added as a provider, and each needed soname as a
/// consumer, keyed by multilib category.
// Portage: LinkageMapELF.py, rebuild (indexing logic)
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

/// Like `os.path.dirname`: everything before the last `/`, or `""`; unlike
/// `Path::parent()`, `dirname("/foo")` is `"/"` (both rely on that here).
fn dirname(path: &str) -> String {
    match path.rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => path[..i].to_string(),
        None => String::new(),
    }
}

/// Like `os.path.basename`: everything after the last `/`.
fn basename(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

/// Read a config file the way `getlibpaths` needs: whitespace-normalize each
/// line, strip a `#`-prefixed token onward, skip empties. A missing file
/// degrades to an empty result.
fn grab_lines(path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let tokens: Vec<&str> = line
                .split_whitespace()
                .take_while(|tok| !tok.starts_with('#'))
                .collect();
            if tokens.is_empty() {
                None
            } else {
                Some(tokens.join(" "))
            }
        })
        .collect()
}

/// The default dynamic-linker library search path: `LD_LIBRARY_PATH` (an
/// explicit parameter, not an ambient env read), every line of the
/// root-scoped `/etc/ld.so.conf`, then the `/usr/lib`/`/lib` defaults --
/// each normalized. The `include` directive for `/etc/ld.so.conf.d/*.conf`
/// is deliberately not expanded.
// Portage: util/__init__.py, getlibpaths
pub fn getlibpaths(root: &Path, ld_library_path: Option<&str>) -> Vec<String> {
    let mut rval: Vec<String> = ld_library_path
        .unwrap_or("")
        .split(':')
        .map(String::from)
        .collect();
    rval.extend(grab_lines(&root.join("etc/ld.so.conf")));
    rval.push("/usr/lib".to_string());
    rval.push("/lib".to_string());
    rval.into_iter()
        .filter(|s| !s.is_empty())
        .map(|s| normalize_path(&s))
        .collect()
}

/// `portage.util.digraph`, narrowed to what `_find_libs_to_preserve` uses:
/// `add(node, parent)` (an edge where `parent` depends on `node`),
/// `root_nodes()` (nodes nothing depends on) and `child_nodes()` (what a
/// node depends on). Nodes are `ObjKey`s; `alt_paths` are tracked alongside.
#[derive(Debug, Clone, Default)]
struct LibGraph {
    children: BTreeMap<ObjKey, BTreeSet<ObjKey>>,
    parents: BTreeMap<ObjKey, BTreeSet<ObjKey>>,
    alt_paths: BTreeMap<ObjKey, BTreeSet<String>>,
}

impl LibGraph {
    fn ensure_node(&mut self, key: &ObjKey) {
        self.children.entry(key.clone()).or_default();
        self.parents.entry(key.clone()).or_default();
    }

    fn add(&mut self, node: &ObjKey, parent: Option<&ObjKey>) {
        self.ensure_node(node);
        if let Some(parent) = parent {
            self.ensure_node(parent);
            self.children.get_mut(parent).unwrap().insert(node.clone());
            self.parents.get_mut(node).unwrap().insert(parent.clone());
        }
    }

    /// Registers (or reuses) the graph node for `path`'s object identity,
    /// recording `path` as one of that node's `alt_paths`. Adds no edge.
    fn path_to_node(&mut self, root: &Path, path: &str) -> ObjKey {
        let key = obj_key(root, path);
        self.alt_paths
            .entry(key.clone())
            .or_default()
            .insert(path.to_string());
        key
    }

    fn root_nodes(&self) -> Vec<ObjKey> {
        self.parents
            .iter()
            .filter(|(_, parents)| parents.is_empty())
            .map(|(node, _)| node.clone())
            .collect()
    }

    fn child_nodes(&self, node: &ObjKey) -> Vec<ObjKey> {
        self.children
            .get(node)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect()
    }
}

/// Arguments of [`find_consumers`].
pub struct FindConsumersArgs<'a> {
    pub root: &'a Path,
    pub map: &'a LinkageMap,
    pub defpath: &'a [String],
    pub obj: &'a str,
    pub exclude_provider: Option<&'a dyn Fn(&str) -> bool>,
    pub greedy: bool,
}

/// `findConsumers`, narrowed to the one calling convention
/// `_find_libs_to_preserve` uses: `obj` is always a path string, and
/// `exclude_provider` is a single callable.
///
/// A consumer is dropped when some *other*, non-excluded provider of the
/// same soname already satisfies it (found in its runpath or `defpath`), so
/// only consumers that would actually break stay. Only consumers that can
/// reach `obj` (its directory is in their runpath or `defpath`) are returned.
/// `Err` when `obj` is not an indexed object at all.
// Portage: LinkageMapELF.py, findConsumers
pub fn find_consumers(args: FindConsumersArgs<'_>) -> Result<BTreeSet<String>, String> {
    let FindConsumersArgs {
        root,
        map,
        defpath,
        obj,
        exclude_provider,
        greedy,
    } = args;
    let obj_key_val = obj_key(root, obj);
    let Some(obj_props) = map.obj_properties.get(&obj_key_val) else {
        return Err(format!("{obj_key_val:?} ({obj}) not in object list"));
    };

    // A same-directory soname link pointing elsewhere: no consumers.
    if !obj_props.soname.is_empty() {
        let soname_link = root
            .join(dirname(obj).trim_start_matches('/'))
            .join(&obj_props.soname);
        let obj_path = root.join(obj.trim_start_matches('/'));
        if let (Ok(soname_st), Ok(obj_st)) = (
            std::fs::metadata(&soname_link),
            std::fs::metadata(&obj_path),
        ) {
            use std::os::unix::fs::MetadataExt;
            if (obj_st.dev(), obj_st.ino()) != (soname_st.dev(), soname_st.ino()) {
                return Ok(BTreeSet::new());
            }
        }
    }

    let category = &obj_props.category;
    let soname = &obj_props.soname;
    let soname_node = map
        .libs
        .get(category)
        .and_then(|arch_map| arch_map.get(soname));

    let defpath_keys: BTreeSet<ObjKey> = defpath.iter().map(|p| obj_key(root, p)).collect();
    let mut satisfied_consumer_keys: BTreeSet<ObjKey> = BTreeSet::new();

    if let Some(soname_node) = soname_node
        && (exclude_provider.is_some() || !greedy)
    {
        let mut relevant_dir_keys: BTreeSet<ObjKey> = BTreeSet::new();
        for provider_key in &soname_node.providers {
            if !greedy && *provider_key == obj_key_val {
                continue;
            }
            let Some(provider_props) = map.obj_properties.get(provider_key) else {
                continue;
            };
            for p in &provider_props.alt_paths {
                let excluded = exclude_provider.is_some_and(|f| f(p));
                if !excluded {
                    relevant_dir_keys.insert(obj_key(root, &dirname(p)));
                }
            }
        }

        if !relevant_dir_keys.is_empty() {
            for consumer_key in &soname_node.consumers {
                let Some(consumer_props) = map.obj_properties.get(consumer_key) else {
                    continue;
                };
                let mut path_keys = defpath_keys.clone();
                path_keys.extend(consumer_props.runpaths.iter().map(|p| obj_key(root, p)));
                if relevant_dir_keys.intersection(&path_keys).next().is_some() {
                    satisfied_consumer_keys.insert(consumer_key.clone());
                }
            }
        }
    }

    let mut result = BTreeSet::new();
    if let Some(soname_node) = soname_node {
        let objs_dir_key = obj_key(root, &dirname(obj));
        for consumer_key in &soname_node.consumers {
            if satisfied_consumer_keys.contains(consumer_key) {
                continue;
            }
            let Some(consumer_props) = map.obj_properties.get(consumer_key) else {
                continue;
            };
            let mut path_keys = defpath_keys.clone();
            path_keys.extend(consumer_props.runpaths.iter().map(|p| obj_key(root, p)));
            if path_keys.contains(&objs_dir_key) {
                result.extend(consumer_props.alt_paths.iter().cloned());
            }
        }
    }
    Ok(result)
}

/// Arguments of [`find_libs_to_preserve`].
pub struct FindLibsToPreserveArgs<'a> {
    pub root: &'a Path,
    pub map: &'a LinkageMap,
    pub defpath: &'a [String],
    pub old_contents: &'a [String],
    pub old_owner_is_owner: &'a dyn Fn(&str) -> bool,
    pub new_owner_is_owner: &'a dyn Fn(&str) -> bool,
}

/// `_find_libs_to_preserve`'s pure computation: the gating conditions are
/// the *caller's* responsibility. `old_contents` is the previous same-slot
/// instance's `CONTENTS` paths; the two closures say whether the old / new
/// instance owns a path (an unmerge-only caller passes `false` always).
///
/// Build a dependency graph from `find_consumers` results (provider ->
/// consumers, skipping a consumer being removed that isn't also a
/// provider); walk from every "root" consumer to find every transitively
/// reachable provider -- the preserve candidates. Hardlink/soname-symlink
/// classification then decides what to actually preserve (skipping a
/// candidate the *new* package already replaces).
// Portage: vartree.py, _find_libs_to_preserve
pub fn find_libs_to_preserve(args: FindLibsToPreserveArgs<'_>) -> BTreeSet<String> {
    let FindLibsToPreserveArgs {
        root,
        map,
        defpath,
        old_contents,
        old_owner_is_owner,
        new_owner_is_owner,
    } = args;
    let mut graph = LibGraph::default();
    let mut consumer_map: BTreeMap<ObjKey, BTreeSet<String>> = BTreeMap::new();
    let mut provider_nodes: BTreeSet<ObjKey> = BTreeSet::new();

    for f in old_contents {
        let Ok(consumers) = find_consumers(FindConsumersArgs {
            root,
            map,
            defpath,
            obj: f,
            exclude_provider: Some(old_owner_is_owner),
            greedy: true,
        }) else {
            continue;
        };
        if consumers.is_empty() {
            continue;
        }
        let provider_node = graph.path_to_node(root, f);
        graph.add(&provider_node, None);
        provider_nodes.insert(provider_node.clone());
        consumer_map.insert(provider_node, consumers);
    }

    for (provider_node, consumers) in &consumer_map {
        for c in consumers {
            let consumer_node = graph.path_to_node(root, c);
            if old_owner_is_owner(c) && !provider_nodes.contains(&consumer_node) {
                continue;
            }
            graph.add(provider_node, Some(&consumer_node));
        }
    }

    let mut preserve_nodes: BTreeSet<ObjKey> = BTreeSet::new();
    for consumer_node in graph.root_nodes() {
        if provider_nodes.contains(&consumer_node) {
            continue;
        }
        let mut node_stack = graph.child_nodes(&consumer_node);
        while let Some(provider_node) = node_stack.pop() {
            if preserve_nodes.contains(&provider_node) {
                continue;
            }
            preserve_nodes.insert(provider_node.clone());
            node_stack.extend(graph.child_nodes(&provider_node));
        }
    }

    let mut preserve_paths: BTreeSet<String> = BTreeSet::new();
    for preserve_node in &preserve_nodes {
        let Some(alt_paths) = graph.alt_paths.get(preserve_node) else {
            continue;
        };
        let mut hardlinks: BTreeSet<String> = BTreeSet::new();
        let mut soname_symlinks: BTreeSet<String> = BTreeSet::new();
        let soname = map
            .obj_properties
            .get(preserve_node)
            .map(|p| p.soname.clone())
            .unwrap_or_default();
        let mut have_replacement_soname_link = false;
        let mut have_replacement_hardlink = false;

        for f in alt_paths {
            let f_abs = root.join(f.trim_start_matches('/'));
            let Ok(meta) = std::fs::symlink_metadata(&f_abs) else {
                continue;
            };
            if meta.file_type().is_file() {
                hardlinks.insert(f.clone());
                if new_owner_is_owner(f) {
                    have_replacement_hardlink = true;
                    if basename(f) == soname {
                        have_replacement_soname_link = true;
                    }
                }
            } else if basename(f) == soname {
                soname_symlinks.insert(f.clone());
                if new_owner_is_owner(f) {
                    have_replacement_soname_link = true;
                }
            }
        }

        if have_replacement_hardlink && have_replacement_soname_link {
            continue;
        }

        if !hardlinks.is_empty() {
            preserve_paths.extend(hardlinks);
            preserve_paths.extend(soname_symlinks);
        }
    }

    preserve_paths
}

/// Soname-keyed reverse lookup across all multilib categories: every recorded
/// path of every object listing `soname` in `DT_NEEDED`. Used as the fallback
/// consumer scan for a preserved library whose owning package already left
/// the vdb (so the library itself is no longer indexed).
pub fn soname_consumers(map: &LinkageMap, soname: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for arch_map in map.libs.values() {
        let Some(node) = arch_map.get(soname) else {
            continue;
        };
        for consumer_key in &node.consumers {
            if let Some(props) = map.obj_properties.get(consumer_key) {
                out.extend(props.alt_paths.iter().cloned());
            }
        }
    }
    out
}

/// Given `consumers_by_preserved` (preserved path -> linking paths, both
/// sides already filtered to files on disk), return the subset of
/// `preserved_paths` that nothing needs.
///
/// A preserved library is *needed* iff it has a consumer that is not itself
/// preserved, or transitively through one that is. Everything else is
/// unneeded -- including a cycle of preserved libraries with no outside
/// consumer. Paths deduplicate by `ObjKey`, so a hardlink alias counts once.
// Portage: vartree.py, _find_unneeded_preserved_nodes
pub fn find_unneeded_preserved(
    root: &Path,
    consumers_by_preserved: &BTreeMap<String, BTreeSet<String>>,
    preserved_paths: &BTreeSet<String>,
) -> BTreeSet<String> {
    let exists = |p: &str| std::fs::symlink_metadata(root.join(p.trim_start_matches('/'))).is_ok();

    let mut alt_paths: BTreeMap<ObjKey, BTreeSet<String>> = BTreeMap::new();
    let mut preserved_nodes: BTreeSet<ObjKey> = BTreeSet::new();
    for p in preserved_paths {
        if !exists(p) {
            continue;
        }
        let key = obj_key(root, p);
        alt_paths.entry(key.clone()).or_default().insert(p.clone());
        preserved_nodes.insert(key);
    }

    // `children[c]` = preserved libs consumed by `c`; `parents[p]` = the
    // consumers of preserved lib `p`.
    let mut children: BTreeMap<ObjKey, BTreeSet<ObjKey>> = BTreeMap::new();
    let mut parents: BTreeMap<ObjKey, BTreeSet<ObjKey>> = BTreeMap::new();
    for (pres, consumers) in consumers_by_preserved {
        let pk = obj_key(root, pres);
        if !preserved_nodes.contains(&pk) {
            continue;
        }
        for c in consumers {
            if !exists(c) {
                continue;
            }
            let ck = obj_key(root, c);
            alt_paths.entry(ck.clone()).or_default().insert(c.clone());
            children.entry(ck.clone()).or_default().insert(pk.clone());
            parents.entry(pk.clone()).or_default().insert(ck.clone());
        }
    }

    let mut needed: BTreeSet<ObjKey> = BTreeSet::new();
    let mut stack: Vec<ObjKey> = Vec::new();
    for pn in &preserved_nodes {
        let has_outside_consumer = parents
            .get(pn)
            .into_iter()
            .flatten()
            .any(|c| !preserved_nodes.contains(c));
        if has_outside_consumer {
            needed.insert(pn.clone());
            stack.push(pn.clone());
        }
    }
    while let Some(node) = stack.pop() {
        for child in children.get(&node).into_iter().flatten() {
            if preserved_nodes.contains(child) && needed.insert(child.clone()) {
                stack.push(child.clone());
            }
        }
    }

    let mut out = BTreeSet::new();
    for pn in &preserved_nodes {
        if !needed.contains(pn)
            && let Some(paths) = alt_paths.get(pn)
        {
            out.extend(paths.iter().cloned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;

    fn tempdir() -> std::path::PathBuf {
        TempDir::new("portuale-needed-elf-test").keep()
    }

    #[test]
    fn read_all_needed_entries_degrades_gracefully_when_var_db_pkg_is_missing() {
        let root = tempdir();
        assert_eq!(read_all_needed_entries(&root), Vec::new());
    }
}
