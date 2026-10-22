//! Conservative on-disk plan cache for `emerge --pretend` runs (backlog #441 S3).
//!
//! Only pretend runs with no side effects are eligible, and only the
//! resolve+display step's stdout/stderr bytes and exit code are cached. The
//! `--getbinpkg` refresh still runs live first (its notes print live; the
//! refreshed `Packages` content feeds the key) and the GLEP 42 news notice
//! prints live after a replay. Anything unsure means no caching: a miss is
//! only slowness, a false hit is a bug. Key design, exclusions and the
//! deliberate deviations live in the code note, not here.

use crate::pretend::*;
use portage_profile::Config;
use portage_repo::{CandidateSource, GraphEntry, PretendOutcome, RepoConfig};
use std::io::Write as _;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::UNIX_EPOCH;

/// Version of the cache file format and the key schema. Bump on any change
/// to either; old files then miss silently.
const SCHEMA_VERSION: u32 = 1;
/// File magic, 8 bytes.
const FILE_MAGIC: &[u8; 8] = b"PTPLANC1";
/// How many plans the GC keeps, newest first.
const KEEP_ENTRIES: usize = 32;
/// A single entry bigger than this is emitted but never stored.
const MAX_ENTRY_BYTES: u64 = 4 * 1024 * 1024;

pub(crate) const ENV_CACHE_DIR: &str = "PORTUALE_CACHE_DIR";
const ENV_NO_CACHE: &str = "PORTUALE_NO_CACHE";
const ENV_DEBUG: &str = "PORTUALE_CACHE_DEBUG";
const ENV_VERIFY: &str = "PORTUALE_CACHE_VERIFY";
const ENV_PROFILE: &str = "PORTUALE_PROFILE";

/// Environment keys that never feed the key: shell bookkeeping plus the
/// cache's own controls (a control var in the key would make `VERIFY=1`
/// runs miss everything they should verify).
const ENV_VOLATILE: &[&str] = &[
    "_",
    "OLDPWD",
    "SHLVL",
    ENV_CACHE_DIR,
    ENV_NO_CACHE,
    ENV_DEBUG,
    ENV_VERIFY,
];

/// `PORTUALE_CACHE_DEBUG=1` prints one hit/miss line per eligible run.
pub(crate) fn debug_enabled() -> bool {
    wart_is_set(ENV_DEBUG)
}

/// `PORTUALE_CACHE_VERIFY=1`: a hit also recomputes fresh and aborts with
/// exit 99 on any divergence.
pub(crate) fn verify_enabled() -> bool {
    wart_is_set(ENV_VERIFY)
}

fn wart_is_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty() && v != "0")
}

/// One stderr line, only when [`debug_enabled`].
pub(crate) fn debug_msg(msg: &str) {
    if debug_enabled() {
        eprintln!("portuale plan cache: {msg}");
    }
}

/// Why a run may not use the cache; `None` means eligible. Takes the parsed
/// options plus the pre-`--pretend`-drop `--ask` state (the parser drops
/// `--ask` under `--pretend`, but a run that asked is out of scope).
pub(crate) fn ineligible_reason(
    opts: &EmergeOpts<'_>,
    ask_requested: bool,
) -> Option<&'static str> {
    if !opts.pretend {
        return Some("not a --pretend run");
    }
    if ask_requested {
        return Some("--ask requested");
    }
    if opts.resume {
        return Some("--resume replays saved state");
    }
    if opts.skipfirst {
        return Some("--skipfirst");
    }
    if opts.fetchonly || opts.fetch_all_uri {
        return Some("fetch has side effects");
    }
    if opts.buildpkgonly {
        return Some("--buildpkgonly");
    }
    if opts.buildpkg_opt.is_some() {
        return Some("--buildpkg");
    }
    if opts.no_cache {
        return Some("--no-cache");
    }
    if std::env::var_os(ENV_NO_CACHE).is_some_and(|v| !v.is_empty() && v != "0") {
        return Some("PORTUALE_NO_CACHE");
    }
    if std::env::var_os(ENV_PROFILE).is_some() {
        return Some("PORTUALE_PROFILE pollutes stderr");
    }
    if opts.deselect || opts.depclean || opts.prune || opts.unmerge {
        return Some("removal action");
    }
    if opts.clean_action || opts.rage_clean || opts.config_action {
        return Some("standalone action");
    }
    if opts.atom_args.contains(&"@preserved-rebuild") {
        return Some("@preserved-rebuild reads live ELF state");
    }
    None
}

/// Where the cache lives, if it is on at all: `$PORTUALE_CACHE_DIR` when set
/// (created when missing), else `/var/cache/portuale` only when that dir
/// already exists and is writable. Either way the `plans/` subdir must be
/// writable or caching is off.
pub(crate) fn resolve_cache_dir() -> Option<PathBuf> {
    let dir = match std::env::var_os(ENV_CACHE_DIR).filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => {
            let dir = PathBuf::from("/var/cache/portuale");
            if !dir.is_dir() {
                return None;
            }
            dir
        }
    };
    if std::fs::create_dir_all(dir.join("plans")).is_err()
        || std::fs::create_dir_all(dir.join("tmp")).is_err()
        || !dir_writable(&dir.join("plans"))
    {
        return None;
    }
    Some(dir)
}

