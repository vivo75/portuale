// Real merge/filesystem mutation (task #55, `PORTING/PROMPT-next.md`'s own
// "Real merge/install/filesystem mutation" section): after running the
// real `install` phase chain (task #54's own `ebuild_phases` module),
// really run `pkg_preinst`, copy `${D}`'s own regular files, directories,
// and symlinks into `${ROOT}`, write a real vdb entry (`CONTENTS`, in the
// exact `obj`/`dir`/`sym` line format real `dblink._format_contents_line`
// uses, plus `CATEGORY`/`SLOT`/`repository`), then really run
// `pkg_postinst` -- mirroring real `dblink.merge()`/`treewalk()`/
// `mergeme()` (`lib/portage/dbapi/vartree.py`, ~6500 lines total) at a
// deliberately narrow v1 scope, the same "narrow v1, document the cut"
// pattern `ebuild_phases`'s own module doc comment already established.
// `pkg_preinst`/`pkg_postinst` run via `ebuild_phases::run_single_phase`,
// not `run_commands` -- real `treewalk()` invokes them directly
// (`EbuildPhase(phase="preinst"/"postinst")`), not through `doebuild()`'s
// own `actionmap_deps` chain the way `pretend`..`install` are.
//
// CONFIG_PROTECT is real too, for `obj` (regular file) entries: real
// `ConfigProtect.isprotected()` path matching (`is_protected`), the real
// MD5-comparison rename-instead-of-overwrite decision (real `dblink.
// _protect()`, into the next `._cfgNNNN_<name>` sibling -- real
// `new_protect_filename()`), and real `vardbapi._conf_mem_file`
// persistence (`read_cfgfiledict`/`write_cfgfiledict`) so a repeat merge
// of an already-offered update doesn't spawn a fresh `._cfgNNNN_` file
// every time. `CONFIG_PROTECT`/`CONFIG_PROTECT_MASK` are read via env
// vars at the `ebuild.rs` CLI boundary (bundled into `MergeOptions`,
// deliberately a struct and not more positional parameters -- this
// pilot already relearned the "positional-parameter pain" lesson once,
// in `--newrepo`'s own bulk-fix saga), defaulting to real `make.
// globals`'s own `CONFIG_PROTECT="/etc"`/`CONFIG_PROTECT_MASK="/etc/
// env.d"`.
//
// `FEATURES=collision-protect` is real too: real `dblink.
// _collision_protect` (`lib/portage/dbapi/vartree.py:3836`), narrowed --
// before `pkg_preinst` ever runs (matching real `merge()`'s own
// ordering exactly: the real abort happens before the real
// `EbuildPhase(phase="preinst")` block, not after), walks the real
// install image (`${D}`) the same way `merge_tree` does but read-only,
// checking each real file/symlink entry (never directories -- real
// `_collision_protect` only ever checks `file_list`/`symlink_list`)
// against the real, on-disk destination: real PMS 13.4's own
// symlink-over-directory ban is checked unconditionally (regardless of
// `FEATURES`); an ordinary collision (destination exists, isn't owned
// by an older installed version of this exact package in the same slot
// -- the one this merge is about to replace -- and isn't
// `CONFIG_PROTECT`'d) only aborts when `FEATURES=collision-protect`
// itself is set (`find_owners` -- real `vardbapi._owners.get_owners()`,
// narrowed to a fresh scan of every installed package's own `CONTENTS`
// rather than a persistent reverse index -- names which other real
// installed package(s) actually claim each colliding path, for the
// abort message).
//
// `preserve-libs` collision exclusion is real too, for the "consult and
// exclude" half only: real `dblink._collision_protect`'s own
// `plib_inodes`/`plib_collisions` handling (`lib/portage/dbapi/
// vartree.py:3860-3985`) -- a colliding path whose real, on-disk
// `(st_dev, st_ino)` matches a path the real `preserved_libs_registry`
// JSON already lists for some other package is excluded from ordinary
// collision reporting *unconditionally* (real `_plib_registry` is
// constructed unconditionally in `vardbapi.__init__`, not gated by
// `FEATURES=preserve-libs` at all -- that flag only gates the
// *registration* side, see below), since the just-merged package
// legitimately takes over that file. After a successful merge, real
// `merge()`'s own post-copy step (`:5095-5159`) is mirrored too:
// `unregister_preserved_libs` drops the taken-over paths from the
// registry (removing the owning `cp:slot` entry entirely once its own
// path list empties) and from the previous owner's own real vdb
// `CONTENTS` (real `removeFromContents`), skipped when the previous
// owner *is* the package just merged (real `if cpv != self.mycpv`). The
// registry itself is a narrow, fixed-shape JSON document (`{"cp:slot":
// [cpv, counter, [paths...]]}`, real `PreservedLibsRegistry.store()`'s
// own `json.dumps(indent="\t", sort_keys=True)`) -- read/written with a
// small hand-rolled parser/writer (`read_plib_registry`/
// `write_plib_registry`) rather than a new `serde_json` dependency,
// matching this pilot's own "small, format-specific parser over a
// generic dependency" precedent (`--json` output, `SRC_URI`'s grammar,
// `grabdict`-format `thirdpartymirrors`).
//
// `FEATURES=protect-owned` is real too: real `dblink.merge()`'s own
// *separate* abort condition alongside `collision-protect`
// (`lib/portage/dbapi/vartree.py:4770-4838`; Python operator precedence
// makes the real check `collision_protect or (protect_owned and
// owners)`) -- `protect_owned` alone only aborts a merge when
// `find_owners` actually identified an owning package for at least one
// collision, unlike `collision_protect` which aborts on any collision
// regardless of whether an owner was found. Real portage's own "None of
// the installed packages claim the file(s)" case (a stray, unowned file
// already on disk) does *not* abort under `protect_owned` alone.
// Reuses `find_owners` (already built for `collision-protect`'s own
// abort message) rather than adding new machinery.
//
// Real blocker exclusion is real too: real `dblink.merge()`'s own
// `mypkglist = others_in_slot + blockers`. Real `dblink._blockers` is
// never computed by `dblink` itself -- it's injected by the real
// depgraph resolver, which already knows the full dependency graph by
// the time a merge runs. This pilot's own `ebuild <file> merge` has no
// depgraph at all (a standalone, single-ebuild real-execution path,
// unlike `emerge --pretend`), so `blocked_installed_packages` is new,
// self-contained machinery: real `repos.conf`/profile/USE config
// resolution (`portage_repo::find_repos` + `portage_profile::
// resolve_config`, brought into the real-execution path for the first
// time here), real effective-USE computation (`portage_repo::
// effective_use_flags`, made `pub` for this), real dependency-string
// flattening (`portage_use_reduce::use_reduce_flat`) against
// `DEPEND`+`RDEPEND`+`BDEPEND`+`PDEPEND`+`IDEPEND`, and real blocker-
// atom matching against every installed package (`portage_dep::
// match_from_list`). Degrades gracefully to an empty blocked set on any
// resolution failure -- see `MergeOptions::config_root`'s own doc
// comment for a real safety issue this surfaced and how it was fixed
// (this pilot's own dev/test machine has a real, populated
// `/etc/portage/repos.conf`, so an ambient env-var default here would
// have made every pre-existing test silently start reading real host
// config).
//
// KNOWN, DOCUMENTED GAPS (v1 scope):
//   - No preserve-libs *registration*/detection side at all: real
//     `_find_libs_to_preserve`/`_linkmap_rebuild` use `LinkageMap`
//     (`scanelf`-based ELF `NEEDED`/soname introspection) to decide
//     *what* a merge should start preserving in the first place -- a
//     real, separately-scoped subsystem (ELF parsing, a persistent
//     linkage graph) this pilot doesn't implement anywhere yet. This
//     slice only ever consults and unregisters a registry some other,
//     unimplemented mechanism (or a hand-seeded fixture, for testing)
//     already populated.
//   - No real `NEEDED`/`LinkageMap` bookkeeping in
//     `unregister_preserved_libs` (real `removeFromContents` also
//     strips matching `NEEDED` lines so stale linkage data doesn't
//     corrupt a future preserve-libs decision) -- moot without the
//     registration side above ever writing `NEEDED` data in the first
//     place.
//   - CONFIG_PROTECT is `obj`-only: a `sym` (symlink) entry under a
//     protected path is never protected here -- real `dblink._protect()`
//     handles symlinks too (comparing the *target string*'s own MD5),
//     but a CONFIG_PROTECT'd symlink is a genuinely rare real-world case
//     (essentially every real protected path is a regular config file).
//   - No `--noconfmem` support at all -- this pilot's own `ebuild` CLI
//     has no such flag, so behavior always matches real portage's own
//     default (`--noconfmem` off, i.e. `IGNORE=0`): an update whose
//     content exactly matches what `read_cfgfiledict` already recorded
//     as previously-offered is applied directly, never re-protected.
//   - `new_protect_filename` always allocates a fresh `._cfgNNNN_`
//     number -- real `new_protect_filename()` also reuses the *last*
//     one when its own content already matches the new update (a purely
//     cosmetic difference: this pilot may leave a few more distinct
//     `._cfgNNNN_` files behind than real portage would for repeated,
//     never-remembered identical content).
//   - No `env_update()`/`ldconfig` triggering -- real `merge()` runs
//     `env_update()` (`/etc/ld.so.cache`-equivalent regeneration,
//     `/etc/env.d` processing) after a successful merge; this pilot has
//     no equivalent machinery at all yet. (`COUNTER` and the atomic
//     `dbtmpdir`-then-rename vdb write are both now real -- see
//     `write_vdb_entry`'s own doc comment.)
//   - Real `os.chown`/permission-preserving `os.chmod` per merged file
//     are not reproduced explicitly -- `std::fs::copy` already preserves
//     a regular file's permission bits on Unix, which covers the common
//     case; ownership is left as whatever the copying process's own
//     default is (this pilot's own single-user dev/test context has no
//     privilege-dropping concept anywhere else either).
//   - Directory-entry merge order is sorted by filename for determinism
//     (this pilot's own test-reproducibility need) rather than real
//     `os.listdir()`'s own arbitrary/OS-dependent order -- `CONTENTS`
//     line order has no real semantic meaning portage itself relies on.
//   - `SLOT` is read directly from the ebuild's own text via a simple
//     `SLOT=...` assignment regex (see `parse_slot`), the same
//     "real-file, direct-text-parsing" shortcut `ebuild_phases::
//     parse_eapi` already takes for `EAPI` -- a `SLOT` computed by real
//     bash logic rather than declared as a literal is out of scope.
//   - `repository` is resolved by walking up from the ebuild's own
//     package directory looking for a `profiles/repo_name` file (real
//     portage's own mechanism for naming a repo), defaulting to the same
//     `"__unknown__"` sentinel `portage_repo::new_repo_changed` already
//     uses when no such file is found at all.

use crate::ebuild_phases;
use crate::env_update;
use md5::{Digest, Md5};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// Whether `command` is the one real merge command this module implements
/// -- `ebuild.rs` checks this alongside `ebuild_phases::
/// is_real_phase_command` before routing to real execution.
pub fn is_real_merge_command(command: &str) -> bool {
    command == "merge"
}

