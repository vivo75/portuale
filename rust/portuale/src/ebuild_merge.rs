// Real merge/filesystem mutation (task #55, `docs/agent-context.md`'s own
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
// CONFIG_PROTECT is real too, for both `obj` (regular file) and `sym`
// (symlink -- real bug #485598: the *target string*'s own MD5 is what's
// compared, not file content) entries: real `ConfigProtect.isprotected()`
// path matching (`is_protected`), the real MD5-comparison
// rename-instead-of-overwrite decision (real `dblink._protect()`, into
// the next `._cfgNNNN_<name>` sibling -- real `new_protect_filename()`,
// including its own "reuse the last `._cfgNNNN_` file when its own
// content/target already matches" logic), and real
// `vardbapi._conf_mem_file` persistence
// (`read_cfgfiledict`/`write_cfgfiledict`) so a repeat merge of an
// already-offered update doesn't spawn a fresh `._cfgNNNN_` file every
// time -- unless `NOCONFMEM` is set (real `--noconfmem`: an `emerge`-only
// CLI flag, real `lib/_emerge/actions.py:2790`, that lands as
// `settings["NOCONFMEM"]`, real `vartree.py:4949`'s own `cfgfiledict[
// "IGNORE"]`; real `bin/ebuild` has no such flag at all, so this pilot
// reads the env var directly, the same "env var, not full config
// resolution" shortcut `CONFIG_PROTECT` itself already uses), which
// forces every already-offered update to be re-protected into a fresh
// `._cfgNNNN_` file regardless of memory. `CONFIG_PROTECT`/
// `CONFIG_PROTECT_MASK`/`NOCONFMEM` are read via env vars at the
// `ebuild.rs` CLI boundary (bundled into `MergeOptions`, deliberately a
// struct and not more positional parameters -- this pilot already
// relearned the "positional-parameter pain" lesson once, in `--newrepo`'s
// own bulk-fix saga), defaulting to real `make.globals`'s own
// `CONFIG_PROTECT="/etc"`/`CONFIG_PROTECT_MASK="/etc/env.d"` (`NOCONFMEM`
// unset). The MD5-comparison decision itself is real `dblink._protect()`'s
// own *type-independent* one (`vartree.py:5434-5480`/`5831-5901`,
// `protect_decision`, shared by the `obj`/`sym` branches): `dest_md5`/
// `dest_link` are always computed from the live destination's own
// lstat'd on-disk type, regardless of the incoming source's own type, so
// a symlink replacing a previously-installed regular file at the same
// path (or vice versa) is real-protected too, not silently overwritten.
// Real `_installed_instance`/`FEATURES=config-protect-if-modified` is
// real too now (`vartree.py:4409-4418`/`5849-5866`): `installed_
// instance_pf` picks the max-`COUNTER` same-slot instance this merge is
// upgrading over (reusing the same real per-package `COUNTER` this
// pilot already writes on every merge), and `protect_decision` consults
// its own real `CONTENTS` (`owned_node_value_pf`) for two distinct real
// behaviors: a path it recorded that's now missing entirely on disk
// (the admin deleted it) always force-diverts (real bug #523684); and,
// only when `config-protect-if-modified` is on (real `make.globals`
// default), a live destination that still matches *exactly* what that
// previous instance installed -- never locally modified -- has the new
// version's content applied directly, distinguishing "this file's own
// default content changed between package versions" from "the admin
// hand-edited it locally".
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
//   - The one non-`NEEDED.ELF.2`-driven branch inside real `LinkageMap.
//     rebuild()` itself is still not ported: live `scanelf` for
//     orphaned preserved libs (`LinkageMapELF.py:233-324`) -- the one
//     real spot a raw ELF header read would matter. Every other real
//     preserve-libs *registration*/detection computation (`needed_elf.rs`:
//     `NeededEntry`, `read_all_needed_entries`, `rebuild`, `getlibpaths`,
//     `find_consumers`, `find_libs_to_preserve`) and the real control-
//     flow wiring into both merge (`unregister_preserved_libs`, this
//     module) and unmerge (`preserve_libs_on_unmerge`, this module) are
//     real now -- see `docs/what-this-proves.md`'s own "`preserve-libs`" sections for
//     the full grounding of each slice.
//   - `merge_tree`'s regular-file copy now mirrors real `movefile()`'s
//     explicit `os.chmod(dest, sstat.st_mode)` with a
//     `std::fs::set_permissions` after the copy (on top of the mode bits
//     `std::fs::copy` already carries over on Unix). Real `movefile()`'s
//     `os.lchown` is still not reproduced -- it needs root, which this
//     pilot's single-user dev/test context never has, so it would only
//     ever no-op; there's no privilege-dropping concept anywhere else in
//     the pilot either.
//   - Directory-entry merge order is sorted by filename for determinism
//     (this pilot's own test-reproducibility need) rather than real
//     `os.listdir()`'s own arbitrary/OS-dependent order -- a deliberate
//     choice, not a gap: `CONTENTS` line order has no semantic meaning
//     portage itself relies on (unmerge re-sorts, `qmerge`/`qlist` sort
//     on read), and determinism is worth more here than bug-compatible
//     arbitrariness.
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
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

/// Whether `command` is the one real merge command this module implements
/// -- `ebuild.rs` checks this alongside `ebuild_phases::
/// is_real_phase_command` before routing to real execution.
pub fn is_real_merge_command(command: &str) -> bool {
    command == "merge"
}

/// Whether `command` is real `qmerge` -- checked separately from
/// `is_real_merge_command` since `ebuild.rs` routes it to `run_qmerge`,
/// not `run_merge` (real `qmerge` skips the `install` phase entirely,
/// see `run_qmerge`'s own doc comment).
pub fn is_real_qmerge_command(command: &str) -> bool {
    command == "qmerge"
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
    /// exact real logic. **Unlike `collision_protect`**, real
    /// `protect-owned` *is* one of real `make.globals`'s own default
    /// `FEATURES` tokens (`cnf/make.globals:77-84`) -- confirmed by
    /// reading it directly (a real, previously-undiscovered mismatch:
    /// this field's own `Default` used to be `false` with a doc comment
    /// incorrectly claiming the same "not in FEATURES by default"
    /// reasoning `collision_protect` genuinely has). `Default` is now
    /// `true`, matching real portage's own actual out-of-the-box
    /// behavior. This pilot's own env-var read still only checks
    /// whether the literal `FEATURES` value (when set at all) contains
    /// the `"protect-owned"` token -- it doesn't *accumulate* onto the
    /// real default set the way real portage's own `+`/`-`-prefixed
    /// `make.conf` `FEATURES` merging does, so setting `FEATURES` to
    /// any *other* token still reads as `protect_owned: false` here,
    /// unlike real portage (which would keep it enabled unless `-
    /// protect-owned` was explicitly given) -- a pre-existing
    /// simplification this fix doesn't attempt to also resolve.
    pub protect_owned: bool,
    /// Real `--noconfmem`/`settings["NOCONFMEM"]` (`lib/_emerge/
    /// actions.py:2790`, `vartree.py:4949`'s own `cfgfiledict["IGNORE"]`):
    /// an `emerge`-only CLI flag with no real `bin/ebuild` equivalent, so
    /// this pilot reads the `NOCONFMEM` env var directly (presence-based,
    /// matching real `"NOCONFMEM" in self.settings`) rather than adding a
    /// CLI flag real `ebuild` doesn't have. Forces every already-offered,
    /// unmodified-since CONFIG_PROTECT update to be re-protected into a
    /// fresh `._cfgNNNN_` file instead of silently reused/applied.
    /// `Default` matches real portage's own default: unset, `false`.
    pub noconfmem: bool,
    /// Real `"config-protect-if-modified" in self.settings.features`
    /// (`vartree.py:5376-5379`): gates `_protect()`'s own `protect_if_
    /// modified` behavior (see `protect_decision`'s own doc comment) --
    /// real `config-protect-if-modified` *is* one of real `make.globals`'s
    /// own default `FEATURES` tokens (`cnf/make.globals:79`), confirmed
    /// by reading it directly, the same category of previously-
    /// undiscovered default-`FEATURES` mismatch `protect_owned`'s own doc
    /// comment already found. `Default` is `true`, matching real
    /// portage's own actual out-of-the-box behavior. Same env-var-not-
    /// full-config-resolution shortcut (and the same "doesn't accumulate
    /// onto the real default set" simplification) `protect_owned`
    /// already uses.
    pub protect_if_modified: bool,
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
            protect_owned: true,
            noconfmem: false,
            protect_if_modified: true,
            // "/dev/null" is a real character device, never a directory
            // -- joining anything under it can never exist on any real
            // filesystem, guaranteeing find_repos always fails cleanly
            // here regardless of what happens to exist on the host.
            config_root: PathBuf::from("/dev/null/no-config-root-configured"),
        }
    }
}

impl MergeOptions {
    /// Real portage's own `settings`-derived merge configuration, but via
    /// the same "read the env var, fall back to `make.globals`'s own
    /// default" shortcut every other real-execution CLI boundary in this
    /// pilot already takes (`PORTAGE_TMPDIR`/`PKGDIR`/... -- no full
    /// profile+`make.conf` resolution): `CONFIG_PROTECT`/
    /// `CONFIG_PROTECT_MASK`, `DISTDIR`, the `FEATURES` tokens
    /// `collision-protect`/`protect-owned`/`config-protect-if-modified`,
    /// `NOCONFMEM` (presence), and `PORTAGE_CONFIGROOT` (real default
    /// `"/"`). Shared by `ebuild <file> merge`/`qmerge` (`ebuild.rs`) and
    /// `emerge <atom>` (`emerge_build::run_source_merge`).
    pub fn from_env(shell: ebuild_phases::ShellBackend, debug: bool) -> Self {
        let d = Self::default();
        let has_feature = |tok: &str| {
            std::env::var("FEATURES")
                .map(|f| f.split_whitespace().any(|t| t == tok))
                .unwrap_or(false)
        };
        Self {
            debug,
            shell,
            config_protect: std::env::var("CONFIG_PROTECT").unwrap_or(d.config_protect),
            config_protect_mask: std::env::var("CONFIG_PROTECT_MASK")
                .unwrap_or(d.config_protect_mask),
            distdir: std::env::var_os("DISTDIR")
                .map(PathBuf::from)
                .unwrap_or(d.distdir),
            collision_protect: has_feature("collision-protect"),
            protect_owned: std::env::var("FEATURES")
                .map(|f| f.split_whitespace().any(|t| t == "protect-owned"))
                .unwrap_or(d.protect_owned),
            protect_if_modified: std::env::var("FEATURES")
                .map(|f| {
                    f.split_whitespace()
                        .any(|t| t == "config-protect-if-modified")
                })
                .unwrap_or(d.protect_if_modified),
            noconfmem: std::env::var_os("NOCONFMEM").is_some(),
            config_root: portage_repo::config_root_from_env(),
        }
    }
}

/// Real `ConfigProtect.isprotected()` (`lib/portage/util/__init__.py`):
/// longest-prefix match against `config_protect` (a whitespace-separated
/// path list, `root`-joined) minus `config_protect_mask`. A protect/mask
/// entry that names a real, on-disk directory matches any path under it
/// (`/etc` matches `/etc/foo` but not `/etcfoo`); one that doesn't (a
/// literal file, or a path that doesn't exist at all) only ever matches
/// exactly. `pub(crate)`: `ebuild_unmerge`'s own `FEATURES=unmerge-orphans`
/// handling reuses this exact real check (real `self.isprotected(obj)`).
pub(crate) fn is_protected(
    root: &Path,
    config_protect: &str,
    config_protect_mask: &str,
    dest: &Path,
) -> bool {
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

/// Real `new_protect_filename()` (`lib/portage/util/__init__.py:1803`):
/// the next unused `._cfgNNNN_<basename>` sibling of `dest`, unless the
/// *highest-numbered existing* `._cfgNNNN_<basename>` sibling already
/// holds the same content/target as `newmd5` -- in which case that
/// existing file is reused instead of allocating a new one. `newmd5`
/// mirrors real `_protect()`'s own `(dest_link or src_md5)` call
/// argument: the pending update's own content MD5 for a regular file, or
/// (for a symlink) the *comparison* target-string this call site chose
/// -- real portage's own naming is misleading here, "newmd5" folds both
/// "an MD5 hex string" and "a raw symlink target string" into the same
/// parameter, compared against whichever kind the last `._cfgNNNN_` file
/// itself turns out to be. `dest` itself is never touched here, the
/// caller writes to the returned path instead. Narrower than real in one
/// way: real `new_protect_filename(mydest, newmd5, force)` returns
/// `mydest` unchanged outright when `mydest` doesn't exist yet and
/// `force` is false -- moot here since every call site already only
/// calls this after confirming `dest` exists (see `merge_tree`'s own obj/
/// sym branches), so `force`'s only real effect (forcing that early
/// return to still allocate a number) never applies and isn't threaded
/// through.
fn new_protect_filename(dest: &Path, newmd5: &str) -> Result<PathBuf, String> {
    let parent = dest
        .parent()
        .ok_or_else(|| format!("{}: has no parent directory", dest.display()))?;
    let basename = dest
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("{}: not a valid filename", dest.display()))?;

    let mut max_num: i64 = -1;
    let mut last_pfile: Option<PathBuf> = None;
    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(rest) = name.strip_prefix("._cfg") {
                if rest.len() > 5 && rest.as_bytes()[4] == b'_' && &rest[5..] == basename {
                    if let Ok(n) = rest[..4].parse::<i64>() {
                        if n > max_num {
                            max_num = n;
                            last_pfile = Some(parent.join(entry.file_name()));
                        }
                    }
                }
            }
        }
    }

    if let Some(old_pfile) = &last_pfile {
        if let Ok(meta) = std::fs::symlink_metadata(old_pfile) {
            if meta.file_type().is_symlink() {
                if let Ok(target) = std::fs::read_link(old_pfile) {
                    if target.to_string_lossy() == newmd5 {
                        return Ok(old_pfile.clone());
                    }
                }
            } else if meta.is_file() {
                if let Ok(md5) = md5_hex(old_pfile) {
                    if md5 == newmd5 {
                        return Ok(old_pfile.clone());
                    }
                }
            }
        }
    }
    Ok(parent.join(format!("._cfg{:04}_{basename}", max_num + 1)))
}