fn dir_writable(dir: &Path) -> bool {
    let Ok(c_dir) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `c_dir` is a valid NUL-terminated path for the call.
    unsafe { libc::access(c_dir.as_ptr(), libc::W_OK) == 0 }
}

/// The key inputs: everything from the brief's key list that is known before
/// the resolve runs (the refreshed binhost indexes land in the edb cache
/// first, so their content is hashable here).
pub(crate) struct KeyInputs<'a> {
    pub(crate) argv: &'a [String],
    pub(crate) config: &'a Config,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) config_root: &'a Path,
    /// Test seam for the unvaryable process facts (hostname, executable
    /// bytes): real values when `None`. Lets the key tests vary inputs no
    /// subprocess could.
    pub(crate) proc_facts: Option<ProcFacts>,
}

/// The hostname and executable bytes feeding the key.
pub(crate) struct ProcFacts {
    pub(crate) hostname: Vec<u8>,
    pub(crate) exe_bytes: Vec<u8>,
}

/// The hex key, or a miss reason for the debug line.
pub(crate) fn build_key(inputs: KeyInputs<'_>) -> Result<String, String> {
    use sha2::Digest as _;
    let KeyInputs {
        argv,
        config,
        repos,
        root,
        config_root,
        proc_facts,
    } = inputs;
    let mut h = sha2::Sha256::new();
    hash_str(&mut h, "portuale-plan-cache");
    hash_u32(&mut h, SCHEMA_VERSION);

    hash_u32(&mut h, argv.len() as u32);
    for arg in argv {
        hash_bytes(&mut h, arg.as_bytes());
    }

    let mut env: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for (k, v) in std::env::vars_os() {
        if k.to_str().is_some_and(|s| ENV_VOLATILE.contains(&s)) {
            continue;
        }
        env.push((k.as_bytes().to_vec(), v.as_bytes().to_vec()));
    }
    env.sort();
    hash_u32(&mut h, env.len() as u32);
    for (k, v) in &env {
        hash_bytes(&mut h, k);
        hash_bytes(&mut h, v);
    }

    // TTY facts that change rendering (colour, title); `TERM` and friends
    // ride the environment above. The hostname feeds the title escapes.
    hash_u8(
        &mut h,
        u8::from(std::io::IsTerminal::is_terminal(&std::io::stdout())),
    );
    hash_u8(
        &mut h,
        u8::from(std::io::IsTerminal::is_terminal(&std::io::stderr())),
    );
    let (hostname, exe_bytes) = match proc_facts {
        Some(facts) => (facts.hostname, facts.exe_bytes),
        None => (hostname_bytes()?, read_exe_bytes()?),
    };
    hash_bytes(&mut h, &hostname);

    // The `./path` atom check consults the real working directory.
    let cwd = std::env::current_dir().map_err(|e| format!("no cwd: {e}"))?;
    hash_bytes(&mut h, cwd.as_os_str().as_bytes());

    // The executable itself (covers the embedded make.globals and every
    // line of code): a new binary always misses.
    hash_bytes(&mut h, &exe_bytes);

    hash_bytes(&mut h, root.as_os_str().as_bytes());
    hash_bytes(&mut h, config_root.as_os_str().as_bytes());
    let running_root = portage_repo::running_root_from_env();
    hash_bytes(&mut h, running_root.as_os_str().as_bytes());

    // Config: everything under `/etc/portage` recursively (make.conf,
    // package.*, sets, repos.conf, binrepos.conf, color.map, the user
    // profile node), plus the env.d layer and the world/sets state. Repo
    // profile trees are hashed per repo below, so any profile chain
    // membership is covered without resolving the chain.
    hash_tree(&mut h, &config_root.join("etc/portage"), "etc-portage")?;
    hash_file_or_absent(&mut h, &root.join("etc/profile.env"));
    if running_root != *root {
        hash_tree(
            &mut h,
            &running_root.join("etc/portage"),
            "running-etc-portage",
        )?;
        hash_file_or_absent(&mut h, &running_root.join("etc/profile.env"));
    }

    // VDB: generation + entry count + world/sets atoms (backend-agnostic:
    // strings, not files, so sqlite/redb roots work too).
    let db = portage_vdb::for_root(root);
    let generation = db
        .generation()
        .map_err(|e| format!("no vdb generation: {e}"))?;
    hash_u64(&mut h, generation);
    let mut entry_count: u64 = 0;
    for cat in db.categories().map_err(|e| format!("no vdb cats: {e}"))? {
        entry_count += db
            .category_entries(&cat)
            .map_err(|e| format!("no vdb entries: {e}"))?
            .len() as u64;
    }
    hash_u64(&mut h, entry_count);
    let world = db.world().map_err(|e| format!("no world: {e}"))?;
    hash_u32(&mut h, world.atoms.len() as u32);
    for atom in &world.atoms {
        hash_str(&mut h, atom);
    }
    let world_sets = db.world_sets().map_err(|e| format!("no world_sets: {e}"))?;
    hash_u32(&mut h, world_sets.sets.len() as u32);
    for set in &world_sets.sets {
        hash_str(&mut h, set);
    }

    // Repos, sorted by name: timestamp/git identity, layout, the whole
    // profiles/ tree, and the md5-cache category fingerprints.
    let mut repos: Vec<&RepoConfig> = repos.iter().collect();
    repos.sort_by(|a, b| a.name.cmp(&b.name));
    hash_u32(&mut h, repos.len() as u32);
    for repo in repos {
        hash_str(&mut h, &repo.name);
        hash_file_or_absent(&mut h, &repo.location.join("metadata/timestamp.chk"));
        hash_git_head(&mut h, &repo.location.join(".git"))?;
        hash_file_or_absent(&mut h, &repo.location.join("metadata/layout.conf"));
        hash_tree(&mut h, &repo.location.join("profiles"), "profiles")?;
        hash_md5_cache_dirs(&mut h, &repo.location.join("metadata/md5-cache"))?;
    }

    // Binary sources: the local pool (the `Packages` index when present,
    // plus a listing fingerprint of the pool itself) and every configured
    // binhost's refreshed index actually used by the resolve.
    hash_file_or_absent(&mut h, &Path::new(&config.pkgdir).join("Packages"));
    hash_pkgdir_listing(&mut h, Path::new(&config.pkgdir))?;
    hash_u32(&mut h, config.binrepos.len() as u32);
    for binrepo in &config.binrepos {
        hash_str(&mut h, &binrepo.name);
        hash_file_or_absent(
            &mut h,
            &crate::emerge_getbinpkg::binhost_cached_packages_file(root, &binrepo.sync_uri),
        );
    }

    // DISTDIR presence+size gates the `f`/`F` bracket and the download
    // totals (see the code note: a brief-silent but conservative addition).
    hash_distdir_listing(&mut h, &portage_repo::distdir_from_env_or_config(config))?;

    Ok(hex_of(h.finalize()))
}

