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

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;

    fn tempdir() -> std::path::PathBuf {
        TempDir::new("portuale-needed-elf-test").keep()
    }
}