/// Real `dblink._protect()`'s own decision, shared by `merge_tree`'s
/// `obj`/`sym` branches (`vartree.py:5434-5480`/`5831-5901`): `dest_md5`/
/// `dest_link` are computed from the *live destination's own lstat'd
/// on-disk type* -- independent of the incoming source's own type, so a
/// type-changing update (a symlink replacing a previously-installed
/// regular file at the same path, or vice versa) is real-protected too,
/// closing the "like-for-like only" v1 cut this module's own doc comment
/// used to document. `src_md5`/`src_link` mirror real `mymd5`/`myto`:
/// always an MD5-shaped string either way (a symlink source's own is the
/// target string's own MD5, real bug #485598), `src_link` only `Some`
/// for a symlink source. Real `force` (from `dest_link != src_link` on a
/// type mismatch) is deliberately not threaded through, for the exact
/// reason `new_protect_filename`'s own doc comment already gives: it
/// only ever changes behavior when `dest` doesn't exist, and this
/// function -- like every existing call site before it -- only reaches
/// `new_protect_filename` after confirming `dest` exists, *except* the
/// one new case below that deliberately doesn't.
///
/// Real `_installed_instance`/`k = self._installed_instance.
/// _match_contents(dest_real)` (`vartree.py:5849-5866`) is real now too:
/// `installed_instance_pf` (the *previous* same-slot instance this
/// merge is upgrading over, if any) is consulted via `owned_node_value_
/// pf` for whatever it recorded at `abs_path`. Two distinct real
/// behaviors, both gated on a match (`k is not False`):
///   - `dest_mode is None` (the live destination doesn't exist at all --
///     the admin deleted or renamed a path the *previous* package
///     installed): real `force = True`, which (since `_protect()`'s own
///     `if protected and dest_mode is not None:` main block is skipped
///     entirely when `dest_mode is None`, leaving `protected`/`move_me`
///     at their initial `True` values) always diverts into a fresh
///     `._cfgNNNN_` sibling -- bug #523684, prompting the admin instead
///     of silently re-creating a path they deliberately removed.
///   - `dest_mode is not None` and real `FEATURES=config-protect-if-
///     modified` (`protect_if_modified`) is on: if the live destination
///     still matches *exactly* what the previous instance's own real
///     `CONTENTS` recorded (an `obj`'s content MD5, or a `sym`'s own
///     target string), the admin never touched it since that install --
///     so it's not "modified" in the sense this feature cares about,
///     and `protected` is cleared outright, applying the new version's
///     content directly even though it differs from `src`. Distinguishes
///     "this file's own default content changed between package
///     versions" from "the admin hand-edited this file locally", which
///     the plain `src_md5 == dest_md5` comparison below can't tell
///     apart on its own.
///
/// Returns `(write_dest, moveme)` -- real `_protect()` returns three
/// values (`dest, protected, moveme`), but every real call site only
/// ever needs `protected` to decide *whether* to call `_protect()` at
/// all (already handled by this function's own caller, `is_protected`)
/// and `moveme` to decide whether `mergeme()`'s own `if moveme:` gate
/// (`vartree.py:5547`/`5749`) actually performs the file write, so this
/// port only threads those two through. Real `moveme` is `False` in
/// exactly one case here: `already_offered && !noconfmem` (real `move_me
/// = protected = bool(cfgfiledict["IGNORE"])` with `IGNORE == 0`,
/// `vartree.py:5877`) -- "confmem rejected this update"
/// (`mergeme()`'s own `zing = "---"`). Real `cfgfiledict` is deliberately
/// left untouched in that one branch too: reaching it requires `src_md5
/// == cfgfiledict.get(dest_real)[0]` in the first place (that's the very
/// definition of "already offered"), so the trailing real `if move_me:
/// cfgfiledict[dest_real] = [src_md5] elif dest_md5 == cfgfiledict.get
/// (dest_real)[0]: del cfgfiledict[dest_real]` (`vartree.py:5888-5895`)
/// hits neither branch: `move_me` is `False` (skipping the first), and
/// `dest_md5 != src_md5` is already established by the earlier `if
/// src_md5 == dest_md5` check having failed (skipping the second, since
/// it can only match by being equal to `src_md5` too). Every other
/// return path keeps `moveme` `true`, matching real `_protect()`'s own
/// `move_me = True` initial default, never cleared on any other branch.
#[allow(clippy::too_many_arguments)]
fn protect_decision(
    root: &Path,
    category: &str,
    installed_instance_pf: Option<&str>,
    protect_if_modified: bool,
    dest: &Path,
    abs_path: &str,
    src_md5: &str,
    cfgfiledict: &mut BTreeMap<String, String>,
    noconfmem: bool,
) -> Result<(PathBuf, bool), String> {
    let matched: Option<(String, String)> =
        installed_instance_pf.and_then(|pf| owned_node_value_pf(root, category, pf, abs_path));

    let Ok(dest_meta) = std::fs::symlink_metadata(dest) else {
        // Real `dest_mode is None`.
        if matched.is_some() {
            // Real bug #523684: force-diverts even though there's
            // nothing on disk to compare against yet.
            cfgfiledict.insert(abs_path.to_string(), src_md5.to_string());
            return Ok((new_protect_filename(dest, src_md5)?, true));
        }
        return Ok((dest.to_path_buf(), true));
    };

    let (dest_md5, dest_link): (Option<String>, Option<String>) =
        if dest_meta.file_type().is_symlink() {
            let target = std::fs::read_link(dest)
                .ok()
                .map(|t| t.to_string_lossy().to_string());
            let md5 = target.as_deref().map(|t| md5_hex_bytes(t.as_bytes()));
            (md5, target)
        } else if dest_meta.is_file() {
            (md5_hex(dest).ok(), None)
        } else {
            (None, None)
        };

    if protect_if_modified {
        if let Some((node_type, value)) = &matched {
            let unmodified_since_installed = match node_type.as_str() {
                "obj" => dest_md5.as_deref() == Some(value.as_str()),
                "sym" => dest_link.as_deref() == Some(value.as_str()),
                _ => false,
            };
            if unmodified_since_installed {
                return Ok((dest.to_path_buf(), true));
            }
        }
    }

    if dest_md5.as_deref() == Some(src_md5) {
        return Ok((dest.to_path_buf(), true));
    }

    let already_offered = cfgfiledict.get(abs_path).map(String::as_str) == Some(src_md5);
    if already_offered && !noconfmem {
        return Ok((dest.to_path_buf(), false));
    }

    let newmd5 = dest_link.as_deref().unwrap_or(src_md5);
    let write_dest = new_protect_filename(dest, newmd5)?;
    cfgfiledict.insert(abs_path.to_string(), src_md5.to_string());
    Ok((write_dest, true))
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

/// `pub(crate)`: also read by `ebuild_unmerge::run_unmerge` (real
/// `_unmerge_pkgfiles()`'s own `stale_confmem` cleanup,
/// `vartree.py:2747`/`2931-2932`/`3106-3109` -- a removed file's
/// `_conf_mem_file` entry is dropped once nothing still owns that path).
pub(crate) fn read_cfgfiledict(root: &Path) -> BTreeMap<String, String> {
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

pub(crate) fn write_cfgfiledict(root: &Path, map: &BTreeMap<String, String>) -> Result<(), String> {
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

/// Real `vardbapi.removeFromContents` (`vartree.py:1244-1310`). A
/// missing vdb entry for `cpv` (already unmerged some other way) is
/// silently a no-op, matching real `removeFromContents`'s own tolerance
/// of a stale registry entry.
///
/// Real `NEEDED`-line stripping ("Also remove corresponding NEEDED
/// lines, so that they do no corrupt LinkageMap data for preserve-libs",
/// `vartree.py:1279-1310`) is real now too, closing a gap this module's
/// own doc comment used to document as moot ("without the registration
/// side above ever writing NEEDED data in the first place") -- moot no
/// longer, since real `NEEDED.ELF.2` generation and the full real
/// `LinkageMap`/preserve-libs computation are both real now (see
/// `needed_elf.rs`). Real `removed` (whether any `CONTENTS` line was
/// actually dropped) gates the whole thing, matching real `if removed:`
/// exactly -- when this package's own `NEEDED.ELF.2` doesn't exist at
/// all (real `except OSError: ... new_needed` stays `None`), nothing is
/// written, matching real `if new_needed is not None:` in
/// `writeContentsToContentsFile`. When it does exist, every entry whose
/// own `filename` (already `ROOT`-relative, this pilot's own `CONTENTS`/
/// `NEEDED.ELF.2` convention -- no `os.path.join(root, ...)` needed the
/// way real Python does, since both sides already agree on the same
/// convention here) still appears among the *surviving* `CONTENTS`
/// paths is kept; every other entry (now pointing at a file this package
/// no longer owns) is dropped -- stale linkage data that would otherwise
/// corrupt a *later* `LinkageMap.rebuild()`'s own preserve-libs decision
/// for some *other* package's own future unmerge.
fn remove_from_contents(root: &Path, cpv: &str, paths: &BTreeSet<String>) -> Result<(), String> {
    let Some((category, pf)) = cpv.split_once('/') else {
        return Ok(());
    };
    let vdb_dir = root.join("var/db/pkg").join(category).join(pf);
    let contents_path = vdb_dir.join("CONTENTS");
    let Ok(text) = std::fs::read_to_string(&contents_path) else {
        return Ok(());
    };
    let mut removed = false;
    let mut surviving_paths: BTreeSet<String> = BTreeSet::new();
    let new_text: String = text
        .lines()
        .filter(|line| {
            let mut parts = line.split_whitespace();
            parts.next();
            let abs_path = parts.next();
            if matches!(abs_path, Some(p) if paths.contains(p)) {
                removed = true;
                false
            } else {
                if let Some(p) = abs_path {
                    surviving_paths.insert(p.to_string());
                }
                true
            }
        })
        .map(|l| format!("{l}\n"))
        .collect();
    std::fs::write(&contents_path, new_text)
        .map_err(|e| format!("{}: {e}", contents_path.display()))?;

    if removed {
        let needed_path = vdb_dir.join("NEEDED.ELF.2");
        if let Ok(needed_text) = std::fs::read_to_string(&needed_path) {
            let new_needed: String = crate::needed_elf::NeededEntry::parse_file(&needed_text)
                .into_iter()
                .filter(|entry| surviving_paths.contains(&entry.filename))
                .map(|entry| entry.to_needed_line())
                .collect();
            std::fs::write(&needed_path, new_needed)
                .map_err(|e| format!("{}: {e}", needed_path.display()))?;
        }
    }
    Ok(())
}

/// Real `PreservedLibsRegistry.register`/`.unregister`
/// (`lib/portage/util/_dyn_libs/PreservedLibsRegistry.py:142-176`): real
/// `unregister(cpv, slot, counter) = register(cpv, slot, counter, [])`,
/// so this one function covers both real calls, matching real `register`
/// exactly. Real `cps = cpv_getkey(cpv) + ":" + slot` (the real registry
/// key: `category/package:slot`, no version) -- `category`/`pn` are
/// passed in already split, rather than re-deriving them from a version
/// string the way real `cpv_getkey` does, since every real caller here
/// already has them split (`ebuild_phases::Environment::split`).
///
/// Empty `paths` (real `unregister`): removes the `cps` entry, but only
/// if it currently records the *same* `cpv` and `counter` -- never
/// blindly erasing a different package's own entry that happens to
/// share this exact slot's own key. Non-empty `paths`: unconditionally
/// overwrites the `cps` entry (real `_normalize_counter` is just a
/// whitespace-trim, not integer parsing, so a plain trimmed-string
/// comparison already matches real behavior exactly).
fn register_preserved_libs(
    registry: &mut PlibRegistry,
    cpv: &str,
    category: &str,
    pn: &str,
    slot: &str,
    counter: &str,
    paths: &[String],
) {
    let cps = format!("{category}/{pn}:{slot}");
    let counter = counter.trim();
    if paths.is_empty() {
        if let Some((entry_cpv, entry_counter, _)) = registry.entries.get(&cps) {
            if entry_cpv == cpv && entry_counter.trim() == counter {
                registry.entries.remove(&cps);
            }
        }
    } else {
        registry
            .entries
            .insert(cps, (cpv.to_string(), counter.to_string(), paths.to_vec()));
    }
}

/// Real `dblink._prune_plib_registry()` (`vartree.py:2228-2314`), called
/// from real `unmerge()` with `unmerge=True` right before real
/// `_unmerge_pkgfiles()` runs (`vartree.py:2493`/`2529` -- confirmed by
/// reading the real call site, not just the method itself), narrowed to
/// the one real shape this pilot's own standalone `ebuild <file>
/// unmerge` always reaches: `unmerge_with_replacement=False`. Real
/// `preserve_paths` (a `_prune_plib_registry` parameter, not to be
/// confused with this function's own *return* value) is only ever
/// non-`None` when a real depgraph-driven upgrade transaction already
/// computed it via a companion `merge()` call in the *same* transaction
/// -- this pilot's own `merge`/`unmerge` are always separate,
/// independent CLI invocations, so this is always the real shape that
/// applies (real `instance_owns_files and not unmerge_with_replacement`
/// collapses to just `instance_owns_files`).
///
/// Real order: rebuild the system-wide `LinkageMap` from every real
/// installed package's own vdb-stored `NEEDED.ELF.2`
/// (`needed_elf::read_all_needed_entries` + `rebuild` -- real `exclude_
/// pkgs=None` in this exact shape, since the package being unmerged
/// hasn't left the vdb yet, so its own data is still really part of the
/// map, matching real behavior exactly). Compute `needed_elf::find_
/// libs_to_preserve` with `new_owner_is_owner` always `false` (matching
/// what real `not unmerge and self.isowner(f)` collapses to when
/// `unmerge` is `true`) and `old_owner_is_owner` real `self.isowner`
/// (`owns_path_pf`, this exact package's own real `CONTENTS`).
/// Unconditionally unregister this package's own prior registry entry
/// first (real `plib_registry.unregister`); if anything is actually
/// preserved, register this package -- the one being removed -- as the
/// new keeper of those paths (real `plib_registry.register`).
///
/// Returns the set of preserved paths (already `ROOT`-relative absolute
/// paths, this pilot's own `CONTENTS` convention) -- the caller is
/// responsible for excluding them from its own real file-removal loop
/// (real "remove the preserved files from our contents so that they
/// won't be unmerged"; this pilot's own vdb entry directory gets deleted
/// wholesale moments later regardless, so there's no separate real
/// `CONTENTS`-file rewrite to also perform here).
pub(crate) fn preserve_libs_on_unmerge(
    root: &Path,
    category: &str,
    pn: &str,
    pf: &str,
    slot: &str,
    contents_text: &str,
) -> Result<BTreeSet<String>, String> {
    if contents_text.trim().is_empty() {
        return Ok(BTreeSet::new());
    }

    let owner_entries = crate::needed_elf::read_all_needed_entries(root);
    let map = crate::needed_elf::rebuild(root, &owner_entries);
    let defpath =
        crate::needed_elf::getlibpaths(root, std::env::var("LD_LIBRARY_PATH").ok().as_deref());

    let old_contents: Vec<String> = contents_text
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1).map(String::from))
        .collect();

    let old_owner_is_owner = |p: &str| owns_path_pf(root, category, pf, p);
    let new_owner_is_owner = |_: &str| false;

    let preserved = crate::needed_elf::find_libs_to_preserve(
        root,
        &map,
        &defpath,
        &old_contents,
        &old_owner_is_owner,
        &new_owner_is_owner,
    );

    let counter_path = root
        .join("var/db/pkg")
        .join(category)
        .join(pf)
        .join("COUNTER");
    let counter = std::fs::read_to_string(&counter_path).unwrap_or_else(|_| "0".to_string());
    let cpv = format!("{category}/{pf}");

    let mut registry = read_plib_registry(root);
    register_preserved_libs(&mut registry, &cpv, category, pn, slot, &counter, &[]);
    if !preserved.is_empty() {
        let paths_vec: Vec<String> = preserved.iter().cloned().collect();
        register_preserved_libs(
            &mut registry,
            &cpv,
            category,
            pn,
            slot,
            &counter,
            &paths_vec,
        );
    }
    write_plib_registry(root, &registry)?;

    Ok(preserved)
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

fn md5_hex_bytes(data: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(data);
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn md5_hex(path: &Path) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(md5_hex_bytes(&data))
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
#[allow(clippy::too_many_arguments)]
fn merge_tree(
    d: &Path,
    root: &Path,
    category: &str,
    installed_instance_pf: Option<&str>,
    protect_if_modified: bool,
    config_protect: &str,
    config_protect_mask: &str,
    noconfmem: bool,
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
                let target_str = target.to_string_lossy().to_string();
                // Real dblink._protect(): a CONFIG_PROTECT'd `sym` entry
                // whose real, live destination differs is diverted to a
                // fresh ._cfgNNNN_ sibling (real bug #485598: the target
                // string's own MD5 is what's hashed, not file content) --
                // `protect_decision` computes the comparison from the
                // dest's own real on-disk type, whatever it actually is
                // (see that function's own doc comment).
                let mut write_dest = dest.clone();
                let mut moveme = true;
                if is_protected(root, config_protect, config_protect_mask, &dest) {
                    let src_md5 = md5_hex_bytes(target_str.as_bytes());
                    (write_dest, moveme) = protect_decision(
                        root,
                        category,
                        installed_instance_pf,
                        protect_if_modified,
                        &dest,
                        &abs_path,
                        &src_md5,
                        cfgfiledict,
                        noconfmem,
                    )?;
                }

                let mtime = mtime_secs(
                    &std::fs::symlink_metadata(&src)
                        .map_err(|e| format!("{}: {e}", src.display()))?,
                )?;
                // Real `moveme == false` ("confmem rejected this update",
                // see `protect_decision`'s own doc comment): skip the
                // write entirely, leaving the live destination completely
                // untouched -- `mtime` above still uses the *source's* own
                // mtime for CONTENTS below either way, matching real
                // `mergeme()`'s own `mymtime = mystat.st_mtime_ns` (set
                // before the `if moveme:` gate, never touched when it's
                // skipped, `vartree.py:5403`/`5547`).
                if moveme {
                    if let Some(parent) = write_dest.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| format!("{}: {e}", parent.display()))?;
                    }
                    if write_dest.exists() || write_dest.symlink_metadata().is_ok() {
                        let _ = std::fs::remove_file(&write_dest);
                    }
                    std::os::unix::fs::symlink(&target, &write_dest)
                        .map_err(|e| format!("{}: {e}", write_dest.display()))?;
                    // Real movefile() preserves the source's own mtime onto
                    // the merged destination -- without this, the freshly
                    // created symlink would get its own "now" mtime, never
                    // matching what's about to be recorded in CONTENTS below
                    // (see ebuild_unmerge.rs's own "!mtime" staleness check,
                    // which relies on this actually holding).
                    let ft = filetime::FileTime::from_unix_time(mtime, 0);
                    filetime::set_symlink_file_times(&write_dest, ft, ft)
                        .map_err(|e| format!("{}: {e}", write_dest.display()))?;
                }
                // Real CONTENTS always records the package's own logical
                // path/target (`abs_path`/`target_str`), never the
                // ._cfgNNNN_ variant a protected write may have actually
                // landed at -- same "logical path, not the protect-file
                // path" rule the `obj` branch below documents.
                contents.push_str(&format_contents_line(
                    "sym",
                    &abs_path,
                    None,
                    Some(&target_str),
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
                // directly, don't re-protect; `NOCONFMEM` forces
                // re-protection regardless of memory, real `cfgfiledict[
                // "IGNORE"]`).
                let mut write_dest = dest.clone();
                let mut moveme = true;
                if is_protected(root, config_protect, config_protect_mask, &dest) {
                    (write_dest, moveme) = protect_decision(
                        root,
                        category,
                        installed_instance_pf,
                        protect_if_modified,
                        &dest,
                        &abs_path,
                        &src_md5,
                        cfgfiledict,
                        noconfmem,
                    )?;
                }

                let mtime = mtime_secs(
                    &std::fs::metadata(&src).map_err(|e| format!("{}: {e}", src.display()))?,
                )?;
                // Real `moveme == false` ("confmem rejected this update",
                // see `protect_decision`'s own doc comment): skip the copy
                // entirely, leaving the live destination completely
                // untouched -- `mtime` above still uses the *source's* own
                // mtime for CONTENTS below either way, matching real
                // `mergeme()`'s own `mymtime = mystat.st_mtime_ns` (set
                // before the `if moveme:` gate, never touched when it's
                // skipped, `vartree.py:5403`/`5749`).
                if moveme {
                    if let Some(parent) = write_dest.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| format!("{}: {e}", parent.display()))?;
                    }
                    std::fs::copy(&src, &write_dest)
                        .map_err(|e| format!("{}: {e}", src.display()))?;
                    // Real `movefile()` explicitly `os.chmod(dest,
                    // sstat.st_mode)`s after the copy/rename (and
                    // `os.lchown`s -- omitted here: it needs root, which
                    // this pilot's single-user dev/test context never
                    // has, and would only ever no-op). `std::fs::copy`
                    // already carries a regular file's permission bits
                    // over on Unix, so this is belt-and-suspenders that
                    // makes the mode explicit and also fixes up a
                    // pre-existing `._cfgNNNN_` sibling should the umask
                    // have masked a bit.
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let mode = std::fs::metadata(&src)
                            .map_err(|e| format!("{}: {e}", src.display()))?
                            .permissions()
                            .mode();
                        std::fs::set_permissions(
                            &write_dest,
                            std::fs::Permissions::from_mode(mode),
                        )
                        .map_err(|e| format!("{}: {e}", write_dest.display()))?;
                    }
                    // Real movefile() preserves the source's own mtime onto
                    // the destination -- std::fs::copy doesn't (the copy
                    // gets a fresh "now" mtime), which would otherwise never
                    // match what's recorded in CONTENTS below (see
                    // ebuild_unmerge.rs's own "!mtime" staleness check).
                    filetime::set_file_mtime(
                        &write_dest,
                        filetime::FileTime::from_unix_time(mtime, 0),
                    )
                    .map_err(|e| format!("{}: {e}", write_dest.display()))?;
                }
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
            } else if file_type.is_fifo()
                || file_type.is_char_device()
                || file_type.is_block_device()
            {
                // Real `mergeme()`'s own `else:` branch ("we are merging a
                // fifo or device node", `vartree.py:5787-5811`): never
                // `_protect()`'d (this branch doesn't call it at all,
                // unlike `obj`/`sym` above), and only actually created
                // when the live destination doesn't already exist yet
                // (real `if mydmode is None:`) -- an existing node at that
                // path is left completely alone, matching real portage's
                // own conservative "don't touch a device/fifo that's
                // already there" behavior. The `CONTENTS` line is written
                // unconditionally either way (real `_format_contents_line`
                // call sits *outside* that `if`), with no digest/mtime/
                // target field at all (real `abs_path=myrealdest` only).
                //
                // Real `movefile()` has no dedicated fifo/device-node
                // logic of its own -- an ordinary `os.rename()` just works
                // for a special file too, since `rename(2)` doesn't care
                // what type of file it's moving (real `movefile()`'s own
                // comment: "we don't yet handle special, so we need to
                // fall back to /bin/mv" only fires on a genuine cross-
                // device `EXDEV` failure). This pilot's own merge step
                // never moves `${D}` content though (every other branch
                // above copies/recreates instead, so `${D}` itself stays
                // intact) -- recreating a fresh node at `write_dest` via
                // real `mkfifo(3)`/`mknod(3)` (matching the source's own
                // real type, permission bits, and -- for a device node --
                // major/minor) is the equivalent "copy" here, the same
                // "recreate, don't move" shape the `sym` branch above
                // already established for symlinks.
                if std::fs::symlink_metadata(&dest).is_err() {
                    if let Some(parent) = dest.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| format!("{}: {e}", parent.display()))?;
                    }
                    create_special_node(&src, &dest, &file_type)?;
                }
                let node_type = if file_type.is_fifo() { "fif" } else { "dev" };
                contents.push_str(&format_contents_line(
                    node_type, &abs_path, None, None, None,
                ));
            }
        }
    }
    Ok(contents)
}