fn hash_u8(h: &mut sha2::Sha256, v: u8) {
    use sha2::Digest as _;
    h.update([v]);
}

fn hash_u32(h: &mut sha2::Sha256, v: u32) {
    use sha2::Digest as _;
    h.update(v.to_le_bytes());
}

fn hash_u64(h: &mut sha2::Sha256, v: u64) {
    use sha2::Digest as _;
    h.update(v.to_le_bytes());
}

fn hash_bytes(h: &mut sha2::Sha256, bytes: &[u8]) {
    use sha2::Digest as _;
    h.update((bytes.len() as u64).to_le_bytes());
    h.update(bytes);
}

fn hash_str(h: &mut sha2::Sha256, s: &str) {
    hash_bytes(h, s.as_bytes());
}

fn hex_of(digest: impl AsRef<[u8]>) -> String {
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

fn hostname_bytes() -> Result<Vec<u8>, String> {
    let mut buf = [0 as libc::c_char; 256];
    // SAFETY: `buf` is a valid 256-byte stack array; `gethostname` writes
    // at most its length including the NUL.
    if unsafe { libc::gethostname(buf.as_mut_ptr(), buf.len()) } != 0 {
        return Err("no hostname".to_string());
    }
    // SAFETY: NUL-terminated by `gethostname` on success.
    let host = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_bytes();
    Ok(host.to_vec())
}

fn read_exe_bytes() -> Result<Vec<u8>, String> {
    let exe = std::env::current_exe().map_err(|e| format!("no exe path: {e}"))?;
    std::fs::read(&exe).map_err(|e| format!("no exe read: {e}"))
}

fn hash_file_or_absent(h: &mut sha2::Sha256, path: &Path) {
    match std::fs::read(path) {
        Ok(bytes) => {
            hash_u8(h, 1);
            hash_bytes(h, &bytes);
        }
        Err(_) => hash_u8(h, 0),
    }
}

/// Recursive content hash of a config tree: regular files by content, a
/// symlink by its target plus the target file's content when it resolves
/// to one, anything else by kind. Sorted by name bytes, so the hash is
/// order-stable. A missing top dir hashes as absent; any other I/O error
/// refuses the key (no caching on unreadable input).
fn hash_tree(h: &mut sha2::Sha256, dir: &Path, tag: &str) -> Result<(), String> {
    hash_str(h, tag);
    let Ok(entries) = std::fs::read_dir(dir) else {
        hash_u8(h, 0);
        return Ok(());
    };
    hash_u8(h, 1);
    let mut names: Vec<std::ffi::OsString> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("readdir {}: {e}", dir.display()))?;
        names.push(entry.file_name());
    }
    names.sort();
    hash_u32(h, names.len() as u32);
    for name in names {
        let path = dir.join(&name);
        hash_bytes(h, name.as_encoded_bytes());
        let ft = std::fs::symlink_metadata(&path)
            .map_err(|e| format!("stat {}: {e}", path.display()))?
            .file_type();
        if ft.is_symlink() {
            hash_u8(h, 1);
            let target = std::fs::read_link(&path)
                .map_err(|e| format!("readlink {}: {e}", path.display()))?;
            hash_bytes(h, target.as_os_str().as_bytes());
            // A symlink to a file stands for that file's content; anything
            // else (dir, dangling) is the target string alone.
            if let Ok(bytes) = std::fs::read(&path) {
                hash_u8(h, 1);
                hash_bytes(h, &bytes);
            } else {
                hash_u8(h, 0);
            }
        } else if ft.is_dir() {
            hash_u8(h, 2);
            hash_tree_contents(h, &path)?;
        } else if ft.is_file() {
            hash_u8(h, 3);
            let bytes =
                std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
            hash_bytes(h, &bytes);
        } else {
            hash_u8(h, 4);
        }
    }
    Ok(())
}