/// Options for `run_merge`, bundled into a struct rather than more
/// positional parameters -- this pilot already relearned the
/// "positional-parameter pain" lesson once, in `--newrepo`'s own
/// bulk-fix saga. `config_protect`/`config_protect_mask` are env-var-
/// sourced at the `ebuild.rs` CLI boundary, the same "env var, not full
/// config resolution" shortcut `PORTAGE_TMPDIR`/`ROOT` already use;
/// `Default` matches real `make.globals`'s own values exactly.
pub struct MergeOptions {
    pub debug: bool,
    pub config_protect: String,
    pub config_protect_mask: String,
    pub distdir: PathBuf,
    pub shell: ebuild_phases::ShellBackend,
    /// Real `"collision-protect" in self.settings.features` -- `FEATURES`
    /// itself isn't in `FEATURES` by default (real `make.globals` never
    /// sets it), so `Default` matches that: `false`.
    pub collision_protect: bool,
    /// Real `"protect-owned" in self.settings.features` (`lib/portage/
    /// dbapi/vartree.py:4718`): a separate abort condition from
    /// `collision_protect` -- see `run_merge`'s own doc comment for the
    /// exact real logic. Same `FEATURES`-default-false `Default`.
    pub protect_owned: bool,
    /// Real `PORTAGE_CONFIGROOT` (`portage_repo::config_root_from_env`'s
    /// own real default: `/` when unset) -- consulted only by
    /// `blocked_installed_packages`'s own real `repos.conf`/profile/USE
    /// resolution (see its own doc comment). Deliberately an explicit
    /// field, not an ambient env read inside this module -- the same
    /// "explicit parameter, not an ambient env read inside library code"
    /// reasoning `portage_fetch::FetchOptions::gentoo_mirrors` already
    /// established, load-bearing here for a genuinely different reason:
    /// this pilot's own dev/test machine has a real, populated
    /// `/etc/portage/repos.conf` (a real Gentoo system), so silently
    /// defaulting to real `/` the way `ebuild.rs`'s own CLI boundary
    /// does would make every test that doesn't override this field read
    /// real host config -- `Default` below uses a deliberately
    /// impossible path instead, so `blocked_installed_packages` always
    /// degrades to an empty blocked set unless a test opts in explicitly.
    pub config_root: PathBuf,
}

impl Default for MergeOptions {
    fn default() -> Self {
        Self {
            debug: false,
            config_protect: "/etc".to_string(),
            config_protect_mask: "/etc/env.d".to_string(),
            distdir: PathBuf::from("/var/cache/distfiles"),
            shell: ebuild_phases::ShellBackend::default(),
            collision_protect: false,
            protect_owned: false,
            // "/dev/null" is a real character device, never a directory
            // -- joining anything under it can never exist on any real
            // filesystem, guaranteeing find_repos always fails cleanly
            // here regardless of what happens to exist on the host.
            config_root: PathBuf::from("/dev/null/no-config-root-configured"),
        }
    }
}

/// Real `ConfigProtect.isprotected()` (`lib/portage/util/__init__.py`):
/// longest-prefix match against `config_protect` (a whitespace-separated
/// path list, `root`-joined) minus `config_protect_mask`. A protect/mask
/// entry that names a real, on-disk directory matches any path under it
/// (`/etc` matches `/etc/foo` but not `/etcfoo`); one that doesn't (a
/// literal file, or a path that doesn't exist at all) only ever matches
/// exactly.
fn is_protected(root: &Path, config_protect: &str, config_protect_mask: &str, dest: &Path) -> bool {
    fn longest_match(root: &Path, list: &str, dest: &Path) -> usize {
        let dest_str = dest.to_string_lossy();
        let mut best = 0;
        for entry in list.split_whitespace() {
            let ppath = root.join(entry.trim_start_matches('/'));
            let ppath_str = ppath.to_string_lossy().trim_end_matches('/').to_string();
            let is_dir = ppath.is_dir();
            let matched = if is_dir {
                dest_str == ppath_str.as_str() || dest_str.starts_with(&format!("{ppath_str}/"))
            } else {
                dest_str == ppath_str.as_str()
            };
            if matched && ppath_str.len() > best {
                best = ppath_str.len();
            }
        }
        best
    }
    let protected_len = longest_match(root, config_protect, dest);
    if protected_len == 0 {
        return false;
    }
    protected_len > longest_match(root, config_protect_mask, dest)
}

/// Real `new_protect_filename()` (`lib/portage/util/__init__.py`): the
/// next unused `._cfgNNNN_<basename>` sibling of `dest` -- `dest` itself
/// is never touched here, the caller writes to the returned path
/// instead. Deliberately narrower than real: always allocates a fresh
/// number rather than reusing the last one when its own content already
/// matches the new update (see this module's own doc comment).
fn new_protect_filename(dest: &Path) -> Result<PathBuf, String> {
    let parent = dest
        .parent()
        .ok_or_else(|| format!("{}: has no parent directory", dest.display()))?;
    let basename = dest
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("{}: not a valid filename", dest.display()))?;

    let mut max_num: i64 = -1;
    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(rest) = name.strip_prefix("._cfg") {
                if rest.len() > 5 && rest.as_bytes()[4] == b'_' && &rest[5..] == basename {
                    if let Ok(n) = rest[..4].parse::<i64>() {
                        max_num = max_num.max(n);
                    }
                }
            }
        }
    }
    Ok(parent.join(format!("._cfg{:04}_{basename}", max_num + 1)))
}

/// Real `vardbapi._conf_mem_file`: `<root>/var/lib/portage/config`, a
/// real, persisted "which src MD5 has already been offered for this
/// path" memory (real `grabdict`/`writedict`'s own `"path value\n"`
/// format) -- without it, re-merging an already-protected update would
/// spawn a fresh `._cfgNNNN_` file every single time, even though the
/// admin has already been shown this exact change once. This pilot's
/// own `ebuild` CLI has no `--noconfmem` flag, so behavior always
/// matches real portage's own default (`--noconfmem` off).
fn cfg_mem_path(root: &Path) -> PathBuf {
    root.join("var/lib/portage/config")
}

fn read_cfgfiledict(root: &Path) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    if let Ok(text) = std::fs::read_to_string(cfg_mem_path(root)) {
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
                map.insert(key.to_string(), value.to_string());
            }
        }
    }
    map
}

fn write_cfgfiledict(root: &Path, map: &BTreeMap<String, String>) -> Result<(), String> {
    let path = cfg_mem_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut text = String::new();
    for (k, v) in map {
        text.push_str(&format!("{k} {v}\n"));
    }
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Real `PreservedLibsRegistry`'s own in-memory shape
/// (`lib/portage/util/_dyn_libs/PreservedLibsRegistry.py`): `"cp:slot"`
/// -> `(cpv, counter, paths)`. `preserved_libs()` mirrors real
/// `getPreservedLibs()` (cpv -> paths, last entry wins on a duplicate
/// cpv across keys -- a corner case with no real relevance here).
type PlibEntries = BTreeMap<String, (String, String, Vec<String>)>;

struct PlibRegistry {
    entries: PlibEntries,
}

impl PlibRegistry {
    fn preserved_libs(&self) -> BTreeMap<String, Vec<String>> {
        let mut out = BTreeMap::new();
        for (cpv, _counter, paths) in self.entries.values() {
            out.insert(cpv.clone(), paths.clone());
        }
        out
    }
}

/// Real `lib/portage/const.py`'s own `PRIVATE_PATH` (`"var/lib/portage"`)
/// joined with `PreservedLibsRegistry`'s own hardcoded filename.
fn plib_registry_path(root: &Path) -> PathBuf {
    root.join("var/lib/portage/preserved_libs_registry")
}

/// A minimal hand-rolled JSON string-literal reader (handling the
/// `\"`/`\\`/`\/`/`\n`/`\t`/`\r`/`\b`/`\f`/`\uXXXX` escapes real
/// `json.dumps` may emit for a path), used only by `parse_plib_registry`
/// below -- narrow by design, not a general JSON parser.
fn parse_json_string(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<String> {
    if chars.next()? != '"' {
        return None;
    }
    let mut out = String::new();
    loop {
        match chars.next()? {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{C}'),
                'u' => {
                    let hex: String = (0..4).map(|_| chars.next()).collect::<Option<String>>()?;
                    let code = u32::from_str_radix(&hex, 16).ok()?;
                    out.push(char::from_u32(code)?);
                }
                _ => return None,
            },
            c => out.push(c),
        }
    }
}

fn skip_json_ws(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
        chars.next();
    }
}

fn parse_json_string_array(
    chars: &mut std::iter::Peekable<std::str::Chars>,
) -> Option<Vec<String>> {
    skip_json_ws(chars);
    if chars.next()? != '[' {
        return None;
    }
    let mut out = Vec::new();
    skip_json_ws(chars);
    if chars.peek() == Some(&']') {
        chars.next();
        return Some(out);
    }
    loop {
        skip_json_ws(chars);
        out.push(parse_json_string(chars)?);
        skip_json_ws(chars);
        match chars.next()? {
            ',' => continue,
            ']' => return Some(out),
            _ => return None,
        }
    }
}

/// Parses exactly the shape real `PreservedLibsRegistry.store()` writes:
/// `{"cp:slot": [cpv, counter, [paths...]], ...}`. Returns `None` on any
/// deviation -- the caller treats that the same as a missing file (real
/// `load()`'s own graceful degrade to `{}` on a corrupt/unreadable file).
fn parse_plib_registry(text: &str) -> Option<PlibEntries> {
    let mut chars = text.chars().peekable();
    let mut entries = BTreeMap::new();
    skip_json_ws(&mut chars);
    if chars.next()? != '{' {
        return None;
    }
    skip_json_ws(&mut chars);
    if chars.peek() == Some(&'}') {
        chars.next();
        return Some(entries);
    }
    loop {
        skip_json_ws(&mut chars);
        let key = parse_json_string(&mut chars)?;
        skip_json_ws(&mut chars);
        if chars.next()? != ':' {
            return None;
        }
        skip_json_ws(&mut chars);
        if chars.next()? != '[' {
            return None;
        }
        skip_json_ws(&mut chars);
        let cpv = parse_json_string(&mut chars)?;
        skip_json_ws(&mut chars);
        if chars.next()? != ',' {
            return None;
        }
        skip_json_ws(&mut chars);
        let counter = parse_json_string(&mut chars)?;
        skip_json_ws(&mut chars);
        if chars.next()? != ',' {
            return None;
        }
        let paths = parse_json_string_array(&mut chars)?;
        skip_json_ws(&mut chars);
        if chars.next()? != ']' {
            return None;
        }
        entries.insert(key, (cpv, counter, paths));
        skip_json_ws(&mut chars);
        match chars.next()? {
            ',' => continue,
            '}' => return Some(entries),
            _ => return None,
        }
    }
}

/// Real `load()`: a missing or unparseable registry file degrades
/// gracefully to an empty registry rather than an error.
fn read_plib_registry(root: &Path) -> PlibRegistry {
    let entries = std::fs::read_to_string(plib_registry_path(root))
        .ok()
        .and_then(|text| parse_plib_registry(&text))
        .unwrap_or_default();
    PlibRegistry { entries }
}

fn json_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Real `store()`'s own `json.dumps(..., indent="\t", sort_keys=True)`
/// layout -- `BTreeMap` already keeps keys sorted.
fn write_plib_registry(root: &Path, registry: &PlibRegistry) -> Result<(), String> {
    let path = plib_registry_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut out = String::from("{\n");
    let n = registry.entries.len();
    for (i, (key, (cpv, counter, paths))) in registry.entries.iter().enumerate() {
        out.push_str(&format!("\t{}: [\n", json_quote(key)));
        out.push_str(&format!("\t\t{},\n", json_quote(cpv)));
        out.push_str(&format!("\t\t{},\n", json_quote(counter)));
        if paths.is_empty() {
            out.push_str("\t\t[]\n");
        } else {
            out.push_str("\t\t[\n");
            for (j, p) in paths.iter().enumerate() {
                out.push_str(&format!("\t\t\t{}", json_quote(p)));
                out.push_str(if j + 1 < paths.len() { ",\n" } else { "\n" });
            }
            out.push_str("\t\t]\n");
        }
        out.push_str("\t]");
        out.push_str(if i + 1 < n { ",\n" } else { "\n" });
    }
    out.push_str("}\n");
    std::fs::write(&path, out).map_err(|e| format!("{}: {e}", path.display()))
}