/// Creates a fresh FIFO or device node at `dest`, matching `src`'s own
/// real type, permission bits, and (for a device) real major/minor
/// (`st_rdev`) -- the "recreate, don't move" equivalent of real
/// `movefile()`'s ordinary same-device `rename(2)` for a special file
/// (see `merge_tree`'s own `fif`/`dev` branch doc comment for why this
/// pilot recreates rather than moves). `mkfifo(3)`/`mknod(3)` both apply
/// the process umask to the mode given, unlike `std::fs::copy`'s own
/// automatic exact permission-bit preservation for a regular file -- an
/// explicit `chmod` afterward closes that gap, so a real, non-default
/// source mode (e.g. `0600`) survives regardless of this process's own
/// umask.
///
/// Real `mknod(2)` genuinely requires root/`CAP_MKNOD` for a real
/// (nonzero major:minor) character or block device -- an unprivileged
/// caller merging a real device node from `${D}` (itself only possible
/// because a privileged build process, e.g. real `udev`, put it there)
/// hits this same real permission wall, surfaced here as an ordinary
/// `Result::Err` rather than a panic.
fn create_special_node(
    src: &Path,
    dest: &Path,
    file_type: &std::fs::FileType,
) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;

    let src_meta = std::fs::symlink_metadata(src).map_err(|e| format!("{}: {e}", src.display()))?;
    let mode = src_meta.mode() & 0o7777;
    let dest_c = std::ffi::CString::new(dest.as_os_str().as_bytes())
        .map_err(|e| format!("{}: {e}", dest.display()))?;

    let ret = if file_type.is_fifo() {
        unsafe { libc::mkfifo(dest_c.as_ptr(), mode) }
    } else {
        let type_bit = if file_type.is_char_device() {
            libc::S_IFCHR
        } else {
            libc::S_IFBLK
        };
        unsafe {
            libc::mknod(
                dest_c.as_ptr(),
                type_bit | mode,
                src_meta.rdev() as libc::dev_t,
            )
        }
    };
    if ret != 0 {
        return Err(format!(
            "{}: {}",
            dest.display(),
            std::io::Error::last_os_error()
        ));
    }
    if unsafe { libc::chmod(dest_c.as_ptr(), mode) } != 0 {
        return Err(format!(
            "{}: {}",
            dest.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
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
/// `env`. Real `dblink.merge()`/`treewalk()` copies *every* file directly
/// under `inforoot` (`${PORTAGE_BUILDDIR}/build-info`) into the vdb
/// wholesale (`vartree.py:4911-4913`, `for x in os.listdir(inforoot):
/// self.copyfile(...)`) -- so this does too: every regular file in
/// `build-info` (the `CATEGORY`/`SLOT`/`KEYWORDS`/`IUSE`/`USE`/`EAPI`/
/// `DEFINED_PHASES`/… `bin/phase-functions.sh __dyn_install` writes, the
/// `DEPEND`/`RDEPEND`/`LICENSE`/… `ebuild_phases::write_post_install_
/// metadata` adds, `NEEDED.ELF.2` from the real `scanelf` QA step,
/// `environment.bz2`, the `<PF>.ebuild` copy) lands in the vdb. Then the
/// merge-generated files that were never in `build-info` are written on
/// top: `CONTENTS` (the real file list, built during the copy loop) and
/// `COUNTER` (real `cpv_counter`, this pilot's own `next_counter`).
/// `CATEGORY`/`SLOT`/`repository` are re-asserted explicitly too -- a
/// standalone `ebuild <file> install` outside a repo checkout has no
/// `build-info/repository`, and `SLOT` here is the sub-slot-stripped
/// main slot the caller resolved.
///
/// Builds the entry in a `MERGING_IDENTIFIER`-prefixed temporary sibling
/// directory first, then
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
    write_vdb_entry_from_dir(
        root,
        &env.category,
        &env.split.pf,
        &env.build_info(),
        slot,
        repository,
        contents,
    )
}

/// The `env`-free core of `write_vdb_entry`: `category`/`pf` name the
/// entry, `build_info_dir` holds the files to copy wholesale. Shared with
/// `emerge_binmerge` (a binpkg merge has no `Environment`).
pub(crate) fn write_vdb_entry_from_dir(
    root: &Path,
    category: &str,
    pf: &str,
    build_info_dir: &Path,
    slot: &str,
    repository: &str,
    contents: &str,
) -> Result<(), String> {
    let cat_dir = root.join("var/db/pkg").join(category);
    let tmp_dir = cat_dir.join(format!("{MERGING_IDENTIFIER}{pf}"));
    let final_dir = cat_dir.join(pf);

    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir).map_err(|e| format!("{}: {e}", tmp_dir.display()))?;
    }
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("{}: {e}", tmp_dir.display()))?;

    // Real `treewalk()`: copy every regular file from `build-info` into
    // the vdb entry (`vartree.py:4911-4913`).
    if let Ok(entries) = std::fs::read_dir(build_info_dir) {
        for entry in entries.flatten() {
            let src = entry.path();
            if src.is_file() {
                if let Some(name) = src.file_name() {
                    std::fs::copy(&src, tmp_dir.join(name))
                        .map_err(|e| format!("{}: {e}", src.display()))?;
                }
            }
        }
    }

    // Merge-generated files never present in `build-info`, plus the
    // caller-resolved values re-asserted (see this function's own doc
    // comment).
    let counter = next_counter(root)?;
    for (name, value) in [
        ("CATEGORY", category),
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

pub(crate) fn read_installed_slot(
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

/// Real `self._installed_instance` selection (`vartree.py:4409-4418`):
/// among every other real, currently-installed version of this exact
/// `category/package/slot`, the one with the highest real `COUNTER`
/// (real `cpv_counter`, this pilot's own real per-package `COUNTER` file
/// -- see `next_counter`'s own doc comment) -- `None` when none exist (a
/// first-ever install, or every other same-slot instance's own
/// `COUNTER` is unreadable). Real `_installed_instance` is only ever
/// set when `slot_matches` (this exact slot has at least one other
/// installed version already) -- naturally true here too, since an
/// empty `own_versions` list has no max at all.
fn installed_instance_pf(root: &Path, category: &str, package: &str, slot: &str) -> Option<String> {
    portage_repo::installed_versions(root, category, package)
        .into_iter()
        .filter(|version| {
            read_installed_slot(root, category, package, version).as_deref() == Some(slot)
        })
        .filter_map(|version| {
            let pf = format!("{package}-{version}");
            let counter: i64 = std::fs::read_to_string(
                root.join("var/db/pkg")
                    .join(category)
                    .join(&pf)
                    .join("COUNTER"),
            )
            .ok()?
            .trim()
            .parse()
            .ok()?;
            Some((pf, counter))
        })
        .max_by_key(|(_, counter)| *counter)
        .map(|(pf, _)| pf)
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
pub(crate) fn owns_path_pf(root: &Path, category: &str, pf: &str, abs_path: &str) -> bool {
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

/// Real `dblnk._match_contents(relative_path)` + `getcontents()[key][0]`:
/// the node type (`"obj"`/`"dir"`/`"sym"`/...) the installed package
/// `category/pf` recorded for `abs_path` in its own real `CONTENTS`, or
/// `None` if it doesn't own that exact path at all. `ebuild_unmerge`'s
/// own bug #326685 "symlink orphan" detection is the one caller: it
/// needs to know not just *whether* another same-slot instance owns a
/// path (`owns_path_pf` above) but specifically *what type* it recorded
/// it as, to tell "still a symlink there too" apart from "reclassified
/// as a real directory".
pub(crate) fn owned_node_type_pf(
    root: &Path,
    category: &str,
    pf: &str,
    abs_path: &str,
) -> Option<String> {
    let path = root
        .join("var/db/pkg")
        .join(category)
        .join(pf)
        .join("CONTENTS");
    let text = std::fs::read_to_string(path).ok()?;
    text.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let node_type = parts.next()?;
        if parts.next() == Some(abs_path) {
            Some(node_type.to_string())
        } else {
            None
        }
    })
}

/// Real `dblnk._match_contents(relative_path)` + `getcontents()[key]`,
/// the *value* half `owned_node_type_pf` above deliberately leaves out:
/// an `obj` entry's own content MD5, or a `sym` entry's own target
/// string -- exactly the values real `_protect()`'s own `data[2]`
/// compares against `dest_md5`/`dest_link` for `protect_if_modified`
/// (see `protect_decision`'s own doc comment). `None` for any other
/// node type (`dir`, etc. -- never real `_protect()`-relevant) or a
/// path this instance doesn't own at all.
fn owned_node_value_pf(
    root: &Path,
    category: &str,
    pf: &str,
    abs_path: &str,
) -> Option<(String, String)> {
    let path = root
        .join("var/db/pkg")
        .join(category)
        .join(pf)
        .join("CONTENTS");
    let text = std::fs::read_to_string(path).ok()?;
    text.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let node_type = parts.next()?;
        if parts.next() != Some(abs_path) {
            return None;
        }
        match node_type {
            "obj" => Some((node_type.to_string(), parts.next()?.to_string())),
            "sym" => {
                if parts.next() != Some("->") {
                    return None;
                }
                Some((node_type.to_string(), parts.next()?.to_string()))
            }
            _ => None,
        }
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
        let repo_aliases: Vec<(String, PathBuf)> = repos
            .iter()
            .flat_map(|r| r.aliases.iter().map(|a| (a.clone(), r.location.clone())))
            .collect();
        let config = portage_profile::resolve_config(
            config_root,
            &main_repo.location,
            &overlay_repos,
            &repo_aliases,
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
            &config.conf_use_tokens,
            &config.package_use_repo,
            &config.package_use,
            &config.package_env_use,
            &config.package_use_user,
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
        Some(blockers_from_flat_deps(root, &flat_deps))
    })()
    .unwrap_or_default()
}

/// The installed-package scan + blocker-atom match half of
/// `blocked_installed_packages` (real `mypkglist`'s `blockers` term),
/// split out so `merge_binpkg` can reuse it: a binary package's
/// `*DEPEND` build-info files are already USE-reduced at build time, so
/// the merge side has a flat token list in hand without ever resolving
/// config/USE against a repo (which a binpkg has no path to). Matches
/// every `!atom`/`!!atom` (`portage_dep::parse_atom`'s own `.blocker`)
/// against every installed vdb entry (`category/pf:slot/sub_slot`, so a
/// slot-restricted blocker like `!dev-libs/foo:0` matches correctly).
/// Weak vs. strong blockers are not distinguished (`dblink.merge()`'s
/// own `mypkglist` construction doesn't either).
fn blockers_from_flat_deps(root: &Path, flat_deps: &[String]) -> HashSet<(String, String)> {
    (|| -> Option<HashSet<(String, String)>> {
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
        for tok in flat_deps {
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
    // `Some` for a source `emerge <atom>` under `FEATURES=buildpkg` /
    // `--buildpkg` (real `_emerge/EbuildBinpkg`): a binpkg of the freshly
    // built `${D}` is written into `$PKGDIR` **before** the vdb merge,
    // matching real portage's `EbuildBuild` -> `EbuildBinpkg` ->
    // `EbuildMerge` task order -- a build failure means nothing is
    // merged. `ebuild <file> merge` (and every internal reuse) passes
    // `None`: `FEATURES=buildpkg` is an `emerge`-flow concept with no
    // real `bin/ebuild` equivalent.
    buildpkg: Option<&crate::ebuild_package::PackageOptions>,
) -> Result<i32, String> {
    let status = ebuild_phases::run_commands(
        ebuild_path,
        &["install"],
        root,
        portage_tmpdir,
        &options.distdir,
        options.debug,
        &options.config_root,
        options.shell,
    )?;
    if status != 0 {
        return Ok(status);
    }
    if let Some(package_options) = buildpkg {
        let status = crate::ebuild_package::package_after_install(
            ebuild_path,
            root,
            portage_tmpdir,
            package_options,
        )?;
        if status != 0 {
            return Ok(status);
        }
    }
    let env = ebuild_phases::compute_environment(ebuild_path, portage_tmpdir)?;
    merge_after_install(ebuild_path, root, portage_tmpdir, &env, options)
}

/// Real `doebuild()`'s own `mydo == "qmerge"` branch
/// (`lib/portage/package/ebuild/doebuild.py:1562-1591`): skips the
/// `install` phase entirely, assuming a prior real `install` (or `merge`,
/// which runs `install` first) already populated `${D}` -- gated on the
/// same real marker real `doebuild()` itself checks, `${PORTAGE_BUILDDIR}/
/// .installed` (see `Environment::installed_marker`'s own doc comment for
/// why this pilot doesn't need to write it itself). Real portage doesn't
/// treat a missing marker as a hard failure (`writemsg(...); return 1`,
/// not a raised exception) -- this pilot's own established idiom for
/// surfacing an internal message through `ebuild.rs`'s own `Err` ->
/// `eprintln!("ebuild: {e}")` path still produces the same real exit code
/// (1) either way (see `ebuild.rs`'s own `Ok(_) => ExitCode::from(1)`
/// fallback), so `Err` is used here for consistency with this module's
/// other "not in the expected state" checks (e.g. `run_unmerge`'s own
/// "not installed" case) rather than hand-rolling a second message-
/// printing path.
pub fn run_qmerge(
    ebuild_path: &Path,
    root: &Path,
    portage_tmpdir: &Path,
    options: &MergeOptions,
) -> Result<i32, String> {
    let env = ebuild_phases::compute_environment(ebuild_path, portage_tmpdir)?;
    if !env.installed_marker().exists() {
        return Err("mydo=qmerge, but the install phase has not been run".to_string());
    }
    merge_after_install(ebuild_path, root, portage_tmpdir, &env, options)
}

/// Real `merge()`'s own body (`lib/portage/dbapi/vartree.py`), shared by
/// both real `merge` (after a fresh `install` phase run) and real
/// `qmerge` (skipping straight here, assuming `install` already ran) --
/// see `run_merge`/`run_qmerge`'s own doc comments.
fn merge_after_install(
    ebuild_path: &Path,
    root: &Path,
    portage_tmpdir: &Path,
    env: &ebuild_phases::Environment,
    options: &MergeOptions,
) -> Result<i32, String> {
    let ebuild_text = std::fs::read_to_string(&env.ebuild_abs)
        .map_err(|e| format!("{}: {e}", env.ebuild_abs.display()))?;
    let slot = parse_slot(&ebuild_text);
    let repository = repository_name_for(&env.pkg_dir).unwrap_or_else(|| "__unknown__".to_string());

    // Real `self._installed_instance` (`vartree.py:4409-4418`), computed
    // early -- before the vdb write below ever touches this exact
    // category/pf's own real `CONTENTS` -- see `installed_instance_pf`'s
    // own doc comment.
    let installed_instance_pf = installed_instance_pf(root, &env.category, &env.split.pn, &slot);

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
    let blocked = blocked_installed_packages(root, &options.config_root, env, &slot, &repository);
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
        &options.config_root,
        options.shell,
    )?;
    if preinst_status != 0 {
        return Ok(preinst_status);
    }

    let mut cfgfiledict = read_cfgfiledict(root);
    let contents = merge_tree(
        &env.d(),
        root,
        &env.category,
        installed_instance_pf.as_deref(),
        options.protect_if_modified,
        &options.config_protect,
        &options.config_protect_mask,
        options.noconfmem,
        &mut cfgfiledict,
    )?;
    write_cfgfiledict(root, &cfgfiledict)?;
    write_vdb_entry(root, env, &slot, &repository, &contents)?;

    if !plib_collisions.is_empty() {
        let cpv = format!("{}/{}", env.category, env.split.pf);
        unregister_preserved_libs(root, &cpv, plib_registry, &plib_collisions)?;
    }

    // Real `dblink.treewalk()`'s replace loop: now that this version's
    // vdb entry is live, unmerge every same-slot version it replaced --
    // see `unmerge_replaced_same_slot`. Real `treewalk()` order: *after*
    // the vdb write, *before* `pkg_postinst` / `env_update`. A same-cpv
    // `Reinstall` finds nothing to unmerge (`write_vdb_entry` already
    // replaced its own entry), matching the pre-replace-loop behaviour.
    let main_slot = slot.split('/').next().unwrap_or("0");
    let replaced = unmerge_replaced_same_slot(
        root,
        &env.category,
        &env.split.pn,
        &env.split.pf,
        main_slot,
        &env.portage_builddir().join("unmerge-src"),
        portage_tmpdir,
        options,
    )?;

    // Real `merge()`'s own ordering: `postinst` runs, but its own exit
    // status never gates anything after it ("It's stupid to bail out
    // here, so keep going regardless of phase return code") -- real
    // `env_update()` always runs next, as long as anything was actually
    // installed (real `if contents:`) or a replaced version was removed.
    let postinst_status = ebuild_phases::run_single_phase(
        ebuild_path,
        "postinst",
        root,
        portage_tmpdir,
        options.debug,
        &options.config_root,
        options.shell,
    )?;

    if !contents.is_empty() || !replaced.is_empty() {
        env_update::run_env_update(root)?;
    }

    Ok(postinst_status)
}

/// Real `dblink.treewalk()`'s replace loop (`vartree.py:5187-5219`):
/// once the *new* version's vdb entry is live, every already-installed
/// **same-slot** version of the same cp is removed --
/// `dblink.unmerge()` (`pkg_prerm` -> delete its files -> `pkg_postrm`)
/// then `dblink.delete()` (drop its vdb entry). Each `pkg_prerm`/
/// `pkg_postrm` runs from *that* version's own vdb-stored
/// `environment.bz2` + `<pf>.ebuild` (`ebuild_phases::
/// run_phase_from_saved_env`, gated on its recorded `DEFINED_PHASES`) --
/// a version merged before the pilot kept those files, or via a bare
/// `ebuild <file> merge` of an older build, has neither and its rm
/// hooks are skipped (documented degrade). Only the files the new
/// version does **not** itself own are deleted (`also_keep = [new_pf]`,
/// folded into real `others_in_slot`). A replace-loop phase failure is
/// logged, never fatal -- real `treewalk()` there is a literal
/// `# TODO: Check status and abort if necessary` that doesn't.
///
/// A *different*-slot version is left untouched (real slot semantics).
/// `scratch_dir` is where the extracted-from-vdb ebuild is laid out for
/// its phase run (`<scratch_dir>/<cat>/<pn>/<old_pf>.ebuild`, so
/// `compute_environment`'s own `<cat>/<pn>/<pf>.ebuild` path parse
/// works). Returns every replaced `PF` (empty when there was nothing to
/// replace) so the caller can fold it into its own `env_update` gate.
/// Shared by `merge_after_install` (source `emerge <atom>` / `ebuild
/// <file> merge`) and `merge_binpkg`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn unmerge_replaced_same_slot(
    root: &Path,
    category: &str,
    package: &str,
    new_pf: &str,
    main_slot: &str,
    scratch_dir: &Path,
    portage_tmpdir: &Path,
    options: &MergeOptions,
) -> Result<Vec<String>, String> {
    // Real merge-then-unmerge: `<package>-<digit...>` vdb-dir names, the
    // same shape `blocked_installed_packages` and `installed_instance_pf`
    // match; exclude the just-written new entry.
    let vdb_cat = root.join("var/db/pkg").join(category);
    let mut replaced: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&vdb_cat) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let is_this_cp = name.starts_with(&format!("{package}-"))
                && name[package.len() + 1..].starts_with(|c: char| c.is_ascii_digit());
            if !is_this_cp || name == new_pf {
                continue;
            }
            let version = &name[package.len() + 1..];
            if read_installed_slot(root, category, package, version).as_deref() == Some(main_slot) {
                replaced.push(name);
            }
        }
    }
    if replaced.is_empty() {
        return Ok(replaced);
    }

    let keep = [new_pf.to_string()];
    for old_pf in &replaced {
        unmerge_one_installed(
            root,
            category,
            package,
            old_pf,
            &keep,
            scratch_dir,
            portage_tmpdir,
            options,
            None,
        )?;
    }
    Ok(replaced)
}