fn hash_tree_contents(h: &mut sha2::Sha256, dir: &Path) -> Result<(), String> {
    let mut entries: Vec<(std::ffi::OsString, std::fs::FileType)> = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| format!("readdir {}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("readdir {}: {e}", dir.display()))?;
        let ft = entry
            .file_type()
            .map_err(|e| format!("stat {}: {e}", entry.path().display()))?;
        entries.push((entry.file_name(), ft));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    hash_u32(h, entries.len() as u32);
    for (name, ft) in entries {
        let path = dir.join(&name);
        hash_bytes(h, name.as_encoded_bytes());
        if ft.is_symlink() {
            hash_u8(h, 1);
            let target = std::fs::read_link(&path)
                .map_err(|e| format!("readlink {}: {e}", path.display()))?;
            hash_bytes(h, target.as_os_str().as_bytes());
            if let Ok(bytes) = std::fs::read(&path) {
                hash_u8(h, 1);
                hash_bytes(h, &bytes);
            } else {
                hash_u8(h, 0);
            }
        } else if ft.is_dir() {
            hash_u8(h, 2);
            hash_tree_contents(h, &path)?;
        } else if ft.is_file() {
            hash_u8(h, 3);
            let bytes =
                std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
            hash_bytes(h, &bytes);
        } else {
            hash_u8(h, 4);
        }
    }
    Ok(())
}

/// `.git/HEAD` plus the ref it names (and `packed-refs` when present), so a
/// git-managed repo misses after a commit or checkout. Absent without git.
fn hash_git_head(h: &mut sha2::Sha256, git_dir: &Path) -> Result<(), String> {
    hash_str(h, "git-head");
    let head_path = git_dir.join("HEAD");
    let Ok(head) = std::fs::read(&head_path) else {
        hash_u8(h, 0);
        return Ok(());
    };
    hash_u8(h, 1);
    hash_bytes(h, &head);
    // The ref `HEAD` names, read as text; `packed-refs` covers packed refs.
    if let Ok(text) = std::fs::read_to_string(&head_path)
        && let Some(name) = text.strip_prefix("ref: ").map(str::trim)
    {
        hash_file_or_absent(h, &git_dir.join(name));
    }
    hash_file_or_absent(h, &git_dir.join("packed-refs"));
    Ok(())
}

/// One `(name, dir mtime, entry count)` triple per md5-cache category dir:
/// the brief's repo-tree fingerprint. A missing tree hashes as absent.
fn hash_md5_cache_dirs(h: &mut sha2::Sha256, md5_cache: &Path) -> Result<(), String> {
    hash_str(h, "md5-cache-dirs");
    let Ok(entries) = std::fs::read_dir(md5_cache) else {
        hash_u8(h, 0);
        return Ok(());
    };
    hash_u8(h, 1);
    let mut cats: Vec<std::ffi::OsString> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("readdir {}: {e}", md5_cache.display()))?;
        if entry
            .file_type()
            .map_err(|e| format!("stat {}: {e}", entry.path().display()))?
            .is_dir()
        {
            cats.push(entry.file_name());
        }
    }
    cats.sort();
    hash_u32(h, cats.len() as u32);
    for cat in cats {
        let path = md5_cache.join(&cat);
        hash_bytes(h, cat.as_encoded_bytes());
        let meta = std::fs::metadata(&path).map_err(|e| format!("stat {}: {e}", path.display()))?;
        let mtime = meta
            .modified()
            .map_err(|e| format!("mtime {}: {e}", path.display()))?;
        let age = mtime
            .duration_since(UNIX_EPOCH)
            .map_err(|e| format!("mtime {}: {e}", path.display()))?;
        hash_u64(h, age.as_secs());
        hash_u32(h, age.subsec_nanos());
        let count = std::fs::read_dir(&path)
            .map_err(|e| format!("readdir {}: {e}", path.display()))?
            .count() as u64;
        hash_u64(h, count);
    }
    Ok(())
}

/// The local pool's listing fingerprint: `(relpath, size, mtime)` per file,
/// recursive. Content changes that keep size+mtime are the documented
/// residual (same class as the brief's md5-cache acceptance).
fn hash_pkgdir_listing(h: &mut sha2::Sha256, pkgdir: &Path) -> Result<(), String> {
    hash_str(h, "pkgdir-listing");
    let mut files: Vec<(String, u64, u64, u32)> = Vec::new();
    collect_pkgdir_files(pkgdir, pkgdir, &mut files)?;
    files.sort();
    hash_u32(h, files.len() as u32);
    for (rel, size, secs, nanos) in files {
        hash_str(h, &rel);
        hash_u64(h, size);
        hash_u64(h, secs);
        hash_u32(h, nanos);
    }
    Ok(())
}

fn collect_pkgdir_files(
    pkgdir: &Path,
    dir: &Path,
    out: &mut Vec<(String, u64, u64, u32)>,
) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry.map_err(|e| format!("readdir {}: {e}", dir.display()))?;
        let path = entry.path();
        let ft = entry
            .file_type()
            .map_err(|e| format!("stat {}: {e}", path.display()))?;
        if ft.is_dir() {
            collect_pkgdir_files(pkgdir, &path, out)?;
        } else if ft.is_file() {
            let meta =
                std::fs::metadata(&path).map_err(|e| format!("stat {}: {e}", path.display()))?;
            let mtime = meta
                .modified()
                .map_err(|e| format!("mtime {}: {e}", path.display()))?;
            let age = mtime
                .duration_since(UNIX_EPOCH)
                .map_err(|e| format!("mtime {}: {e}", path.display()))?;
            let rel = path
                .strip_prefix(pkgdir)
                .map_err(|e| format!("rel {}: {e}", path.display()))?
                .to_string_lossy()
                .into_owned();
            out.push((rel, meta.len(), age.as_secs(), age.subsec_nanos()));
        }
    }
    Ok(())
}

