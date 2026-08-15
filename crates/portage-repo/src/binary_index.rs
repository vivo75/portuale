//! The binary package index (`Packages`) and binary candidates.

use crate::*;

/// Parses `<pkgdir>/Packages` into one `HashMap` per package entry, skipping
/// the first (global header) block. Trusts the index outright (Portage's
/// default `FEATURES="pkgdir-index-trusted"`); revalidation lives one layer
/// up. A missing file is an empty list.
pub fn read_packages_index(pkgdir: &Path) -> Vec<HashMap<String, String>> {
    let path = pkgdir.join("Packages");
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    parse_packages_index(&text).1
}

/// Splits index text into its global header block and one `HashMap` per
/// package entry. [`read_packages_index`] drops the header.
pub fn parse_packages_index(text: &str) -> (HashMap<String, String>, Vec<HashMap<String, String>>) {
    let mut blocks: Vec<HashMap<String, String>> = Vec::new();
    let mut current: HashMap<String, String> = HashMap::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
            continue;
        }
        if let Some((key, value)) = line.split_once(": ") {
            current.insert(key.to_string(), value.to_string());
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    // The first block is the global header, never a package entry.
    if blocks.is_empty() {
        (HashMap::new(), Vec::new())
    } else {
        let header = blocks.remove(0);
        (header, blocks)
    }
}

/// A parsed binary-package index: `Packages` blocks, or scan-synthesized
/// entries when there is no `Packages`. The read-vs-scan decision is made
/// once, at the CLI boundary.
#[derive(Debug, Clone, Default)]
pub struct BinaryIndex {
    pub(crate) entries: Vec<HashMap<String, String>>,
    /// `(category, package)` -> indices into `entries`, built once: a remote
    /// index has ~18 000 entries, so a linear scan per lookup was quadratic.
    pub(crate) by_cp: HashMap<(String, String), Vec<usize>>,
}

impl PartialEq for BinaryIndex {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries
    }
}

impl Eq for BinaryIndex {}

impl BinaryIndex {
    /// Parses `<pkgdir>/Packages`; a missing file is an empty index.
    pub fn from_pkgdir(pkgdir: &Path) -> Self {
        Self::from_entries(read_packages_index(pkgdir))
    }

    /// Builds from caller-assembled entries (a `$PKGDIR` scan of each
    /// `.tbz2`/`.gpkg.tar`'s embedded metadata as `Packages`-style records).
    pub fn from_entries(entries: Vec<HashMap<String, String>>) -> Self {
        let mut by_cp: HashMap<(String, String), Vec<usize>> = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            if let Some((cat, pkg, _)) = e.get("CPV").and_then(|cpv| split_cpv(cpv)) {
                by_cp.entry((cat, pkg)).or_default().push(i);
            }
        }
        Self { entries, by_cp }
    }

    pub(crate) fn entries(&self) -> &[HashMap<String, String>] {
        &self.entries
    }

    /// The index records for `category/package` -- O(matches), not a scan.
    pub(crate) fn entries_for_cp(
        &self,
        category: &str,
        package: &str,
    ) -> impl Iterator<Item = &HashMap<String, String>> {
        self.by_cp
            .get(&(category.to_string(), package.to_string()))
            .into_iter()
            .flatten()
            .map(move |&i| &self.entries[i])
    }
}

/// `BinaryIndex::from_pkgdir`, memoised by directory: a ~20 MB index re-parsed
/// per atom made big `--getbinpkg` graphs take minutes. A live refresh installs
/// a per-directory override here *before* resolution (`Some` = use it, `None`
/// = the binrepo contributes nothing); the override wins over memo and disk.
pub(crate) fn cached_binary_index(pkgdir: &Path) -> std::sync::Arc<BinaryIndex> {
    if let Some(overridden) = remote_binary_index_override(pkgdir) {
        return overridden.unwrap_or_default();
    }
    static CACHE: OnceLock<RwLock<HashMap<PathBuf, std::sync::Arc<BinaryIndex>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    if let Some(hit) = cache.read().unwrap().get(pkgdir) {
        return hit.clone();
    }
    let idx = std::sync::Arc::new(BinaryIndex::from_pkgdir(pkgdir));
    cache
        .write()
        .unwrap()
        .insert(pkgdir.to_path_buf(), idx.clone());
    idx
}

/// The live-refresh override table for [`cached_binary_index`]. Cleared where a
/// run starts; unit-test temp dirs never collide.
pub(crate) static REMOTE_INDEX_OVERRIDE: OnceLock<
    RwLock<HashMap<PathBuf, Option<std::sync::Arc<BinaryIndex>>>>,
> = OnceLock::new();

pub(crate) fn remote_binary_index_override(
    pkgdir: &Path,
) -> Option<Option<std::sync::Arc<BinaryIndex>>> {
    REMOTE_INDEX_OVERRIDE
        .get()
        .and_then(|table| table.read().unwrap().get(pkgdir).cloned())
}