/// Remove ONE already-installed version from the vdb -- real
/// `dblink.unmerge()` (`pkg_prerm` -> delete its files -> `pkg_postrm`)
/// then `dblink.delete()` (drop the vdb dir) -- for a single
/// `<category>/<pf>`. Both phase hooks run from *that version's own*
/// vdb-stored `environment.bz2` + `<pf>.ebuild`
/// (`ebuild_phases::run_phase_from_saved_env`, gated on its recorded
/// `DEFINED_PHASES`); a version installed before the pilot kept those
/// files has neither and its rm hooks are skipped (documented degrade).
/// A phase failure is logged, never fatal -- real `treewalk()`'s replace
/// loop is a literal `# TODO: Check status and abort if necessary` that
/// doesn't, and real `emerge -C`'s own loop only aborts on the
/// file-removal core failing, not on a phase.
///
/// `also_keep` is folded into real `others_in_slot` so a path a
/// replacing version now owns is left in place -- empty for a standalone
/// `emerge -C`, `[new_pf]` for `treewalk()`'s replace loop.
/// `scratch_dir` holds the ebuild extracted from the vdb for its phase
/// run (`<scratch_dir>/<cat>/<pn>/<pf>.ebuild`, so
/// `compute_environment`'s path parse works). Shared by
/// `unmerge_replaced_same_slot` and `pretend.rs`'s real `emerge -C`.
///
/// `backup` is `Some` only for the standalone removal paths with
/// `FEATURES=unmerge-backup` (real `dblink._pre_unmerge_backup`, run at
/// the very top of `dblink.unmerge()` -- before `pkg_prerm`, before a
/// single file is touched): a `quickpkg` of the still-installed package
/// into `$PKGDIR` (`ebuild_package::quickpkg_from_vdb`). A quickpkg
/// failure aborts this package's unmerge, real `unmerge()`'s own
/// `if retval != os.EX_OK: ... return retval`. `treewalk()`'s replace
/// loop passes `None` -- its own `_pre_merge_backup`/`downgrade-backup`
/// path is a documented cut.
#[allow(clippy::too_many_arguments)]
pub(crate) fn unmerge_one_installed(
    root: &Path,
    category: &str,
    package: &str,
    pf: &str,
    also_keep: &[String],
    scratch_dir: &Path,
    portage_tmpdir: &Path,
    options: &MergeOptions,
    backup: Option<&crate::ebuild_package::PackageOptions>,
) -> Result<(), String> {
    let vdb_dir = root.join("var/db/pkg").join(category).join(pf);
    let unmerge_options = crate::ebuild_unmerge::UnmergeOptions {
        debug: options.debug,
        shell: options.shell,
        config_protect: options.config_protect.clone(),
        config_protect_mask: options.config_protect_mask.clone(),
        config_root: options.config_root.clone(),
        ..Default::default()
    };

    if let Some(pkg_options) = backup {
        match crate::ebuild_package::quickpkg_from_vdb(
            root,
            category,
            package,
            pf,
            scratch_dir,
            portage_tmpdir,
            pkg_options,
            &options.config_protect,
            &options.config_protect_mask,
        ) {
            Ok(Some(path)) => {
                println!(">>> Building backup package for {category}/{pf}");
                println!(">>> Wrote {}", path.display());
            }
            Ok(None) => {}
            Err(e) => return Err(format!("!!! FAILED prerm: quickpkg: {e}")),
        }
    }

    let run_hook = |phase: &str| -> Result<i32, String> {
        let defined = std::fs::read_to_string(vdb_dir.join("DEFINED_PHASES")).unwrap_or_default();
        if !vdb_dir.join("environment.bz2").is_file()
            || !vdb_dir.join(format!("{pf}.ebuild")).is_file()
            || !defined.split_whitespace().any(|d| d == phase)
        {
            return Ok(0);
        }
        run_vdb_saved_env_phase(
            root,
            category,
            package,
            pf,
            phase,
            scratch_dir,
            portage_tmpdir,
            options,
        )
    };

    let prerm_status = run_hook("prerm")?;
    if prerm_status != 0 {
        eprintln!("{category}/{pf}: FAILED prerm ({prerm_status}) -- unmerge continues");
    }
    crate::ebuild_unmerge::unmerge_pkgfiles(
        root,
        category,
        package,
        pf,
        also_keep,
        &unmerge_options,
    )?;
    let postrm_status = run_hook("postrm")?;
    if postrm_status != 0 {
        eprintln!("{category}/{pf}: FAILED postrm ({postrm_status}) -- unmerge continues");
    }
    crate::ebuild_unmerge::delete_vdb_dir(root, category, pf)?;
    Ok(())
}

/// Run one phase for an already-installed `<category>/<pf>` straight from
/// its own vdb-stored `environment.bz2` + `<pf>.ebuild`, copied into
/// `scratch_dir` as `<cat>/<pn>/<pf>.ebuild` so `compute_environment`'s
/// `<cat>/<pn>/<pf>.ebuild` path parse works (the vdb layout is
/// `<cat>/<pf>/<pf>.ebuild`). Errors if the vdb entry carries no saved
/// environment or ebuild (a package installed before the pilot started
/// keeping them). Unlike `unmerge_one_installed`'s own internal hook
/// runner this does **not** gate on `DEFINED_PHASES` -- the caller
/// decides (real `emerge --config` runs `pkg_config` unconditionally,
/// real `doebuild(ebuildpath, "config", ...)`). Shared by
/// `unmerge_one_installed`'s `pkg_prerm`/`pkg_postrm` and
/// `pretend.rs::run_config_action`'s `pkg_config`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_vdb_saved_env_phase(
    root: &Path,
    category: &str,
    package: &str,
    pf: &str,
    phase: &str,
    scratch_dir: &Path,
    portage_tmpdir: &Path,
    options: &MergeOptions,
) -> Result<i32, String> {
    let vdb_dir = root.join("var/db/pkg").join(category).join(pf);
    let env = vdb_dir.join("environment.bz2");
    let ebuild = vdb_dir.join(format!("{pf}.ebuild"));
    if !env.is_file() || !ebuild.is_file() {
        return Err(format!(
            "{}: no saved build environment (installed before the pilot kept one?)",
            vdb_dir.display()
        ));
    }
    let src_dir = scratch_dir.join(category).join(package);
    std::fs::create_dir_all(&src_dir).map_err(|e| format!("{}: {e}", src_dir.display()))?;
    let dst = src_dir.join(format!("{pf}.ebuild"));
    std::fs::copy(&ebuild, &dst).map_err(|e| format!("{}: {e}", ebuild.display()))?;
    crate::ebuild_phases::run_phase_from_saved_env(
        &dst,
        &env,
        phase,
        root,
        portage_tmpdir,
        options.debug,
        &options.config_root,
        options.shell,
    )
}