/// Flat `(name, size, mtime)` listing of `DISTDIR`: presence+size gate the
/// `f`/`F` bracket and the download totals.
fn hash_distdir_listing(h: &mut sha2::Sha256, distdir: &Path) -> Result<(), String> {
    hash_str(h, "distdir-listing");
    let Ok(entries) = std::fs::read_dir(distdir) else {
        hash_u8(h, 0);
        return Ok(());
    };
    hash_u8(h, 1);
    let mut files: Vec<(Vec<u8>, u64, u64, u32)> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("readdir {}: {e}", distdir.display()))?;
        let meta = std::fs::symlink_metadata(entry.path())
            .map_err(|e| format!("stat {}: {e}", entry.path().display()))?;
        if !meta.is_file() {
            continue;
        }
        let mtime = meta
            .modified()
            .map_err(|e| format!("mtime {}: {e}", entry.path().display()))?;
        let age = mtime
            .duration_since(UNIX_EPOCH)
            .map_err(|e| format!("mtime {}: {e}", entry.path().display()))?;
        files.push((
            entry.file_name().as_encoded_bytes().to_vec(),
            meta.len(),
            age.as_secs(),
            age.subsec_nanos(),
        ));
    }
    files.sort();
    hash_u32(h, files.len() as u32);
    for (name, size, secs, nanos) in files {
        hash_bytes(h, &name);
        hash_u64(h, size);
        hash_u64(h, secs);
        hash_u32(h, nanos);
    }
    Ok(())
}

/// A repo file the plan reads but the pre-resolve key cannot enumerate: the
/// `Manifest` (download sizes) and the resolved `.ebuild` (metadata
/// fallback) of every merging ebuild entry. Absence is a state too: a
/// `Manifest` appearing later changes the download totals, so
/// absent-must-stay-absent is verified like any other state. Stored with
/// the plan, verified before a replay; an unreadable (non-missing) file
/// refuses the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileFp {
    pub(crate) path: PathBuf,
    pub(crate) state: FileFpState,
}

/// Present with size+mtime, or missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileFpState {
    Absent,
    Present {
        size: u64,
        mtime_secs: i64,
        mtime_nanos: u32,
    },
}

/// Collect the post-hoc fingerprints, or `None` when the store must be
/// skipped (an unresolvable repo, a missing file, a non-merging plan).
/// `None` also covers resolve errors, which carry no entries at all.
pub(crate) fn collect_posthoc(entries: &[GraphEntry], repos: &[RepoConfig]) -> Option<Vec<FileFp>> {
    let mut out: Vec<FileFp> = Vec::new();
    for entry in entries {
        if entry.source != CandidateSource::Ebuild {
            continue;
        }
        let version = match &entry.outcome {
            PretendOutcome::New { version } => version,
            PretendOutcome::Reinstall { version, .. } => version,
            PretendOutcome::Upgrade { to, .. } => to,
            PretendOutcome::Downgrade { to, .. } => to,
            _ => continue,
        };
        let repo_name = entry.repo_name.as_deref()?;
        let location = repos.iter().find(|r| r.name == repo_name)?.location.clone();
        let dir = location.join(&entry.category).join(&entry.package);
        out.push(stat_fp(&dir.join("Manifest"))?);
        out.push(stat_fp(
            &dir.join(format!("{}-{version}.ebuild", entry.package)),
        )?);
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out.dedup();
    Some(out)
}

fn stat_fp(path: &Path) -> Option<FileFp> {
    match std::fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(FileFp {
            path: path.to_path_buf(),
            state: FileFpState::Absent,
        }),
        Err(_) => None,
        Ok(meta) => {
            if !meta.is_file() {
                return None;
            }
            let age = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
            // i64 is enough for the foreseeable future (u64 would wrap the format).
            let secs = i64::try_from(age.as_secs()).ok()?;
            Some(FileFp {
                path: path.to_path_buf(),
                state: FileFpState::Present {
                    size: meta.len(),
                    mtime_secs: secs,
                    mtime_nanos: age.subsec_nanos(),
                },
            })
        }
    }
}

/// Re-stat every stored fingerprint: a mismatch (or an unreadable file)
/// means the repo moved under the key, so the candidate is a miss.
pub(crate) fn verify_posthoc(fps: &[FileFp]) -> bool {
    fps.iter().all(|fp| stat_fp(&fp.path).as_ref() == Some(fp))
}

/// The cached resolve+display step.
pub(crate) struct CachedPlan {
    pub(crate) code: u8,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) fps: Vec<FileFp>,
}

fn plans_dir(dir: &Path) -> PathBuf {
    dir.join("plans")
}

fn key_path(dir: &Path, key: &str) -> PathBuf {
    plans_dir(dir).join(format!("{key}.plan"))
}