/// Real `_lstat_inode_map`: `(st_dev, st_ino)` -> every registered
/// `(cpv, path)` pair currently lstat-able at that inode (multiple paths
/// may share an inode via hardlinks). A path the registry names but that
/// no longer exists on disk is silently skipped, matching real
/// `_lstat_inode_map`'s own `except OSError` -> `continue`.
fn plib_inode_map(
    root: &Path,
    preserved: &BTreeMap<String, Vec<String>>,
) -> HashMap<(u64, u64), Vec<(String, String)>> {
    let mut map: HashMap<(u64, u64), Vec<(String, String)>> = HashMap::new();
    for (cpv, paths) in preserved {
        for p in paths {
            let full = root.join(p.trim_start_matches('/'));
            if let Ok(meta) = std::fs::symlink_metadata(&full) {
                map.entry((meta.dev(), meta.ino()))
                    .or_default()
                    .push((cpv.clone(), p.clone()));
            }
        }
    }
    map
}

/// Real `dblink.merge()`'s own post-copy step (`lib/portage/dbapi/
/// vartree.py:5095-5159`): any path this merge's own `find_collisions`
/// matched against a currently-registered preserved lib is now
/// legitimately owned by the just-merged package instead of its
/// previous, registered owner. Drops the taken-over paths from the
/// registry (removing the owning `cp:slot` entry entirely once its own
/// path list empties) and from the previous owner's own real vdb
/// `CONTENTS` (real `removeFromContents`) -- skipped when the previous
/// owner *is* the package that was just merged (real `if cpv !=
/// self.mycpv`: re-merging the exact same cpv already replaces its own
/// vdb entry wholesale, so there's nothing stale left to strip).
fn unregister_preserved_libs(
    root: &Path,
    merging_cpv: &str,
    mut registry: PlibRegistry,
    plib_collisions: &BTreeMap<String, BTreeSet<String>>,
) -> Result<(), String> {
    for (cpv, paths) in plib_collisions {
        let mut empty_key = None;
        for (key, (entry_cpv, _counter, entry_paths)) in registry.entries.iter_mut() {
            if entry_cpv == cpv {
                entry_paths.retain(|p| !paths.contains(p));
                if entry_paths.is_empty() {
                    empty_key = Some(key.clone());
                }
                break;
            }
        }
        if let Some(key) = empty_key {
            registry.entries.remove(&key);
        }

        if cpv != merging_cpv {
            remove_from_contents(root, cpv, paths)?;
        }
    }
    write_plib_registry(root, &registry)
}

/// Real `vardbapi.removeFromContents`, narrowed to the `CONTENTS`-line
/// removal itself (real `NEEDED`-line stripping is a documented gap --
/// see this module's own doc comment). A missing vdb entry for `cpv`
/// (already unmerged some other way) is silently a no-op, matching real
/// `removeFromContents`'s own tolerance of a stale registry entry.
fn remove_from_contents(root: &Path, cpv: &str, paths: &BTreeSet<String>) -> Result<(), String> {
    let Some((category, pf)) = cpv.split_once('/') else {
        return Ok(());
    };
    let contents_path = root
        .join("var/db/pkg")
        .join(category)
        .join(pf)
        .join("CONTENTS");
    let Ok(text) = std::fs::read_to_string(&contents_path) else {
        return Ok(());
    };
    let new_text: String = text
        .lines()
        .filter(|line| {
            let mut parts = line.split_whitespace();
            parts.next();
            !matches!(parts.next(), Some(p) if paths.contains(p))
        })
        .map(|l| format!("{l}\n"))
        .collect();
    std::fs::write(&contents_path, new_text)
        .map_err(|e| format!("{}: {e}", contents_path.display()))
}

/// Real PMS: unlike `EAPI` (restricted to the ebuild's own first real
/// line), `SLOT` may appear anywhere among an ebuild's own top-level
/// variable assignments -- this scans every line for the first literal
/// `SLOT=...` match. No match at all defaults to `"0"` (the same default
/// `portage_repo::installed_candidates`/`split_slot` already use for a
/// missing `SLOT`).
fn parse_slot(ebuild_text: &str) -> String {
    let slot_re = regex::Regex::new(r#"^[ \t]*SLOT=(?:"([^"]*)"|'([^']*)'|(\S*))[ \t]*(#.*)?$"#)
        .expect("static regex is valid");
    for line in ebuild_text.lines() {
        if let Some(caps) = slot_re.captures(line) {
            let value = caps
                .get(1)
                .or_else(|| caps.get(2))
                .or_else(|| caps.get(3))
                .map(|m| m.as_str())
                .unwrap_or("");
            return value.to_string();
        }
    }
    "0".to_string()
}

/// Real portage's own mechanism for naming a repo (`layout.conf`'s
/// `repo-name` key aside, the canonical source is a repo's own
/// `profiles/repo_name` file, first line): walks up from `pkg_dir`
/// (`<category>/<package>`) through every ancestor, returning the first
/// `profiles/repo_name` found. `None` when no ancestor has one at all
/// (e.g. a standalone ebuild file outside any repo checkout).
fn repository_name_for(pkg_dir: &Path) -> Option<String> {
    for ancestor in pkg_dir.ancestors() {
        let candidate = ancestor.join("profiles").join("repo_name");
        if let Ok(text) = std::fs::read_to_string(&candidate) {
            let name = text.lines().next().unwrap_or("").trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn md5_hex(path: &Path) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = Md5::new();
    hasher.update(&data);
    let digest = hasher.finalize();
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

/// Real `dblink._format_contents_line`: `<type> <path>[ <md5>| -> <target>][ <mtime>]\n`.
fn format_contents_line(
    node_type: &str,
    abs_path: &str,
    md5_digest: Option<&str>,
    symlink_target: Option<&str>,
    mtime_secs: Option<i64>,
) -> String {
    let mut fields = vec![node_type.to_string(), abs_path.to_string()];
    if let Some(md5) = md5_digest {
        fields.push(md5.to_string());
    } else if let Some(target) = symlink_target {
        fields.push(format!("-> {target}"));
    }
    if let Some(mtime) = mtime_secs {
        fields.push(mtime.to_string());
    }
    format!("{}\n", fields.join(" "))
}

/// `pub(crate)`: `ebuild_unmerge`'s own mtime-staleness check
/// (`remove_contents`) reuses this exact conversion to compare a live
/// file's current mtime against a `CONTENTS`-recorded one.
pub(crate) fn mtime_secs(metadata: &std::fs::Metadata) -> Result<i64, String> {
    use std::time::UNIX_EPOCH;
    let mtime = metadata
        .modified()
        .map_err(|e| format!("reading mtime: {e}"))?;
    Ok(mtime
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("mtime before epoch: {e}"))?
        .as_secs() as i64)
}

/// Walks `d` (real `${D}`) and merges every entry into `root` (real
/// `${ROOT}`), returning the accumulated real `CONTENTS` text (see this
/// module's own doc comment for the exact line format and the v1 scope
/// cuts -- no chown, sorted-by-name traversal order). `cfgfiledict` is
/// read once by the caller before this runs and written back once after
/// -- real `vardbapi._conf_mem_file` semantics (a single, whole-merge
/// read/update/write, not a per-file one).
fn merge_tree(
    d: &Path,
    root: &Path,
    config_protect: &str,
    config_protect_mask: &str,
    cfgfiledict: &mut BTreeMap<String, String>,
) -> Result<String, String> {
    let mut contents = String::new();
    let mut stack: Vec<PathBuf> = vec![PathBuf::new()];
    while let Some(relative_dir) = stack.pop() {
        let src_dir = d.join(&relative_dir);
        let mut children: Vec<PathBuf> = std::fs::read_dir(&src_dir)
            .map_err(|e| format!("{}: {e}", src_dir.display()))?
            .filter_map(|e| e.ok())
            .map(|e| relative_dir.join(e.file_name()))
            .collect();
        children.sort();

        for relative_path in children {
            let src = d.join(&relative_path);
            let dest = root.join(&relative_path);
            let abs_path = format!("/{}", relative_path.display());
            let file_type = std::fs::symlink_metadata(&src)
                .map_err(|e| format!("{}: {e}", src.display()))?
                .file_type();

            if file_type.is_symlink() {
                let target =
                    std::fs::read_link(&src).map_err(|e| format!("{}: {e}", src.display()))?;
                if dest.exists() || dest.symlink_metadata().is_ok() {
                    let _ = std::fs::remove_file(&dest);
                }
                std::os::unix::fs::symlink(&target, &dest)
                    .map_err(|e| format!("{}: {e}", dest.display()))?;
                let mtime = mtime_secs(
                    &std::fs::symlink_metadata(&src)
                        .map_err(|e| format!("{}: {e}", src.display()))?,
                )?;
                // Real movefile() preserves the source's own mtime onto
                // the merged destination -- without this, the freshly
                // created symlink would get its own "now" mtime, never
                // matching what's about to be recorded in CONTENTS below
                // (see ebuild_unmerge.rs's own "!mtime" staleness check,
                // which relies on this actually holding).
                let ft = filetime::FileTime::from_unix_time(mtime, 0);
                filetime::set_symlink_file_times(&dest, ft, ft)
                    .map_err(|e| format!("{}: {e}", dest.display()))?;
                contents.push_str(&format_contents_line(
                    "sym",
                    &abs_path,
                    None,
                    Some(&target.to_string_lossy()),
                    Some(mtime),
                ));
            } else if file_type.is_dir() {
                std::fs::create_dir_all(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
                contents.push_str(&format_contents_line("dir", &abs_path, None, None, None));
                stack.push(relative_path);
            } else if file_type.is_file() {
                let src_md5 = md5_hex(&src)?;
                // Real dblink._protect(): a protected path whose real
                // on-disk content differs from what's about to be merged
                // gets diverted to a fresh ._cfgNNNN_ sibling instead of
                // overwritten -- unless cfgfiledict already remembers
                // this exact src_md5 as a previously-offered update for
                // this path (real "--noconfmem off" default: apply it
                // directly, don't re-protect).
                let mut write_dest = dest.clone();
                if is_protected(root, config_protect, config_protect_mask, &dest) {
                    if let Ok(dest_meta) = std::fs::metadata(&dest) {
                        if dest_meta.is_file() {
                            let dest_md5 = md5_hex(&dest)?;
                            if dest_md5 != src_md5 {
                                let already_offered = cfgfiledict.get(&abs_path) == Some(&src_md5);
                                if !already_offered {
                                    write_dest = new_protect_filename(&dest)?;
                                }
                                cfgfiledict.insert(abs_path.clone(), src_md5.clone());
                            }
                        }
                    }
                }

                if let Some(parent) = write_dest.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("{}: {e}", parent.display()))?;
                }
                std::fs::copy(&src, &write_dest).map_err(|e| format!("{}: {e}", src.display()))?;
                let mtime = mtime_secs(
                    &std::fs::metadata(&src).map_err(|e| format!("{}: {e}", src.display()))?,
                )?;
                // Real movefile() preserves the source's own mtime onto
                // the destination -- std::fs::copy doesn't (the copy
                // gets a fresh "now" mtime), which would otherwise never
                // match what's recorded in CONTENTS below (see
                // ebuild_unmerge.rs's own "!mtime" staleness check).
                filetime::set_file_mtime(&write_dest, filetime::FileTime::from_unix_time(mtime, 0))
                    .map_err(|e| format!("{}: {e}", write_dest.display()))?;
                // Real CONTENTS always records the package's own logical
                // path (`abs_path`) and the *source*'s own MD5 -- never
                // the ._cfgNNNN_ variant a protected write may have
                // actually landed at (real dblink.mergeme(): `abs_path=
                // myrealdest, md5_digest=mymd5`, both computed before
                // `_protect()` ever runs). The vdb still considers this
                // package the owner of the *logical* path either way.
                contents.push_str(&format_contents_line(
                    "obj",
                    &abs_path,
                    Some(&src_md5),
                    None,
                    Some(mtime),
                ));
            }
            // fifo/device nodes: real mergeme() handles these too, but no
            // fixture this pilot has needs them -- out of scope for now.
        }
    }
    Ok(contents)
}

/// Real `lib/portage/const.py`'s own `CACHE_PATH` (`var/cache/edb`): the
/// global, monotonically-increasing merge counter lives at
/// `<root>/var/cache/edb/counter`, a bare integer with no trailing
/// newline (`write_atomic(self._counter_path, str(counter))`). Real
/// `vardbapi.counter_tick_core()` treats a missing or corrupt file as
/// `-1` (so the very first merge anywhere gets `COUNTER=0`), then
/// increments and writes back. Not reproduced here: real
/// `get_counter_tick_core()`'s own extra safety net of scanning every
/// already-installed package's own `COUNTER` for a higher value, in case
/// the global file itself is stale/corrupt relative to the vdb -- a
/// corner case with no real relevance to this pilot's own synthetic
/// fixtures.
fn next_counter(root: &Path) -> Result<i64, String> {
    let counter_path = root.join("var/cache/edb/counter");
    let previous: i64 = std::fs::read_to_string(&counter_path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(-1);
    let next = previous + 1;
    if let Some(parent) = counter_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(&counter_path, next.to_string())
        .map_err(|e| format!("{}: {e}", counter_path.display()))?;
    Ok(next)
}

/// Real `lib/portage/const.py`'s own `MERGING_IDENTIFIER` (`"-MERGING-"`):
/// the prefix real `dblink.dbtmpdir` uses for its own temporary,
/// not-yet-finalized vdb entry directory, a sibling of the real vdb entry
/// under the same `<category>` directory.
const MERGING_IDENTIFIER: &str = "-MERGING-";

/// Writes a real vdb entry under `root` for the package described by
/// `env` -- `CATEGORY`/`SLOT`/`repository`/`COUNTER`/`CONTENTS`, matching
/// the same one-value-per-file convention this pilot's own fixtures and
/// `portage_repo`'s own vdb readers already use. Builds the entry in a
/// `MERGING_IDENTIFIER`-prefixed temporary sibling directory first, then
/// atomically renames it into place -- mirroring real `dblink.merge()`'s
/// own `dbtmpdir`-then-`_movefile()` approach (both are guaranteed to sit
/// on the same filesystem, under the same `<category>` directory, so
/// `std::fs::rename` alone is already atomic here, the same guarantee
/// real `_movefile()` relies on for a same-device move). A crash
/// mid-write leaves at most a stale, harmless `MERGING_IDENTIFIER`
/// leftover -- never a half-written *final* vdb entry.
fn write_vdb_entry(
    root: &Path,
    env: &ebuild_phases::Environment,
    slot: &str,
    repository: &str,
    contents: &str,
) -> Result<(), String> {
    let cat_dir = root.join("var/db/pkg").join(&env.category);
    let tmp_dir = cat_dir.join(format!("{MERGING_IDENTIFIER}{}", env.split.pf));
    let final_dir = cat_dir.join(&env.split.pf);

    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir).map_err(|e| format!("{}: {e}", tmp_dir.display()))?;
    }
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("{}: {e}", tmp_dir.display()))?;

    let counter = next_counter(root)?;
    for (name, value) in [
        ("CATEGORY", env.category.as_str()),
        ("SLOT", slot),
        ("repository", repository),
    ] {
        std::fs::write(tmp_dir.join(name), format!("{value}\n"))
            .map_err(|e| format!("{}: {e}", tmp_dir.join(name).display()))?;
    }
    std::fs::write(tmp_dir.join("CONTENTS"), contents)
        .map_err(|e| format!("{}: {e}", tmp_dir.join("CONTENTS").display()))?;
    std::fs::write(tmp_dir.join("COUNTER"), counter.to_string())
        .map_err(|e| format!("{}: {e}", tmp_dir.join("COUNTER").display()))?;

    if final_dir.exists() {
        std::fs::remove_dir_all(&final_dir).map_err(|e| format!("{}: {e}", final_dir.display()))?;
    }
    std::fs::rename(&tmp_dir, &final_dir).map_err(|e| format!("{}: {e}", final_dir.display()))?;
    Ok(())
}