/// Every package under `<source_root>/var/db/pkg` as a `Packages`-style record
/// (Portage `bintree._populate_additional` reads the vdb the same way). Keys
/// mirror what `binary_candidates_from_index` / `read_binary_metadata` read.
pub(crate) fn quickpkg_direct_index_entries(source_root: &Path) -> Vec<HashMap<String, String>> {
    let mut out = Vec::new();
    for pkg in all_installed_packages(source_root) {
        let (cat, name, ver) = (
            pkg.category.as_str(),
            pkg.package.as_str(),
            pkg.version.as_str(),
        );
        let read = |key: &str| read_vdb_string(source_root, cat, name, ver, key);
        let raw_slot = read("SLOT");
        let slot = if raw_slot.is_empty() {
            "0".to_string()
        } else {
            raw_slot
        };
        let mut entry: HashMap<String, String> = HashMap::new();
        entry.insert("CPV".to_string(), format!("{cat}/{name}-{ver}"));
        entry.insert("SLOT".to_string(), slot);
        for key in [
            "KEYWORDS",
            "USE",
            "IUSE",
            "LICENSE",
            "PROPERTIES",
            "RESTRICT",
            "DEPEND",
            "RDEPEND",
            "BDEPEND",
            "PDEPEND",
            "IDEPEND",
        ] {
            let v = read(key);
            if !v.is_empty() {
                entry.insert(key.to_string(), v);
            }
        }
        let repo = read("repository");
        if !repo.is_empty() {
            entry.insert("REPO".to_string(), repo);
        }
        out.push(entry);
    }
    out
}

/// Picks one `Packages` entry of `cat/pkg-version` (binpkg-multi-instance
/// aware). `Some(id)` selects exactly that `BUILD_ID`;
/// `None` the newest build -- highest `(BUILD_TIME, BUILD_ID)`, Portage
/// `dbapi._cmp_cpv` order.
pub(crate) fn pick_binary_instance<'a>(
    entries: &'a [HashMap<String, String>],
    want_cpv: &str,
    build_id: Option<&str>,
) -> Option<&'a HashMap<String, String>> {
    let matching = entries
        .iter()
        .filter(|e| e.get("CPV").map(String::as_str) == Some(want_cpv));
    match build_id {
        Some(id) => matching
            .into_iter()
            .find(|e| e.get("BUILD_ID").map(String::as_str) == Some(id)),
        None => matching.max_by_key(|e| {
            (
                e.get("BUILD_TIME")
                    .and_then(|t| t.trim().parse::<i64>().ok())
                    .unwrap_or(i64::MIN),
                e.get("BUILD_ID")
                    .and_then(|t| t.trim().parse::<i64>().ok())
                    .unwrap_or(i64::MIN),
            )
        }),
    }
}

/// The raw `Packages` record for `category/package-version` from the first
/// binrepo (in order) listing that exact CPV: the repo (sync URI, signature
/// policy) plus the record (`PATH`, `SIZE`, digests). `None` if no binrepo
/// lists it. Newest build when several exist; use
/// [`find_remote_binpkg_instance`] for a specific `BUILD_ID`.
/// Portage: gpkg per-binrepo signature policy
pub fn find_remote_binpkg<'a>(
    binrepos: &'a [portage_profile::BinRepo],
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> Option<(&'a portage_profile::BinRepo, HashMap<String, String>)> {
    find_remote_binpkg_instance(FindRemoteBinpkgInstanceArgs {
        binrepos,
        root,
        category,
        package,
        version,
        build_id: None,
    })
}

/// Arguments of [`find_remote_binpkg_instance`]; the result borrows
/// only `binrepos` (`'a`).
pub struct FindRemoteBinpkgInstanceArgs<'a, 'b> {
    pub binrepos: &'a [portage_profile::BinRepo],
    pub root: &'b Path,
    pub category: &'b str,
    pub package: &'b str,
    pub version: &'b str,
    pub build_id: Option<&'b str>,
}

/// [`find_remote_binpkg`] for one specific build: `Some(build_id)`
/// scans the binrepos in order for exactly that `BUILD_ID`, falling back to
/// the newest build of the version.
pub fn find_remote_binpkg_instance<'a>(
    args: FindRemoteBinpkgInstanceArgs<'a, '_>,
) -> Option<(&'a portage_profile::BinRepo, HashMap<String, String>)> {
    let FindRemoteBinpkgInstanceArgs {
        binrepos,
        root,
        category,
        package,
        version,
        build_id,
    } = args;
    let want_cpv = format!("{category}/{package}-{version}");
    if build_id.is_some() {
        for binrepo in binrepos {
            let index = cached_binary_index(&binrepo.packages_dir(root));
            if let Some(entry) = pick_binary_instance(index.entries(), &want_cpv, build_id) {
                return Some((binrepo, entry.clone()));
            }
        }
    }
    for binrepo in binrepos {
        let index = cached_binary_index(&binrepo.packages_dir(root));
        if let Some(entry) = pick_binary_instance(index.entries(), &want_cpv, None) {
            return Some((binrepo, entry.clone()));
        }
    }
    None
}