/// The [`std::process::ExitCode`] has no accessor, so the code rides the
/// `Debug` digits (`ExitCode(unix_exit_status(N))` today): the trailing
/// digit run. Pinned by a unit test; a parse failure can only under-report
/// as 1, never replay a wrong success.
pub(crate) fn exit_code_to_u8(code: ExitCode) -> u8 {
    let s = format!("{code:?}");
    let digits: String = s
        .chars()
        .rev()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    digits.parse().unwrap_or(1)
}

/// Load and validate a candidate: ownership (self or root), no group/world
/// writability, exact format and length. Anything else is a silent miss
/// with a debug reason.
pub(crate) fn load(dir: &Path, key: &str) -> Option<CachedPlan> {
    let path = key_path(dir, key);
    let file = std::fs::File::open(&path).ok()?;
    // `fstat`, not `stat`: the checks bind the open file, not the path.
    let meta = file.metadata().ok()?;
    // SAFETY: `geteuid` has no preconditions.
    let euid = unsafe { libc::geteuid() };
    if meta.uid() != euid && meta.uid() != 0 {
        debug_msg(&format!("miss (key={key}: owned by uid {})", meta.uid()));
        return None;
    }
    if meta.mode() & 0o022 != 0 {
        debug_msg(&format!("miss (key={key}: group/world-writable)"));
        return None;
    }
    let bytes = read_all(&file).ok()?;
    match parse_entry(&bytes) {
        Some(plan) => Some(plan),
        None => {
            debug_msg(&format!("miss (key={key}: corrupt entry)"));
            None
        }
    }
}

#[cfg(unix)]
fn read_all(file: &std::fs::File) -> std::io::Result<Vec<u8>> {
    use std::io::Read as _;
    let mut buf = Vec::new();
    let mut handle = file;
    handle.read_to_end(&mut buf)?;
    Ok(buf)
}

/// A verified load: entry parses and every post-hoc fingerprint still
/// matches the live tree.
pub(crate) fn load_and_verify(dir: &Path, key: &str) -> Option<CachedPlan> {
    let plan = load(dir, key)?;
    if !verify_posthoc(&plan.fps) {
        debug_msg(&format!("miss (key={key}: repo files changed)"));
        return None;
    }
    debug_msg(&format!(
        "hit (key={key}: {} stdout + {} stderr bytes, code {})",
        plan.stdout.len(),
        plan.stderr.len(),
        plan.code
    ));
    Some(plan)
}

fn parse_entry(bytes: &[u8]) -> Option<CachedPlan> {
    let mut cur = bytes;
    const HEADER: usize = 8 + 4 + 1 + 8;
    if cur.len() < HEADER || &cur[..8] != FILE_MAGIC {
        return None;
    }
    cur = &cur[8..];
    let version = u32::from_le_bytes(cur[..4].try_into().ok()?);
    if version != SCHEMA_VERSION {
        return None;
    }
    let code = cur[4];
    let stdout_len = u64::from_le_bytes(cur[5..13].try_into().ok()?) as usize;
    cur = &cur[13..];
    let stdout = cur.get(..stdout_len)?.to_vec();
    cur = &cur[stdout_len..];
    if cur.len() < 8 {
        return None;
    }
    let stderr_len = u64::from_le_bytes(cur[..8].try_into().ok()?) as usize;
    cur = &cur[8..];
    let stderr = cur.get(..stderr_len)?.to_vec();
    cur = &cur[stderr_len..];
    if cur.len() < 4 {
        return None;
    }
    let fp_count = u32::from_le_bytes(cur[..4].try_into().ok()?) as usize;
    cur = &cur[4..];
    let mut fps = Vec::with_capacity(fp_count.min(1024));
    for _ in 0..fp_count {
        if cur.len() < 4 {
            return None;
        }
        let path_len = u32::from_le_bytes(cur[..4].try_into().ok()?) as usize;
        cur = &cur[4..];
        let path_bytes = cur.get(..path_len)?;
        let path = PathBuf::from(std::ffi::OsStr::from_bytes(path_bytes));
        cur = &cur[path_len..];
        let present = match cur.first() {
            Some(0) => false,
            Some(1) => true,
            _ => return None,
        };
        cur = &cur[1..];
        let state = if present {
            if cur.len() < 8 + 8 + 4 {
                return None;
            }
            let size = u64::from_le_bytes(cur[..8].try_into().ok()?);
            let mtime_secs = i64::from_le_bytes(cur[8..16].try_into().ok()?);
            let mtime_nanos = u32::from_le_bytes(cur[16..20].try_into().ok()?);
            cur = &cur[20..];
            FileFpState::Present {
                size,
                mtime_secs,
                mtime_nanos,
            }
        } else {
            FileFpState::Absent
        };
        fps.push(FileFp { path, state });
    }
    // No trailing garbage: a short write or a splice is a miss, not a replay.
    if !cur.is_empty() {
        return None;
    }
    Some(CachedPlan {
        code,
        stdout,
        stderr,
        fps,
    })
}

/// What [`store`] writes: the fresh step's bytes plus its fingerprints.
pub(crate) struct StoredEntry<'a> {
    pub(crate) code: u8,
    pub(crate) stdout: &'a [u8],
    pub(crate) stderr: &'a [u8],
    pub(crate) fps: &'a [FileFp],
}