fn read_installed_slot(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> Option<String> {
    let path = root
        .join("var/db/pkg")
        .join(category)
        .join(format!("{package}-{version}"))
        .join("SLOT");
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().split('/').next().unwrap_or("").to_string())
}

/// Whether the installed package at `<root>/var/db/pkg/<category>/
/// <package>-<version>` already claims `abs_path` in its own real
/// `CONTENTS` (second whitespace-separated field of any line, the same
/// format `format_contents_line` writes).
fn owns_path(root: &Path, category: &str, package: &str, version: &str, abs_path: &str) -> bool {
    let path = root
        .join("var/db/pkg")
        .join(category)
        .join(format!("{package}-{version}"))
        .join("CONTENTS");
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    text.lines().any(|line| {
        let mut parts = line.split_whitespace();
        parts.next();
        parts.next() == Some(abs_path)
    })
}

/// Same real `CONTENTS`-ownership check as `owns_path`, but keyed by a
/// bare `category`/`pf` pair (`"package-version"`, real portage's own
/// vdb directory-name convention) rather than a split `package`/
/// `version` -- what `blocked_installed_packages` below already has on
/// hand, since it discovers installed packages by scanning real vdb
/// directory names directly rather than through `installed_versions`'s
/// own `package`-scoped lookup.
fn owns_path_pf(root: &Path, category: &str, pf: &str, abs_path: &str) -> bool {
    let path = root
        .join("var/db/pkg")
        .join(category)
        .join(pf)
        .join("CONTENTS");
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    text.lines().any(|line| {
        let mut parts = line.split_whitespace();
        parts.next();
        parts.next() == Some(abs_path)
    })
}