/// Merge an already-downloaded binary package (`.tbz2` xpak or
/// `.gpkg.tar`) into `root`'s vdb -- real portage's `_emerge/Binpkg`
/// task, narrowed. Extracts the binpkg image, copies it into `${ROOT}`
/// exactly as `merge_tree` does for a source build (CONFIG_PROTECT
/// included), writes the vdb entry from the binpkg's own metadata plus
/// the freshly-generated `CONTENTS`, and runs `env_update()`/`ldconfig`.
///
/// All four install/remove `pkg_*` phase hooks run, each from a saved
/// bash environment (`ebuild_phases::run_phase_from_saved_env`) and only
/// when the relevant `DEFINED_PHASES` names it (real `_defined_phases`),
/// so a binpkg that defines none -- the common case -- spawns no shell:
///   - `pkg_setup` -> `pkg_preinst` from the *new* binpkg's own
///     `environment.bz2` + `<pf>.ebuild`, before a single file is copied
///     (real `_emerge/Binpkg`: `setup` is an `EbuildPhase` right after
///     metadata extraction; `dblink.treewalk()` runs `preinst` before
///     `mergeme()`).
///   - for every same-slot version this merge replaces, real
///     `dblink.unmerge()` inside `treewalk()`'s replace loop:
///     `pkg_prerm`, then remove that version's files, then `pkg_postrm`,
///     then drop its vdb entry -- each phase from *that* version's own
///     vdb-stored `environment.bz2` + `<pf>.ebuild`. A phase failure
///     here is logged, not fatal (real "TODO: Check status and abort if
///     necessary" -- it doesn't).
///   - `pkg_postinst` from the new binpkg, after the vdb entry is live
///     and every replaced version is gone, before `env_update()`.
///
/// `FEATURES=collision-protect` / `protect-owned`, real blocker
/// exclusion (`mypkglist = others_in_slot + blockers` -- the blocker
/// term reads the binpkg's already-USE-reduced `*DEPEND` build-info
/// files, `blockers_from_flat_deps`), and preserve-libs collision
/// exclusion + `unregister_preserved_libs` all run now, identical to the
/// source `merge_after_install`.
///
/// **v1 cuts, all deliberate** (same "narrow the first slice, document
/// it" pattern as every other real-execution feature here):
///   - a binpkg (or a replaced version) carrying no `environment.bz2` /
///     `<pf>.ebuild` -- older, or built before the pilot kept them --
///     gets no hooks: a documented degrade, not a fallback to
///     re-sourcing the ebuild.
///   - a *different*-slot installed version is left untouched (real slot
///     semantics); the replace also skips the preserve-libs /
///     reverse-dependency check `dblink.unmerge()` would otherwise do.
pub fn merge_binpkg(
    binpkg_path: &Path,
    root: &Path,
    portage_tmpdir: &Path,
    options: &MergeOptions,
) -> Result<i32, String> {
    // Peek the embedded metadata first -- real portage knows the cpv
    // (and so `${PORTAGE_BUILDDIR}`) before it extracts anything. This
    // lets the image land straight in `${PORTAGE_BUILDDIR}/image`, the
    // exact `${D}` a real `pkg_preinst`/`pkg_postinst` expects.
    let name = binpkg_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let meta = if name.ends_with(".gpkg.tar") {
        crate::binpkg::read_gpkg_metadata(binpkg_path)?
    } else {
        crate::binpkg::read_xpak_metadata(binpkg_path)?
    };
    let meta_get = |key: &str| -> Option<String> {
        meta.get(key)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let category = meta_get("CATEGORY")
        .ok_or_else(|| format!("{}: binpkg has no CATEGORY", binpkg_path.display()))?;
    let pf =
        meta_get("PF").ok_or_else(|| format!("{}: binpkg has no PF", binpkg_path.display()))?;
    let package = pf
        .rsplit_once('-')
        .and_then(|(rest, last)| {
            // strip `-<version>` (and an optional `-r<rev>`)
            if last.starts_with(|c: char| c.is_ascii_digit()) {
                if let Some((pn, v)) = rest.rsplit_once('-') {
                    if v.starts_with('r') && v[1..].chars().all(|c| c.is_ascii_digit()) {
                        return Some(pn.to_string());
                    }
                }
                Some(rest.to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| pf.clone());
    // Real vdb `SLOT` keeps the full `slot/sub_slot`; `merge_tree` only
    // wants the main slot for the installed-instance lookup.
    let full_slot = meta_get("SLOT").unwrap_or_else(|| "0".to_string());
    let main_slot = full_slot.split('/').next().unwrap_or("0").to_string();
    let repository = meta_get("repository")
        .or_else(|| meta_get("REPO"))
        .unwrap_or_else(|| "__unknown__".to_string());

    // Real `${PORTAGE_BUILDDIR}` = `${PORTAGE_TMPDIR}/portage/<cat>/<pf>`
    // -- `ebuild_phases::compute_environment` derives the same path from
    // the (extracted) ebuild, so this is exactly where a `pkg_preinst`/
    // `pkg_postinst` run will look for `${D}` / `${T}`.
    let builddir = portage_tmpdir.join("portage").join(&category).join(&pf);
    if builddir.exists() {
        std::fs::remove_dir_all(&builddir).map_err(|e| format!("{}: {e}", builddir.display()))?;
    }
    let image = builddir.join("image");
    let build_info = builddir.join("build-info");
    crate::binpkg::extract_binpkg(binpkg_path, &image, &build_info)?;

    // Real `_emerge/Binpkg`: `pkg_setup`/`pkg_preinst`/`pkg_postinst`
    // run from the extracted `<pf>.ebuild` + the `bunzip2`'d
    // `environment.bz2` (see `ebuild_phases::run_phase_from_saved_env`).
    // Gated on `DEFINED_PHASES` (real `_defined_phases`) so a binpkg
    // that defines neither -- the common case -- spawns no shell at all.
    // Both files must be present (an older binpkg, or one built before
    // the pilot kept them, gets no hooks -- a documented degrade).
    let defined_phases = meta_get("DEFINED_PHASES").unwrap_or_default();
    let phase_defined = |p: &str| defined_phases.split_whitespace().any(|d| d == p);
    let saved_env = build_info.join("environment.bz2");
    let extracted_ebuild = {
        let src = build_info.join(format!("{pf}.ebuild"));
        if src.is_file() && saved_env.is_file() {
            let pkgdir = builddir.join("ebuild-src").join(&category).join(&package);
            std::fs::create_dir_all(&pkgdir).map_err(|e| format!("{}: {e}", pkgdir.display()))?;
            let dst = pkgdir.join(format!("{pf}.ebuild"));
            std::fs::copy(&src, &dst).map_err(|e| format!("{}: {e}", src.display()))?;
            Some(dst)
        } else {
            None
        }
    };
    let run_hook = |phase: &str| -> Result<i32, String> {
        match &extracted_ebuild {
            Some(ebuild) if phase_defined(phase) => crate::ebuild_phases::run_phase_from_saved_env(
                ebuild,
                &saved_env,
                phase,
                root,
                portage_tmpdir,
                options.debug,
                &options.config_root,
                options.shell,
            ),
            _ => Ok(0),
        }
    };

    // Real `_emerge/Binpkg` order: `pkg_setup` (an `EbuildPhase`) runs
    // right after the metadata is extracted, before `unpack_contents` /
    // the merge.
    let setup_status = run_hook("setup")?;
    if setup_status != 0 {
        return Ok(setup_status);
    }

    // Real `dblink.merge()`'s own `_collision_protect` check, run before
    // `pkg_preinst` and the file copy (real `treewalk()` ordering) --
    // now shared with the source `merge_after_install`. A binary package
    // carries no ebuild/repo, so `mypkglist`'s blocker term
    // (`blockers_from_flat_deps`) reads the already-USE-reduced
    // `*DEPEND` build-info files directly. The preserve-libs registry is
    // consulted unconditionally (real `_plib_registry` is never `None`).
    let plib_registry = read_plib_registry(root);
    let plib_inodes = plib_inode_map(root, &plib_registry.preserved_libs());
    let mut flat_deps: Vec<String> = Vec::new();
    for dep_key in ["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"] {
        if let Ok(d) = std::fs::read_to_string(build_info.join(dep_key)) {
            flat_deps.extend(d.split_whitespace().map(String::from));
        }
    }
    let blocked = blockers_from_flat_deps(root, &flat_deps);
    let (collisions, symlink_collisions, plib_collisions) = find_collisions(
        &image,
        root,
        &category,
        &package,
        &main_slot,
        &options.config_protect,
        &options.config_protect_mask,
        &plib_inodes,
        &blocked,
    )?;
    // Real `dblink.merge()`'s own abort condition (Python operator
    // precedence: `collision_protect or (protect_owned and owners)`);
    // identical to `merge_after_install`.
    let protect_owned_abort = options.protect_owned
        && !collisions.is_empty()
        && !find_owners(root, &collisions).is_empty();
    if !symlink_collisions.is_empty()
        || (options.collision_protect && !collisions.is_empty())
        || protect_owned_abort
    {
        let cpv = format!("{category}/{pf}");
        return Err(collision_message(
            root,
            &cpv,
            &collisions,
            &symlink_collisions,
        ));
    }

    // Real `dblink.treewalk()` order: `pkg_preinst` runs before a single
    // file is copied.
    let preinst_status = run_hook("preinst")?;
    if preinst_status != 0 {
        return Ok(preinst_status);
    }

    let installed_instance = installed_instance_pf(root, &category, &package, &main_slot);
    let mut cfgfiledict = read_cfgfiledict(root);
    let contents = merge_tree(
        &image,
        root,
        &category,
        installed_instance.as_deref(),
        options.protect_if_modified,
        &options.config_protect,
        &options.config_protect_mask,
        options.noconfmem,
        &mut cfgfiledict,
    )?;
    write_cfgfiledict(root, &cfgfiledict)?;
    write_vdb_entry_from_dir(
        root,
        &category,
        &pf,
        &build_info,
        &full_slot,
        &repository,
        &contents,
    )?;

    // Real `treewalk()`: a preserved lib this new version now provides
    // itself is taken over from the `preserved_libs_registry` and
    // stripped from the previous owner's `CONTENTS` -- identical to
    // `merge_after_install`.
    if !plib_collisions.is_empty() {
        let cpv = format!("{category}/{pf}");
        unregister_preserved_libs(root, &cpv, plib_registry, &plib_collisions)?;
    }

    // Real merge-then-unmerge: the new version's vdb entry now exists,
    // so drop every same-slot version it replaced (see
    // `unmerge_replaced_same_slot`).
    let replaced_same_slot = unmerge_replaced_same_slot(
        root,
        &category,
        &package,
        &pf,
        &main_slot,
        &builddir.join("unmerge-src"),
        portage_tmpdir,
        options,
    )?;

    // Real `treewalk()` order: `pkg_postinst` runs after the vdb entry
    // is live *and* every replaced same-slot version is gone, but before
    // `env_update()`. Its own non-zero exit is logged, never fatal (real
    // `_postinst_failure` -- "It's stupid to bail out here").
    let postinst_status = run_hook("postinst")?;
    if postinst_status != 0 {
        eprintln!(
            "{category}/{pf}: FAILED postinst ({postinst_status}) -- merge kept (real _postinst_failure)"
        );
    }

    if !contents.is_empty() || !replaced_same_slot.is_empty() {
        env_update::run_env_update(root)?;
    }

    let _ = std::fs::remove_dir_all(&builddir);
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
    fn is_real_qmerge_command_covers_exactly_qmerge() {
        assert!(is_real_qmerge_command("qmerge"));
        assert!(!is_real_qmerge_command("merge"));
        assert!(!is_real_qmerge_command("unmerge"));
        assert!(!is_real_qmerge_command("install"));
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
    fn installed_instance_pf_picks_the_highest_counter_same_slot_version() {
        let tmp = tempdir();
        let root = tmp.join("ROOT");
        for (version, slot, counter) in [("1.0", "0", "3"), ("2.0", "0", "7"), ("3.0", "1", "99")] {
            let vdb_dir = root
                .join("var/db/pkg/dev-libs")
                .join(format!("instpkg-{version}"));
            std::fs::create_dir_all(&vdb_dir).unwrap();
            std::fs::write(vdb_dir.join("SLOT"), format!("{slot}\n")).unwrap();
            std::fs::write(vdb_dir.join("COUNTER"), counter).unwrap();
        }

        assert_eq!(
            installed_instance_pf(&root, "dev-libs", "instpkg", "0"),
            Some("instpkg-2.0".to_string()),
            "the higher-COUNTER same-slot version wins, not the higher version number"
        );
        assert_eq!(
            installed_instance_pf(&root, "dev-libs", "instpkg", "2"),
            None,
            "no installed version at all in this slot"
        );
    }

    #[test]
    fn owned_node_value_pf_reads_an_obj_and_a_sym_entrys_own_value() {
        let tmp = tempdir();
        let root = tmp.join("ROOT");
        let vdb_dir = root.join("var/db/pkg/dev-libs/instpkg-1.0");
        std::fs::create_dir_all(&vdb_dir).unwrap();
        std::fs::write(
            vdb_dir.join("CONTENTS"),
            "obj /etc/foo.conf abc123 100\nsym /etc/link -> target 100\ndir /etc\n",
        )
        .unwrap();

        assert_eq!(
            owned_node_value_pf(&root, "dev-libs", "instpkg-1.0", "/etc/foo.conf"),
            Some(("obj".to_string(), "abc123".to_string()))
        );
        assert_eq!(
            owned_node_value_pf(&root, "dev-libs", "instpkg-1.0", "/etc/link"),
            Some(("sym".to_string(), "target".to_string()))
        );
        assert_eq!(
            owned_node_value_pf(&root, "dev-libs", "instpkg-1.0", "/etc"),
            None,
            "a dir entry has no MD5/target value at all"
        );
        assert_eq!(
            owned_node_value_pf(&root, "dev-libs", "instpkg-1.0", "/etc/nope"),
            None
        );
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
        let contents = merge_tree(
            &d,
            &root,
            "dev-libs",
            None,
            false,
            "/etc",
            "/etc/env.d",
            false,
            &mut cfgfiledict,
        )
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
        // A newmd5 that never matches any of these files' own content --
        // isolates this test from the reuse behavior covered by
        // new_protect_filename_reuses_last_file_with_matching_content
        // below.
        let no_match = "no-such-md5";

        assert_eq!(
            new_protect_filename(&dest, no_match).unwrap(),
            tmp.join("._cfg0000_foo.conf")
        );

        std::fs::write(tmp.join("._cfg0000_foo.conf"), b"x").unwrap();
        assert_eq!(
            new_protect_filename(&dest, no_match).unwrap(),
            tmp.join("._cfg0001_foo.conf")
        );

        std::fs::write(tmp.join("._cfg0007_foo.conf"), b"x").unwrap();
        assert_eq!(
            new_protect_filename(&dest, no_match).unwrap(),
            tmp.join("._cfg0008_foo.conf")
        );

        // A same-prefixed file for a *different* basename doesn't count.
        std::fs::write(tmp.join("._cfg0099_other.conf"), b"x").unwrap();
        assert_eq!(
            new_protect_filename(&dest, no_match).unwrap(),
            tmp.join("._cfg0008_foo.conf")
        );
    }

    #[test]
    fn new_protect_filename_reuses_last_file_with_matching_content() {
        let tmp = tempdir();
        std::fs::create_dir_all(&tmp).unwrap();
        let dest = tmp.join("foo.conf");
        std::fs::write(tmp.join("._cfg0000_foo.conf"), b"old update").unwrap();
        std::fs::write(tmp.join("._cfg0001_foo.conf"), b"newest update").unwrap();
        let newest_md5 = md5_hex(&tmp.join("._cfg0001_foo.conf")).unwrap();

        // The highest-numbered sibling's own content already matches --
        // reuse it instead of allocating ._cfg0002_.
        assert_eq!(
            new_protect_filename(&dest, &newest_md5).unwrap(),
            tmp.join("._cfg0001_foo.conf")
        );

        // A newmd5 that doesn't match the highest-numbered sibling
        // allocates a fresh number as usual, even though an *older*
        // sibling (._cfg0000_) would have matched -- real
        // new_protect_filename() only ever compares against the last one.
        let old_md5 = md5_hex(&tmp.join("._cfg0000_foo.conf")).unwrap();
        assert_eq!(
            new_protect_filename(&dest, &old_md5).unwrap(),
            tmp.join("._cfg0002_foo.conf")
        );
    }

    #[test]
    fn new_protect_filename_reuses_last_symlink_with_matching_target() {
        let tmp = tempdir();
        std::fs::create_dir_all(&tmp).unwrap();
        let dest = tmp.join("link.conf");
        std::os::unix::fs::symlink("old-target", tmp.join("._cfg0000_link.conf")).unwrap();

        assert_eq!(
            new_protect_filename(&dest, "old-target").unwrap(),
            tmp.join("._cfg0000_link.conf")
        );
        assert_eq!(
            new_protect_filename(&dest, "new-target").unwrap(),
            tmp.join("._cfg0001_link.conf")
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
        merge_tree(
            &d,
            &root,
            "dev-libs",
            None,
            false,
            "/etc",
            "",
            false,
            &mut cfgfiledict,
        )
        .expect("merge_tree succeeds");

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
        merge_tree(
            &d,
            &root,
            "dev-libs",
            None,
            false,
            "/etc",
            "",
            false,
            &mut cfgfiledict,
        )
        .expect("merge_tree succeeds");

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
        let contents = merge_tree(
            &d,
            &root,
            "dev-libs",
            None,
            false,
            "/etc",
            "",
            false,
            &mut cfgfiledict,
        )
        .expect("merge_tree succeeds");

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
    fn merge_tree_protect_if_modified_applies_directly_when_dest_still_matches_the_installed_instance(
    ) {
        // Real `_installed_instance`/`protect_if_modified`
        // (`vartree.py:5849-5866`): the live destination still holds
        // *exactly* what the previous same-slot instance's own real
        // CONTENTS recorded -- the admin never touched it -- so even
        // though it differs from the new src content, it's not
        // "modified" in the sense this feature cares about, and the new
        // content is applied directly instead of diverted.
        let tmp = tempdir();
        let d = tmp.join("D");
        let root = tmp.join("ROOT");
        std::fs::create_dir_all(d.join("etc")).unwrap();
        std::fs::write(d.join("etc/foo.conf"), b"new content").unwrap();
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(root.join("etc/foo.conf"), b"old content").unwrap();
        let old_md5 = md5_hex(&root.join("etc/foo.conf")).unwrap();

        let vdb_dir = root.join("var/db/pkg/dev-libs/foopkg-1.0");
        std::fs::create_dir_all(&vdb_dir).unwrap();
        std::fs::write(vdb_dir.join("SLOT"), "0\n").unwrap();
        std::fs::write(
            vdb_dir.join("CONTENTS"),
            format!("obj /etc/foo.conf {old_md5} 100\n"),
        )
        .unwrap();
        std::fs::write(vdb_dir.join("COUNTER"), "5").unwrap();

        let mut cfgfiledict = BTreeMap::new();
        let contents = merge_tree(
            &d,
            &root,
            "dev-libs",
            Some("foopkg-1.0"),
            true,
            "/etc",
            "",
            false,
            &mut cfgfiledict,
        )
        .expect("merge_tree succeeds");

        assert_eq!(
            std::fs::read_to_string(root.join("etc/foo.conf")).unwrap(),
            "new content",
            "unmodified-since-installed content is overwritten directly, not protected"
        );
        assert!(!root.join("etc/._cfg0000_foo.conf").exists());
        let new_md5 = md5_hex(&d.join("etc/foo.conf")).unwrap();
        assert!(contents
            .lines()
            .any(|l| l.starts_with(&format!("obj /etc/foo.conf {new_md5} "))));
    }

    #[test]
    fn merge_tree_still_protects_a_locally_modified_file_despite_protect_if_modified() {
        // Same setup as above, but the live destination's own content no
        // longer matches what the installed instance recorded -- the
        // admin *did* modify it locally, so protect_if_modified must not
        // waive protection here.
        let tmp = tempdir();
        let d = tmp.join("D");
        let root = tmp.join("ROOT");
        std::fs::create_dir_all(d.join("etc")).unwrap();
        std::fs::write(d.join("etc/foo.conf"), b"new content").unwrap();
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(root.join("etc/foo.conf"), b"the admin's own local edits").unwrap();

        let vdb_dir = root.join("var/db/pkg/dev-libs/foopkg-1.0");
        std::fs::create_dir_all(&vdb_dir).unwrap();
        std::fs::write(vdb_dir.join("SLOT"), "0\n").unwrap();
        std::fs::write(
            vdb_dir.join("CONTENTS"),
            "obj /etc/foo.conf deadbeefdeadbeefdeadbeefdeadbeef 100\n",
        )
        .unwrap();
        std::fs::write(vdb_dir.join("COUNTER"), "5").unwrap();

        let mut cfgfiledict = BTreeMap::new();
        merge_tree(
            &d,
            &root,
            "dev-libs",
            Some("foopkg-1.0"),
            true,
            "/etc",
            "",
            false,
            &mut cfgfiledict,
        )
        .expect("merge_tree succeeds");

        assert_eq!(
            std::fs::read_to_string(root.join("etc/foo.conf")).unwrap(),
            "the admin's own local edits",
            "locally-modified content is still protected"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("etc/._cfg0000_foo.conf")).unwrap(),
            "new content"
        );
    }

    #[test]
    fn merge_tree_force_protects_a_path_the_installed_instance_recorded_but_the_admin_deleted() {
        // Real bug #523684 (`vartree.py:5852-5859`): the installed
        // instance's own CONTENTS recorded this exact path, but nothing
        // exists there on disk at all right now (the admin deleted or
        // renamed it) -- real `force = True` diverts into a fresh
        // ._cfgNNNN_ sibling instead of silently re-creating the path
        // the admin deliberately removed, even though there's nothing
        // to compare content against.
        let tmp = tempdir();
        let d = tmp.join("D");
        let root = tmp.join("ROOT");
        std::fs::create_dir_all(d.join("etc")).unwrap();
        std::fs::write(d.join("etc/foo.conf"), b"new content").unwrap();
        std::fs::create_dir_all(root.join("etc")).unwrap();

        let vdb_dir = root.join("var/db/pkg/dev-libs/foopkg-1.0");
        std::fs::create_dir_all(&vdb_dir).unwrap();
        std::fs::write(vdb_dir.join("SLOT"), "0\n").unwrap();
        std::fs::write(
            vdb_dir.join("CONTENTS"),
            "obj /etc/foo.conf deadbeefdeadbeefdeadbeefdeadbeef 100\n",
        )
        .unwrap();
        std::fs::write(vdb_dir.join("COUNTER"), "5").unwrap();

        let mut cfgfiledict = BTreeMap::new();
        merge_tree(
            &d,
            &root,
            "dev-libs",
            Some("foopkg-1.0"),
            false,
            "/etc",
            "",
            false,
            &mut cfgfiledict,
        )
        .expect("merge_tree succeeds");

        assert!(
            !root.join("etc/foo.conf").exists(),
            "the admin's own deletion is respected -- nothing is silently re-created"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("etc/._cfg0000_foo.conf")).unwrap(),
            "new content"
        );
    }

    #[test]
    fn merge_tree_remembers_an_already_offered_update_and_leaves_the_live_file_untouched() {
        // Real `move_me = protected = bool(cfgfiledict["IGNORE"])` with
        // `IGNORE == 0` (`vartree.py:5877`, "confmem rejected this
        // update"): re-merging an already-offered, unmodified-since
        // update skips the write entirely -- the live destination stays
        // exactly what the admin last left it as, no second `._cfg0001_`
        // file spawned either. See `protect_decision`'s own doc comment
        // for the full real trace.
        let tmp = tempdir();
        let d = tmp.join("D");
        let root = tmp.join("ROOT");
        std::fs::create_dir_all(d.join("etc")).unwrap();
        std::fs::write(d.join("etc/foo.conf"), b"new content").unwrap();
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(root.join("etc/foo.conf"), b"user's own edits").unwrap();

        let mut cfgfiledict = BTreeMap::new();
        merge_tree(
            &d,
            &root,
            "dev-libs",
            None,
            false,
            "/etc",
            "",
            false,
            &mut cfgfiledict,
        )
        .expect("first merge_tree succeeds");
        assert!(root.join("etc/._cfg0000_foo.conf").exists());

        // Re-merging the exact same new content again: already
        // remembered in cfgfiledict, so real portage leaves the live
        // destination completely untouched this time -- no second
        // ._cfg0001_ file spawned either.
        let contents = merge_tree(
            &d,
            &root,
            "dev-libs",
            None,
            false,
            "/etc",
            "",
            false,
            &mut cfgfiledict,
        )
        .expect("second merge_tree succeeds");
        assert_eq!(
            std::fs::read_to_string(root.join("etc/foo.conf")).unwrap(),
            "user's own edits",
            "the admin's own live edits must survive a re-offered, already-remembered update"
        );
        assert!(!root.join("etc/._cfg0001_foo.conf").exists());

        // CONTENTS still logically records this package as the owner of
        // the *new* content, using the source's own MD5 -- real
        // `mergeme()`'s own `mymtime = mystat.st_mtime_ns` (the source's
        // own mtime, set before the real `if moveme:` gate and never
        // touched when it's skipped) flowing into `_format_contents_line`
        // regardless of `moveme`.
        let new_md5 = md5_hex(&d.join("etc/foo.conf")).unwrap();
        assert!(
            contents.contains(&format!("obj /etc/foo.conf {new_md5}")),
            "{contents}"
        );
    }

    #[test]
    fn merge_tree_noconfmem_reprotects_an_already_offered_update() {
        let tmp = tempdir();
        let d = tmp.join("D");
        let root = tmp.join("ROOT");
        std::fs::create_dir_all(d.join("etc")).unwrap();
        std::fs::write(d.join("etc/foo.conf"), b"new content").unwrap();
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(root.join("etc/foo.conf"), b"user's own edits").unwrap();

        let mut cfgfiledict = BTreeMap::new();
        merge_tree(
            &d,
            &root,
            "dev-libs",
            None,
            false,
            "/etc",
            "",
            false,
            &mut cfgfiledict,
        )
        .expect("first merge_tree succeeds");
        assert!(root.join("etc/._cfg0000_foo.conf").exists());

        // Re-merging the exact same content with NOCONFMEM-equivalent
        // (noconfmem=true): unlike the default (dest gets directly
        // overwritten, see the test above), this forces re-protection
        // every time regardless of cfgfiledict memory -- real
        // `--noconfmem`/`cfgfiledict["IGNORE"]` -- so the logical path is
        // left alone again. `new_protect_filename`'s own "reuse the last
        // file when content already matches" logic (this slice's own
        // third piece) then reuses ._cfg0000_ rather than spawning a
        // ._cfg0001_ with identical content, so the *visible* difference
        // from the default isn't a new numbered file -- it's that the
        // logical path itself is protected instead of overwritten.
        merge_tree(
            &d,
            &root,
            "dev-libs",
            None,
            false,
            "/etc",
            "",
            true,
            &mut cfgfiledict,
        )
        .expect("second merge_tree succeeds");
        assert!(!root.join("etc/._cfg0001_foo.conf").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("etc/._cfg0000_foo.conf")).unwrap(),
            "new content"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("etc/foo.conf")).unwrap(),
            "user's own edits"
        );
    }

    #[test]
    fn merge_tree_protects_a_changed_symlink_under_a_protected_path() {
        let tmp = tempdir();
        let d = tmp.join("D");
        let root = tmp.join("ROOT");
        std::fs::create_dir_all(d.join("etc")).unwrap();
        std::os::unix::fs::symlink("new-target", d.join("etc/link.conf")).unwrap();
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::os::unix::fs::symlink("users-own-target", root.join("etc/link.conf")).unwrap();

        let mut cfgfiledict = BTreeMap::new();
        let contents = merge_tree(
            &d,
            &root,
            "dev-libs",
            None,
            false,
            "/etc",
            "",
            false,
            &mut cfgfiledict,
        )
        .expect("merge_tree succeeds");

        // The real, logical path is untouched...
        assert_eq!(
            std::fs::read_link(root.join("etc/link.conf")).unwrap(),
            PathBuf::from("users-own-target")
        );
        // ...and the new target lands in a ._cfg0000_ sibling instead.
        assert_eq!(
            std::fs::read_link(root.join("etc/._cfg0000_link.conf")).unwrap(),
            PathBuf::from("new-target")
        );
        // CONTENTS still records the logical path with the *new* target.
        assert!(contents
            .lines()
            .any(|l| l.starts_with("sym /etc/link.conf -> new-target")));
    }

    #[test]
    fn merge_tree_does_not_protect_an_unchanged_symlink() {
        let tmp = tempdir();
        let d = tmp.join("D");
        let root = tmp.join("ROOT");
        std::fs::create_dir_all(d.join("etc")).unwrap();
        std::os::unix::fs::symlink("same-target", d.join("etc/link.conf")).unwrap();
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::os::unix::fs::symlink("same-target", root.join("etc/link.conf")).unwrap();

        let mut cfgfiledict = BTreeMap::new();
        merge_tree(
            &d,
            &root,
            "dev-libs",
            None,
            false,
            "/etc",
            "",
            false,
            &mut cfgfiledict,
        )
        .expect("merge_tree succeeds");

        assert_eq!(
            std::fs::read_link(root.join("etc/link.conf")).unwrap(),
            PathBuf::from("same-target")
        );
        assert!(!root.join("etc/._cfg0000_link.conf").exists());
    }

    #[test]
    fn merge_tree_protects_a_symlink_source_replacing_a_regular_file_dest() {
        // Real dblink._protect()'s own type-independent comparison
        // (vartree.py:5434-5480): dest_md5/dest_link are computed from
        // the live destination's own on-disk type regardless of the
        // incoming source's type -- a symlink source landing on a path
        // the admin's own regular file still occupies is real-protected
        // too, not silently overwritten.
        let tmp = tempdir();
        let d = tmp.join("D");
        let root = tmp.join("ROOT");
        std::fs::create_dir_all(d.join("etc")).unwrap();
        std::os::unix::fs::symlink("new-target", d.join("etc/thing.conf")).unwrap();
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(root.join("etc/thing.conf"), b"the admin's own regular file").unwrap();

        let mut cfgfiledict = BTreeMap::new();
        let contents = merge_tree(
            &d,
            &root,
            "dev-libs",
            None,
            false,
            "/etc",
            "",
            false,
            &mut cfgfiledict,
        )
        .expect("merge_tree succeeds");

        // The admin's own regular file at the logical path is untouched...
        assert_eq!(
            std::fs::read_to_string(root.join("etc/thing.conf")).unwrap(),
            "the admin's own regular file"
        );
        // ...and the new symlink lands in a ._cfg0000_ sibling instead.
        assert_eq!(
            std::fs::read_link(root.join("etc/._cfg0000_thing.conf")).unwrap(),
            PathBuf::from("new-target")
        );
        assert!(contents
            .lines()
            .any(|l| l.starts_with("sym /etc/thing.conf -> new-target")));
    }

    #[test]
    fn merge_tree_protects_a_regular_file_source_replacing_a_symlink_dest() {
        // The mirror image of the test above: an `obj` source landing on
        // a path a symlink (admin-installed, or left over from a
        // previous, differently-shaped package version) still occupies.
        let tmp = tempdir();
        let d = tmp.join("D");
        let root = tmp.join("ROOT");
        std::fs::create_dir_all(d.join("etc")).unwrap();
        std::fs::write(d.join("etc/thing.conf"), b"new regular content").unwrap();
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::os::unix::fs::symlink("admins-own-target", root.join("etc/thing.conf")).unwrap();

        let mut cfgfiledict = BTreeMap::new();
        let contents = merge_tree(
            &d,
            &root,
            "dev-libs",
            None,
            false,
            "/etc",
            "",
            false,
            &mut cfgfiledict,
        )
        .expect("merge_tree succeeds");

        // The admin's own symlink at the logical path is untouched...
        assert_eq!(
            std::fs::read_link(root.join("etc/thing.conf")).unwrap(),
            PathBuf::from("admins-own-target")
        );
        // ...and the new regular-file content lands in a ._cfg0000_
        // sibling instead.
        assert_eq!(
            std::fs::read_to_string(root.join("etc/._cfg0000_thing.conf")).unwrap(),
            "new regular content"
        );
        assert!(contents
            .lines()
            .any(|l| l.starts_with("obj /etc/thing.conf")));
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

        let status = run_merge(
            &ebuild,
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
            None,
        )
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
    fn real_merge_writes_the_full_build_info_into_the_vdb_entry() {
        // `dev-libs/packagepkg` has a real md5-cache entry with
        // `RDEPEND="dev-libs/samepkg"`. Real `treewalk()` copies every
        // `build-info` file into the vdb, and
        // `ebuild_phases::write_post_install_metadata` now writes the
        // dependency-string metadata files there -- so the merged vdb
        // entry carries `RDEPEND`/`EAPI`/`KEYWORDS`/…, not just the
        // former `CATEGORY`/`SLOT`/`repository`/`CONTENTS`/`COUNTER`.
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();
        let repo_root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/packagepkg/packagepkg-1.0.ebuild");

        let status = run_merge(
            &ebuild,
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
            None,
        )
        .expect("run_merge succeeds");
        assert_eq!(status, 0);

        let vdb = root.join("var/db/pkg/dev-libs/packagepkg-1.0");
        let read = |f: &str| {
            std::fs::read_to_string(vdb.join(f))
                .unwrap_or_else(|e| panic!("vdb/{f}: {e}"))
                .trim()
                .to_string()
        };
        assert_eq!(read("RDEPEND"), "dev-libs/samepkg");
        assert_eq!(read("EAPI"), "8");
        assert_eq!(read("KEYWORDS"), "amd64");
        assert_eq!(read("SLOT"), "0");
        // The bundled ebuild + saved environment come across too (real
        // `build-info` members).
        assert!(vdb.join("packagepkg-1.0.ebuild").is_file());
        assert!(vdb.join("environment.bz2").is_file());
        // An empty md5-cache value (`DEPEND=`) is not written at all
        // (real portage unlinks it; `bin/phase-functions.sh` never wrote
        // it).
        assert!(!vdb.join("DEPEND").exists());
    }

    #[test]
    fn run_qmerge_fails_when_the_install_phase_has_not_been_run() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/mergepkg/mergepkg-1.0.ebuild");

        let err = run_qmerge(&ebuild, &root, &portage_tmpdir, &MergeOptions::default())
            .expect_err("qmerge without a prior install must fail");
        assert!(err.contains("install phase has not been run"), "{err}");
        // Nothing was written at all.
        assert!(!root.join("usr/share/mergepkg").exists());
    }

    #[test]
    fn run_qmerge_merges_without_rerunning_the_install_phase() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/mergepkg/mergepkg-1.0.ebuild");

        // Real qmerge assumes a prior real `install` already populated
        // ${D} -- run only the install phase directly here (not
        // run_merge, which would also run qmerge's own merge logic).
        let install_status = ebuild_phases::run_commands(
            &ebuild,
            &["install"],
            &root,
            &portage_tmpdir,
            &MergeOptions::default().distdir,
            false,
            &MergeOptions::default().config_root,
            ebuild_phases::ShellBackend::default(),
        )
        .expect("install phase succeeds");
        assert_eq!(install_status, 0);
        // Real bin/phase-functions.sh's own __dyn_install already leaves
        // this marker behind -- confirms the test's own setup is
        // faithful to what a real prior `ebuild <file> install` run
        // leaves for qmerge to find.
        assert!(portage_tmpdir
            .join("portage/dev-libs/mergepkg-1.0/.installed")
            .exists());

        let status = run_qmerge(&ebuild, &root, &portage_tmpdir, &MergeOptions::default())
            .expect("run_qmerge succeeds");
        assert_eq!(status, 0);

        assert!(root.join("usr/share/mergepkg/hello.txt").is_file());
        let vdb_dir = root.join("var/db/pkg/dev-libs/mergepkg-1.0");
        assert!(vdb_dir.join("CONTENTS").is_file());
        // Real pkg_preinst/pkg_postinst still run -- qmerge only skips
        // the install phase itself, not merge()'s own body.
        let t_dir = portage_tmpdir.join("portage/dev-libs/mergepkg-1.0/temp");
        assert!(t_dir.join("preinst-ran-before-merge").is_file());
        assert!(t_dir.join("postinst-ran-after-merge").is_file());
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
            run_merge(
                &ebuild,
                &root,
                &portage_tmpdir,
                &MergeOptions::default(),
                None
            )
            .unwrap(),
            0
        );
        let first_counter: i64 = std::fs::read_to_string(vdb_dir.join("COUNTER"))
            .unwrap()
            .parse()
            .unwrap();

        assert_eq!(
            run_merge(
                &ebuild,
                &root,
                &portage_tmpdir,
                &MergeOptions::default(),
                None
            )
            .unwrap(),
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

        let status = run_merge(
            &ebuild,
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
            None,
        )
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

    #[test]
    fn real_merge_protects_a_locally_modified_etc_symlink_via_the_full_cli_path() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();
        // Simulate a pre-existing, locally-modified /etc symlink -- as if
        // this package (or an earlier version of it) had installed a
        // default target the admin then repointed by hand.
        std::os::unix::fs::symlink("admins-own-target", root.join("etc/configsympkg.conf"))
            .unwrap();

        let repo_root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/configsympkg/configsympkg-1.0.ebuild");

        let status = run_merge(
            &ebuild,
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
            None,
        )
        .expect("run_merge succeeds");
        assert_eq!(status, 0);

        // The real, logical /etc/configsympkg.conf is never touched.
        assert_eq!(
            std::fs::read_link(root.join("etc/configsympkg.conf")).unwrap(),
            PathBuf::from("admins-own-target")
        );
        // The new target the ebuild wanted to install lands in a real
        // ._cfg0000_ sibling instead.
        assert_eq!(
            std::fs::read_link(root.join("etc/._cfg0000_configsympkg.conf")).unwrap(),
            PathBuf::from("new-target")
        );
        // The vdb's own CONTENTS still considers /etc/configsympkg.conf
        // (the logical path) this package's own -- not the ._cfg
        // variant.
        let contents =
            std::fs::read_to_string(root.join("var/db/pkg/dev-libs/configsympkg-1.0/CONTENTS"))
                .unwrap();
        assert!(contents
            .lines()
            .any(|l| l.starts_with("sym /etc/configsympkg.conf -> new-target")));
        assert!(!contents.contains("._cfg0000_configsympkg.conf"));
    }

    fn collision_fixture(name: &str) -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs")
            .join(name)
            .join(format!("{name}-1.0.ebuild"))
    }

    /// Both `FEATURES=collision-protect` **and** `protect-owned` off: an
    /// ordinary file collision is merged over (`collisionpkg-c`
    /// overwrites `collisionpkg-a`'s own `shared.txt`). Real
    /// `protect-owned` *is* one of real `make.globals`'s own default
    /// `FEATURES` tokens (`cnf/make.globals:77-84`, confirmed by reading
    /// it directly), unlike `collision-protect` -- so real portage's own
    /// actual default behavior for this exact scenario (an identifiable
    /// owner for the collision, `collisionpkg-a`) is to *abort*, not
    /// merge over; `MergeOptions::default()` alone (`protect_owned:
    /// true`) reproduces that real default correctly. This test
    /// therefore sets `protect_owned: false` explicitly rather than
    /// relying on `MergeOptions::default()` -- see
    /// `protect_owned_alone_aborts_when_an_owner_is_identified` for the
    /// real-default (`protect_owned: true`) case.
    #[test]
    fn ordinary_collision_is_merged_over_with_both_collision_protect_and_protect_owned_off() {
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
            None,
        )
        .expect("collisionpkg-a merges cleanly");

        let options = MergeOptions {
            protect_owned: false,
            ..MergeOptions::default()
        };
        let status = run_merge(
            &collision_fixture("collisionpkg-c"),
            &root,
            &portage_tmpdir,
            &options,
            None,
        )
        .expect("run_merge should not itself error");
        assert_eq!(status, 0);
        assert_eq!(
            std::fs::read_to_string(root.join("usr/share/collisiontest/shared.txt")).unwrap(),
            "hello from collisionpkg-c\n"
        );
    }

    /// `MergeOptions::default()`, no overrides at all: an ordinary file
    /// collision with an identifiable owner now aborts, matching real
    /// portage's own real out-of-the-box behavior (real `protect-owned`
    /// is a default-on `FEATURES` token, see `MergeOptions::
    /// protect_owned`'s own doc comment). Complements
    /// `protect_owned_alone_aborts_when_an_owner_is_identified` below,
    /// which proves the same real logic via an explicit `protect_owned:
    /// true` rather than relying on the real default.
    #[test]
    fn ordinary_collision_aborts_by_real_default_via_protect_owned() {
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
            None,
        )
        .expect("collisionpkg-a merges cleanly");

        let err = run_merge(
            &collision_fixture("collisionpkg-c"),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
            None,
        )
        .expect_err("protect-owned is on by real default, so this should abort");
        assert!(err.contains("dev-libs/collisionpkg-a-1.0"), "{err}");
        assert!(err.contains("/usr/share/collisiontest/shared.txt"), "{err}");

        // Nothing was written: the file is still collisionpkg-a's own.
        assert_eq!(
            std::fs::read_to_string(root.join("usr/share/collisiontest/shared.txt")).unwrap(),
            "hello from collisionpkg-a\n"
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
            None,
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
            None,
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
            None,
        )
        .expect("collisionpkg-a merges cleanly");

        let err = run_merge(
            &collision_fixture("collisionpkg-b"),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
            None,
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
            None,
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
            None,
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

    /// `blockers_from_flat_deps` (the `blockers` term of real
    /// `mypkglist = others_in_slot + blockers`, factored out of
    /// `blocked_installed_packages` for `merge_binpkg`'s own use): every
    /// `!atom`/`!!atom` in an already-flat dep list is matched against
    /// the installed vdb, non-blocker atoms ignored.
    #[test]
    fn blockers_from_flat_deps_matches_only_blocker_atoms_against_the_vdb() {
        let tmp = tempdir();
        let root = tmp.join("root");
        for (pf, slot) in [("blockedpkg-1.0", "0"), ("normalpkg-2.0", "3")] {
            let vdb = root.join("var/db/pkg/dev-libs").join(pf);
            std::fs::create_dir_all(&vdb).unwrap();
            std::fs::write(vdb.join("SLOT"), format!("{slot}\n")).unwrap();
        }

        let deps = [
            "!dev-libs/blockedpkg".to_string(), // blocker, installed -> matched
            "dev-libs/normalpkg".to_string(),   // not a blocker -> ignored
            "!!dev-libs/notinstalled".to_string(), // blocker, not installed -> no match
        ];
        let blocked = blockers_from_flat_deps(&root, &deps);
        assert_eq!(
            blocked,
            HashSet::from([("dev-libs".to_string(), "blockedpkg-1.0".to_string())])
        );

        // A slot-restricted blocker only matches the matching slot.
        assert!(blockers_from_flat_deps(&root, &["!dev-libs/normalpkg:0".to_string()]).is_empty());
        assert_eq!(
            blockers_from_flat_deps(&root, &["!dev-libs/normalpkg:3".to_string()]),
            HashSet::from([("dev-libs".to_string(), "normalpkg-2.0".to_string())])
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
            None,
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

    /// Real `unregister` (`register(cpv, slot, counter, [])`): removes
    /// the `cps` entry only when it still records the *same* `cpv` and
    /// `counter` -- a different package (or a stale counter) sharing the
    /// same `category/pn:slot` key must survive untouched.
    #[test]
    fn register_preserved_libs_unregister_only_matches_the_same_cpv_and_counter() {
        let mut registry = PlibRegistry {
            entries: BTreeMap::new(),
        };
        register_preserved_libs(
            &mut registry,
            "dev-libs/foo-1.0",
            "dev-libs",
            "foo",
            "0",
            "5",
            &["/usr/lib/libfoo.so.1".to_string()],
        );
        assert!(registry.entries.contains_key("dev-libs/foo:0"));

        // Wrong counter: real `unregister` must leave the entry alone.
        register_preserved_libs(
            &mut registry,
            "dev-libs/foo-1.0",
            "dev-libs",
            "foo",
            "0",
            "9",
            &[],
        );
        assert!(
            registry.entries.contains_key("dev-libs/foo:0"),
            "a stale counter must not unregister someone else's live entry"
        );

        // Wrong cpv (a different version currently holding this slot):
        // same real "leave it alone" rule.
        register_preserved_libs(
            &mut registry,
            "dev-libs/foo-2.0",
            "dev-libs",
            "foo",
            "0",
            "5",
            &[],
        );
        assert!(
            registry.entries.contains_key("dev-libs/foo:0"),
            "a different cpv must not unregister someone else's live entry"
        );

        // Matching cpv and counter: real `unregister` removes it.
        register_preserved_libs(
            &mut registry,
            "dev-libs/foo-1.0",
            "dev-libs",
            "foo",
            "0",
            "5",
            &[],
        );
        assert!(!registry.entries.contains_key("dev-libs/foo:0"));
    }

    /// Real `register` with non-empty `paths`: unconditionally overwrites
    /// whatever the `cps` key already held, even a different package's
    /// own entry -- no same-cpv/counter guard the way empty-`paths`
    /// (`unregister`) has.
    #[test]
    fn register_preserved_libs_with_paths_unconditionally_overwrites() {
        let mut registry = PlibRegistry {
            entries: BTreeMap::new(),
        };
        register_preserved_libs(
            &mut registry,
            "dev-libs/foo-1.0",
            "dev-libs",
            "foo",
            "0",
            "5",
            &["/usr/lib/libfoo.so.1".to_string()],
        );
        register_preserved_libs(
            &mut registry,
            "dev-libs/foo-2.0",
            "dev-libs",
            "foo",
            "0",
            "6",
            &["/usr/lib/libfoo.so.2".to_string()],
        );

        let (cpv, counter, paths) = registry.entries.get("dev-libs/foo:0").unwrap();
        assert_eq!(cpv, "dev-libs/foo-2.0");
        assert_eq!(counter, "6");
        assert_eq!(paths, &["/usr/lib/libfoo.so.2".to_string()]);
    }

    /// Real `_prune_plib_registry`'s own early-exit shape for a package
    /// that owns no files at all (empty `CONTENTS`): this pilot's own
    /// `preserve_libs_on_unmerge` short-circuits to an empty preserved
    /// set without touching the registry or rebuilding the linkage map.
    #[test]
    fn preserve_libs_on_unmerge_short_circuits_on_empty_contents() {
        let tmp = tempdir();
        let preserved =
            preserve_libs_on_unmerge(&tmp, "dev-libs", "foo", "foo-1.0", "0", "").unwrap();
        assert!(preserved.is_empty());
        assert!(!plib_registry_path(&tmp).exists());
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
            None,
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
            None,
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
            None,
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
            None,
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

    /// Real `removeFromContents`'s own `NEEDED`-line stripping
    /// (`vartree.py:1279-1310`): a `NEEDED.ELF.2` entry for a path this
    /// call actually removed from `CONTENTS` is dropped too, while an
    /// entry for a path that's still owned survives untouched -- real
    /// stale-linkage-data prevention for a *later* `LinkageMap.
    /// rebuild()`'s own preserve-libs decision (see `remove_from_
    /// contents`'s own doc comment).
    #[test]
    fn remove_from_contents_prunes_the_matching_needed_elf2_entry_too() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let vdb_dir = root.join("var/db/pkg/dev-libs/foo-1.0");
        std::fs::create_dir_all(&vdb_dir).unwrap();
        std::fs::write(
            vdb_dir.join("CONTENTS"),
            "obj /usr/lib/a.so abc123 100\nobj /usr/lib/b.so def456 100\n",
        )
        .unwrap();
        std::fs::write(
            vdb_dir.join("NEEDED.ELF.2"),
            "X86_64;/usr/lib/a.so;liba.so.1;;\nX86_64;/usr/lib/b.so;libb.so.1;;\n",
        )
        .unwrap();

        let mut paths = BTreeSet::new();
        paths.insert("/usr/lib/a.so".to_string());
        remove_from_contents(&root, "dev-libs/foo-1.0", &paths)
            .expect("remove_from_contents succeeds");

        let contents = std::fs::read_to_string(vdb_dir.join("CONTENTS")).unwrap();
        assert!(!contents.contains("/usr/lib/a.so"));
        assert!(contents.contains("/usr/lib/b.so"));

        let needed = std::fs::read_to_string(vdb_dir.join("NEEDED.ELF.2")).unwrap();
        assert!(
            !needed.contains("/usr/lib/a.so"),
            "the entry for the removed path must be pruned: {needed}"
        );
        assert!(
            needed.contains("/usr/lib/b.so"),
            "the entry for the still-owned path must survive: {needed}"
        );
    }

    /// Real `if new_needed is not None:` (`writeContentsToContentsFile`):
    /// when this package never had a `NEEDED.ELF.2` at all, nothing is
    /// written -- no file is conjured into existence just because a
    /// `CONTENTS` entry happened to be removed.
    #[test]
    fn remove_from_contents_does_not_create_a_needed_elf2_file_that_never_existed() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let vdb_dir = root.join("var/db/pkg/dev-libs/foo-1.0");
        std::fs::create_dir_all(&vdb_dir).unwrap();
        std::fs::write(vdb_dir.join("CONTENTS"), "obj /usr/lib/a.so abc123 100\n").unwrap();

        let mut paths = BTreeSet::new();
        paths.insert("/usr/lib/a.so".to_string());
        remove_from_contents(&root, "dev-libs/foo-1.0", &paths)
            .expect("remove_from_contents succeeds");

        assert!(!vdb_dir.join("NEEDED.ELF.2").exists());
    }

    /// Real `if removed:` (`vartree.py:1279`): when none of the given
    /// `paths` actually matched a real `CONTENTS` entry, `NEEDED.ELF.2`
    /// isn't even read, let alone rewritten -- untouched, byte for byte.
    #[test]
    fn remove_from_contents_leaves_needed_elf2_untouched_when_nothing_was_removed() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let vdb_dir = root.join("var/db/pkg/dev-libs/foo-1.0");
        std::fs::create_dir_all(&vdb_dir).unwrap();
        std::fs::write(vdb_dir.join("CONTENTS"), "obj /usr/lib/a.so abc123 100\n").unwrap();
        let original_needed = "X86_64;/usr/lib/a.so;liba.so.1;;\n";
        std::fs::write(vdb_dir.join("NEEDED.ELF.2"), original_needed).unwrap();

        let mut paths = BTreeSet::new();
        paths.insert("/usr/lib/nonexistent.so".to_string());
        remove_from_contents(&root, "dev-libs/foo-1.0", &paths)
            .expect("remove_from_contents succeeds");

        assert_eq!(
            std::fs::read_to_string(vdb_dir.join("NEEDED.ELF.2")).unwrap(),
            original_needed
        );
    }

    /// Real, end-to-end proof of `merge_tree`'s own new `fif` branch:
    /// merging a real FIFO node (created via real `mkfifo(1)`, no
    /// special privilege needed unlike a device node) actually creates a
    /// real FIFO at the destination and records a real `fif` `CONTENTS`
    /// line with no digest/mtime/target field at all (real
    /// `_format_contents_line(node_type="fif", abs_path=myrealdest)`).
    /// Re-merging over an already-existing node is a real no-op (real
    /// `if mydmode is None:` only creates when nothing's there yet) --
    /// proven by planting an unrelated real file at the destination
    /// first and confirming it survives untouched. Unmerging leaves the
    /// node in place too, matching real `_unmerge_pkgfiles()`'s own
    /// `"fif"`/`"dev"` branches never calling `unlink()` at all (see
    /// `ebuild_unmerge::remove_contents`'s own doc comment) -- this
    /// pilot's own vdb entry is still removed as normal either way.
    #[test]
    fn real_merge_creates_a_real_fifo_and_records_a_fif_contents_line() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let ebuild = collision_fixture("fifopkg");
        let status = run_merge(
            &ebuild,
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
            None,
        )
        .expect("run_merge succeeds");
        assert_eq!(status, 0);

        let fifo_path = root.join("usr/lib/fifopkg/myfifo");
        let meta = std::fs::symlink_metadata(&fifo_path).expect("the real FIFO was created");
        assert!(meta.file_type().is_fifo(), "{:?}", meta.file_type());

        let contents =
            std::fs::read_to_string(root.join("var/db/pkg/dev-libs/fifopkg-1.0/CONTENTS")).unwrap();
        assert!(
            contents.contains("fif /usr/lib/fifopkg/myfifo\n"),
            "{contents}"
        );

        // Re-merging (a real reinstall) must not recreate the node --
        // plant something else there and confirm it survives.
        std::fs::remove_file(&fifo_path).unwrap();
        std::fs::write(&fifo_path, b"not actually a fifo anymore").unwrap();
        let status = run_merge(
            &ebuild,
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
            None,
        )
        .expect("second run_merge succeeds");
        assert_eq!(status, 0);
        assert_eq!(
            std::fs::read_to_string(&fifo_path).unwrap(),
            "not actually a fifo anymore",
            "an existing node at that path must be left completely alone"
        );

        // Restore a real FIFO before unmerging, so the "leave it in
        // place" assertion below is actually meaningful.
        std::fs::remove_file(&fifo_path).unwrap();
        unsafe {
            use std::os::unix::ffi::OsStrExt;
            let c_path = std::ffi::CString::new(fifo_path.as_os_str().as_bytes()).unwrap();
            assert_eq!(libc::mkfifo(c_path.as_ptr(), 0o644), 0);
        }

        let unmerge_status = crate::ebuild_unmerge::run_unmerge(
            &ebuild,
            &root,
            &portage_tmpdir,
            &crate::ebuild_unmerge::UnmergeOptions::default(),
        )
        .expect("run_unmerge succeeds");
        assert_eq!(unmerge_status, 0);
        assert!(
            std::fs::symlink_metadata(&fifo_path)
                .map(|m| m.file_type().is_fifo())
                .unwrap_or(false),
            "real portage never unlinks a fif/dev CONTENTS entry on unmerge"
        );
        assert!(
            !root.join("var/db/pkg/dev-libs/fifopkg-1.0").exists(),
            "the vdb entry itself is still removed, same as any other unmerge"
        );
    }

    /// Device-node creation (`mknod(2)` with `S_IFCHR`/`S_IFBLK`) genuinely
    /// requires root/`CAP_MKNOD` for a *real* (nonzero major:minor)
    /// device on a real Linux system -- confirmed empirically both via a
    /// plain standalone `mknod(2)` call and via this very function, as
    /// this pilot's own unprivileged dev/test user. (A privilege-free
    /// carve-out does exist for `mknod(path, S_IFCHR, 0)` specifically --
    /// the real kernel's own overlayfs "whiteout" convention, `dev_t ==
    /// 0` never being a usable real device -- which is precisely why
    /// this test passes `/dev/null` itself as `src`, not an arbitrary
    /// regular file: only a real char device's own real, nonzero `rdev`
    /// actually exercises the real privileged path.) Not reproducible as
    /// a real, live end-to-end test in this environment, unlike the
    /// `fif` case above. This narrower test instead confirms
    /// `create_special_node` itself propagates that real failure cleanly
    /// via `Result` (no panic) -- a permission error surfacing as an
    /// ordinary merge failure, not a crash.
    #[test]
    fn create_special_node_reports_a_permission_failure_cleanly_rather_than_panicking() {
        let tmp = tempdir();
        let dest = tmp.join("devnode");

        let dev_null = Path::new("/dev/null");
        let dev_null_type = std::fs::symlink_metadata(dev_null).unwrap().file_type();
        assert!(dev_null_type.is_char_device());

        let err = create_special_node(dev_null, &dest, &dev_null_type)
            .expect_err("mknod(2) for a real, nonzero-rdev char device requires root");
        assert!(
            err.contains("Operation not permitted") || err.contains("permitted"),
            "{err}"
        );
        assert!(
            !dest.exists(),
            "a failed mknod must not leave a partial node behind"
        );
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
            None,
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
            None,
        )
        .expect("run_merge succeeds");
        assert_eq!(status, 0);

        let marker = std::fs::read_to_string(root.join("ldconfig-was-invoked"))
            .expect("the real ROOT-scoped ldconfig binary was really invoked");
        assert!(marker.contains("-X"), "{marker}");
        assert!(marker.contains("-r"), "{marker}");
    }

    /// Real, end-to-end proof that `NEEDED.ELF.2` -- real, unmodified
    /// `bin/misc-functions.sh install_qa_check`'s own real `scanelf`-
    /// driven output, generated by the new post-install misc-functions
    /// step (`ebuild_phases::run_commands_async`) -- actually lands in
    /// the real vdb entry, matching real `dblink.merge()`'s own
    /// `treewalk()` (`vartree.py:4912-4913`) copying it out of
    /// `build-info` (see `write_vdb_entry`'s own doc comment for why
    /// this pilot copies only this one build-info file, not the whole
    /// directory). Installs a real, dynamically-linked ELF binary
    /// (`/bin/true`, whatever the real host machine actually has) so
    /// real `scanelf` has something genuine to report on.
    #[test]
    fn real_merge_copies_a_real_needed_elf2_into_the_vdb() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let status = run_merge(
            &collision_fixture("elfpkg"),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
            None,
        )
        .expect("run_merge succeeds");
        assert_eq!(status, 0);

        let needed =
            std::fs::read_to_string(root.join("var/db/pkg/dev-libs/elfpkg-1.0/NEEDED.ELF.2"))
                .expect("NEEDED.ELF.2 should have been copied into the real vdb entry");
        assert!(
            needed.contains("/usr/bin/true"),
            "should report the real installed binary's own path: {needed}"
        );

        // `crate::needed_elf::NeededEntry::parse_file` end to end against
        // this real, live `scanelf`-generated vdb file -- not just the
        // hand-crafted lines its own unit tests already cover.
        let entries = crate::needed_elf::NeededEntry::parse_file(&needed);
        let entry = entries
            .iter()
            .find(|e| e.filename == "/usr/bin/true")
            .expect("the real parser should find the real installed binary's own entry");
        assert_eq!(entry.arch, "X86_64");
        assert!(
            entry.needed.iter().any(|n| n.starts_with("libc.so")),
            "{:?}",
            entry.needed
        );

        // `crate::needed_elf::read_all_needed_entries` end to end: the
        // real vdb walk finds this exact package's own real entry too.
        let all = crate::needed_elf::read_all_needed_entries(&root);
        let (cpv, cpv_entries) = all
            .iter()
            .find(|(cpv, _)| cpv == "dev-libs/elfpkg-1.0")
            .expect("read_all_needed_entries should find the real installed package");
        assert_eq!(cpv, "dev-libs/elfpkg-1.0");
        assert!(cpv_entries.iter().any(|e| e.filename == "/usr/bin/true"));

        // `crate::needed_elf::rebuild` end to end: the real installed
        // binary's own real DT_NEEDED entries (whatever the real host's
        // own /bin/true actually links against -- typically libc.so.6)
        // get indexed as real consumers, keyed by its own real multilib
        // category.
        let map = crate::needed_elf::rebuild(&root, &all);
        let key = crate::needed_elf::obj_key(&root, "/usr/bin/true");
        let props = map
            .obj_properties
            .get(&key)
            .expect("rebuild should index the real installed binary");
        assert_eq!(props.owner, "dev-libs/elfpkg-1.0");
        assert!(!props.needed.is_empty(), "{:?}", props.needed);
        let consumed_somewhere = map.libs.values().any(|sonames| {
            sonames
                .values()
                .any(|soname_map| soname_map.consumers.contains(&key))
        });
        assert!(consumed_somewhere);
    }

    /// Real, end-to-end proof of the full preserve-libs pipeline this
    /// pilot's own `preserve_libs_on_unmerge` (see its own doc comment
    /// above) actually wires into real `ebuild_unmerge::run_unmerge`:
    /// merge a real library, merge a real consumer that's genuinely
    /// linked against it (real `DT_NEEDED: libpreservetest.so.1`, baked
    /// in by real `gcc` at fixture build time -- see the two fixture
    /// ebuilds' own comments for why each independently rebuilds a
    /// throwaway same-sonamed copy to link against), then unmerge the
    /// library while the consumer is still installed. Real `_find_libs_
    /// to_preserve` should find the still-installed consumer's own
    /// `NEEDED.ELF.2` entry still needing this soname, so the real
    /// library file must survive on disk (filtered out of `CONTENTS`
    /// before `remove_contents`'s own per-file loop ever sees it -- see
    /// `ebuild_unmerge::remove_contents`'s own `preserved_paths` doc
    /// comment) and the real on-disk registry must record it under this
    /// exact package's own real `category/pn:slot` key.
    #[test]
    fn real_unmerge_preserves_a_still_needed_shared_library() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let lib_ebuild = collision_fixture("libpreservetest");
        let consumer_ebuild = collision_fixture("consumepreservetest");

        let lib_status = run_merge(
            &lib_ebuild,
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
            None,
        )
        .expect("run_merge (library) succeeds");
        assert_eq!(lib_status, 0);

        let consumer_status = run_merge(
            &consumer_ebuild,
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
            None,
        )
        .expect("run_merge (consumer) succeeds");
        assert_eq!(consumer_status, 0);

        let lib_path = root.join("usr/lib/libpreservetest.so.1");
        assert!(
            lib_path.is_file(),
            "sanity: the library was really installed"
        );

        let unmerge_status = crate::ebuild_unmerge::run_unmerge(
            &lib_ebuild,
            &root,
            &portage_tmpdir,
            &crate::ebuild_unmerge::UnmergeOptions::default(),
        )
        .expect("run_unmerge succeeds");
        assert_eq!(unmerge_status, 0);

        assert!(
            lib_path.is_file(),
            "the still-needed shared library must survive unmerge, preserved on disk"
        );
        assert!(
            !root
                .join("var/db/pkg/dev-libs/libpreservetest-1.0")
                .exists(),
            "the vdb entry itself is still removed, same as any other unmerge"
        );

        let registry = read_plib_registry(&root);
        let preserved = registry.preserved_libs();
        let paths = preserved
            .get("dev-libs/libpreservetest-1.0")
            .expect("the real registry should record this package as the new keeper");
        assert!(
            paths.iter().any(|p| p == "/usr/lib/libpreservetest.so.1"),
            "{paths:?}"
        );
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
            None,
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
            None,
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
    /// `fixtures` tree, which has its own real `repos.conf`) --
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
            None,
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
            None,
        )
        .expect("a blocker-excluded collision is not an abort");
        assert_eq!(status, 0);
        assert_eq!(
            std::fs::read_to_string(root.join("usr/share/mergeblockertest/shared.txt")).unwrap(),
            "hello from mergeblockerpkg\n"
        );
    }
}