/// Store a fresh plan: serialize, write temp+rename, GC. Best-effort: any
/// failure just skips the store, the run already printed its output.
pub(crate) fn store(dir: &Path, key: &str, entry: StoredEntry<'_>) {
    let StoredEntry {
        code,
        stdout,
        stderr,
        fps,
    } = entry;
    if stdout.len() as u64 + stderr.len() as u64 > MAX_ENTRY_BYTES {
        debug_msg(&format!("skip store (key={key}: over size cap)"));
        return;
    }
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(FILE_MAGIC);
    buf.extend_from_slice(&SCHEMA_VERSION.to_le_bytes());
    buf.push(code);
    buf.extend_from_slice(&(stdout.len() as u64).to_le_bytes());
    buf.extend_from_slice(stdout);
    buf.extend_from_slice(&(stderr.len() as u64).to_le_bytes());
    buf.extend_from_slice(stderr);
    buf.extend_from_slice(&(fps.len() as u32).to_le_bytes());
    for fp in fps {
        let path_bytes = fp.path.as_os_str().as_bytes();
        buf.extend_from_slice(&(path_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(path_bytes);
        match &fp.state {
            FileFpState::Absent => buf.push(0),
            FileFpState::Present {
                size,
                mtime_secs,
                mtime_nanos,
            } => {
                buf.push(1);
                buf.extend_from_slice(&size.to_le_bytes());
                buf.extend_from_slice(&mtime_secs.to_le_bytes());
                buf.extend_from_slice(&mtime_nanos.to_le_bytes());
            }
        }
    }
    let tmp = tmp_path(dir);
    let wrote = (|| {
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(&buf)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, key_path(dir, key))?;
        Ok::<(), std::io::Error>(())
    })();
    if let Err(e) = wrote {
        let _ = std::fs::remove_file(&tmp);
        debug_msg(&format!("skip store (key={key}: {e})"));
        return;
    }
    debug_msg(&format!(
        "stored (key={key}: {} stdout + {} stderr bytes)",
        stdout.len(),
        stderr.len()
    ));
    gc(dir);
}

static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn tmp_path(dir: &Path) -> PathBuf {
    let n = TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    dir.join("tmp")
        .join(format!("plan-{}-{n}.tmp", std::process::id()))
}

/// Keep the newest [`KEEP_ENTRIES`] plans by mtime; best-effort.
fn gc(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(plans_dir(dir)) else {
        return;
    };
    let mut plans: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "plan") {
            let mtime = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(UNIX_EPOCH);
            plans.push((mtime, path));
        }
    }
    if plans.len() <= KEEP_ENTRIES {
        return;
    }
    plans.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (_, path) in plans.into_iter().skip(KEEP_ENTRIES) {
        let _ = std::fs::remove_file(path);
    }
}

/// Replay cached bytes onto the real streams.
pub(crate) fn emit(stdout: &[u8], stderr: &[u8]) {
    let _ = std::io::stdout()
        .lock()
        .write_all(stdout)
        .and_then(|_| std::io::stdout().lock().flush());
    let _ = std::io::stderr()
        .lock()
        .write_all(stderr)
        .and_then(|_| std::io::stderr().lock().flush());
}

pub(crate) fn replay(cached: &CachedPlan) -> ExitCode {
    emit(&cached.stdout, &cached.stderr);
    ExitCode::from(cached.code)
}

/// One-line summary of a verify mismatch: which stream differs, with
/// lengths and content hashes (never the full bytes).
pub(crate) fn diff_summary(cached: &CachedPlan, fresh: &FreshBody) -> String {
    use sha2::Digest as _;
    let mut parts: Vec<String> = Vec::new();
    if cached.code != fresh.code {
        parts.push(format!("exit {} != {}", cached.code, fresh.code));
    }
    if cached.stdout != fresh.stdout {
        parts.push(format!(
            "stdout {} != {} bytes (sha {} != {})",
            cached.stdout.len(),
            fresh.stdout.len(),
            hex_of(sha2::Sha256::digest(&cached.stdout)),
            hex_of(sha2::Sha256::digest(&fresh.stdout)),
        ));
    }
    if cached.stderr != fresh.stderr {
        parts.push(format!(
            "stderr {} != {} bytes (sha {} != {})",
            cached.stderr.len(),
            fresh.stderr.len(),
            hex_of(sha2::Sha256::digest(&cached.stderr)),
            hex_of(sha2::Sha256::digest(&fresh.stderr)),
        ));
    }
    if parts.is_empty() {
        parts.push("identical".to_string());
    }
    parts.join("; ")
}

/// Verify-mode abort: the hit disagreed with a fresh resolve under the same
/// key. Loud, and exit 99 so no harness mistakes it for a resolve result.
pub(crate) fn verify_failed(key: &str, detail: &str) -> ExitCode {
    eprintln!("portuale plan cache VERIFY FAILED: key={key}: {detail}");
    eprintln!(
        "The cached plan differs from a fresh resolve with the same key: refusing to replay."
    );
    ExitCode::from(99)
}

/// The fd-level capture of the resolve+display step. `stdout` is
/// line-buffered per handle and `stderr` unbuffered, so flushing both
/// before the swap and after the body leaves no caller bytes behind; every
/// `println!`/`eprintln!` inside lands in the temp files instead.
pub(crate) struct Capture {
    out_tmp: PathBuf,
    err_tmp: PathBuf,
    saved_out: std::os::fd::RawFd,
    saved_err: std::os::fd::RawFd,
    _out_file: std::fs::File,
    _err_file: std::fs::File,
    active: bool,
}