/// Real `mypkglist = others_in_slot + blockers` (`dblink.merge()`'s own
/// blocker half -- `others_in_slot` is already `find_collisions`'s own
/// `own_versions`). Real `dblink._blockers` is never computed by
/// `dblink` itself: it's injected by the real depgraph resolver, which
/// already knows the full dependency graph by the time a merge runs.
/// This pilot's own `ebuild <file> merge` has no depgraph at all (a
/// standalone, single-ebuild real-execution path, unlike `emerge
/// --pretend`) -- so this is a new, self-contained computation:
/// resolves real `repos.conf`/profile/USE config for the merging
/// package's own repo (`portage_repo::find_repos` +
/// `portage_profile::resolve_config`, the exact same machinery
/// `pretend.rs` already uses, including its own real `masters =`
/// resolution), computes the merging package's own effective USE the
/// same way `portage_repo`'s own (now `pub`) `effective_use_flags`
/// always has, flattens its own real `DEPEND`+`RDEPEND`+`BDEPEND`+
/// `PDEPEND`+`IDEPEND` (`portage_use_reduce::use_reduce_flat`) against
/// it, and matches every blocker atom found (`!atom`/`!!atom`,
/// `portage_dep::parse_atom`'s own `.blocker`) against every real
/// installed package (`portage_dep::match_from_list`, which -- real,
/// verified behavior already relied on elsewhere in this pilot --
/// ignores an atom's blocker marker entirely when matching, so the
/// blocker atom string can be passed in as-is). Real weak vs. strong
/// blockers are not distinguished (`dblink.merge()`'s own `mypkglist`
/// construction doesn't either -- both kinds exclude a collision the
/// same way). Returns every matched installed package as a bare
/// `(category, pf)` pair.
///
/// Degrades gracefully to an empty set on any resolution failure
/// (missing `repos.conf`, unreadable md5-cache, an ebuild path outside
/// any real repo, etc.) -- config resolution isn't guaranteed to
/// succeed in every context `ebuild <file> merge` is used from (unlike
/// `emerge --pretend`, this pilot's own real-execution CLI has never
/// required it before this slice), and a collision that would have been
/// excluded here just gets reported as an ordinary one instead: never a
/// false negative in the direction that could silently corrupt a real
/// merge.
fn blocked_installed_packages(
    root: &Path,
    config_root: &Path,
    env: &ebuild_phases::Environment,
    slot: &str,
    repository: &str,
) -> HashSet<(String, String)> {
    (|| -> Option<HashSet<(String, String)>> {
        let repo_root = ebuild_phases::repo_root_for(&env.pkg_dir)?;
        let metadata =
            portage_repo::read_md5_cache(&repo_root, &env.category, &env.split.pf).ok()?;

        let repos = portage_repo::find_repos(config_root).ok()?;
        let main_repo = repos.iter().find(|r| r.is_main)?;
        let overlay_repos: Vec<(String, PathBuf)> = repos
            .iter()
            .filter(|r| !r.is_main)
            .map(|r| (r.name.clone(), r.location.clone()))
            .collect();
        let repo_masters: HashMap<String, Vec<PathBuf>> = repos
            .iter()
            .map(|r| (r.name.clone(), r.masters.clone()))
            .collect();
        let config = portage_profile::resolve_config(
            config_root,
            &main_repo.location,
            &overlay_repos,
            &main_repo.name,
            &repo_masters,
        )
        .ok()?;

        let iuse = metadata.get("IUSE").map(String::as_str).unwrap_or_default();
        let keywords: Vec<String> = metadata
            .get("KEYWORDS")
            .map(|s| s.split_whitespace().map(String::from).collect())
            .unwrap_or_default();
        let candidate_str = format!(
            "{}/{}-{}:{slot}/{slot}::{repository}",
            env.category, env.split.pn, env.split.pvr
        );
        let use_flags = portage_repo::effective_use_flags(
            iuse,
            &config.use_tokens,
            &config.package_use,
            &config.package_use_force,
            &config.package_use_mask,
            &config.use_force,
            &config.use_mask,
            &config.use_stable_force,
            &config.use_stable_mask,
            &config.package_use_stable_force,
            &config.package_use_stable_mask,
            &keywords,
            &config.accept_keywords,
            &config.package_accept_keywords,
            &candidate_str,
            &env.category,
            &env.split.pn,
        );

        let dep_keys = ["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"];
        let mut depstr = String::new();
        for dep_key in dep_keys {
            if let Some(d) = metadata.get(dep_key) {
                depstr.push_str(d);
                depstr.push(' ');
            }
        }
        let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
        let flat_deps = portage_use_reduce::use_reduce_flat(
            &tokens,
            &use_flags,
            portage_use_reduce::MatchMode::Normal,
        )
        .ok()?;

        // Every real installed package, as (category, pf, candidate_str)
        // -- candidate_str needs real SLOT info too (a blocker atom can
        // be slot-restricted, e.g. `!dev-libs/foo:0`), read directly
        // from each vdb entry's own SLOT file, mirroring real
        // `resolve_blockers`'s own candidate-string construction
        // (portage-repo) rather than the bare `category/pf` shortcut
        // `find_owners`'s own error-reporting-only scan uses (which
        // never needs slot-restricted matching at all).
        let pkg_root = root.join("var/db/pkg");
        let categories = std::fs::read_dir(&pkg_root).ok()?;
        let installed: Vec<(String, String, String)> = categories
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .flat_map(|category_entry| {
                let category_name = category_entry.file_name().to_string_lossy().to_string();
                std::fs::read_dir(category_entry.path())
                    .into_iter()
                    .flatten()
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().is_dir())
                    .filter_map(move |pkg_entry| {
                        let pf = pkg_entry.file_name().to_string_lossy().to_string();
                        let slot = std::fs::read_to_string(pkg_entry.path().join("SLOT"))
                            .ok()?
                            .trim()
                            .to_string();
                        let (slot, sub_slot) = slot
                            .split_once('/')
                            .map(|(s, ss)| (s.to_string(), ss.to_string()))
                            .unwrap_or_else(|| (slot.clone(), slot.clone()));
                        Some((category_name.clone(), pf, slot, sub_slot))
                    })
                    .map(|(category, pf, slot, sub_slot)| {
                        let candidate_str = format!("{category}/{pf}:{slot}/{sub_slot}");
                        (category, pf, candidate_str)
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let installed_strs: Vec<&str> = installed.iter().map(|(_, _, s)| s.as_str()).collect();
        let by_str: HashMap<&str, &(String, String, String)> = installed_strs
            .iter()
            .copied()
            .zip(installed.iter())
            .collect();

        let mut blocked: HashSet<(String, String)> = HashSet::new();
        for tok in &flat_deps {
            let Some(dep_atom) = portage_dep::parse_atom(tok) else {
                continue;
            };
            if dep_atom.blocker == portage_dep::Blocker::None {
                continue;
            }
            if let Some(matched) = portage_dep::match_from_list(tok, &installed_strs) {
                for m in matched {
                    if let Some((category, pf, _)) = by_str.get(m) {
                        blocked.insert((category.clone(), pf.clone()));
                    }
                }
            }
        }
        Some(blocked)
    })()
    .unwrap_or_default()
}

/// Real PMS 13.4's own symlink-over-directory ban (checked
/// unconditionally, regardless of `FEATURES`) plus real `FEATURES=
/// collision-protect`'s own ordinary-collision detection, plus real
/// preserve-libs collision exclusion and real blocker exclusion (`mypkglist
/// = others_in_slot + blockers` -- see `blocked_installed_packages`'s own
/// doc comment for the full real grounding; `FEATURES=protect-owned` is
/// real too, but decided by the caller, `run_merge`, using this
/// function's own `collisions` result together with `find_owners`, not
/// inside this function itself). Walks `d` (the real install image,
/// `${D}`) the same way `merge_tree` does,
/// but read-only and file/symlink-only (real `_collision_protect` never
/// checks directories at all -- a directory merging into an existing
/// directory is normal, not a collision). Returns `(collisions,
/// symlink_collisions, plib_collisions)` as real, `ROOT`-relative
/// absolute paths (`plib_collisions` keyed by the preserved lib's own
/// owning cpv); the caller decides whether `collisions` alone should
/// abort the merge (gated on `FEATURES=collision-protect`) --
/// `symlink_collisions` always should, and `plib_collisions` never does
/// (real `_collision_protect` excludes those from `collisions`
/// unconditionally, regardless of `FEATURES`).
type CollisionsResult =
    Result<(Vec<String>, Vec<String>, BTreeMap<String, BTreeSet<String>>), String>;

#[allow(clippy::too_many_arguments)]
fn find_collisions(
    d: &Path,
    root: &Path,
    category: &str,
    package: &str,
    slot: &str,
    config_protect: &str,
    config_protect_mask: &str,
    plib_inodes: &HashMap<(u64, u64), Vec<(String, String)>>,
    blocked: &HashSet<(String, String)>,
) -> CollisionsResult {
    let own_versions: Vec<String> = portage_repo::installed_versions(root, category, package)
        .into_iter()
        .filter(|version| {
            read_installed_slot(root, category, package, version).as_deref() == Some(slot)
        })
        .collect();

    let mut collisions = Vec::new();
    let mut symlink_collisions = Vec::new();
    let mut plib_collisions: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut stack: Vec<PathBuf> = vec![PathBuf::new()];
    while let Some(relative_dir) = stack.pop() {
        let src_dir = d.join(&relative_dir);
        let mut children: Vec<PathBuf> = std::fs::read_dir(&src_dir)
            .map_err(|e| format!("{}: {e}", src_dir.display()))?
            .filter_map(|e| e.ok())
            .map(|e| relative_dir.join(e.file_name()))
            .collect();
        children.sort();

        for relative_path in children {
            let src = d.join(&relative_path);
            let dest = root.join(&relative_path);
            let abs_path = format!("/{}", relative_path.display());
            let file_type = std::fs::symlink_metadata(&src)
                .map_err(|e| format!("{}: {e}", src.display()))?
                .file_type();

            if file_type.is_dir() {
                stack.push(relative_path);
                continue;
            }

            let Ok(dest_meta) = std::fs::symlink_metadata(&dest) else {
                continue;
            };

            if file_type.is_symlink() && dest_meta.is_dir() {
                symlink_collisions.push(abs_path);
                continue;
            }

            if let Some(plibs) = plib_inodes.get(&(dest_meta.dev(), dest_meta.ino())) {
                for (cpv, path) in plibs {
                    plib_collisions
                        .entry(cpv.clone())
                        .or_default()
                        .insert(path.clone());
                }
                continue;
            }

            let owned = own_versions
                .iter()
                .any(|version| owns_path(root, category, package, version, &abs_path))
                || blocked
                    .iter()
                    .any(|(bcat, bpf)| owns_path_pf(root, bcat, bpf, &abs_path));
            if owned || is_protected(root, config_protect, config_protect_mask, &dest) {
                continue;
            }
            collisions.push(abs_path);
        }
    }
    Ok((collisions, symlink_collisions, plib_collisions))
}

/// Real `vardbapi._owners.get_owners()`, narrowed: for each of
/// `collisions`, walks every installed package under `<root>/var/db/
/// pkg` (all categories, all packages -- real portage keeps a
/// persistent reverse index for this; this pilot just scans fresh every
/// time, acceptable for a real, but not performance-critical, error-
/// reporting path only reached when a merge is about to abort anyway)
/// and returns the `category/pf` -> claimed-paths map for whichever
/// ones actually claim it.
fn find_owners(root: &Path, collisions: &[String]) -> BTreeMap<String, Vec<String>> {
    let mut owners: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let pkg_root = root.join("var/db/pkg");
    let Ok(categories) = std::fs::read_dir(&pkg_root) else {
        return owners;
    };
    for category_entry in categories.filter_map(|e| e.ok()) {
        let category_path = category_entry.path();
        if !category_path.is_dir() {
            continue;
        }
        let category_name = category_entry.file_name().to_string_lossy().to_string();
        let Ok(packages) = std::fs::read_dir(&category_path) else {
            continue;
        };
        for pkg_entry in packages.filter_map(|e| e.ok()) {
            let pkg_path = pkg_entry.path();
            if !pkg_path.is_dir() {
                continue;
            }
            let pf = pkg_entry.file_name().to_string_lossy().to_string();
            let Ok(text) = std::fs::read_to_string(pkg_path.join("CONTENTS")) else {
                continue;
            };
            let mut claimed = Vec::new();
            for line in text.lines() {
                let mut parts = line.split_whitespace();
                parts.next();
                if let Some(path) = parts.next() {
                    if collisions.iter().any(|c| c == path) {
                        claimed.push(path.to_string());
                    }
                }
            }
            if !claimed.is_empty() {
                owners
                    .entry(format!("{category_name}/{pf}"))
                    .or_default()
                    .extend(claimed);
            }
        }
    }
    owners
}

/// Real "package NOT merged due to file collisions" abort message,
/// narrowed to what this pilot can cheaply compute (see this module's
/// own module doc comment): every colliding path, annotated with
/// whichever other real installed package(s) `find_owners` found
/// actually claiming it (`(unclaimed)` when none did -- a real,
/// possible outcome: a stray file on disk with no owner at all, real
/// portage's own "None of the installed packages claim the file(s)"
/// case).
fn collision_message(
    root: &Path,
    cpv: &str,
    collisions: &[String],
    symlink_collisions: &[String],
) -> String {
    let mut lines = Vec::new();
    if !symlink_collisions.is_empty() {
        lines.push(format!(
            "Package '{cpv}' NOT merged: one or more collisions between \
             symlinks and directories, forbidden by PMS section 13.4:"
        ));
        for f in symlink_collisions {
            lines.push(format!("\t{f}"));
        }
    }
    if !collisions.is_empty() {
        lines.push(
            "This package will overwrite one or more files that may belong \
             to other packages:"
                .to_string(),
        );
        let owners = find_owners(root, collisions);
        for (owner, paths) in &owners {
            lines.push(format!("{owner}:"));
            for f in paths {
                lines.push(format!("\t{f}"));
            }
        }
        let claimed: std::collections::HashSet<&String> = owners.values().flatten().collect();
        let unclaimed: Vec<&String> = collisions.iter().filter(|f| !claimed.contains(f)).collect();
        if !unclaimed.is_empty() {
            lines.push("(unclaimed):".to_string());
            for f in unclaimed {
                lines.push(format!("\t{f}"));
            }
        }
        lines.push(format!(
            "Package '{cpv}' NOT merged due to file collisions."
        ));
    }
    lines.join("\n")
}

/// Real `merge()`'s own first step is always the real `install` phase
/// chain having already completed (`actionmap_deps["merge"] ==
/// ["install"]`) -- run here directly rather than requiring the caller
/// to have run it first, exactly like `ebuild_phases::run_commands`
/// itself already chains `install`'s own prerequisites automatically.
/// Real `PORTAGE_BUILDDIR`-relative resume markers make re-running an
/// already-done `install` chain cheap, so this is safe to call even when
/// the caller's own command list already ran `install` immediately
/// before `merge` (see `ebuild.rs`'s own dispatch loop).
pub fn run_merge(
    ebuild_path: &Path,
    root: &Path,
    portage_tmpdir: &Path,
    options: &MergeOptions,
) -> Result<i32, String> {
    let status = ebuild_phases::run_commands(
        ebuild_path,
        &["install"],
        root,
        portage_tmpdir,
        &options.distdir,
        options.debug,
        options.shell,
    )?;
    if status != 0 {
        return Ok(status);
    }

    let env = ebuild_phases::compute_environment(ebuild_path, portage_tmpdir)?;
    let ebuild_text = std::fs::read_to_string(&env.ebuild_abs)
        .map_err(|e| format!("{}: {e}", env.ebuild_abs.display()))?;
    let slot = parse_slot(&ebuild_text);
    let repository = repository_name_for(&env.pkg_dir).unwrap_or_else(|| "__unknown__".to_string());

    // Real `merge()`'s own ordering: the collision-protect abort check
    // (`_collision_protect`) happens before `pkg_preinst` ever runs, not
    // after -- confirmed by reading it, the real `EbuildPhase(phase=
    // "preinst")` block sits strictly after the real `if abort: return
    // 1` check. The preserve-libs registry is consulted unconditionally
    // here too (real `_plib_registry` is never `None` in practice --
    // see this module's own doc comment), regardless of
    // `FEATURES=collision-protect`.
    let plib_registry = read_plib_registry(root);
    let plib_inodes = plib_inode_map(root, &plib_registry.preserved_libs());
    // Real `mypkglist = others_in_slot + blockers` -- see
    // `blocked_installed_packages`'s own doc comment for the full real
    // grounding (this is genuinely new machinery: `ebuild <file> merge`
    // has never resolved real config/USE at all before this).
    let blocked = blocked_installed_packages(root, &options.config_root, &env, &slot, &repository);
    let (collisions, symlink_collisions, plib_collisions) = find_collisions(
        &env.d(),
        root,
        &env.category,
        &env.split.pn,
        &slot,
        &options.config_protect,
        &options.config_protect_mask,
        &plib_inodes,
        &blocked,
    )?;
    // Real `dblink.merge()`'s own abort condition (`vartree.py:4830-
    // 4838`, Python operator precedence: `collision_protect or
    // (protect_owned and owners)`): a symlink-over-directory violation
    // always aborts; otherwise `collision_protect` alone aborts on any
    // collision, but `protect_owned` alone only aborts when an actual
    // owning package was identified for at least one collision (real
    // "None of the installed packages claim the file(s)" case does
    // *not* abort under `protect_owned` alone). `find_owners` is only
    // computed here (a second time, alongside `collision_message`'s own
    // call) when `protect_owned` might actually need it -- matching real
    // `get_owners()` itself only running when `collision_protect or
    // protect_owned or symlink_collisions`.
    let protect_owned_abort = options.protect_owned
        && !collisions.is_empty()
        && !find_owners(root, &collisions).is_empty();
    if !symlink_collisions.is_empty()
        || (options.collision_protect && !collisions.is_empty())
        || protect_owned_abort
    {
        let cpv = format!("{}/{}", env.category, env.split.pf);
        return Err(collision_message(
            root,
            &cpv,
            &collisions,
            &symlink_collisions,
        ));
    }

    // Real `dblink.treewalk()`'s own order: `pkg_preinst` runs before
    // anything is copied, `pkg_postinst` only after the vdb entry is
    // fully written -- `run_single_phase` (not `run_commands`) since
    // neither is part of `install`'s own `actionmap_deps` chain (real
    // `treewalk()` invokes them directly, not through `doebuild()`).
    let preinst_status = ebuild_phases::run_single_phase(
        ebuild_path,
        "preinst",
        root,
        portage_tmpdir,
        options.debug,
        options.shell,
    )?;
    if preinst_status != 0 {
        return Ok(preinst_status);
    }

    let mut cfgfiledict = read_cfgfiledict(root);
    let contents = merge_tree(
        &env.d(),
        root,
        &options.config_protect,
        &options.config_protect_mask,
        &mut cfgfiledict,
    )?;
    write_cfgfiledict(root, &cfgfiledict)?;
    write_vdb_entry(root, &env, &slot, &repository, &contents)?;

    if !plib_collisions.is_empty() {
        let cpv = format!("{}/{}", env.category, env.split.pf);
        unregister_preserved_libs(root, &cpv, plib_registry, &plib_collisions)?;
    }

    // Real `merge()`'s own ordering: `postinst` runs, but its own exit
    // status never gates anything after it ("It's stupid to bail out
    // here, so keep going regardless of phase return code") -- real
    // `env_update()` always runs next, as long as anything was actually
    // installed (real `if contents:`).
    let postinst_status = ebuild_phases::run_single_phase(
        ebuild_path,
        "postinst",
        root,
        portage_tmpdir,
        options.debug,
        options.shell,
    )?;

    if !contents.is_empty() {
        env_update::run_env_update(root)?;
    }

    Ok(postinst_status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_real_merge_command_covers_exactly_merge() {
        assert!(is_real_merge_command("merge"));
        assert!(!is_real_merge_command("qmerge"));
        assert!(!is_real_merge_command("unmerge"));
        assert!(!is_real_merge_command("install"));
    }

    #[test]
    fn parse_slot_reads_a_literal_assignment_anywhere_in_the_file() {
        assert_eq!(parse_slot("EAPI=8\nSLOT=\"0\"\n"), "0");
        assert_eq!(parse_slot("SLOT=\"2/5\"\n"), "2/5");
        assert_eq!(parse_slot("SLOT='1'\n"), "1");
        assert_eq!(parse_slot("SLOT=0\n"), "0");
    }

    #[test]
    fn parse_slot_defaults_to_0_when_missing() {
        assert_eq!(parse_slot("EAPI=8\nDESCRIPTION=x\n"), "0");
        assert_eq!(parse_slot(""), "0");
    }

    #[test]
    fn format_contents_line_matches_real_dblink_format() {
        assert_eq!(
            format_contents_line("dir", "/usr/share/x", None, None, None),
            "dir /usr/share/x\n"
        );
        assert_eq!(
            format_contents_line("obj", "/usr/share/x/f", Some("abc123"), None, Some(100)),
            "obj /usr/share/x/f abc123 100\n"
        );
        assert_eq!(
            format_contents_line("sym", "/usr/lib/x.so", None, Some("x.so.1"), Some(100)),
            "sym /usr/lib/x.so -> x.so.1 100\n"
        );
    }

    #[test]
    fn repository_name_for_finds_the_nearest_ancestor_repo_name() {
        let tmp = tempdir();
        let repo = tmp.join("myrepo");
        let pkg_dir = repo.join("dev-libs/foo");
        std::fs::create_dir_all(repo.join("profiles")).unwrap();
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(repo.join("profiles/repo_name"), "myrepo\n").unwrap();
        assert_eq!(repository_name_for(&pkg_dir), Some("myrepo".to_string()));
    }

    #[test]
    fn repository_name_for_is_none_when_no_ancestor_has_one() {
        let tmp = tempdir();
        let pkg_dir = tmp.join("dev-libs/foo");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        assert_eq!(repository_name_for(&pkg_dir), None);
    }

    #[test]
    fn merge_tree_copies_files_dirs_and_symlinks_and_writes_matching_contents() {
        let tmp = tempdir();
        let d = tmp.join("D");
        let root = tmp.join("ROOT");
        std::fs::create_dir_all(d.join("usr/share/x")).unwrap();
        std::fs::write(d.join("usr/share/x/hello.txt"), b"hello").unwrap();
        std::os::unix::fs::symlink("hello.txt", d.join("usr/share/x/link.txt")).unwrap();
        std::fs::create_dir_all(&root).unwrap();

        let mut cfgfiledict = BTreeMap::new();
        let contents = merge_tree(&d, &root, "/etc", "/etc/env.d", &mut cfgfiledict)
            .expect("merge_tree succeeds");

        assert!(root.join("usr/share/x/hello.txt").is_file());
        assert_eq!(
            std::fs::read_to_string(root.join("usr/share/x/hello.txt")).unwrap(),
            "hello"
        );
        assert_eq!(
            std::fs::read_link(root.join("usr/share/x/link.txt")).unwrap(),
            PathBuf::from("hello.txt")
        );

        assert!(contents.contains("dir /usr\n"));
        assert!(contents.contains("dir /usr/share\n"));
        assert!(contents.contains("dir /usr/share/x\n"));
        assert!(contents
            .lines()
            .any(|l| l.starts_with("obj /usr/share/x/hello.txt ")));
        assert!(contents.contains("sym /usr/share/x/link.txt -> hello.txt"));
    }

    #[test]
    fn is_protected_matches_only_under_a_real_protected_directory() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("etc")).unwrap();

        assert!(is_protected(&root, "/etc", "", &root.join("etc/foo.conf")));
        assert!(is_protected(&root, "/etc", "", &root.join("etc")));
        // Real bug #379899-adjacent case: "/etc" must not match
        // "/etcfoobaz" just because it's a string prefix.
        assert!(!is_protected(&root, "/etc", "", &root.join("etcfoobaz")));
        assert!(!is_protected(&root, "/etc", "", &root.join("var/foo")));
    }

    #[test]
    fn is_protected_a_literal_file_entry_matches_only_exactly() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(root.join("etc/single.conf"), b"x").unwrap();

        assert!(is_protected(
            &root,
            "/etc/single.conf",
            "",
            &root.join("etc/single.conf")
        ));
        assert!(!is_protected(
            &root,
            "/etc/single.conf",
            "",
            &root.join("etc/other.conf")
        ));
    }

    #[test]
    fn is_protected_respects_mask_exclusion_via_longest_prefix() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("etc/env.d")).unwrap();

        assert!(is_protected(
            &root,
            "/etc",
            "/etc/env.d",
            &root.join("etc/foo.conf")
        ));
        // Masked: /etc/env.d is a longer, more specific match than /etc.
        assert!(!is_protected(
            &root,
            "/etc",
            "/etc/env.d",
            &root.join("etc/env.d/10-foo")
        ));
    }

    #[test]
    fn new_protect_filename_allocates_sequential_numbers() {
        let tmp = tempdir();
        std::fs::create_dir_all(&tmp).unwrap();
        let dest = tmp.join("foo.conf");

        assert_eq!(
            new_protect_filename(&dest).unwrap(),
            tmp.join("._cfg0000_foo.conf")
        );

        std::fs::write(tmp.join("._cfg0000_foo.conf"), b"x").unwrap();
        assert_eq!(
            new_protect_filename(&dest).unwrap(),
            tmp.join("._cfg0001_foo.conf")
        );

        std::fs::write(tmp.join("._cfg0007_foo.conf"), b"x").unwrap();
        assert_eq!(
            new_protect_filename(&dest).unwrap(),
            tmp.join("._cfg0008_foo.conf")
        );

        // A same-prefixed file for a *different* basename doesn't count.
        std::fs::write(tmp.join("._cfg0099_other.conf"), b"x").unwrap();
        assert_eq!(
            new_protect_filename(&dest).unwrap(),
            tmp.join("._cfg0008_foo.conf")
        );
    }

    #[test]
    fn merge_tree_does_not_protect_a_brand_new_file() {
        let tmp = tempdir();
        let d = tmp.join("D");
        let root = tmp.join("ROOT");
        std::fs::create_dir_all(d.join("etc")).unwrap();
        std::fs::write(d.join("etc/foo.conf"), b"new content").unwrap();
        std::fs::create_dir_all(&root).unwrap();

        let mut cfgfiledict = BTreeMap::new();
        merge_tree(&d, &root, "/etc", "", &mut cfgfiledict).expect("merge_tree succeeds");

        assert_eq!(
            std::fs::read_to_string(root.join("etc/foo.conf")).unwrap(),
            "new content"
        );
        assert!(!root.join("etc/._cfg0000_foo.conf").exists());
    }

    #[test]
    fn merge_tree_leaves_an_unchanged_protected_file_alone() {
        let tmp = tempdir();
        let d = tmp.join("D");
        let root = tmp.join("ROOT");
        std::fs::create_dir_all(d.join("etc")).unwrap();
        std::fs::write(d.join("etc/foo.conf"), b"same content").unwrap();
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(root.join("etc/foo.conf"), b"same content").unwrap();

        let mut cfgfiledict = BTreeMap::new();
        merge_tree(&d, &root, "/etc", "", &mut cfgfiledict).expect("merge_tree succeeds");

        assert_eq!(
            std::fs::read_to_string(root.join("etc/foo.conf")).unwrap(),
            "same content"
        );
        assert!(!root.join("etc/._cfg0000_foo.conf").exists());
    }

    #[test]
    fn merge_tree_protects_a_changed_file_under_a_protected_path() {
        let tmp = tempdir();
        let d = tmp.join("D");
        let root = tmp.join("ROOT");
        std::fs::create_dir_all(d.join("etc")).unwrap();
        std::fs::write(d.join("etc/foo.conf"), b"new content").unwrap();
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(root.join("etc/foo.conf"), b"user's own edits").unwrap();

        let mut cfgfiledict = BTreeMap::new();
        let contents =
            merge_tree(&d, &root, "/etc", "", &mut cfgfiledict).expect("merge_tree succeeds");

        // The real, logical path is untouched...
        assert_eq!(
            std::fs::read_to_string(root.join("etc/foo.conf")).unwrap(),
            "user's own edits"
        );
        // ...and the new content lands in a ._cfg0000_ sibling instead.
        assert_eq!(
            std::fs::read_to_string(root.join("etc/._cfg0000_foo.conf")).unwrap(),
            "new content"
        );
        // CONTENTS still records the logical path with the *new*
        // content's own MD5 (real dblink.mergeme()'s own behavior --
        // see merge_tree's own doc comment).
        let new_md5 = md5_hex(&d.join("etc/foo.conf")).unwrap();
        assert!(contents
            .lines()
            .any(|l| l.starts_with(&format!("obj /etc/foo.conf {new_md5} "))));
        assert_eq!(cfgfiledict.get("/etc/foo.conf"), Some(&new_md5));
    }

    #[test]
    fn merge_tree_remembers_an_already_offered_update_and_stops_re_protecting_it() {
        let tmp = tempdir();
        let d = tmp.join("D");
        let root = tmp.join("ROOT");
        std::fs::create_dir_all(d.join("etc")).unwrap();
        std::fs::write(d.join("etc/foo.conf"), b"new content").unwrap();
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(root.join("etc/foo.conf"), b"user's own edits").unwrap();

        let mut cfgfiledict = BTreeMap::new();
        merge_tree(&d, &root, "/etc", "", &mut cfgfiledict).expect("first merge_tree succeeds");
        assert!(root.join("etc/._cfg0000_foo.conf").exists());

        // Re-merging the exact same new content again: already
        // remembered in cfgfiledict, so this time it's applied directly
        // -- no second ._cfg0001_ file spawned.
        merge_tree(&d, &root, "/etc", "", &mut cfgfiledict).expect("second merge_tree succeeds");
        assert_eq!(
            std::fs::read_to_string(root.join("etc/foo.conf")).unwrap(),
            "new content"
        );
        assert!(!root.join("etc/._cfg0001_foo.conf").exists());
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "portuale-ebuild-merge-test-{}-{}",
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
    fn real_merge_lands_files_and_a_symlink_and_writes_a_real_vdb_entry() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/mergepkg/mergepkg-1.0.ebuild");

        let status = run_merge(&ebuild, &root, &portage_tmpdir, &MergeOptions::default())
            .expect("run_merge succeeds");
        assert_eq!(status, 0);

        assert!(root.join("usr/share/mergepkg/hello.txt").is_file());
        assert_eq!(
            std::fs::read_to_string(root.join("usr/share/mergepkg/hello.txt"))
                .unwrap()
                .trim(),
            "hello from mergepkg"
        );
        let link = root.join("usr/share/mergepkg/hello-link.txt");
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            PathBuf::from("hello.txt")
        );

        let vdb_dir = root.join("var/db/pkg/dev-libs/mergepkg-1.0");
        assert_eq!(
            std::fs::read_to_string(vdb_dir.join("CATEGORY"))
                .unwrap()
                .trim(),
            "dev-libs"
        );
        assert_eq!(
            std::fs::read_to_string(vdb_dir.join("SLOT"))
                .unwrap()
                .trim(),
            "0"
        );
        assert_eq!(
            std::fs::read_to_string(vdb_dir.join("repository"))
                .unwrap()
                .trim(),
            "testrepo"
        );
        let contents = std::fs::read_to_string(vdb_dir.join("CONTENTS")).unwrap();
        assert!(contents
            .lines()
            .any(|l| l.starts_with("obj /usr/share/mergepkg/hello.txt ")));
        assert!(contents.contains("sym /usr/share/mergepkg/hello-link.txt -> hello.txt"));

        let counter: i64 = std::fs::read_to_string(vdb_dir.join("COUNTER"))
            .unwrap()
            .parse()
            .expect("COUNTER is a bare integer");
        assert!(counter >= 0);

        // Real dblink.merge()'s own atomic dbtmpdir-then-rename: no
        // MERGING_IDENTIFIER-prefixed temp directory should survive a
        // successful merge.
        assert!(!root
            .join("var/db/pkg/dev-libs/-MERGING-mergepkg-1.0")
            .exists());

        // Real pkg_preinst/pkg_postinst ordering proof: the fixture's own
        // hooks only touch these markers if, respectively, the merged
        // file was *not yet* visible under ${ROOT} (preinst) and *was
        // already* visible, vdb entry included (postinst) -- see
        // mergepkg-1.0.ebuild's own pkg_preinst/pkg_postinst.
        let t_dir = portage_tmpdir.join("portage/dev-libs/mergepkg-1.0/temp");
        assert!(
            t_dir.join("preinst-ran-before-merge").is_file(),
            "pkg_preinst must run, and see the file not yet merged"
        );
        assert!(
            t_dir.join("postinst-ran-after-merge").is_file(),
            "pkg_postinst must run, and see the file (and vdb entry) already merged"
        );
    }

    #[test]
    fn re_merging_the_same_package_replaces_the_vdb_entry_and_bumps_counter() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/mergepkg/mergepkg-1.0.ebuild");
        let vdb_dir = root.join("var/db/pkg/dev-libs/mergepkg-1.0");

        assert_eq!(
            run_merge(&ebuild, &root, &portage_tmpdir, &MergeOptions::default()).unwrap(),
            0
        );
        let first_counter: i64 = std::fs::read_to_string(vdb_dir.join("COUNTER"))
            .unwrap()
            .parse()
            .unwrap();

        assert_eq!(
            run_merge(&ebuild, &root, &portage_tmpdir, &MergeOptions::default()).unwrap(),
            0
        );
        let second_counter: i64 = std::fs::read_to_string(vdb_dir.join("COUNTER"))
            .unwrap()
            .parse()
            .unwrap();

        assert!(second_counter > first_counter);
        // Still a single, intact entry -- not a leftover-plus-new-copy.
        assert!(root.join("usr/share/mergepkg/hello.txt").is_file());
        assert!(!root
            .join("var/db/pkg/dev-libs/-MERGING-mergepkg-1.0")
            .exists());
    }

    #[test]
    fn real_merge_protects_a_locally_modified_etc_file_via_the_full_cli_path() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();
        // Simulate a pre-existing, locally-modified /etc file -- as if
        // this package (or an earlier version of it) had installed a
        // default that the admin then edited by hand.
        std::fs::write(root.join("etc/configpkg.conf"), b"admin's own edits\n").unwrap();

        let repo_root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/configpkg/configpkg-1.0.ebuild");

        let status = run_merge(&ebuild, &root, &portage_tmpdir, &MergeOptions::default())
            .expect("run_merge succeeds");
        assert_eq!(status, 0);

        // The real, logical /etc/configpkg.conf is never touched.
        assert_eq!(
            std::fs::read_to_string(root.join("etc/configpkg.conf")).unwrap(),
            "admin's own edits\n"
        );
        // The new content the ebuild wanted to install lands in a real
        // ._cfg0000_ sibling instead.
        assert_eq!(
            std::fs::read_to_string(root.join("etc/._cfg0000_configpkg.conf")).unwrap(),
            "new content from configpkg\n"
        );
        // The vdb's own CONTENTS still considers /etc/configpkg.conf
        // (the logical path) this package's own -- not the ._cfg
        // variant.
        let contents =
            std::fs::read_to_string(root.join("var/db/pkg/dev-libs/configpkg-1.0/CONTENTS"))
                .unwrap();
        assert!(contents
            .lines()
            .any(|l| l.starts_with("obj /etc/configpkg.conf ")));
        assert!(!contents.contains("._cfg0000_configpkg.conf"));
    }

    fn collision_fixture(name: &str) -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs")
            .join(name)
            .join(format!("{name}-1.0.ebuild"))
    }

    /// `FEATURES=collision-protect` off (`MergeOptions::default()`):
    /// real portage's own default behavior -- an ordinary file
    /// collision is merged over anyway (`collisionpkg-c` overwrites
    /// `collisionpkg-a`'s own `shared.txt`), matching every pre-existing
    /// test in this file that never set `collision_protect` at all.
    #[test]
    fn ordinary_collision_is_merged_over_when_collision_protect_is_off() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        run_merge(
            &collision_fixture("collisionpkg-a"),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
        )
        .expect("collisionpkg-a merges cleanly");

        let status = run_merge(
            &collision_fixture("collisionpkg-c"),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
        )
        .expect("run_merge should not itself error");
        assert_eq!(status, 0);
        assert_eq!(
            std::fs::read_to_string(root.join("usr/share/collisiontest/shared.txt")).unwrap(),
            "hello from collisionpkg-c\n"
        );
    }

    /// `FEATURES=collision-protect` on: real `dblink._collision_
    /// protect`'s own abort -- `collisionpkg-c` would overwrite
    /// `collisionpkg-a`'s own, different-package `shared.txt`, so the
    /// merge aborts *before* writing anything (the file is left exactly
    /// as `collisionpkg-a` installed it) and the error names
    /// `collisionpkg-a` as the real owning package (`find_owners`).
    #[test]
    fn ordinary_collision_aborts_the_merge_and_names_the_owner_when_collision_protect_is_on() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        run_merge(
            &collision_fixture("collisionpkg-a"),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
        )
        .expect("collisionpkg-a merges cleanly");

        let options = MergeOptions {
            collision_protect: true,
            ..MergeOptions::default()
        };
        let err = run_merge(
            &collision_fixture("collisionpkg-c"),
            &root,
            &portage_tmpdir,
            &options,
        )
        .expect_err("collision-protect should abort the merge");
        assert!(err.contains("dev-libs/collisionpkg-a-1.0"), "{err}");
        assert!(err.contains("/usr/share/collisiontest/shared.txt"), "{err}");
        assert!(err.contains("NOT merged due to file collisions"), "{err}");

        // Nothing was written: the file is still collisionpkg-a's own,
        // and collisionpkg-c's own vdb entry was never created.
        assert_eq!(
            std::fs::read_to_string(root.join("usr/share/collisiontest/shared.txt")).unwrap(),
            "hello from collisionpkg-a\n"
        );
        assert!(!root.join("var/db/pkg/dev-libs/collisionpkg-c-1.0").exists());
    }

    /// Real PMS 13.4's own symlink-over-directory ban: unconditional,
    /// regardless of `FEATURES` -- `collisionpkg-b` installs a symlink
    /// exactly where `collisionpkg-a` already installed a real
    /// directory (`adir`), which aborts the merge even with
    /// `collision_protect: false` (`MergeOptions::default()`).
    #[test]
    fn symlink_over_directory_always_aborts_regardless_of_collision_protect() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        run_merge(
            &collision_fixture("collisionpkg-a"),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
        )
        .expect("collisionpkg-a merges cleanly");

        let err = run_merge(
            &collision_fixture("collisionpkg-b"),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
        )
        .expect_err("a symlink-over-directory violation should always abort");
        assert!(err.contains("PMS section 13.4"), "{err}");
        assert!(err.contains("/usr/share/collisiontest/adir"), "{err}");

        // The real directory collisionpkg-a installed is still a real
        // directory -- never replaced by collisionpkg-b's own symlink.
        assert!(root.join("usr/share/collisiontest/adir").is_dir());
    }

    /// `FEATURES=protect-owned` alone (no `collision-protect`): real
    /// `dblink.merge()`'s own separate abort condition aborts once
    /// `find_owners` actually identifies an owning package for the
    /// collision -- `collisionpkg-c` colliding with `collisionpkg-a`'s
    /// own, different-package `shared.txt` is exactly that case.
    #[test]
    fn protect_owned_alone_aborts_when_an_owner_is_identified() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        run_merge(
            &collision_fixture("collisionpkg-a"),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
        )
        .expect("collisionpkg-a merges cleanly");

        let options = MergeOptions {
            protect_owned: true,
            ..MergeOptions::default()
        };
        let err = run_merge(
            &collision_fixture("collisionpkg-c"),
            &root,
            &portage_tmpdir,
            &options,
        )
        .expect_err("protect-owned alone should abort once an owner is identified");
        assert!(err.contains("dev-libs/collisionpkg-a-1.0"), "{err}");
        assert!(err.contains("/usr/share/collisiontest/shared.txt"), "{err}");
        assert!(err.contains("NOT merged due to file collisions"), "{err}");

        assert_eq!(
            std::fs::read_to_string(root.join("usr/share/collisiontest/shared.txt")).unwrap(),
            "hello from collisionpkg-a\n"
        );
    }

    /// Real "None of the installed packages claim the file(s)" case:
    /// `FEATURES=protect-owned` alone must *not* abort when the
    /// colliding destination is a stray file with no owning vdb entry
    /// at all -- the distinguishing behavior from `collision-protect`,
    /// which would abort unconditionally on any collision.
    #[test]
    fn protect_owned_alone_does_not_abort_an_unclaimed_stray_file() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(root.join("usr/share/collisiontest")).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        std::fs::write(
            root.join("usr/share/collisiontest/shared.txt"),
            "a stray, unowned file\n",
        )
        .unwrap();

        let options = MergeOptions {
            protect_owned: true,
            ..MergeOptions::default()
        };
        let status = run_merge(
            &collision_fixture("collisionpkg-c"),
            &root,
            &portage_tmpdir,
            &options,
        )
        .expect("protect-owned alone must not abort an unclaimed collision");
        assert_eq!(status, 0);
        assert_eq!(
            std::fs::read_to_string(root.join("usr/share/collisiontest/shared.txt")).unwrap(),
            "hello from collisionpkg-c\n"
        );
    }

    #[test]
    fn plib_registry_round_trips_through_json() {
        let mut entries = BTreeMap::new();
        entries.insert(
            "dev-libs/preservepkg-old:0".to_string(),
            (
                "dev-libs/preservepkg-old-1.0".to_string(),
                "5".to_string(),
                vec![
                    "/usr/lib/preservedtest/libfoo.so.1".to_string(),
                    "/usr/lib/preservedtest/libfoo.so".to_string(),
                ],
            ),
        );
        let tmp = tempdir();
        write_plib_registry(
            &tmp,
            &PlibRegistry {
                entries: entries.clone(),
            },
        )
        .expect("write succeeds");

        let text = std::fs::read_to_string(plib_registry_path(&tmp)).unwrap();
        let parsed = parse_plib_registry(&text).expect("real json.dumps-shaped output parses back");
        assert_eq!(parsed, entries);
    }

    #[test]
    fn read_plib_registry_degrades_gracefully_when_missing_or_corrupt() {
        let tmp = tempdir();
        // No file at all -- real load()'s own ENOENT -> {} degrade.
        assert!(read_plib_registry(&tmp).entries.is_empty());

        std::fs::create_dir_all(plib_registry_path(&tmp).parent().unwrap()).unwrap();
        std::fs::write(plib_registry_path(&tmp), b"not json at all").unwrap();
        assert!(read_plib_registry(&tmp).entries.is_empty());
    }

    #[test]
    fn plib_inode_map_skips_paths_that_no_longer_exist_on_disk() {
        let tmp = tempdir();
        std::fs::create_dir_all(tmp.join("usr/lib")).unwrap();
        std::fs::write(tmp.join("usr/lib/real.so"), b"x").unwrap();

        let mut preserved = BTreeMap::new();
        preserved.insert(
            "dev-libs/foo-1.0".to_string(),
            vec![
                "/usr/lib/real.so".to_string(),
                "/usr/lib/gone.so".to_string(),
            ],
        );
        let map = plib_inode_map(&tmp, &preserved);
        assert_eq!(
            map.len(),
            1,
            "only the still-existing path gets an inode entry"
        );
        let meta = std::fs::symlink_metadata(tmp.join("usr/lib/real.so")).unwrap();
        assert_eq!(
            map.get(&(meta.dev(), meta.ino())),
            Some(&vec![(
                "dev-libs/foo-1.0".to_string(),
                "/usr/lib/real.so".to_string()
            )])
        );
    }

    /// Sanity baseline (this pilot's own "fixtures must actually
    /// distinguish the new behavior" rule): with no preserve-libs
    /// registry entry at all, `preservepkg-new` colliding with
    /// `preservepkg-old` on the exact same path is an ordinary
    /// collision-protect abort, same as `collisionpkg-a`/`-c` above --
    /// proving the fixture pair is a genuine collision before the next
    /// test shows the registry excluding it.
    #[test]
    fn preservepkg_new_collides_ordinarily_without_a_registry_entry() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        run_merge(
            &collision_fixture("preservepkg-old"),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
        )
        .expect("preservepkg-old merges cleanly");

        let options = MergeOptions {
            collision_protect: true,
            ..MergeOptions::default()
        };
        let err = run_merge(
            &collision_fixture("preservepkg-new"),
            &root,
            &portage_tmpdir,
            &options,
        )
        .expect_err("without a registry entry this is an ordinary collision");
        assert!(err.contains("dev-libs/preservepkg-old-1.0"), "{err}");
        assert!(err.contains("/usr/lib/preservedtest/libfoo.so.1"), "{err}");
    }

    /// Real `_collision_protect`'s own preserve-libs exclusion: with
    /// `preservepkg-old`'s own already-merged file registered in
    /// `preserved_libs_registry` (hand-seeded here -- this pilot has no
    /// registration/detection side yet, see this module's own doc
    /// comment), `preservepkg-new` colliding on that exact path is
    /// excluded from collision-protect's abort entirely (even with
    /// `collision_protect: true`) and takes over the file; afterwards
    /// the registry no longer lists the path (its only entry, so the
    /// whole `cp:slot` key is dropped) and `preservepkg-old`'s own vdb
    /// `CONTENTS` no longer claims it either.
    #[test]
    fn preserve_libs_registry_entry_excludes_the_collision_and_hands_ownership_over() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        run_merge(
            &collision_fixture("preservepkg-old"),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
        )
        .expect("preservepkg-old merges cleanly");

        let mut entries = BTreeMap::new();
        entries.insert(
            "dev-libs/preservepkg-old:0".to_string(),
            (
                "dev-libs/preservepkg-old-1.0".to_string(),
                "0".to_string(),
                vec!["/usr/lib/preservedtest/libfoo.so.1".to_string()],
            ),
        );
        write_plib_registry(&root, &PlibRegistry { entries })
            .expect("seeding the registry succeeds");

        let options = MergeOptions {
            collision_protect: true,
            ..MergeOptions::default()
        };
        let status = run_merge(
            &collision_fixture("preservepkg-new"),
            &root,
            &portage_tmpdir,
            &options,
        )
        .expect("a preserved-lib collision is excluded, not aborted");
        assert_eq!(status, 0);

        assert_eq!(
            std::fs::read_to_string(root.join("usr/lib/preservedtest/libfoo.so.1")).unwrap(),
            "new library content\n"
        );

        // preservepkg-old's own CONTENTS no longer claims the path
        // preservepkg-new just took over.
        let old_contents =
            std::fs::read_to_string(root.join("var/db/pkg/dev-libs/preservepkg-old-1.0/CONTENTS"))
                .unwrap();
        assert!(!old_contents.contains("/usr/lib/preservedtest/libfoo.so.1"));

        // The registry entry is gone entirely -- it had exactly one
        // path, and that path is no longer preserved.
        let registry_text = std::fs::read_to_string(plib_registry_path(&root)).unwrap();
        let registry = parse_plib_registry(&registry_text).unwrap();
        assert!(registry.is_empty());
    }

    fn env_update_fixture() -> PathBuf {
        collision_fixture("envupdatepkg")
    }

    /// Real `merge()`'s own ordering: `env_update()` runs after
    /// `postinst`, so it sees the merge's *own* just-installed
    /// `/etc/env.d/50-envupdatetest` (the fixture installs its own env.d
    /// entry, not a separately-merged package's) and regenerates
    /// `/etc/profile.env`/`/etc/csh.env`/`/etc/environment.d/
    /// 10-gentoo-env.conf`/`/etc/ld.so.conf` from it.
    #[test]
    fn real_merge_regenerates_env_update_outputs_from_its_own_env_d_entry() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let status = run_merge(
            &env_update_fixture(),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
        )
        .expect("run_merge succeeds");
        assert_eq!(status, 0);

        assert!(root.join("etc/env.d/50-envupdatetest").is_file());

        let ld_so_conf = std::fs::read_to_string(root.join("etc/ld.so.conf")).unwrap();
        assert!(
            ld_so_conf.contains("/usr/lib/envupdatetest"),
            "{ld_so_conf}"
        );

        let profile_env = std::fs::read_to_string(root.join("etc/profile.env")).unwrap();
        assert!(
            profile_env.contains("export ENVUPDATETEST_VAR='hello from envupdatetest'"),
            "{profile_env}"
        );
        // LDPATH itself never appears in profile.env -- only ld.so.conf.
        assert!(!profile_env.contains("LDPATH"));

        let csh_env = std::fs::read_to_string(root.join("etc/csh.env")).unwrap();
        assert!(
            csh_env.contains("setenv ENVUPDATETEST_VAR 'hello from envupdatetest'"),
            "{csh_env}"
        );

        let systemd_env =
            std::fs::read_to_string(root.join("etc/environment.d/10-gentoo-env.conf")).unwrap();
        assert!(
            systemd_env.contains("ENVUPDATETEST_VAR=hello from envupdatetest"),
            "{systemd_env}"
        );
    }

    /// Real `env_update()` invokes the *target `ROOT`'s own*
    /// `<ROOT>/sbin/ldconfig` (never a host `PATH` lookup -- see
    /// `env_update.rs`'s own module doc comment). Seeding a fake,
    /// marker-writing executable there before merging proves this
    /// pilot's own real subprocess invocation, the same "prove it with a
    /// marker file" style already used for `pkg_preinst`/`pkg_postinst`
    /// ordering elsewhere in this file.
    #[test]
    fn real_merge_invokes_a_real_root_scoped_ldconfig_when_one_is_present() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        std::fs::create_dir_all(root.join("sbin")).unwrap();
        std::fs::write(
            root.join("sbin/ldconfig"),
            "#!/bin/sh\necho \"$@\" > \"$3/ldconfig-was-invoked\"\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            root.join("sbin/ldconfig"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();

        let status = run_merge(
            &env_update_fixture(),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
        )
        .expect("run_merge succeeds");
        assert_eq!(status, 0);

        let marker = std::fs::read_to_string(root.join("ldconfig-was-invoked"))
            .expect("the real ROOT-scoped ldconfig binary was really invoked");
        assert!(marker.contains("-X"), "{marker}");
        assert!(marker.contains("-r"), "{marker}");
    }

    fn fixtures_root() -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
    }

    /// Sanity baseline (this pilot's own "fixtures must actually
    /// distinguish the new behavior" rule): with `MergeOptions::default()`
    /// (its own deliberately-inert `config_root` sentinel, see that
    /// field's own doc comment), real config/USE resolution never even
    /// attempts, so `blocked_installed_packages` degrades to an empty
    /// set -- `mergeblockerpkg` colliding with `mergeblockedbypkg` on
    /// `/usr/share/mergeblockertest/shared.txt` is an ordinary
    /// collision-protect abort, exactly like `collisionpkg-a`/`-c`
    /// above, proving the fixture pair is a genuine collision before the
    /// next test shows real blocker resolution excluding it.
    #[test]
    fn mergeblockerpkg_collides_ordinarily_without_config_resolution() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        run_merge(
            &collision_fixture("mergeblockedbypkg"),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
        )
        .expect("mergeblockedbypkg merges cleanly");

        let options = MergeOptions {
            collision_protect: true,
            ..MergeOptions::default()
        };
        let err = run_merge(
            &collision_fixture("mergeblockerpkg"),
            &root,
            &portage_tmpdir,
            &options,
        )
        .expect_err("without config resolution this is an ordinary collision");
        assert!(err.contains("dev-libs/mergeblockedbypkg-1.0"), "{err}");
        assert!(
            err.contains("/usr/share/mergeblockertest/shared.txt"),
            "{err}"
        );
    }

    /// Real `mypkglist = others_in_slot + blockers`: `mergeblockerpkg`'s own
    /// real `RDEPEND="!dev-libs/mergeblockedbypkg"` -- flattened via real
    /// config/USE resolution rooted at `config_root` (the real
    /// `PORTING/fixtures` tree, which has its own real `repos.conf`) --
    /// matches the already-installed `mergeblockedbypkg`, so the collision on
    /// `/usr/share/mergeblockertest/shared.txt` is excluded even with
    /// `collision_protect: true`, and `mergeblockerpkg` takes over the file.
    #[test]
    fn mergeblockerpkg_excludes_the_collision_via_a_real_blocker_atom() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        run_merge(
            &collision_fixture("mergeblockedbypkg"),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
        )
        .expect("mergeblockedbypkg merges cleanly");

        let options = MergeOptions {
            collision_protect: true,
            config_root: fixtures_root(),
            ..MergeOptions::default()
        };
        let status = run_merge(
            &collision_fixture("mergeblockerpkg"),
            &root,
            &portage_tmpdir,
            &options,
        )
        .expect("a blocker-excluded collision is not an abort");
        assert_eq!(status, 0);
        assert_eq!(
            std::fs::read_to_string(root.join("usr/share/mergeblockertest/shared.txt")).unwrap(),
            "hello from mergeblockerpkg\n"
        );
    }
}