/// Run `body` with stdout/stderr captured, suppression on, streams
/// restored: the fresh bytes plus the exit for store+emit. A capture setup
/// failure is `Err` (the caller runs the body uncached instead); a body
/// panic restores the fds via `Drop` and propagates.
pub(crate) struct FreshBody {
    pub(crate) code: u8,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) posthoc: Option<Vec<FileFp>>,
}

pub(crate) fn captured_body(
    tmp_dir: &Path,
    body: impl FnOnce() -> (u8, Option<Vec<FileFp>>),
) -> std::io::Result<FreshBody> {
    struct SuppressGuard;
    impl SuppressGuard {
        fn set() -> Self {
            crate::pretend::news::PLAN_CACHE_CAPTURING
                .store(true, std::sync::atomic::Ordering::Relaxed);
            SuppressGuard
        }
    }
    impl Drop for SuppressGuard {
        fn drop(&mut self) {
            crate::pretend::news::PLAN_CACHE_CAPTURING
                .store(false, std::sync::atomic::Ordering::Relaxed);
        }
    }
    let _suppress = SuppressGuard::set();
    let cap = Capture::begin(tmp_dir)?;
    let (code, posthoc) = body();
    let (stdout, stderr) = cap.finish()?;
    Ok(FreshBody {
        code,
        stdout,
        stderr,
        posthoc,
    })
}

impl Capture {
    fn begin(tmp_dir: &Path) -> std::io::Result<Self> {
        use std::os::fd::AsRawFd as _;
        use std::os::unix::fs::OpenOptionsExt as _;
        std::io::stdout().flush()?;
        std::io::stderr().flush()?;
        let mut out_file = None;
        let mut err_file = None;
        let mut out_tmp = PathBuf::new();
        let mut err_tmp = PathBuf::new();
        for _ in 0..16 {
            let n = TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let pid = std::process::id();
            out_tmp = tmp_dir.join(format!("cap-{pid}-{n}.out"));
            err_tmp = tmp_dir.join(format!("cap-{pid}-{n}.err"));
            let out = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&out_tmp);
            let err = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&err_tmp);
            match (out, err) {
                (Ok(o), Ok(e)) => {
                    out_file = Some(o);
                    err_file = Some(e);
                    break;
                }
                _ => {
                    let _ = std::fs::remove_file(&out_tmp);
                    let _ = std::fs::remove_file(&err_tmp);
                }
            }
        }
        let (Some(out_file), Some(err_file)) = (out_file, err_file) else {
            return Err(std::io::Error::other("no capture tempfile"));
        };
        // SAFETY: `dup` returns an owned fd (or -1); the error arm returns
        // before any swap, so the stdio table is untouched on failure.
        let saved_out = unsafe { libc::dup(libc::STDOUT_FILENO) };
        let saved_err = unsafe { libc::dup(libc::STDERR_FILENO) };
        if saved_out < 0 || saved_err < 0 {
            unsafe {
                if saved_out >= 0 {
                    libc::close(saved_out);
                }
                if saved_err >= 0 {
                    libc::close(saved_err);
                }
            }
            let _ = std::fs::remove_file(&out_tmp);
            let _ = std::fs::remove_file(&err_tmp);
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: both fds are valid open files; `dup2` swaps atomically.
        unsafe {
            if libc::dup2(out_file.as_raw_fd(), libc::STDOUT_FILENO) < 0
                || libc::dup2(err_file.as_raw_fd(), libc::STDERR_FILENO) < 0
            {
                let e = std::io::Error::last_os_error();
                libc::dup2(saved_out, libc::STDOUT_FILENO);
                libc::dup2(saved_err, libc::STDERR_FILENO);
                libc::close(saved_out);
                libc::close(saved_err);
                let _ = std::fs::remove_file(&out_tmp);
                let _ = std::fs::remove_file(&err_tmp);
                return Err(e);
            }
        }
        Ok(Capture {
            out_tmp,
            err_tmp,
            saved_out,
            saved_err,
            _out_file: out_file,
            _err_file: err_file,
            active: true,
        })
    }

    fn restore(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        // SAFETY: the saved fds are the pre-swap stdio; restoring them can
        // only fail catastrophically, in which case there is nothing to do.
        unsafe {
            libc::dup2(self.saved_out, libc::STDOUT_FILENO);
            libc::dup2(self.saved_err, libc::STDERR_FILENO);
            libc::close(self.saved_out);
            libc::close(self.saved_err);
        }
    }

    fn finish(mut self) -> std::io::Result<(Vec<u8>, Vec<u8>)> {
        std::io::stdout().flush()?;
        std::io::stderr().flush()?;
        self.restore();
        let stdout = std::fs::read(&self.out_tmp)?;
        let stderr = std::fs::read(&self.err_tmp)?;
        let _ = std::fs::remove_file(&self.out_tmp);
        let _ = std::fs::remove_file(&self.err_tmp);
        Ok((stdout, stderr))
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        // A body panic restores the stdio table here, then propagates.
        self.restore();
        let _ = std::fs::remove_file(&self.out_tmp);
        let _ = std::fs::remove_file(&self.err_tmp);
    }
}
