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
// "IGNORE"]`; real `bin/ebuild` has no such flag at all, so portuale
// reads the env var directly, the same "env var, not full config
// resolution" shortcut `CONFIG_PROTECT` itself already uses), which
// forces every already-offered update to be re-protected into a fresh
// `._cfgNNNN_` file regardless of memory. `CONFIG_PROTECT`/
// `CONFIG_PROTECT_MASK`/`NOCONFMEM` are read via env vars at the
// `ebuild.rs` CLI boundary (bundled into `MergeOptions`, deliberately a
// struct and not more positional parameters -- portuale already
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
// portuale already writes on every merge), and `protect_decision` consults
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
// `_collision_protect` only ever checks `file_list`/`symlink_list`,
// which real builds with `os.walk`: a symlink pointing to a directory
// lands in `dirs`, never in `file_list`/`symlink_list`, so it is never
// collision-checked at all) against the real, on-disk destination:
// real PMS 13.4's own symlink-over-directory ban covers only the
// checked symlinks (file-target/dangling ones) and is unconditional
// (regardless of `FEATURES`); a directory-target symlink over a real
// directory is ignored by the check and, at merge time, lands at the
// first `dest.backup.NNNN` instead (real `mergeme()`'s own
// symlink-over-directory branch), keeping the directory -- e.g.
// linux-firmware's `nvidia/ad10x` symlinks over installed directories
// merge with exit 0 under plain `protect-owned`. An ordinary collision (destination exists, isn't owned
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
// matching portuale's own "small, format-specific parser over a
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
// the time a merge runs. Portuale's own `ebuild <file> merge` has no
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
// (portuale's own dev/test machine has a real, populated
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
//   - `merge_tree`'s regular-file and symlink writes mirror real
//     `movefile()`'s **atomic replacement**: the new file/link is
//     materialized at a temporary sibling (`.{name}._portage_merge_.{pid}`)
//     and `rename(2)`d over the destination, so the existing inode is
//     never written to. That is load-bearing, not cosmetic (backlog #96):
//     a `std::fs::copy` in place truncates the live inode, and while the
//     kernel refuses to open a running *executable* for write (`ETXTBSY`
//     -- which is what used to abort a `/bin/bash` merge mid-copy), an
//     mmap'd *shared library* opens fine, so every running process
//     mapping it would execute the rewritten pages. Real is safe for the
//     same reason -- `os.rename` same-device (`movefile.py:231`),
//     copy-to-`#new`-then-rename cross-device (`:346`); see this file's
//     own `replace_file_atomic`/`replace_symlink_atomic`. Attribute
//     parity is real `movefile()`'s own explicit `os.chmod`/`os.chown`,
//     reproduced with `std::fs::set_permissions` on the temporary file
//     before the rename (on top of the mode bits `std::fs::copy` already
//     carries over on Unix). **Shipped
//     2026-09-05:** real `movefile()`'s `os.lchown`/`os.chown` (symlink,
//     regular file) and real `mergeme()`'s own directory
//     `os.chown`/`os.chmod` (a *newly created* directory only -- an
//     already-existing one is left alone) are now reproduced via
//     `lchown_or_chown` (`libc::lchown`/`libc::chown`, see its own doc
//     comment) -- see this file's own `lchown_or_chown` for the
//     root/non-root behavior (unconditional, matching real; fails with
//     `EPERM` like real does when genuinely privileged and not root).
//     There's still no privilege-*dropping* concept anywhere in
//     portuale (real userpriv/fakeroot's own reason for existing) --
//     see the "sandbox / build isolation" section of
//     `docs/scope-backlog.md` for that separate, still-correct
//     non-goal.
//   - Directory-entry merge order is sorted by filename for determinism
//     (portuale's own test-reproducibility need) rather than real
//     `os.listdir()`'s own arbitrary/OS-dependent order -- a deliberate
//     choice, not a gap: `CONTENTS` line order has no semantic meaning
//     portage itself relies on (unmerge re-sorts, `qmerge`/`qlist` sort
//     on read), and determinism is worth more here than bug-compatible
//     arbitrariness.
//   - `SLOT` is read the way real `dblink.treewalk()` reads it
//     (`vartree.py:4455-4498`): from `build-info/SLOT`, which the
//     install phase's `__dyn_install` filled with the *evaluated*
//     `${SLOT}` -- so a `SLOT` computed by real bash logic (e.g.
//     `SLOT="$(ver_cut 1)"`) is already evaluated by the time the merge
//     path reads it. The full real fallback chain (build-info value ->
//     `settings["SLOT"]` write-back -> `!!! SLOT is undefined` abort,
//     plus the `_eqawarn` QA Notice on a settings divergence) lives in
//     `merge_after_install`.
//   - `repository` is resolved by walking up from the ebuild's own
//     package directory looking for a `profiles/repo_name` file (real
//     portage's own mechanism for naming a repo), defaulting to the same
//     `"__unknown__"` sentinel `portage_repo::new_repo_changed` already
//     uses when no such file is found at all.

use crate::ebuild_phases;
use crate::env_update;
use md5::{Digest, Md5};
use mrg_director::PackagesDb as _;
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
/// positional parameters -- portuale already relearned the
/// "positional-parameter pain" lesson once, in `--newrepo`'s own
/// bulk-fix saga. `config_protect`/`config_protect_mask` are env-var-
/// sourced at the `ebuild.rs` CLI boundary, the same "env var, not full
/// config resolution" shortcut `PORTAGE_TMPDIR`/`ROOT` already use;
/// `Default` matches real `make.globals`'s own values exactly.
#[derive(Clone)]
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
    /// behavior. Portuale's own env-var read still only checks
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
    /// portuale reads the `NOCONFMEM` env var directly (presence-based,
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
    /// portuale's own dev/test machine has a real, populated
    /// `/etc/portage/repos.conf` (a real Gentoo system), so silently
    /// defaulting to real `/` the way `ebuild.rs`'s own CLI boundary
    /// does would make every test that doesn't override this field read
    /// real host config -- `Default` below uses a deliberately
    /// impossible path instead, so `blocked_installed_packages` always
    /// degrades to an empty blocked set unless a test opts in explicitly.
    pub config_root: PathBuf,
    /// Extra environment for every ebuild phase this merge runs
    /// (`install` via `run_merge`, `pkg_preinst`/`pkg_postinst` via
    /// `merge_after_install`). The `emerge <atom>` source path sets
    /// `[("USE", "<resolved enabled IUSE flags>")]` here -- computed per
    /// entry by `merge_one_source_entry`, so `bin/ebuild.sh`'s own
    /// `use()` sees the real flags instead of the `""` `phase_env_vars`
    /// leaves. Empty (`Default`) for a standalone `ebuild <file> merge`
    /// / `qmerge` and every test. See `ebuild_phases::run_commands_async`.
    pub build_env: Vec<(String, String)>,
    /// The resolved config this merge's `build_env` came from, when one
    /// is in scope (`emerge <atom>` and friends; #37 S2). `None` for a
    /// standalone `ebuild <file> merge`/`qmerge` and for tests -- those
    /// keep the curated base env. `emerge_build::entry_build_env` uses
    /// it to compute each entry's own resolved `USE` (`portage_profile::
    /// portage_use` over `portage_repo::candidate_effective_use_flags`)
    /// and the per-package rows of real `config.environ()` (`phase_
    /// environ_pkg`), instead of the enabled-IUSE-only `USE` this field
    /// used to carry. An `Arc` so the per-entry `options.clone()` is a
    /// pointer copy.
    pub resolved_config: Option<std::sync::Arc<portage_profile::Config>>,
    /// `/etc/portage/package.env`'s non-`USE` scalar half
    /// (`Config::package_env_vars`): `(atom, [(KEY, value)])` pairs.
    /// `emerge_build::entry_build_env` matches a build-bound entry's cpv
    /// against each atom and layers the matching vars over `build_env`
    /// (scalars lose to the calling environment, incrementals fold in
    /// `[base, pkg, calling-env]` order) -- real `_grab_pkg_env` into
    /// `configdict["pkg"]`. Empty (`Default`) everywhere else.
    pub package_env_vars: Vec<(String, Vec<(String, String)>)>,
    /// Real `Scheduler._background_mode`'s own `PORTAGE_LOG_FILE`
    /// redirection, extended to this merge's own `pkg_preinst`/
    /// `pkg_postinst`/`pkg_prerm`/`pkg_postrm` hooks -- previously only
    /// the `install` phase chain (`emerge_build.rs`'s own
    /// `build_one_source_entry`/`run_build_scheduler`) had a log path at
    /// all, so every hook this module runs (`merge_after_install`'s
    /// `preinst`/`postinst`, and the same-slot-replace/binary-merge
    /// `prerm`/`postrm`/`setup` hooks reached through `unmerge_one_
    /// installed`/`run_binary_merge`'s own `run_hook` closures) printed
    /// straight to the terminal even under `-jN`/`--quiet-build`,
    /// interleaving with -- or outliving -- the per-job build log real
    /// portage keeps genuinely separate. `Some` only when the caller
    /// already derived a `build.log` path for this same package's
    /// `install` phase (`emerge_build.rs`'s own `build_log_path`) --
    /// `run_one_phase`'s own log file is opened in append mode, so a
    /// hook's own output lands after that phase's, in the same file,
    /// exactly the way real portage's own single `PORTAGE_LOG_FILE`
    /// spans a whole package's build-then-merge lifecycle. `None`
    /// (`Default`) for a standalone `ebuild <file> merge`/`qmerge`/
    /// `unmerge`, `emerge -C`, and every test -- terminal output there
    /// already matches what real portage's own foreground run does.
    pub log_file: Option<PathBuf>,
    /// Real `INSTALL_MASK` as `preinst_mask()` (`bin/misc-functions.sh`)
    /// resolves it: the configured `INSTALL_MASK` (`make.conf`/profile/
    /// bashrc) with the `no{man,info,doc}` `FEATURES` tokens folded in
    /// (each appends `/usr/share/<man|info|doc>`). Real `dblink.
    /// treewalk()` applies this to the staged image -- deleting every
    /// matched path -- *before* `CONTENTS` is recorded and before
    /// collision-protect, so a masked file never lands in `${ROOT}` nor
    /// in the vdb `CONTENTS`. Written verbatim into the vdb entry's own
    /// `INSTALL_MASK` file too (real copies `build-info/INSTALL_MASK`
    /// wholesale). Empty (`Default`) = nothing masked, no `INSTALL_MASK`
    /// file written. See [`crate::install_mask`].
    pub install_mask: String,
    /// Whether any of `nodoc`/`noman`/`noinfo` is in `FEATURES` -- real
    /// `dblink.treewalk()` then also `rmdir`s a now-empty `ED/usr/share`
    /// after applying the mask. `false` (`Default`) leaves an emptied
    /// `usr/share` in place, exactly as real does when the mask emptied
    /// it for some other reason.
    pub install_mask_prunes_usr_share: bool,
    /// Merge-time gpkg signature policy (real `gpkg._verify_binpkg`'s
    /// own GPG layer -- see `crate::binpkg::GpgVerify`): enforced by
    /// `merge_binpkg` (via `extract_binpkg`) before anything is
    /// unpacked. `from_env` resolves it from `FEATURES` /
    /// `BINPKG_GPG_VERIFY_*`; `Default` is the same resolution (these
    /// are env reads either way -- the struct carries no other state).
    pub gpg_verify: crate::binpkg::GpgVerify,
    /// The resolved, merge-time `FEATURES` incremental list (space-
    /// joined), for `merge_binpkg`'s `PORTAGE_UPDATE_ENV` vdb-environment
    /// regeneration -- real portage's stored vdb env carries the fully-
    /// resolved list, not the binpkg's build-time one. The `emerge`
    /// production paths set this from `Config::resolved_incremental`
    /// ("FEATURES"); `from_env` / `Default` fall back to the raw
    /// `$FEATURES` string (all `ebuild <file> merge` has), and an empty
    /// value leaves the phase env's own `FEATURES` in place.
    pub features: String,
    /// The calling environment's own `PORTAGE_TMPDIR`, `Some` iff the
    /// process environment carries the key (a CLI-boundary read).
    /// Real's `env` layer outranks the `pkg` layer, so a per-package
    /// `package.env` `PORTAGE_TMPDIR` only re-derives the build directory
    /// when this is `None` (backlog #99, resolved per entry by
    /// `ebuild_phases::resolve_entry_portage_tmpdir`). `from_env` reads
    /// it from the process env; `Default` (tests) carries none.
    pub process_tmpdir: Option<PathBuf>,
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
            resolved_config: None,
            protect_owned: true,
            noconfmem: false,
            protect_if_modified: true,
            // "/dev/null" is a real character device, never a directory
            // -- joining anything under it can never exist on any real
            // filesystem, guaranteeing find_repos always fails cleanly
            // here regardless of what happens to exist on the host.
            config_root: PathBuf::from("/dev/null/no-config-root-configured"),
            build_env: Vec::new(),
            package_env_vars: Vec::new(),
            log_file: None,
            install_mask: String::new(),
            install_mask_prunes_usr_share: false,
            gpg_verify: crate::binpkg::GpgVerify::default(),
            features: String::new(),
            process_tmpdir: None,
        }
    }
}

impl MergeOptions {
    /// Real portage's own `settings`-derived merge configuration, but via
    /// the same "read the env var, fall back to `make.globals`'s own
    /// default" shortcut every other real-execution CLI boundary in this
    /// portuale already takes (`PORTAGE_TMPDIR`/`PKGDIR`/... -- no full
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
        let env_features: Vec<String> = std::env::var("FEATURES")
            .map(|f| f.split_whitespace().map(String::from).collect())
            .unwrap_or_default();
        let (install_mask, install_mask_prunes_usr_share) = crate::install_mask::resolve(
            &std::env::var("INSTALL_MASK").unwrap_or_default(),
            &env_features,
        );
        Self {
            debug,
            shell,
            resolved_config: None,
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
            build_env: Vec::new(),
            package_env_vars: Vec::new(),
            log_file: d.log_file,
            // Real `preinst_mask()`: configured `INSTALL_MASK` + the
            // `no{man,info,doc}` `FEATURES` fold. The `emerge <atom>`
            // production path overrides both fields from the resolved
            // `Config` (make.conf `INSTALL_MASK`/`FEATURES`) right after
            // this; the env read is the `ebuild <file> merge`/`qmerge`
            // fallback, matching every other var here.
            install_mask,
            install_mask_prunes_usr_share,
            gpg_verify: crate::binpkg::GpgVerify::from_env(),
            // `ebuild <file> merge` fallback: the raw `$FEATURES` string
            // (the `emerge` paths overwrite this with the resolved
            // incremental list right after, as with `install_mask`).
            features: std::env::var("FEATURES").unwrap_or_default(),
            // The calling env's own `PORTAGE_TMPDIR` (presence is the
            // `env`-over-`pkg` precedence input for #99); the `emerge`
            // paths keep `from_env`'s value like every other var here.
            process_tmpdir: std::env::var_os("PORTAGE_TMPDIR").map(PathBuf::from),
        }
    }
}

impl MergeOptions {
    /// Apply the **resolved** `FEATURES` list (real `settings.features`
    /// from `config.environ()`) and re-derive every merge-time token that
    /// follows it. The `emerge` paths call this right after `from_env`
    /// (which reads the raw process env as the `ebuild <file>` fallback);
    /// `ebuild <file> merge`/`qmerge` keep `from_env`'s values. This is
    /// what makes a `make.conf` `FEATURES=collision-protect` (or
    /// `-protect-owned`) take effect on the `emerge` paths -- real reads
    /// `settings.features`, never the calling env, for all three.
    pub fn set_resolved_features(&mut self, features: &str) {
        let has = |token: &str| features.split_whitespace().any(|t| t == token);
        self.collision_protect = has("collision-protect");
        self.protect_owned = has("protect-owned");
        self.protect_if_modified = has("config-protect-if-modified");
        self.gpg_verify = crate::binpkg::GpgVerify::from_features(features);
        self.features = features.to_string();
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
    if let Ok(entries) = portage_util::read_dir_entries(parent) {
        for entry in entries {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(rest) = name.strip_prefix("._cfg")
                && rest.len() > 5
                && rest.as_bytes()[4] == b'_'
                && &rest[5..] == basename
                && let Ok(n) = rest[..4].parse::<i64>()
                && n > max_num
            {
                max_num = n;
                last_pfile = Some(parent.join(entry.file_name()));
            }
        }
    }

    if let Some(old_pfile) = &last_pfile
        && let Ok(meta) = std::fs::symlink_metadata(old_pfile)
    {
        if meta.file_type().is_symlink() {
            if let Ok(target) = std::fs::read_link(old_pfile)
                && target.to_string_lossy() == newmd5
            {
                return Ok(old_pfile.clone());
            }
        } else if meta.is_file()
            && let Ok(md5) = md5_hex(old_pfile)
            && md5 == newmd5
        {
            return Ok(old_pfile.clone());
        }
    }
    Ok(parent.join(format!("._cfg{:04}_{basename}", max_num + 1)))
}

/// Real `dblink._new_backup_path` (`vartree.py:_new_backup_path`): the
/// first `dest.backup.NNNN` (zero-padded 4 digits from `0000`) whose
/// `lstat` fails -- i.e. no file, symlink, or directory there yet.
/// Used by `merge_tree`'s own symlink-over-directory branch below: real
/// `mergeme()` never overwrites a real directory with a symlink, it
/// merges the symlink under the backup name and keeps the directory.
fn new_backup_path(dest: &Path) -> PathBuf {
    let dest_str = dest.display().to_string();
    let mut n = 0;
    loop {
        let candidate = PathBuf::from(format!("{dest_str}.backup.{n:04}"));
        if std::fs::symlink_metadata(&candidate).is_err() {
            return candidate;
        }
        n += 1;
    }
}

/// A free temporary sibling of `dest`, in the same directory (so the
/// final `rename(2)` is always same-filesystem), named like real
/// `movefile()`'s own merge temporary: `.{basename}._portage_merge_.{pid}`
/// (`lib/portage/util/movefile.py:322`'s `NamedTemporaryFile` prefix),
/// with a counter suffix only on the (pid-reuse / parallel-merge)
/// collision case. Checks with `symlink_metadata`, so an existing
/// symlink or directory at the candidate counts as taken.
fn unique_sibling_path(dest: &Path) -> Result<PathBuf, String> {
    let parent = dest
        .parent()
        .ok_or_else(|| format!("{}: has no parent directory", dest.display()))?;
    let name = dest
        .file_name()
        .ok_or_else(|| format!("{}: not a valid filename", dest.display()))?
        .to_string_lossy();
    let base = parent.join(format!(".{name}._portage_merge_.{}", std::process::id()));
    if std::fs::symlink_metadata(&base).is_err() {
        return Ok(base);
    }
    for n in 0.. {
        let candidate = parent.join(format!(
            ".{name}._portage_merge_.{}.{n}",
            std::process::id()
        ));
        if std::fs::symlink_metadata(&candidate).is_err() {
            return Ok(candidate);
        }
    }
    unreachable!("the counter loop always finds a free name")
}

/// Real `movefile()`'s own **atomic replacement** for a regular file:
/// copy the source's bytes to [`unique_sibling_path`]'s temporary, apply
/// real `movefile()`'s `_apply_stat` (owner/group, mode) and the
/// source's own mtime, then `rename(2)` it over `dest`. The existing
/// `dest` inode is never opened for write -- see this module's own doc
/// comment ("KNOWN, DOCUMENTED GAPS") and backlog #96 for why that is
/// load-bearing: an in-place `std::fs::copy` truncates the live inode,
/// which is `ETXTBSY` for a running executable but succeeds for an
/// mmap'd shared library, so a `sys-libs/readline` merge rewrote
/// `libreadline.so.8.3` underneath every running bash and a `glibc`
/// merge would rewrite `libc.so.6` underneath the entire system. Real is
/// safe by construction: same-device `os.rename` (`movefile.py:231`),
/// cross-device copy to `dest + "#new"` then rename (`:346`).
///
/// `mtime` is the source's own mtime in seconds (already computed by the
/// caller for `CONTENTS`), so the temporary gets exactly the value the
/// caller will record -- never a second stat that could disagree. On any
/// failure the temporary is removed before returning; the destination is
/// left untouched.
fn replace_file_atomic(src: &Path, dest: &Path, mtime: i64) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let tmp = unique_sibling_path(dest)?;
    let outcome = (|| -> Result<(), String> {
        std::fs::copy(src, &tmp).map_err(|e| format!("{}: {e}", src.display()))?;
        let src_meta = std::fs::metadata(src).map_err(|e| format!("{}: {e}", src.display()))?;
        // `std::fs::copy` already carries the mode bits over on Unix;
        // the explicit chmod fixes up a umask-masked temporary. The
        // chown is not redundant -- `std::fs::copy` never touches
        // ownership. See `lchown_or_chown`'s own doc comment.
        lchown_or_chown(&tmp, src_meta.uid(), src_meta.gid(), false)?;
        std::fs::set_permissions(
            &tmp,
            std::fs::Permissions::from_mode(src_meta.permissions().mode()),
        )
        .map_err(|e| format!("{}: {e}", tmp.display()))?;
        filetime::set_file_mtime(&tmp, filetime::FileTime::from_unix_time(mtime, 0))
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, dest)
            .map_err(|e| format!("{} -> {}: {e}", tmp.display(), dest.display()))
    })();
    if outcome.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    outcome
}

/// Real `movefile()`'s atomic replacement for a symlink
/// (`lib/portage/util/movefile.py:213-265`): create the new link at
/// [`unique_sibling_path`]'s temporary, apply `lchown` + the source's
/// own mtime, then `rename(2)` it over `dest`. The remove-then-symlink
/// pair this replaces could leave the path briefly absent and, more
/// importantly, was the second in-place mutation `merge_tree` had; one
/// rule now covers both branches: never write through an existing
/// inode. `rename(2)` replaces an existing file or symlink atomically
/// (a real directory cannot appear here: the symlink-over-directory case
/// is diverted to `new_backup_path` by the caller).
fn replace_symlink_atomic(
    target: &Path,
    src: &Path,
    dest: &Path,
    mtime: i64,
) -> Result<(), String> {
    let tmp = unique_sibling_path(dest)?;
    let outcome = (|| -> Result<(), String> {
        std::os::unix::fs::symlink(target, &tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        let src_meta =
            std::fs::symlink_metadata(src).map_err(|e| format!("{}: {e}", src.display()))?;
        lchown_or_chown(&tmp, src_meta.uid(), src_meta.gid(), true)?;
        let ft = filetime::FileTime::from_unix_time(mtime, 0);
        filetime::set_symlink_file_times(&tmp, ft, ft)
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, dest)
            .map_err(|e| format!("{} -> {}: {e}", tmp.display(), dest.display()))
    })();
    if outcome.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    outcome
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

    if protect_if_modified && let Some((node_type, value)) = &matched {
        let unmodified_since_installed = match node_type.as_str() {
            "obj" => dest_md5.as_deref() == Some(value.as_str()),
            "sym" => dest_link.as_deref() == Some(value.as_str()),
            _ => false,
        };
        if unmodified_since_installed {
            return Ok((dest.to_path_buf(), true));
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
/// admin has already been shown this exact change once. Portuale's
/// own `ebuild` CLI has no `--noconfmem` flag, so behavior always
/// matches real portage's own default (`--noconfmem` off).
///
/// Stored by the root's [`portage_vdb::InstalledDb`] (feat#157 S1.4:
/// `config_memory` / `set_config_memory`; on `files` the same
/// `<root>/var/lib/portage/config` file, read and written exactly as
/// before).
///
/// `pub(crate)`: also read by `ebuild_unmerge::run_unmerge` (real
/// `_unmerge_pkgfiles()`'s own `stale_confmem` cleanup,
/// `vartree.py:2747`/`2931-2932`/`3106-3109` -- a removed file's
/// `_conf_mem_file` entry is dropped once nothing still owns that path).
/// A missing or unreadable store is empty.
pub(crate) fn read_cfgfiledict(root: &Path) -> BTreeMap<String, String> {
    portage_vdb::for_root(root)
        .config_memory()
        .map(|memory| memory.entries)
        .unwrap_or_default()
}

/// Replace the config memory, unconditionally (one `WriteTxn`, committed
/// at once; on `files` a plain in-place write).
pub(crate) fn write_cfgfiledict(root: &Path, map: &BTreeMap<String, String>) -> Result<(), String> {
    let db = portage_vdb::for_root(root);
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    txn.set_config_memory(&portage_vdb::ConfigMemory {
        entries: map.clone(),
    })
    .map_err(|e| e.to_string())?;
    txn.commit().map_err(|e| e.to_string())
}

/// Real `PreservedLibsRegistry`'s own in-memory shape
/// (`lib/portage/util/_dyn_libs/PreservedLibsRegistry.py`): `"cp:slot"`
/// -> `(cpv, counter, paths)`. `preserved_libs()` mirrors real
/// `getPreservedLibs()` (cpv -> paths, last entry wins on a duplicate
/// cpv across keys -- a corner case with no real relevance here).
///
/// `orig_entries` is the file content as parsed, snapshotted before
/// `prune_non_existing` runs, for real `store()`'s own
/// `self._data == self._data_orig` early return (`store()` in
/// `PreservedLibsRegistry.py`): `write_plib_registry` skips the write
/// -- not even creating the parent directory -- when `entries` still
/// equals it, so a merge/unmerge with no preserved-lib activity leaves
/// the file's bytes **and** mtime untouched (backlog #167).
///
/// One deliberate difference from real's literal comparison: real
/// compares loaded JSON lists against live tuples, which are never `==`
/// in Python, so real rewrites a non-empty registry (identical bytes --
/// a tuple serializes exactly like the list it was loaded from -- but a
/// touched mtime) on every `store()`. Portuale compares semantically
/// and skips that mtime-only rewrite; the observable bytes stay
/// identical to real's.
type PlibEntries = BTreeMap<String, (String, String, Vec<String>)>;

#[derive(Clone)]
struct PlibRegistry {
    entries: PlibEntries,
    orig_entries: PlibEntries,
}

/// The writes of one standalone unmerge that a database backend commits
/// together with the row's deletion (feat#157 S4.2, `retire_entry`):
/// collected while the unmerge runs, applied in one transaction after
/// `pkg_postrm`. `None` everywhere on `files`, where every write is made
/// when the unmerge makes it (the S1.5 syscall sequence).
///
/// - `registry`: the preserved-libs registry as the unmerge left it
///   in memory. `preserve_libs_on_unmerge` and the prune that follows
///   read it back from here instead of the store (the store still holds
///   the state before the unmerge); its `orig_entries` stay the loaded
///   store, so the one `set_preserved_libs` writes nothing when the
///   unmerge changed nothing.
/// - `config_memory`: the pruned config memory (`stale_confmem`).
/// - `files`: the W4 rewrites (`remove_from_contents`) of the *other*
///   installed entries that owned preserved libraries pruned here.
#[derive(Default)]
pub(crate) struct RetireWrites {
    registry: Option<PlibRegistry>,
    config_memory: Option<BTreeMap<String, String>>,
    files: Vec<(portage_vdb::EntryKey, String, Vec<u8>)>,
}

impl RetireWrites {
    /// `Some` when `root`'s backend commits a retirement as one
    /// transaction (the database backends); `None` on `files`.
    pub(crate) fn for_root(root: &Path) -> Option<Self> {
        portage_vdb::for_root(root)
            .replace_in_publish()
            .then(Self::default)
    }

    pub(crate) fn set_config_memory(&mut self, map: BTreeMap<String, String>) {
        self.config_memory = Some(map);
    }

    /// One transaction: the W4 rewrites, the registry (only if it
    /// changed), the config memory, and the deletion of `key`.
    pub(crate) fn commit(self, root: &Path, key: &portage_vdb::EntryKey) -> Result<(), String> {
        let db = portage_vdb::for_root(root);
        let mut txn = db.begin_write().map_err(|e| e.to_string())?;
        for (entry, name, data) in &self.files {
            txn.replace_file(entry, name, data)
                .map_err(|e| e.to_string())?;
        }
        if let Some(libs) = self.registry.as_ref().and_then(plib_store) {
            txn.set_preserved_libs(&libs).map_err(|e| e.to_string())?;
        }
        if let Some(map) = self.config_memory {
            txn.set_config_memory(&portage_vdb::ConfigMemory { entries: map })
                .map_err(|e| e.to_string())?;
        }
        txn.delete_entry(key).map_err(|e| e.to_string())?;
        #[cfg(test)]
        tests::publish_hook(root);
        txn.commit().map_err(|e| e.to_string())
    }
}

/// The registry an unmerge in progress sees: the in-memory one when an
/// earlier step of this unmerge deferred its write, else the stored one
/// (pruned of vanished paths either way, like `read_plib_registry`).
fn plib_registry_for(root: &Path, retire: Option<&RetireWrites>) -> PlibRegistry {
    match retire.and_then(|r| r.registry.as_ref()) {
        Some(registry) => {
            let mut registry = registry.clone();
            prune_non_existing(root, &mut registry);
            registry
        }
        None => read_plib_registry(root),
    }
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
/// joined with `PreservedLibsRegistry`'s own hardcoded filename: where
/// the `files` backend keeps the registry. Tests only; production code
/// goes through [`portage_vdb::InstalledDb::preserved_libs`] (S1.4).
#[cfg(test)]
fn plib_registry_path(root: &Path) -> PathBuf {
    root.join("var/lib/portage/preserved_libs_registry")
}

/// The registry's file format (real `json.dumps` shape) is parsed by
/// [`portage_vdb::parse_preserved_libs`]. Tests only: production reads go
/// through [`read_plib_registry`].
#[cfg(test)]
fn parse_plib_registry(text: &str) -> Option<PlibEntries> {
    portage_vdb::parse_preserved_libs(text).map(plib_entries_from_vdb)
}

/// [`portage_vdb::PreservedLibsEntry`] records as this module's
/// `(cpv, counter, paths)` tuples.
fn plib_entries_from_vdb(
    entries: BTreeMap<String, portage_vdb::PreservedLibsEntry>,
) -> PlibEntries {
    entries
        .into_iter()
        .map(|(key, e)| (key, (e.cpv, e.counter, e.paths)))
        .collect()
}

/// The reverse of [`plib_entries_from_vdb`].
fn plib_entries_to_vdb(entries: &PlibEntries) -> BTreeMap<String, portage_vdb::PreservedLibsEntry> {
    entries
        .iter()
        .map(|(key, (cpv, counter, paths))| {
            (
                key.clone(),
                portage_vdb::PreservedLibsEntry {
                    cpv: cpv.clone(),
                    counter: counter.clone(),
                    paths: paths.clone(),
                },
            )
        })
        .collect()
}

/// Real `load()`: a missing or unparseable registry file degrades
/// gracefully to an empty registry rather than an error. Like real
/// `load()` (`PreservedLibsRegistry.py:load`), the parsed snapshot is
/// kept as `orig_entries` and `prune_non_existing` runs immediately --
/// every consumer below therefore sees the pruned registry, exactly as
/// real consumers of `load()` do. The stored registry comes from the
/// root's [`portage_vdb::InstalledDb::preserved_libs`] (S1.4; on `files`
/// the same one read of `var/lib/portage/preserved_libs_registry`); the
/// `lstat`-based prune stays here (N6).
fn read_plib_registry(root: &Path) -> PlibRegistry {
    let parsed: PlibEntries = portage_vdb::for_root(root)
        .preserved_libs()
        .map(|libs| plib_entries_from_vdb(libs.entries))
        .unwrap_or_default();
    let mut registry = PlibRegistry {
        orig_entries: parsed.clone(),
        entries: parsed,
    };
    prune_non_existing(root, &mut registry);
    registry
}

/// Lexical POSIX `normpath` (real `os.path.normpath`), for
/// `plib_abssymlink` below: collapses `.`/`..`/duplicate separators
/// without touching the filesystem.
fn norm_posix_path(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for comp in path.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    let mut out = parts.join("/");
    if absolute {
        out.insert(0, '/');
    }
    if out.is_empty() {
        out.push('.');
    }
    out
}

/// Real `portage.abssymlink(symlink, target)` (`lib/portage/__init__.py`):
/// the absolute path of a symlink's target -- the target itself when
/// absolute, otherwise resolved against the symlink's own directory and
/// normalized. `path` is the stored registry path (always absolute).
fn plib_abssymlink(path: &str, target: &str) -> String {
    if target.starts_with('/') {
        norm_posix_path(target)
    } else {
        let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        norm_posix_path(&format!("{dir}/{target}"))
    }
}

/// Real `PreservedLibsRegistry.pruneNonExisting`
/// (`PreservedLibsRegistry.py:180-219`): drop every recorded path that
/// no longer exists on disk (`lstat` failure), rebuild each surviving
/// entry as regular files first (in stored order), then the symlinks
/// whose `abssymlink` target is one of those regular files (a tool like
/// `eselect-opengl` may have repointed a soname symlink elsewhere -- bug
/// #406837 -- and the orphaned hardlink is found separately), and drop
/// the entry entirely when nothing survives. Only symlinks and regular
/// files count (real `S_ISLNK`/`S_ISREG`); anything else is neither.
fn prune_non_existing(root: &Path, registry: &mut PlibRegistry) {
    let mut dead_keys = Vec::new();
    for (cps, (_cpv, _counter, paths)) in registry.entries.iter_mut() {
        let mut ordered: Vec<String> = Vec::new();
        let mut hardlinks: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut symlinks: Vec<(String, String)> = Vec::new();
        for f in paths.iter() {
            let full = root.join(f.trim_start_matches('/'));
            let meta = match std::fs::symlink_metadata(&full) {
                Ok(meta) => meta,
                Err(_) => continue,
            };
            if meta.file_type().is_symlink() {
                match std::fs::read_link(&full) {
                    Ok(target) => symlinks.push((f.clone(), target.to_string_lossy().into_owned())),
                    Err(_) => continue,
                }
            } else if meta.file_type().is_file() {
                hardlinks.insert(f.clone());
                ordered.push(f.clone());
            }
        }
        for (f, target) in &symlinks {
            if hardlinks.contains(&plib_abssymlink(f, target)) {
                ordered.push(f.clone());
            }
        }
        if ordered.is_empty() {
            dead_keys.push(cps.clone());
        } else {
            *paths = ordered;
        }
    }
    for key in dead_keys {
        registry.entries.remove(&key);
    }
}

/// Real `store()`'s own `json.dumps(..., ensure_ascii=False,
/// indent="\t", sort_keys=True)` layout (`PreservedLibsRegistry.store`;
/// serialised by [`portage_vdb::format_preserved_libs`] and stored by
/// the root's `WriteTxn::set_preserved_libs`, S1.4)
/// -- `BTreeMap` already keeps keys sorted -- written via a plain
/// `fs::write` (real `atomic_ofstream`'s own atomicity is a
/// portuale-wide cut, not this slice's). An empty dict serializes as
/// exactly `{}` and carries no trailing newline, matching Python's own
/// output byte-for-byte. And like real `store()`, an unchanged registry
/// (`entries == orig_entries`, see `PlibRegistry`) is not rewritten at
/// all: not even the parent directory is created, so a packaged 0-byte
/// file stays 0 bytes and a missing file stays missing.
///
/// (`SANDBOX_ON` is not honored: real checks it because its own
/// registry mutations run inside sandboxed phases, while portuale's run
/// in portuale's own unsandboxed process -- the phases it spawns are
/// separate bash children.)
fn write_plib_registry(root: &Path, registry: &PlibRegistry) -> Result<(), String> {
    let Some(libs) = plib_store(registry) else {
        return Ok(());
    };
    let db = portage_vdb::for_root(root);
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    txn.set_preserved_libs(&libs).map_err(|e| e.to_string())?;
    txn.commit().map_err(|e| e.to_string())
}

/// What [`write_plib_registry`] stores: `None` when the registry is
/// unchanged since it was loaded (real `store()` writes nothing then).
fn plib_store(registry: &PlibRegistry) -> Option<portage_vdb::PreservedLibs> {
    (registry.entries != registry.orig_entries).then(|| portage_vdb::PreservedLibs {
        entries: plib_entries_to_vdb(&registry.entries),
        loaded: plib_entries_to_vdb(&registry.orig_entries),
    })
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
/// own `filename` (already `ROOT`-relative, portuale's own `CONTENTS`/
/// `NEEDED.ELF.2` convention -- no `os.path.join(root, ...)` needed the
/// way real Python does, since both sides already agree on the same
/// convention here) still appears among the *surviving* `CONTENTS`
/// paths is kept; every other entry (now pointing at a file this package
/// no longer owns) is dropped -- stale linkage data that would otherwise
/// corrupt a *later* `LinkageMap.rebuild()`'s own preserve-libs decision
/// for some *other* package's own future unmerge.
fn remove_from_contents(root: &Path, cpv: &str, paths: &BTreeSet<String>) -> Result<(), String> {
    remove_from_contents_into(root, cpv, paths, None)
}

/// [`remove_from_contents`], optionally collecting the rewrites into
/// `retire` (feat#157 S4.2) instead of committing each at once.
fn remove_from_contents_into(
    root: &Path,
    cpv: &str,
    paths: &BTreeSet<String>,
    mut retire: Option<&mut RetireWrites>,
) -> Result<(), String> {
    let Some((category, pf)) = cpv.split_once('/') else {
        return Ok(());
    };
    // W4 through the backend: `read_entry_text` is today's `read_to_string(..)
    // else return Ok(())` (missing, unreadable or non-UTF-8 all stop here),
    // `replace_file` its two in-place `std::fs::write`s.
    let key = portage_vdb::EntryKey::new(category, pf);
    let Some(text) = read_entry_text(root, category, pf, "CONTENTS") else {
        return Ok(());
    };
    let db = portage_vdb::for_root(root);
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
    let mut replace = |name: &str, data: String| -> Result<(), String> {
        if let Some(retire) = retire.as_deref_mut() {
            retire
                .files
                .push((key.clone(), name.to_string(), data.into_bytes()));
            return Ok(());
        }
        let mut txn = db.begin_write().map_err(|e| e.to_string())?;
        txn.replace_file(&key, name, data.as_bytes())
            .map_err(|e| e.to_string())?;
        txn.commit().map_err(|e| e.to_string())
    };
    replace("CONTENTS", new_text)?;

    if removed && let Some(needed_text) = read_entry_text(root, category, pf, "NEEDED.ELF.2") {
        let new_needed: String = crate::needed_elf::NeededEntry::parse_file(&needed_text)
            .into_iter()
            .filter(|entry| surviving_paths.contains(&entry.filename))
            .map(|entry| entry.to_needed_line())
            .collect();
        replace("NEEDED.ELF.2", new_needed)?;
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
        if let Some((entry_cpv, entry_counter, _)) = registry.entries.get(&cps)
            && entry_cpv == cpv
            && entry_counter.trim() == counter
        {
            registry.entries.remove(&cps);
        }
    } else {
        registry
            .entries
            .insert(cps, (cpv.to_string(), counter.to_string(), paths.to_vec()));
    }
}

/// `read_all_needed_entries` plus real `LinkageMap.rebuild()`'s own
/// preserved-libs branch (`needed_elf::scan_preserved_lib_entries`):
/// the shared linkage-map input of the preserve-libs computations
/// (`find_preserve_paths_for_merge`, `preserve_libs_on_unmerge`, and
/// the post-unmerge prune `find_unused_preserved_libs` below).
/// Scanned entries join their owner's already-present group when one
/// exists (real groups everything by owner for the bundled-library
/// runpath inference), else form a new one; vdb entries come first, so
/// on a same-inode conflict the vdb data wins (real indexes the
/// `scanelf` line first instead -- the two describe the same live file,
/// so they agree in practice). Skipped entirely when there is nothing
/// to scan (backlog #178's second bump is the first merge that ever
/// has anything to scan).
///
/// `exclude_cpv` is real `LinkageMap.rebuild()`'s own `exclude_pkgs` (a
/// package being unmerged contributes neither `NEEDED.ELF.2` lines nor
/// registry orphans -- its data "would only serve to corrupt the
/// `LinkageMap`"): the unmerged instance in replacement mode, `None`
/// everywhere else. `replacement_preserved` is real `preserve_paths`
/// (the merge-side just-preserved set, owner -> paths, not yet
/// registered at prune time, so it must be passed explicitly): scanned
/// under the replacing cpv -- real indexes it with owner `None` (and
/// skips the bundled-library runpath inference for `None`); grouping
/// it under the replacing cpv is the closest this `String`-keyed input
/// gets, and the owner only feeds that inference, never the keep/drop
/// verdict.
///
/// `replacement_needed` is real `LinkageMap.rebuild()`'s own
/// `include_file` (backlog #224): the replacing package's
/// `NEEDED.ELF.2` lines, fed explicitly because its vdb entry still
/// sits in its `-MERGING-<pf>` temporary at prune time and enumeration
/// (`read_all_needed_entries`, real `cpv_all()`) skips it -- without
/// the feed a preserved library whose only remaining consumer is the
/// replacing package looks orphaned and is pruned. Real indexes those
/// lines with owner `None` (`LinkageMapELF.rebuild` processes the
/// include file first); grouped here under the replacing cpv for the
/// same `String`-keyed reason as `replacement_preserved` above, with
/// the same verdict-neutrality (the owner only feeds the same-owner
/// bundled-library runpath inference, never the keep/drop verdict --
/// and these lines genuinely share one owner, so the inference is, if
/// anything, more faithful). Empty on every path but the replace-loop
/// prune and the merge-side preserve computation
/// (`find_preserve_paths_for_merge`, backlog #229): standalone
/// `emerge -C` has no replacing package, and the
/// merge-end prune runs after the rename into place, when the new
/// entry's own lines are already enumerated.
fn linkage_owner_entries(
    root: &Path,
    preserved: &BTreeMap<String, Vec<String>>,
    exclude_cpv: Option<&str>,
    replacement_preserved: &BTreeMap<String, Vec<String>>,
    replacement_needed: &[(String, Vec<crate::needed_elf::NeededEntry>)],
) -> Vec<(String, Vec<crate::needed_elf::NeededEntry>)> {
    let mut owner_entries: Vec<(String, Vec<crate::needed_elf::NeededEntry>)> =
        crate::needed_elf::read_all_needed_entries(root)
            .into_iter()
            .filter(|(cpv, _)| Some(cpv.as_str()) != exclude_cpv)
            .collect();
    for (owner, entries) in replacement_needed {
        if let Some(slot) = owner_entries.iter_mut().find(|(o, _)| o == owner) {
            slot.1.extend(entries.iter().cloned());
        } else {
            owner_entries.push((owner.clone(), entries.clone()));
        }
    }
    let mut scan_input = preserved.clone();
    for (owner, paths) in replacement_preserved {
        scan_input
            .entry(owner.clone())
            .or_default()
            .extend(paths.iter().cloned());
    }
    if let Some(excluded) = exclude_cpv {
        scan_input.remove(excluded);
    }
    if scan_input.is_empty() {
        return owner_entries;
    }
    for (owner, entries) in crate::needed_elf::scan_preserved_lib_entries(root, &scan_input) {
        if let Some(slot) = owner_entries.iter_mut().find(|(o, _)| o == &owner) {
            slot.1.extend(entries);
        } else {
            owner_entries.push((owner, entries));
        }
    }
    owner_entries
}

fn owner_entries_with_preserved_orphans(
    root: &Path,
    preserved: &BTreeMap<String, Vec<String>>,
) -> Vec<(String, Vec<crate::needed_elf::NeededEntry>)> {
    linkage_owner_entries(root, preserved, None, &BTreeMap::new(), &[])
}

/// Real `dblink.treewalk()`'s own `needed = os.path.join(inforoot,
/// LinkageMap._needed_aux_key)` (backlogs #224/#229): the replacing
/// package's `NEEDED.ELF.2` lines, read from its `-MERGING-<new_pf>`
/// temporary vdb entry, which holds the build-info copy from
/// `populate_vdb_tmp` (every build-info file is copied there, the
/// `scanelf` QA step's `NEEDED.ELF.2` included). Real's replace loop
/// passes that path as `dblink.unmerge(needed=...)` into
/// `_prune_plib_registry`, whose `_linkmap_rebuild(include_file=needed,
/// ...)` indexes the lines explicitly -- because the entry is still
/// `-MERGING-`, `cpv_all()` (portuale's own `read_all_needed_entries`,
/// which skips `-MERGING-` names) never yields them. Grouped under the
/// replacing cpv (see `linkage_owner_entries` for why that owner
/// choice is verdict-neutral). Empty when the temporary entry carries
/// no `NEEDED.ELF.2` at all (a package with no ELF content -- real
/// `grabfile` on a missing file degrades the same way), so callers on
/// paths with no replacing package simply never call this.
fn replacement_needed_entries(
    root: &Path,
    category: &str,
    new_pf: &str,
) -> Vec<(String, Vec<crate::needed_elf::NeededEntry>)> {
    // The pending entry's file (`files`: `-MERGING-<new_pf>/NEEDED.ELF.2`,
    // one `open`); unreadable or non-UTF-8 is "no lines", as before.
    let Some(text) = portage_vdb::for_root(root)
        .read_pending_file(
            &portage_vdb::EntryKey::new(category, new_pf),
            "NEEDED.ELF.2",
        )
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes).ok())
    else {
        return Vec::new();
    };
    let entries = crate::needed_elf::NeededEntry::parse_file(&text);
    if entries.is_empty() {
        return Vec::new();
    }
    vec![(format!("{category}/{new_pf}"), entries)]
}

/// Real `dblink.treewalk`'s own pre-replace-loop preserve-libs
/// computation (the `_linkmap_rebuild(include_file=needed)` +
/// `_find_libs_to_preserve()` block): rebuild the system-wide
/// `LinkageMap` and select the installed same-slot instance's libraries
/// that are still needed (`_find_libs_to_preserve()`, `unmerge=False`).
/// `new_image_paths` is the just-merged image's own path set (parsed
/// from `merge_tree`'s `CONTENTS` text) -- real `self.isowner(f)` on the
/// merging instance, whose `CONTENTS` is already written at this point,
/// so a library the new version itself ships is never preserved.
/// Returns `None` when no same-slot instance is installed (a first-ever
/// install preserves nothing) or it owns no files (real
/// `installed_instance.getcontents()` falsy skips the rebuild); otherwise
/// the preserve set plus the old instance's own raw `CONTENTS` text (for
/// entry injection below).
///
/// Must run after `merge_tree` (the files are on disk for `lstat`) and
/// after `populate_vdb_tmp` (backlog #183: the new vdb entry *is*
/// written by then, into its `-MERGING-<pf>` temporary -- the doc used
/// to say it was not). Enumeration (`read_all_needed_entries`, real
/// `cpv_all()`) still skips `-MERGING-` names, so the new package
/// reaches the linkage map only through the explicit include feed below
/// (real `include_file=needed`, backlog #229): without it a library
/// owned by the replaced instance whose only consumer is the replacing
/// package would not be preserved. Runs before `write_vdb_tmp_contents`.
fn find_preserve_paths_for_merge(
    root: &Path,
    category: &str,
    package: &str,
    new_pf: &str,
    main_slot: &str,
    new_image_paths: &BTreeSet<String>,
) -> Option<(BTreeSet<String>, String)> {
    let old_pf = installed_instance_pf(root, category, package, main_slot)?;
    let old_contents_text =
        read_entry_text(root, category, &old_pf, "CONTENTS").unwrap_or_default();
    let old_contents: Vec<String> = old_contents_text
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1).map(String::from))
        .collect();
    if old_contents.is_empty() {
        return None;
    }

    // Real `LinkageMap.rebuild()` sees the registry's own preserved
    // libs too (`_linkmap_rebuild` loads them via the orphan `scanelf`
    // branch): without them a previously-preserved library the replaced
    // instance no longer ships in its own `NEEDED.ELF.2` is invisible
    // here, and a second consecutive soname bump would preserve nothing
    // (backlog #178).
    let preserved = read_plib_registry(root).preserved_libs();
    // Real `LinkageMap.rebuild(include_file=needed)` (backlog #229):
    // the replacing package's `-MERGING-<new_pf>/NEEDED.ELF.2` lines
    // (live since `populate_vdb_tmp`, skipped by enumeration) join the
    // linkage input first, owner verdict-neutral -- see
    // `linkage_owner_entries`. A library owned by the replaced
    // instance whose only consumer is the replacing package is
    // preserved only because of this feed.
    let replacement_needed = replacement_needed_entries(root, category, new_pf);
    let owner_entries = linkage_owner_entries(
        root,
        &preserved,
        None,
        &BTreeMap::new(),
        &replacement_needed,
    );
    let map = crate::needed_elf::rebuild(root, &owner_entries);
    let defpath =
        crate::needed_elf::getlibpaths(root, std::env::var("LD_LIBRARY_PATH").ok().as_deref());

    let old_owner_is_owner = |p: &str| owns_path_pf(root, category, &old_pf, p);
    let new_owner_is_owner = |p: &str| new_image_paths.contains(p);

    Some((
        crate::needed_elf::find_libs_to_preserve(
            root,
            &map,
            &defpath,
            &old_contents,
            &old_owner_is_owner,
            &new_owner_is_owner,
        ),
        old_contents_text,
    ))
}

/// Real `dblink._add_preserve_libs_to_contents` (`vartree.py:3775-3826`):
/// copy the preserved paths' own entries from the replaced instance's
/// `CONTENTS` into the merging package's (digest/mtime carried over
/// verbatim -- the files are *not* reinstalled), printing real
/// `>>> needed    {obj|sym} <path>` per path in `sorted()` order.
/// `new_image_paths` is the just-merged image's own path set: a
/// preserved path already there keeps its own fresh entry (real assigns
/// into a dict, so re-adding is a no-op, never a duplicate line).
/// Returns the `CONTENTS` lines to append plus the surviving preserve set:
/// a path with no entry in the old `CONTENTS` cannot be preserved (real
/// `!!! File ... will not be preserved due to missing contents entry`)
/// and is dropped, exactly like real dropping it from `preserve_paths`.
fn inject_preserved_libs_into_contents(
    old_contents_text: &str,
    preserve_paths: &BTreeSet<String>,
    new_image_paths: &BTreeSet<String>,
) -> (String, BTreeSet<String>) {
    let mut lines = String::new();
    let mut surviving = BTreeSet::new();
    for f in preserve_paths {
        let Some(entry) = old_contents_text.lines().find(|line| {
            let mut parts = line.split_whitespace();
            parts.next();
            parts.next() == Some(f.as_str())
        }) else {
            eprintln!("!!! File '{f}' will not be preserved due to missing contents entry");
            continue;
        };
        surviving.insert(f.clone());
        let obj_type = entry.split_whitespace().next().unwrap_or("obj");
        println!(">>> needed    {obj_type} {f}");
        if !new_image_paths.contains(f) {
            lines.push_str(entry);
            lines.push('\n');
        }
    }
    (lines, surviving)
}

/// Real `dblink.treewalk()`'s own post-replace-loop registration
/// (`vartree.py:5266-5272`): `register(self.mycpv, slot, counter,
/// sorted(preserve_paths))` -- the one `cp:slot` record is **replaced**
/// by the merging package's `(cpv, counter, paths)`, never merged with
/// or re-attributed from the old entry (real `register()`'s own
/// unconditional overwrite, `PreservedLibsRegistry.py:165-169`). Reads
/// the counter back from the just-written new vdb entry (real
/// `counter_tick()`'s own value, the same one `COUNTER` carries).
/// No-op when nothing was preserved (real `if preserve_paths:`), leaving
/// the replace loop's unregistration of the old record as the final
/// state. Keep #167's serialisation and write-only-on-change rules
/// intact: `register_preserved_libs` + `write_plib_registry` do that.
fn register_merge_preserved_libs(
    root: &Path,
    category: &str,
    pn: &str,
    new_pf: &str,
    main_slot: &str,
    preserve_paths: &BTreeSet<String>,
) -> Result<(), String> {
    if preserve_paths.is_empty() {
        return Ok(());
    }
    let new_counter =
        read_entry_text(root, category, new_pf, "COUNTER").unwrap_or_else(|| "0".to_string());
    let registry = merge_plib_registration(
        root,
        category,
        pn,
        new_pf,
        main_slot,
        preserve_paths,
        &new_counter,
    );
    write_plib_registry(root, &registry)
}

/// The registry [`register_merge_preserved_libs`] stores, with the
/// counter given: the freshly loaded (and pruned) registry plus the
/// merging package's `register(...)`. Split out so a database backend can
/// store it in the publishing transaction (feat#157 S4.1), reading the
/// counter from the still-pending entry.
fn merge_plib_registration(
    root: &Path,
    category: &str,
    pn: &str,
    new_pf: &str,
    main_slot: &str,
    preserve_paths: &BTreeSet<String>,
    new_counter: &str,
) -> PlibRegistry {
    let new_cpv = format!("{category}/{new_pf}");
    let mut registry = read_plib_registry(root);
    let paths_vec: Vec<String> = preserve_paths.iter().cloned().collect();
    register_preserved_libs(
        &mut registry,
        &new_cpv,
        category,
        pn,
        main_slot,
        new_counter,
        &paths_vec,
    );
    registry
}

/// Real `dblink._prune_plib_registry`, called from real `unmerge()`
/// with `unmerge=True` right before real `_unmerge_pkgfiles()` runs.
/// Real `preserve_paths` (a `_prune_plib_registry` parameter, not to be
/// confused with this function's own *return* value) is only ever
/// non-`None` when a real depgraph-driven upgrade transaction already
/// computed it via a companion `merge()` call in the *same* transaction:
/// `None` on the standalone path (portuale's own `merge`/`unmerge` are
/// always separate, independent CLI invocations), the merge-side set on
/// the replace loop. An instance that owns no files still unregisters
/// (real `instance_owns_files` gates only the rebuild and the scans).
///
/// Real order: rebuild the system-wide `LinkageMap` from every real
/// installed package's own vdb-stored `NEEDED.ELF.2`
/// (`linkage_owner_entries` + `rebuild` -- real `exclude_pkgs=None` on
/// the standalone path, since the package being unmerged hasn't left
/// the vdb yet, so its own data is still really part of the map,
/// matching real behavior exactly). Compute `needed_elf::find_
/// libs_to_preserve` with `new_owner_is_owner` always `false` (matching
/// what real `not unmerge and self.isowner(f)` collapses to when
/// `unmerge` is `true`) and `old_owner_is_owner` real `self.isowner`
/// (`owns_path_pf`, this exact package's own real `CONTENTS`).
/// Unconditionally unregister this package's own prior registry entry
/// first (real `plib_registry.unregister`); if anything is actually
/// preserved, register this package -- the one being removed -- as the
/// new keeper of those paths (real `plib_registry.register`).
///
/// `is_replacement` is real `unmerge_with_replacement`
/// (`preserve_paths is not None`, set exactly when `treewalk()`'s own
/// replace loop drives this unmerge): real then skips the
/// `_find_libs_to_preserve(unmerge=True)` re-registration entirely and
/// only unregisters the old entry -- the preserved set was already
/// computed merge-side (`find_preserve_paths_for_merge`) and injected
/// into the replacing package's own `CONTENTS`, so the files survive
/// removal through same-slot ownership (`remove_contents`' own
/// `is_owned` skip) and the record lands under the merging package via
/// `register_merge_preserved_libs`. Returns an empty set in that mode;
/// the standalone (`emerge -C`) mode returns the preserved paths for
/// the caller to exclude from its own real file-removal loop.
///
/// Returns the set of preserved paths (already `ROOT`-relative absolute
/// paths, portuale's own `CONTENTS` convention) -- the caller is
/// responsible for excluding them from its own real file-removal loop
/// (real "remove the preserved files from our contents so that they
/// won't be unmerged"; portuale's own vdb entry directory gets deleted
/// wholesale moments later regardless, so there's no separate real
/// `CONTENTS`-file rewrite to also perform here).
#[cfg(test)]
pub(crate) fn preserve_libs_on_unmerge(
    root: &Path,
    category: &str,
    pn: &str,
    pf: &str,
    slot: &str,
    contents_text: &str,
    is_replacement: bool,
) -> Result<BTreeSet<String>, String> {
    preserve_libs_on_unmerge_into(
        root,
        category,
        pn,
        pf,
        slot,
        contents_text,
        is_replacement,
        None,
    )
}

/// [`preserve_libs_on_unmerge`], optionally keeping the registry in
/// `retire` instead of writing it (feat#157 S4.2).
#[allow(clippy::too_many_arguments)]
pub(crate) fn preserve_libs_on_unmerge_into(
    root: &Path,
    category: &str,
    pn: &str,
    pf: &str,
    slot: &str,
    contents_text: &str,
    is_replacement: bool,
    retire: Option<&mut RetireWrites>,
) -> Result<BTreeSet<String>, String> {
    // Real `_prune_plib_registry` still runs `unregister()` for an
    // instance that owns no files (`instance_owns_files` gates only the
    // linkmap rebuild and the preserve/prune scans below); only the
    // preserve-set computation is skipped.
    let instance_owns_files = !contents_text.trim().is_empty();

    let counter = read_entry_text(root, category, pf, "COUNTER").unwrap_or_else(|| "0".to_string());
    let cpv = format!("{category}/{pf}");

    let mut registry = read_plib_registry(root);
    // The orphan scan below must see the registry *before* this
    // package's own entry is unregistered: real `_prune_plib_registry`
    // rebuilds the `LinkageMap` (whose orphan `scanelf` branch reads the
    // registry) before `unregister()` runs, so a previously-preserved
    // library this instance no longer ships in its own `NEEDED.ELF.2`
    // is still indexed for the computation.
    let preserved = if is_replacement || !instance_owns_files {
        BTreeSet::new()
    } else {
        let old_contents: Vec<String> = contents_text
            .lines()
            .filter_map(|line| line.split_whitespace().nth(1).map(String::from))
            .collect();

        let owner_entries = owner_entries_with_preserved_orphans(root, &registry.preserved_libs());
        let map = crate::needed_elf::rebuild(root, &owner_entries);
        let defpath =
            crate::needed_elf::getlibpaths(root, std::env::var("LD_LIBRARY_PATH").ok().as_deref());

        let old_owner_is_owner = |p: &str| owns_path_pf(root, category, pf, p);
        let new_owner_is_owner = |_: &str| false;

        crate::needed_elf::find_libs_to_preserve(
            root,
            &map,
            &defpath,
            &old_contents,
            &old_owner_is_owner,
            &new_owner_is_owner,
        )
    };

    register_preserved_libs(&mut registry, &cpv, category, pn, slot, &counter, &[]);
    if !is_replacement && !preserved.is_empty() {
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
    match retire {
        Some(retire) => retire.registry = Some(registry),
        None => write_plib_registry(root, &registry)?,
    }

    Ok(if is_replacement {
        BTreeSet::new()
    } else {
        preserved
    })
}

/// Every currently-registered preserved-library path, keyed by the cpv
/// that owns it (real `PreservedLibsRegistry.getPreservedLibs()`). A thin
/// `pub(crate)` reader for the resolver-side `@preserved-rebuild` set and
/// the `display_preserved_libs` advisory, both of which live outside this
/// module.
pub(crate) fn preserved_lib_paths(root: &Path) -> BTreeMap<String, Vec<String>> {
    read_plib_registry(root).preserved_libs()
}

/// Real `dblink._find_unused_preserved_libs`: the registered
/// preserved libraries that no installed package links against any
/// more, keyed by the cpv that owns them (so the caller can prune each
/// owner's `CONTENTS`). Rebuilds the system-wide `LinkageMap` from
/// every installed `NEEDED.ELF.2` **plus** the registry orphans no
/// `NEEDED.ELF.2` indexes (`linkage_owner_entries`, real
/// `LinkageMap.rebuild`'s own preserved-libs branch) -- without the
/// orphans a just-preserved library is invisible here -- and, for each
/// registered path that still exists on disk, collects its consumers
/// -- from the linkage map when the path is still an indexed object,
/// otherwise via a basename==soname reverse lookup
/// (`needed_elf::soname_consumers`) for a library whose owning package
/// has already left the vdb. The consumer/preserved graph is then
/// reduced by `needed_elf::find_unneeded_preserved` (real
/// `_find_unneeded_preserved_nodes`, cycle-aware).
///
/// `unmerge_no_replacement` + `being_unmerged`: real "also eliminate
/// consumers that are going to be unmerged if unmerge_no_replacement is
/// True" (real `unmerge_no_replacement = unmerge and not
/// unmerge_with_replacement`): on a plain unmerge with no replacement,
/// a consumer that is itself entirely owned by the package now being
/// removed does not keep a library alive; in replacement mode those
/// consumers survive through the replacing package, so they still
/// count. `being_unmerged` returns true for such a path.
///
/// `exclude_cpv` + `replacement_preserved`: real `exclude_pkgs` +
/// `preserve_paths` (see `linkage_owner_entries`): in replacement mode
/// the unmerged instance's own linkmap data is excluded while the
/// merge-side just-preserved set is scanned in, so a preserved library
/// whose only linkmap consumer is a just-preserved file is kept. Both
/// are empty/`None` on every other path.
///
/// `replacement_needed`: real `include_file` (see
/// `linkage_owner_entries`): the replacing package's own
/// `NEEDED.ELF.2` lines, so a preserved library whose only remaining
/// consumer is the replacing package itself survives the
/// replace-loop prune. Empty on every other path (standalone
/// `emerge -C` has no replacing package; the merge-end prune runs
/// after the rename into place, when the new entry is enumerated).
///
/// Deliberate narrowing vs. real: the per-consumer "an alternative,
/// non-preserved provider of the same soname is installed" edge removal
/// (real "erroneously preserved due to a move from one directory to
/// another") is not reproduced -- portuale has never had a preserved-lib
/// directory-move path, and the collision-protect takeover
/// (`unregister_preserved_libs`) already covers the same-path case.
#[cfg(test)]
pub(crate) fn find_unused_preserved_libs(
    root: &Path,
    unmerge_no_replacement: bool,
    being_unmerged: &dyn Fn(&str) -> bool,
    exclude_cpv: Option<&str>,
    replacement_preserved: &BTreeMap<String, Vec<String>>,
    replacement_needed: &[(String, Vec<crate::needed_elf::NeededEntry>)],
) -> BTreeMap<String, BTreeSet<String>> {
    find_unused_preserved_libs_in(
        root,
        read_plib_registry(root),
        unmerge_no_replacement,
        being_unmerged,
        exclude_cpv,
        replacement_preserved,
        replacement_needed,
    )
}

/// [`find_unused_preserved_libs`] over an explicit registry (the one an
/// unmerge in progress holds in memory, feat#157 S4.2).
fn find_unused_preserved_libs_in(
    root: &Path,
    registry: PlibRegistry,
    unmerge_no_replacement: bool,
    being_unmerged: &dyn Fn(&str) -> bool,
    exclude_cpv: Option<&str>,
    replacement_preserved: &BTreeMap<String, Vec<String>>,
    replacement_needed: &[(String, Vec<crate::needed_elf::NeededEntry>)],
) -> BTreeMap<String, BTreeSet<String>> {
    let plib_dict = registry.preserved_libs();
    if plib_dict.is_empty() {
        return BTreeMap::new();
    }

    let owner_entries = linkage_owner_entries(
        root,
        &plib_dict,
        exclude_cpv,
        replacement_preserved,
        replacement_needed,
    );
    let map = crate::needed_elf::rebuild(root, &owner_entries);
    let defpath =
        crate::needed_elf::getlibpaths(root, std::env::var("LD_LIBRARY_PATH").ok().as_deref());

    let all_preserved: BTreeSet<String> = plib_dict.values().flatten().cloned().collect();
    let mut path_cpv: BTreeMap<String, String> = BTreeMap::new();
    for (cpv, paths) in &plib_dict {
        for p in paths {
            path_cpv.insert(p.clone(), cpv.clone());
        }
    }

    let exists = |p: &str| std::fs::symlink_metadata(root.join(p.trim_start_matches('/'))).is_ok();

    let mut consumers_by_preserved: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for path in &all_preserved {
        if !exists(path) {
            continue;
        }
        let indexed = crate::needed_elf::find_consumers(root, &map, &defpath, path, None, true);
        let mut consumers: BTreeSet<String> = match indexed {
            Ok(set) => set,
            // Not an indexed object (owning package gone) -- fall back to
            // "does any installed object still need this soname".
            Err(_) => {
                let soname = path.rsplit('/').next().unwrap_or(path);
                crate::needed_elf::soname_consumers(&map, soname)
            }
        };
        if unmerge_no_replacement {
            consumers.retain(|c| !being_unmerged(c));
        }
        consumers_by_preserved.insert(path.clone(), consumers);
    }

    let unneeded =
        crate::needed_elf::find_unneeded_preserved(root, &consumers_by_preserved, &all_preserved);

    let mut cpv_lib_map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for path in unneeded {
        if let Some(cpv) = path_cpv.get(&path) {
            cpv_lib_map.entry(cpv.clone()).or_default().insert(path);
        }
    }
    cpv_lib_map
}

/// Real `dblink._prune_plib_registry`'s own tail
/// (`_remove_preserved_libs` + the `removeFromContents` loop +
/// `pruneNonExisting`), which real portage runs at the end of **both** a
/// merge (`treewalk` -- "For gcc upgrades, preserved libs have to be
/// removed after the library path has been updated") and an unmerge.
///
/// Deletes every preserved-library file `find_unused_preserved_libs`
/// reports as unneeded (tolerating an already-gone file), removes empty
/// parent directories, strips the paths from each still-installed
/// owner's `CONTENTS`/`NEEDED.ELF.2` (`remove_from_contents`), rewrites
/// the registry with those paths gone (and drops any entry whose paths
/// no longer exist on disk -- real `pruneNonExisting`). Prints real
/// `<<< !needed  {obj|sym} <path>` per removed file. Returns the removed
/// `ROOT`-relative paths.
///
/// `unmerge_no_replacement` / `being_unmerged` / `exclude_cpv` /
/// `replacement_preserved` / `replacement_needed` are passed straight
/// through to `find_unused_preserved_libs` (see its own doc comment
/// for the real grounding); every caller except the replace-loop
/// unmerge passes `exclude_cpv=None` and empty feeds.
pub(crate) fn prune_unused_preserved_libs(
    root: &Path,
    unmerge_no_replacement: bool,
    being_unmerged: &dyn Fn(&str) -> bool,
    exclude_cpv: Option<&str>,
    replacement_preserved: &BTreeMap<String, Vec<String>>,
    replacement_needed: &[(String, Vec<crate::needed_elf::NeededEntry>)],
) -> Result<Vec<String>, String> {
    prune_unused_preserved_libs_into(
        root,
        unmerge_no_replacement,
        being_unmerged,
        exclude_cpv,
        replacement_preserved,
        replacement_needed,
        None,
    )
}

/// [`prune_unused_preserved_libs`], optionally reading the registry from
/// `retire` and leaving its writes (the registry, the W4 rewrites) there
/// instead of committing them (feat#157 S4.2). The files it removes are
/// removed at once either way.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prune_unused_preserved_libs_into(
    root: &Path,
    unmerge_no_replacement: bool,
    being_unmerged: &dyn Fn(&str) -> bool,
    exclude_cpv: Option<&str>,
    replacement_preserved: &BTreeMap<String, Vec<String>>,
    replacement_needed: &[(String, Vec<crate::needed_elf::NeededEntry>)],
    mut retire: Option<&mut RetireWrites>,
) -> Result<Vec<String>, String> {
    let cpv_lib_map = find_unused_preserved_libs_in(
        root,
        plib_registry_for(root, retire.as_deref()),
        unmerge_no_replacement,
        being_unmerged,
        exclude_cpv,
        replacement_preserved,
        replacement_needed,
    );

    let mut removed: Vec<String> = Vec::new();
    let mut parent_dirs: BTreeSet<PathBuf> = BTreeSet::new();
    let all_removed: BTreeSet<String> = cpv_lib_map.values().flatten().cloned().collect();
    for path in &all_removed {
        let abs = root.join(path.trim_start_matches('/'));
        let obj_type = match std::fs::symlink_metadata(&abs) {
            Ok(m) if m.file_type().is_symlink() => "sym",
            _ => "obj",
        };
        match std::fs::remove_file(&abs) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", abs.display())),
        }
        if let Some(parent) = abs.parent() {
            parent_dirs.insert(parent.to_path_buf());
        }
        println!("<<< !needed  {obj_type} {}", abs.display());
        removed.push(path.clone());
    }

    // Real "Remove empty parent directories if possible" -- walk upward
    // from each, stopping at the first non-empty one.
    for mut dir in parent_dirs {
        while dir.starts_with(root) && dir != root {
            if std::fs::remove_dir(&dir).is_err() {
                break;
            }
            match dir.parent() {
                Some(p) => dir = p.to_path_buf(),
                None => break,
            }
        }
    }

    // Strip the removed paths from every still-installed owner's vdb
    // CONTENTS/NEEDED.ELF.2, then rewrite the registry.
    let mut registry = plib_registry_for(root, retire.as_deref());
    for (cpv, paths) in &cpv_lib_map {
        let cat_pf = cpv.split_once('/');
        let still_installed = cat_pf
            .map(|(cat, pf)| {
                portage_vdb::for_root(root)
                    .has_entry(&portage_vdb::EntryKey::new(cat, pf))
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if still_installed {
            remove_from_contents_into(root, cpv, paths, retire.as_deref_mut())?;
        }
        for (entry_cpv, _counter, entry_paths) in registry.entries.values_mut() {
            if entry_cpv == cpv {
                entry_paths.retain(|p| !paths.contains(p));
            }
        }
    }

    // Real `_remove_preserved_libs`'s own tail (`vartree.py:3995`):
    // `self.vartree.dbapi._plib_registry.pruneNonExisting()` -- drop a
    // registry entry once none of its recorded paths exist on disk any
    // more (rebuilding survivors file-then-symlink, not just retaining).
    prune_non_existing(root, &mut registry);
    match retire {
        Some(retire) => retire.registry = Some(registry),
        None => write_plib_registry(root, &registry)?,
    }

    Ok(removed)
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

/// Hex MD5 of a file's bytes. Shared with the remote-merge server
/// (`remote_bundle::build_bundle` writes `build-info/BINPKGMD5`, backlog
/// #170): the same digest the local `merge_binpkg` records, so a later
/// index rebuild sees a still-current instance either way.
pub(crate) fn md5_hex(path: &Path) -> Result<String, String> {
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
        let children: Vec<PathBuf> = portage_util::read_dir_entries(&src_dir)
            .map_err(|e| format!("{}: {e}", src_dir.display()))?
            .into_iter()
            .map(|e| relative_dir.join(e.file_name()))
            .collect();

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
                let mut write_dest = dest.to_path_buf();
                let mut moveme = true;
                let protected_path = is_protected(root, config_protect, config_protect_mask, &dest);
                if protected_path {
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
                // Real `mergeme()`'s own symlink-over-directory branch
                // (`vartree.py`: `if mydmode is not None and S_ISDIR and
                // not protected`): a symlink can never replace a real
                // directory -- the symlink lands at the first
                // `dest.backup.NNNN` instead and the directory is kept.
                // This is the merge-time half of the `find_collisions`
                // exclusion above: directory-target symlinks are never
                // collision-checked (real `os.walk` puts them in `dirs`),
                // so they always reach here, and real still exits 0.
                // (File-target symlinks never reach here: they always
                // aborted in `_collision_protect` first.) `CONTENTS`
                // below still records the logical `abs_path`, like real.
                if moveme
                    && !protected_path
                    && write_dest == dest
                    && std::fs::symlink_metadata(&dest)
                        .map(|m| m.file_type().is_dir())
                        .unwrap_or(false)
                {
                    write_dest = new_backup_path(&dest);
                    eprintln!("Installation of a symlink is blocked by a directory:");
                    eprintln!("  '{}'", dest.display());
                    eprintln!("This symlink will be merged with a different name:");
                    eprintln!("  '{}'", write_dest.display());
                    eprintln!();
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
                    // Real `movefile()` installs a symlink by an atomic
                    // rename too -- see `replace_symlink_atomic`'s own
                    // doc comment (backlog #96).
                    replace_symlink_atomic(&target, &src, &write_dest, mtime)?;
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
                // Real `mergeme()`: a *newly created* directory gets
                // `os.chmod`/`os.chown`ed to the source's own recorded
                // mode/owner (`vartree.py:5865-5867`); a destination that
                // already exists as a directory is left completely alone
                // (the `--- {mydest}/` "kept as-is" branch, `:5808-5810`)
                // -- an existing shared directory (e.g. `/usr/lib`) must
                // never have its ownership clobbered by every package
                // that happens to also install into it.
                let dir_existed = dest.is_dir();
                std::fs::create_dir_all(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
                if !dir_existed {
                    use std::os::unix::fs::PermissionsExt;
                    let src_meta =
                        std::fs::metadata(&src).map_err(|e| format!("{}: {e}", src.display()))?;
                    lchown_or_chown(&dest, src_meta.uid(), src_meta.gid(), false)?;
                    std::fs::set_permissions(
                        &dest,
                        std::fs::Permissions::from_mode(src_meta.permissions().mode()),
                    )
                    .map_err(|e| format!("{}: {e}", dest.display()))?;
                }
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
                let mut write_dest = dest.to_path_buf();
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
                    // Real `mergeme()`'s `if self._needs_move(mysrc,
                    // mydest, mymode, mydmode)` gate (bug #722270): a
                    // destination that already is a regular file with
                    // the same mode and byte-identical content is left
                    // completely in place -- inode, ownership and xattrs
                    // survive the rebuild, and only the mtime is
                    // refreshed (`os.utime(mydest, ns=(mymtime,
                    // mymtime))`). A diverted `._cfgNNNN_` write never
                    // takes this path: real clears `mydmode` whenever
                    // `_protect` diverts (`vartree.py:5645-5647`), so it
                    // always moves there, and `write_dest != dest` says
                    // the same thing here.
                    if write_dest == dest && !needs_move(&src, &write_dest) {
                        filetime::set_file_mtime(
                            &write_dest,
                            filetime::FileTime::from_unix_time(mtime, 0),
                        )
                        .map_err(|e| format!("{}: {e}", write_dest.display()))?;
                    } else {
                        // Real `movefile()`'s atomic replacement -- the
                        // destination's existing inode is never opened for
                        // write (backlog #96): see
                        // `replace_file_atomic`'s own doc comment for why
                        // that is a safety requirement, not an
                        // optimization.
                        replace_file_atomic(&src, &write_dest, mtime)?;
                    }
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
                // device `EXDEV` failure). Portuale's own merge step
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
/// portuale recreates rather than moves). `mkfifo(3)`/`mknod(3)` both apply
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
        // SAFETY: `dest_c` is a CString that outlives the call; takes a
        // NUL-terminated path pointer plus a plain mode int.
        unsafe { libc::mkfifo(dest_c.as_ptr(), mode) }
    } else {
        let type_bit = if file_type.is_char_device() {
            libc::S_IFCHR
        } else {
            libc::S_IFBLK
        };
        // SAFETY: `dest_c` is a CString that outlives the call; takes a
        // NUL-terminated path pointer plus plain mode/dev ints.
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
    // SAFETY: `dest_c` is a CString that outlives the call; takes a
    // NUL-terminated path pointer plus a plain mode int.
    if unsafe { libc::chmod(dest_c.as_ptr(), mode) } != 0 {
        return Err(format!(
            "{}: {}",
            dest.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// Real `movefile()`'s ownership preservation: `lchown(dest, sstat.
/// st_uid, sstat.st_gid)` for a symlink (real `movefile.py:255`, via
/// `portage.data.lchown` -- a thin `os.lchown` wrapper, `data.py:18`),
/// `os.chown(dest, sstat.st_uid, sstat.st_gid)` for anything else (real
/// `_apply_stat`, `movefile.py:28-30`) -- both set `dest`'s owner/group
/// to match the *source*'s own recorded uid/gid from `${D}` (whatever
/// the build phases, or an ebuild's own `fowners`, actually left
/// there).
///
/// Real portage runs merges as root in normal operation, so this always
/// succeeds there; it's not gated on "am I root" here either, matching
/// real's own unconditional call with no privilege check at this call
/// site. Run unprivileged with the source already owned by the calling
/// process (the common single-user case `${D}`'s files are almost
/// always in), `chown(path, self_uid, self_gid)` is permitted regardless
/// and still succeeds. Only a genuinely privileged ownership change
/// attempted without root hits the same real `EPERM` real portage would
/// -- propagated here as an ordinary merge failure, exactly like every
/// other I/O error in this function, never silently swallowed.
fn lchown_or_chown(dest: &Path, uid: u32, gid: u32, is_symlink: bool) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    let dest_c = std::ffi::CString::new(dest.as_os_str().as_bytes())
        .map_err(|e| format!("{}: {e}", dest.display()))?;
    let ret = if is_symlink {
        // SAFETY: `dest_c` is a CString that outlives the call; takes a
        // NUL-terminated path pointer plus plain uid/gid ints.
        unsafe { libc::lchown(dest_c.as_ptr(), uid, gid) }
    } else {
        // SAFETY: `dest_c` is a CString that outlives the call; takes a
        // NUL-terminated path pointer plus plain uid/gid ints.
        unsafe { libc::chown(dest_c.as_ptr(), uid, gid) }
    };
    if ret != 0 {
        return Err(format!(
            "{}: {}",
            dest.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// Real `dblink._needs_move` (`vartree.py:6363`): `true` unless the
/// destination already exists as a regular file with the same full mode
/// and byte-identical content. When it returns `false`, real's
/// `mergeme` does not call `movefile` at all (`:5916`'s
/// `if self._needs_move(...)` gate, bug #722270) -- the destination's
/// inode, ownership and xattrs survive the rebuild and only its mtime is
/// refreshed, which is exactly why the L3 ncurses case keeps `1:1`:
/// the stage3 image's `curses.h`/terminfo are byte-identical to the
/// rebuilt ones, so real leaves them alone while a fresh copy would
/// come out `0:0`.
///
/// Narrowing: real also compares xattrs when `FEATURES=xattr`
/// (`_cmpxattr`, `PORTAGE_XATTR_EXCLUDE`-aware); portuale's merge copies
/// no xattrs, so xattrs are treated as equal (the ordinary no-xattr
/// case). Real's `filecmp.cmp(shallow=False)` is a byte compare after
/// the mode check; [`files_equal`] does the same in fixed chunks.
fn needs_move(src: &Path, dest: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Ok(src_meta) = std::fs::symlink_metadata(src) else {
        return true;
    };
    let Ok(dest_meta) = std::fs::symlink_metadata(dest) else {
        return true;
    };
    if !dest_meta.is_file() || src_meta.permissions().mode() != dest_meta.permissions().mode() {
        return true;
    }
    !files_equal(src, dest).unwrap_or(false)
}

/// Byte-for-byte comparison for [`needs_move`] (real
/// `filecmp.cmp(shallow=False)`, which also short-circuits on a size
/// mismatch first). Chunked so a large distfile-scale object never
/// materializes in memory twice.
fn files_equal(a: &Path, b: &Path) -> std::io::Result<bool> {
    let mut fa = std::fs::File::open(a)?;
    let mut fb = std::fs::File::open(b)?;
    if fa.metadata()?.len() != fb.metadata()?.len() {
        return Ok(false);
    }
    let mut ba = [0u8; 64 * 1024];
    let mut bb = [0u8; 64 * 1024];
    loop {
        let na = read_filling(&mut fa, &mut ba)?;
        let nb = read_filling(&mut fb, &mut bb)?;
        if na != nb || ba[..na] != bb[..nb] {
            return Ok(false);
        }
        if na == 0 {
            return Ok(true);
        }
    }
}

/// `read` until `buf` is full or EOF (a plain `Read::read` may return a
/// short count for regular files too, which would otherwise misalign the
/// chunk comparison).
fn read_filling(reader: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    use std::io::Read as _;
    let mut filled = 0;
    while filled < buf.len() {
        let n = reader.read(&mut buf[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}

/// Real `dblink.treewalk()`'s own `self.dbdir = self.dbtmpdir;
/// self.delete(); ensure_dirs(self.dbtmpdir)` step (`vartree.py`, right
/// after the collision-protect abort gate and before `pkg_preinst` runs,
/// long before a single file is copied to `${ROOT}`): wipe a stale
/// `MERGING_IDENTIFIER`-prefixed temporary sibling left by a killed
/// previous merge of this exact `category/pf`, then (re)create it empty.
/// Both merge paths call this before `pkg_preinst`, so a SIGKILL any time
/// from here on leaves `-MERGING-<pf>` behind exactly like real (the
/// `l32` C4 killed-mid-merge invariant) -- and every portuale vdb reader
/// skips such names (see [`portage_util::is_merging_vdb_entry`]).
///
/// feat#157 S1.4: the entry is written through the root's
/// [`portage_vdb::WriteTxn`] (`begin_entry`; on `files` the same
/// `stat` / `remove_dir_all` / `create_dir_all` as before). Each of the
/// merge's VDB steps opens and commits its own transaction: the pending
/// entry outlives a transaction (crate doc item 3), and on `files` every
/// call is applied at once, so the write order is the call order below.
fn create_vdb_tmp(root: &Path, category: &str, pf: &str) -> Result<(), String> {
    let key = portage_vdb::EntryKey::new(category, pf);
    let db = portage_vdb::for_root(root);
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    txn.begin_entry(&key).map_err(|e| e.to_string())?;
    txn.commit().map_err(|e| e.to_string())
}

/// Real `dblink.treewalk()`'s own info-file + `COUNTER` step (`vartree.py`,
/// after `pkg_preinst`, before `_merge_contents` copies a single file to
/// `${ROOT}`): copy *every* regular file directly under `build_info_dir`
/// (`${PORTAGE_BUILDDIR}/build-info`) into the temporary vdb entry real's
/// `for x in os.listdir(inforoot): self.copyfile(...)` copies -- the
/// `CATEGORY`/`SLOT`/`KEYWORDS`/`IUSE`/`USE`/`EAPI`/`DEFINED_PHASES`/…
/// `bin/phase-functions.sh __dyn_install` writes, the
/// `DEPEND`/`RDEPEND`/`LICENSE`/… `ebuild_phases::write_post_install_
/// metadata` adds, `NEEDED.ELF.2` from the real `scanelf` QA step,
/// `environment.bz2`, the `<PF>.ebuild` copy -- then the merge-generated
/// files that were never in `build-info`: `CATEGORY`/`SLOT`/`repository`
/// re-asserted explicitly (a standalone `ebuild <file> install` outside a
/// repo checkout has no `build-info/repository`, and `SLOT` here is the
/// caller-resolved full slot) and `COUNTER` (real `cpv_counter`,
/// portuale's own `next_counter`, ticked here so the replace loop below
/// records the new value from the temporary entry). The directory itself
/// must already exist (see [`create_vdb_tmp`]).
///
/// The counter is `WriteTxn::next_counter` (on `files` real's
/// `counter_tick_core` since #306: under the VDB lock, the max of the
/// `counter` file and every installed package's own `COUNTER`, plus one,
/// written atomically).
fn populate_vdb_tmp(
    root: &Path,
    category: &str,
    pf: &str,
    build_info_dir: &Path,
    slot: &str,
    repository: &str,
) -> Result<(), String> {
    let key = portage_vdb::EntryKey::new(category, pf);
    let db = portage_vdb::for_root(root);
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    if let Ok(entries) = portage_util::read_dir_entries(build_info_dir) {
        for entry in entries {
            let src = entry.path();
            if src.is_file()
                && let Some(name) = src.file_name()
            {
                // Entry file names are UTF-8 in the interface; every
                // build-info name is a fixed ASCII key or `<PF>.ebuild`.
                let name = name
                    .to_str()
                    .ok_or_else(|| format!("{}: file name is not UTF-8", src.display()))?;
                txn.copy_entry_file(&key, name, &src)
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    let counter = txn.next_counter().map_err(|e| e.to_string())?;
    for (name, value) in [
        ("CATEGORY", category),
        ("SLOT", slot),
        ("repository", repository),
    ] {
        txn.put_entry_file(&key, name, format!("{value}\n").as_bytes())
            .map_err(|e| e.to_string())?;
    }
    txn.put_entry_file(&key, "COUNTER", counter.to_string().as_bytes())
        .map_err(|e| e.to_string())?;
    txn.commit().map_err(|e| e.to_string())
}

/// Real `dblink.treewalk()`'s own `CONTENTS` + metadata-consolidation tail
/// (`vartree.py`: `CONTENTS` is written into `dbtmpdir` by
/// `_merge_contents` while the image is copied to `${ROOT}`, and
/// `_consolidate_to_metadata_file(self.dbtmpdir)` is the last write into
/// `dbtmpdir` before the rename below): record the merge-generated file
/// list, then fold every per-field file into the consolidated `metadata`
/// file. Runs after the file copy, before the old instances are unmerged
/// (neither touches the temporary directory, so real's
/// consolidate-after-unmerge and this consolidate-before-unmerge are the
/// same bytes); the rename into place still waits for the replace loop
/// (see [`publish_vdb_tmp`]).
fn write_vdb_tmp_contents(
    root: &Path,
    category: &str,
    pf: &str,
    contents: &str,
) -> Result<(), String> {
    let key = portage_vdb::EntryKey::new(category, pf);
    let db = portage_vdb::for_root(root);
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    // Real `_consolidate_to_metadata_file(self.dbtmpdir)` (`vartree.py`):
    // the last write into the temp vdb dir before the rename -- fold
    // every per-field file real's `_in_metadata_file()` accepts into one
    // `metadata` file. Its `#dir_mtime=` trailer records the dir's
    // `st_mtime_ns` and real's reader rejects the file if the dir changed
    // afterwards, so it must come after CONTENTS above and the
    // `#dir_mtime=` line must be *appended* (a plain write, no new dir
    // entry) after the body is on disk. The rename below does not touch
    // the dir's own mtime, so the value survives the move.
    // `seal_entry` is that consolidation (moved into `portage-vdb`,
    // S1.4): body, `stat`, `#dir_mtime=` appended last.
    txn.put_entry_file(&key, "CONTENTS", contents.as_bytes())
        .map_err(|e| e.to_string())?;
    txn.seal_entry(&key).map_err(|e| e.to_string())?;
    txn.commit().map_err(|e| e.to_string())
}

/// Real `dblink.treewalk()`'s own move into place (`vartree.py`: after
/// the replace loop unmerged every same-slot version,
/// `self.dbdir = self.dbpkgdir; self.delete(); _movefile(self.dbtmpdir,
/// self.dbpkgdir)`): drop the old live entry, then atomically rename the
/// temporary sibling into place. Both sit under the same `<category>`
/// directory, hence guaranteed the same filesystem, so `std::fs::rename`
/// alone is already atomic here -- the same guarantee real `_movefile()`
/// relies on for a same-device move. A crash before this leaves the stale
/// `MERGING_IDENTIFIER` leftover (readers skip it; the next merge's
/// [`create_vdb_tmp`] wipes it) -- never a half-written *final* entry,
/// except real's own delete-then-move exposure on a same-pf reinstall,
/// which this shares exactly.
///
/// `WriteTxn::finish_entry` (S1.4; on `files` the same `stat`,
/// `remove_dir_all` and `rename` as before).
fn publish_vdb_tmp(root: &Path, category: &str, pf: &str) -> Result<(), String> {
    let key = portage_vdb::EntryKey::new(category, pf);
    let db = portage_vdb::for_root(root);
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    txn.finish_entry(&key).map_err(|e| e.to_string())?;
    txn.commit().map_err(|e| e.to_string())
}

/// The merge's publish step: the new entry goes live, the replaced
/// same-slot entries go away, and the merge's preserved-libs registration
/// (real `vartree.py:5266-5272`) is stored.
///
/// - `files` ([`portage_vdb::InstalledDb::replace_in_publish`] is
///   `false`): exactly the S1.4 calls, [`publish_vdb_tmp`] then
///   [`register_merge_preserved_libs`]. The replace loop already deleted
///   each entry of `replaced` (`unmerge_one_installed`), so it is unused.
/// - database backends: **one transaction** (design §9, feat#157 S4.1):
///   [`portage_vdb::WriteTxn::finish_entry_replacing`] deletes the rows of
///   `replaced` and publishes the pending row (which also raises the
///   counter high-water mark to its `COUNTER`), and the registry update
///   rides along through `set_preserved_libs`. The counter it records is
///   read from the pending entry, the bytes `populate_vdb_tmp` stored, the
///   same value the published entry carries. Until this commit every
///   reader (`has_version` included) still sees the old instances; a
///   crash before it leaves them installed and the new row `merging`.
#[allow(clippy::too_many_arguments)]
fn publish_merged_entry(
    root: &Path,
    category: &str,
    pn: &str,
    pf: &str,
    main_slot: &str,
    preserve_paths: &BTreeSet<String>,
    replaced: &[String],
) -> Result<(), String> {
    let db = portage_vdb::for_root(root);
    if !db.replace_in_publish() {
        publish_vdb_tmp(root, category, pf)?;
        return register_merge_preserved_libs(root, category, pn, pf, main_slot, preserve_paths);
    }
    let key = portage_vdb::EntryKey::new(category, pf);
    let libs = if preserve_paths.is_empty() {
        None
    } else {
        let counter = db
            .read_pending_file(&key, "COUNTER")
            .ok()
            .flatten()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or_else(|| "0".to_string());
        plib_store(&merge_plib_registration(
            root,
            category,
            pn,
            pf,
            main_slot,
            preserve_paths,
            &counter,
        ))
    };
    let old: Vec<portage_vdb::EntryKey> = replaced
        .iter()
        .map(|old_pf| portage_vdb::EntryKey::new(category, old_pf.as_str()))
        .collect();
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    txn.finish_entry_replacing(&key, &old)
        .map_err(|e| e.to_string())?;
    if let Some(libs) = &libs {
        txn.set_preserved_libs(libs).map_err(|e| e.to_string())?;
    }
    #[cfg(test)]
    tests::publish_hook(root);
    txn.commit().map_err(|e| e.to_string())
}

/// Where `pkg_postinst`'s `PORTAGE_UPDATE_ENV` points (real
/// `vartree.py:5334-5337`): the live entry's own `environment.bz2`.
/// `files`: `<entry>/environment.bz2`, rewritten in place by bash, and
/// `scratch` is `None`. A database backend has no entry directory, so the
/// stored file is copied into `scratch_dir` and the phase rewrites that
/// copy; [`absorb_update_env`] stores it back (feat#157 S4.1, N9).
struct UpdateEnvTarget {
    path: PathBuf,
    scratch: Option<PathBuf>,
}

fn update_env_target(
    root: &Path,
    category: &str,
    pf: &str,
    scratch_dir: &Path,
) -> Result<UpdateEnvTarget, String> {
    let db = portage_vdb::for_root(root);
    let key = portage_vdb::EntryKey::new(category, pf);
    if let Some(entry) = db.entry_path(&key) {
        return Ok(UpdateEnvTarget {
            path: entry.join("environment.bz2"),
            scratch: None,
        });
    }
    portage_vdb::materialize_files(db.as_ref(), &key, &["environment.bz2"], scratch_dir)
        .map_err(|e| e.to_string())?;
    Ok(UpdateEnvTarget {
        path: scratch_dir.join("environment.bz2"),
        scratch: Some(scratch_dir.to_path_buf()),
    })
}

/// After `pkg_postinst`: on a database backend, store the rewritten
/// scratch `environment.bz2` into the live entry (one `replace_file`
/// commit, only when the bytes changed). Real rewrites the file after the
/// entry is published too, so this is a separate commit there as well.
/// No-op on `files`.
fn absorb_update_env(
    root: &Path,
    category: &str,
    pf: &str,
    target: &UpdateEnvTarget,
) -> Result<(), String> {
    let Some(dir) = &target.scratch else {
        return Ok(());
    };
    let db = portage_vdb::for_root(root);
    portage_vdb::absorb_file(
        db.as_ref(),
        &portage_vdb::EntryKey::new(category, pf),
        dir,
        "environment.bz2",
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// Real `dblink.treewalk()`'s `preinst_mask` + `install_mask_dir` step
/// (`vartree.py:4581-4610`, run before `_collision_protect` and before a
/// single file is copied to `${ROOT}`): apply `options.install_mask` to
/// the staged image `d`, deleting every matched path, then -- when a
/// `no{man,info,doc}` `FEATURE` folded a `/usr/share/*` entry in --
/// `rmdir` a now-empty `<d>/usr/share`. The resolved mask is also
/// written to `<build_info>/INSTALL_MASK` (real `preinst_mask` writes it
/// there; `populate_vdb_tmp` then copies it into the vdb
/// wholesale like every other build-info file). A no-op with an empty
/// mask -- and then no `INSTALL_MASK` file, matching real's own
/// `[[ -n ${x} ]] && echo … > INSTALL_MASK`.
///
/// Shared with the remote-merge server (`remote_bundle::build_bundle`,
/// backlog #170): the bundle is staged from the same extraction, so the
/// mask must prune the same image before `collect_filemeta` runs.
pub(crate) fn apply_install_mask(
    d: &Path,
    build_info: &Path,
    options: &MergeOptions,
) -> Result<(), String> {
    let value = options.install_mask.trim();
    if value.is_empty() {
        return Ok(());
    }
    if build_info.is_dir() {
        std::fs::write(build_info.join("INSTALL_MASK"), format!("{value}\n"))
            .map_err(|e| format!("{}: {e}", build_info.join("INSTALL_MASK").display()))?;
    }
    let mask = crate::install_mask::InstallMask::new(value);
    if mask.is_empty() {
        return Ok(());
    }
    crate::install_mask::install_mask_dir(d, &mask)
        .map_err(|e| format!("{}: install_mask_dir: {e}", d.display()))?;
    if options.install_mask_prunes_usr_share {
        let _ = std::fs::remove_dir(d.join("usr/share"));
    }
    Ok(())
}

/// The installed `SLOT` (main slot only) of
/// `<root>/var/db/pkg/<category>/<package>-<version>`, read through the
/// shared [`portage_repo::vdb_entry_slot`] seam every other
/// installed-metadata consumer uses: a valid consolidated `metadata`
/// snapshot serves the field with no per-entry `open()`, and a
/// multi-line value collapses to one space-separated line the way real
/// `_aux_get` normalises (`" ".join(myd.split())`,
/// `vartree.py:975-1053`). A missing or empty `SLOT` reads as
/// `Some("0")`, matching real `aux_get`'s invalid-`SLOT` → `"0"`
/// translation (`:967-972`; O5 ruling 2026-09-23, which folds #115's
/// empty-`SLOT` half into this slice — #115 S1 extends the same
/// translation to present-but-invalid values upstream in
/// `portage_repo::vdb_aux_get`, so they arrive here already `"0"`; the
/// `EAPI` and `_mtime_` thirds stay #115's documented cuts). The return stays
/// `Option<String>` so the five callers' `None` arms keep compiling
/// untouched, but it is now always `Some`: every caller only queries
/// listed installed versions, so "no such entry" cannot reach here in
/// practice (and real `aux_get` raises `KeyError` there — portuale has
/// no such signal, per `vdb_aux_get`'s own doc).
pub(crate) fn read_installed_slot(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> Option<String> {
    let pf = format!("{package}-{version}");
    let raw = portage_repo::vdb_entry_slot(root, category, &pf);
    let main = raw.split('/').next().unwrap_or("");
    if main.is_empty() {
        Some("0".to_string())
    } else {
        Some(main.to_string())
    }
}

/// Real `self._installed_instance` selection (`vartree.py:4409-4418`):
/// among every other real, currently-installed version of this exact
/// `category/package/slot`, the one with the highest real `COUNTER`
/// (real `cpv_counter`, portuale's own real per-package `COUNTER` file
/// -- see `WriteTxn::next_counter`, called by `populate_vdb_tmp`) -- `None` when none exist (a
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
            let counter: i64 = read_entry_text(root, category, &pf, "COUNTER")?
                .trim()
                .parse()
                .ok()?;
            Some((pf, counter))
        })
        .max_by_key(|(_, counter)| *counter)
        .map(|(pf, _)| pf)
}

/// Real `dblink.treewalk`'s own `REPLACING_VERSIONS` set
/// (`vartree.py:4768-4771`) just before `pkg_preinst`: the versions of
/// the installed same-slot instances this merge replaces. Portuale's
/// per-merge analogue is the single `installed_instance_pf` it already
/// selects (the highest-`COUNTER` same-slot instance -- real
/// `others_in_slot`'s own selection); its version is what an ebuild's
/// `ver_replacing` gate (real for EAPI >= 9, the `eapi9-ver` eclass for
/// EAPI < 9) sees. Real's general `doebuild_environment` setter
/// (`doebuild.py:1317-1322`) builds the same value for
/// `preinst`/`postinst`/`pretend`/`setup` from
/// `vardb.match(cpv_slot) + vardb.match("=" + cpv)`. `None` for a
/// first-ever install (no other same-slot instance), matching real's
/// empty string.
fn replacing_versions(pn: &str, installed_instance_pf: Option<&str>) -> Option<String> {
    let version = installed_instance_pf?.strip_prefix(pn)?.strip_prefix('-')?;
    (!version.is_empty()).then(|| version.to_string())
}

/// Whether the installed package at `<root>/var/db/pkg/<category>/
/// <package>-<version>` already claims `abs_path` in its own real
/// `CONTENTS` (the path field of its recognized `obj`/`sym`/`dir`/
/// `dev`/`fif`/`bin` lines, the same set `format_contents_line` writes),
/// read through the `mrg_director::PackagesDb` seam
/// ([`mrg_director::VdbReader`] over `root`) rather than the file
/// directly. `abs_path` is a logical absolute path (what
/// `find_collisions` builds); the seam's `contents_files` returns the
/// same paths with the vdb record's one leading `/` stripped (its
/// documented shape), so the leading `/` is removed here to meet it --
/// the exact inverse of that strip on every entry
/// `format_contents_line` writes.
fn owns_path(root: &Path, category: &str, package: &str, version: &str, abs_path: &str) -> bool {
    let relative = abs_path.strip_prefix('/').unwrap_or(abs_path);
    mrg_director::VdbReader::new(root)
        .contents_files(category, package, version)
        .iter()
        .any(|path| path == relative)
}

/// The `CONTENTS` text of the vdb entry `category/pf`: the live entry
/// first, falling back to its `-MERGING-<pf>` temporary when the live
/// one is absent. The fallback is real `dblink.getcontents()` resolving
/// through `dbdir`, which *is* `dbtmpdir` for the package being merged
/// while the replace loop unmerges the instances it replaces (real
/// `treewalk()`: the new entry is renamed into place only after) -- so
/// an `others_in_slot`/`also_keep` ownership check against the replacing
/// version sees the just-written file list. Enumeration skips are
/// unaffected (those never name a `pf` explicitly; see
/// [`portage_util::is_merging_vdb_entry`]): outside a merge the live
/// entry always exists when these are queried, so the fallback only ever
/// fires mid-merge.
fn read_contents_pf(root: &Path, category: &str, pf: &str) -> Option<String> {
    if let Some(text) = read_entry_text(root, category, pf, "CONTENTS") {
        return Some(text);
    }
    portage_vdb::for_root(root)
        .read_pending_file(&portage_vdb::EntryKey::new(category, pf), "CONTENTS")
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

/// `entry/<name>.is_file()` of old: one `stat` (following symlinks)
/// through the root's [`portage_vdb::InstalledDb::file_meta`]; a missing
/// entry or file, or any error, is `false`.
pub(crate) fn entry_file_is_regular(root: &Path, category: &str, pf: &str, name: &str) -> bool {
    portage_vdb::for_root(root)
        .file_meta(&portage_vdb::EntryKey::new(category, pf), name)
        .ok()
        .flatten()
        .is_some_and(|meta| meta.mode & 0o170000 == 0o100000)
}

/// One file of the live entry `category/pf` as UTF-8 text, through the
/// root's [`portage_vdb::InstalledDb::read_file`] (`files`: one `open`
/// of `var/db/pkg/<category>/<pf>/<name>`). `None` when the entry or the
/// file is missing, unreadable or not UTF-8 -- every case today's
/// `read_to_string(..).ok()` callers treated alike.
pub(crate) fn read_entry_text(root: &Path, category: &str, pf: &str, name: &str) -> Option<String> {
    portage_vdb::for_root(root)
        .read_file(&portage_vdb::EntryKey::new(category, pf), name)
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

/// Same real `CONTENTS`-ownership check as `owns_path`, but keyed by a
/// bare `category`/`pf` pair (`"package-version"`, real portage's own
/// vdb directory-name convention) rather than a split `package`/
/// `version` -- what `blocked_installed_packages` below already has on
/// hand, since it discovers installed packages by scanning real vdb
/// directory names directly rather than through `installed_versions`'s
/// own `package`-scoped lookup.
///
/// Deliberately stays a direct `CONTENTS` read rather than routing
/// through `PackagesDb`: the trait keys `contents_files` by a split
/// `(package, version)` and has no `pf`-keyed query, and both callers
/// (the blocker set above, `ebuild_unmerge`'s same-slot orphan check)
/// hold only the bare vdb directory name -- going through the seam would
/// split the `pf` apart only for `VdbReader` to join it back together.
pub(crate) fn owns_path_pf(root: &Path, category: &str, pf: &str, abs_path: &str) -> bool {
    let Some(text) = read_contents_pf(root, category, pf) else {
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
    let text = read_contents_pf(root, category, pf)?;
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
    let text = read_contents_pf(root, category, pf)?;
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
/// Portuale's own `ebuild <file> merge` has no depgraph at all (a
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
/// verified behavior already relied on elsewhere in portuale --
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
/// `emerge --pretend`, portuale's own real-execution CLI has never
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
            portage_repo::repo_aux_metadata(&repo_root, &env.category, &env.split.pf).ok()?;

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
            root,
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
            &config,
            iuse,
            &keywords,
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
        // Category by category, like the old walk: list the category,
        // then read each entry's `SLOT`, then the next category.
        let db = portage_vdb::for_root(root);
        let installed: Vec<(String, String, String)> = db
            .categories()
            .ok()?
            .into_iter()
            .flat_map(|category_name| {
                db.category_entries(&category_name)
                    .into_iter()
                    .flatten()
                    .map(move |pf| {
                        // #116: through the vdb seam, so this scan sees the
                        // same normalised `SLOT` every other consumer does
                        // (a missing field is `""` -> `("", "")` here).
                        let slot = portage_repo::vdb_entry_slot(root, &category_name, &pf);
                        let (slot, sub_slot) = slot
                            .split_once('/')
                            .map(|(s, ss)| (s.to_string(), ss.to_string()))
                            .unwrap_or_else(|| (slot.clone(), slot.clone()));
                        let candidate_str = format!("{category_name}/{pf}:{slot}/{sub_slot}");
                        (category_name.clone(), pf, candidate_str)
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
/// unconditionally, regardless of `FEATURES`, but only for the symlinks
/// real actually checks -- file-target/dangling ones; a symlink whose
/// target is a directory is in real `os.walk`'s `dirs`, never in
/// `file_list`/`symlink_list`, so it is skipped here, like real) plus real `FEATURES=
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
        let children: Vec<PathBuf> = portage_util::read_dir_entries(&src_dir)
            .map_err(|e| format!("{}: {e}", src_dir.display()))?
            .into_iter()
            .map(|e| relative_dir.join(e.file_name()))
            .collect();

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

            // Real `os.walk(srcroot)` (which builds real `filelist`/
            // `linklist`, `vartree.py:4625-4681`) puts a symlink that
            // points to a directory into the `dirs` list, not `files` --
            // so it never reaches `filelist`/`linklist` and is never
            // collision-checked at all (verified: `link_to_dir` lands in
            // `dirs`, `link_to_file`/dangling land in `files`). A
            // directory-target symlink colliding with a real directory
            // (e.g. linux-firmware's `nvidia/ad10x` symlinks over
            // installed directories) is therefore ignored here, matching
            // real: no `symlink_collisions` entry, no abort. Only
            // file-target/dangling symlinks are checked below. `metadata`
            // follows the link exactly like `os.walk`'s own `is_dir()`
            // does (image-relative for relative targets); a dangling
            // link errors here and falls through to the check, like real.
            if file_type.is_symlink()
                && std::fs::metadata(&src).map(|m| m.is_dir()).unwrap_or(false)
            {
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
/// persistent reverse index for this; portuale just scans fresh every
/// time, acceptable for a real, but not performance-critical, error-
/// reporting path only reached when a merge is about to abort anyway)
/// and returns the `category/pf` -> claimed-paths map for whichever
/// ones actually claim it.
///
/// The scan is [`portage_vdb::InstalledDb::owners`] (S1.5: the old
/// two-level walk and per-entry `CONTENTS` read moved into `FilesDb`
/// unchanged). Two things the old loop did stay here because they live
/// above `portage-vdb`: an entry whose directory name does not split as
/// `<package>-<version>` (`portage_repo::split_pf`) claims nothing (the
/// old walk skipped it before reading its `CONTENTS`; it is now read and
/// its claims dropped), and the result is regrouped as `category/pf` ->
/// the matched collision strings (the same absolute strings the old
/// direct read pushed). `owns_path` keeps reading through the
/// `mrg_director::PackagesDb` seam.
fn find_owners(root: &Path, collisions: &[String]) -> BTreeMap<String, Vec<String>> {
    let mut owners: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let paths: Vec<&[u8]> = collisions.iter().map(|c| c.as_bytes()).collect();
    let Ok(claims) = portage_vdb::for_root(root).owners(&paths) else {
        return owners;
    };
    for (path, key) in claims {
        // The old walk skipped an entry name that is not
        // `<package>-<version>` before reading its `CONTENTS`.
        if portage_repo::split_pf(&key.pf).is_none() {
            continue;
        }
        owners
            .entry(format!("{}/{}", key.category, key.pf))
            .or_default()
            .push(String::from_utf8_lossy(&path).into_owned());
    }
    owners
}

/// Real "package NOT merged due to file collisions" abort message,
/// narrowed to what portuale can cheaply compute (see this module's
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

/// `token` in the merge's own `FEATURES`: the resolved incremental list
/// (`MergeOptions::features`, the `emerge` paths, #37 S2) when set, else
/// the raw `$FEATURES` process env (the standalone `ebuild <file>`
/// fallback `MergeOptions::from_env` documents). Mirrors
/// `emerge_build::resolved_features`' precedence; kept private to the
/// merge path so its `noclean` gate and the phase env can never disagree.
pub(crate) fn feature_enabled(options: &MergeOptions, token: &str) -> bool {
    let env_features;
    let features: &str = if options.features.is_empty() {
        env_features = std::env::var("FEATURES").unwrap_or_default();
        &env_features
    } else {
        &options.features
    };
    features.split_whitespace().any(|t| t == token)
}

/// Hand this package's `${T}/logging/<phase>` files to every configured
/// elog module -- real `dblink.merge()`'s own `self._elog_process()`
/// call, which sits between the success/die hooks and the post-merge
/// `clean` in `dbapi/vartree.py:6160-6198`. Running it there (rather
/// than in a batch after the whole `emerge` returns) is what keeps the
/// messages alive now that backlog #42's clean removes `${T}`: real
/// `elog_process` is per-package, before the clean, and so is this.
///
/// `run_merge`/`run_qmerge` (through `merge_after_install`) and
/// `merge_binpkg` (a binary merge's hooks) each call it once on their
/// success path; `--buildpkgonly` keeps its pre-existing "no elog"
/// behavior (real processes it in `_buildpkgonly_success_hook_exit`,
/// portuale does not yet).
pub(crate) fn process_merge_elog(
    env: &ebuild_phases::Environment,
    root: &Path,
    options: &MergeOptions,
) {
    let cpv = format!("{}/{}", env.category, env.split.pf);
    crate::elog::process_batch(
        &crate::elog::logdir(root),
        &root.display().to_string(),
        &[(cpv, env.t())],
        None,
        &crate::color::Colorizer::new(crate::color::resolve_havecolor(None)),
        // The merge's resolved list when one was set (#37 S2); the
        // standalone `ebuild <file> merge` fallback is the raw env.
        feature_enabled(options, "split-elog"),
    );
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
    run_merge_with_hook(ebuild_path, root, portage_tmpdir, options, buildpkg, None)
}

/// [`run_merge`], with a hook real's `Scheduler._build_exit` fires
/// between the two halves: real wraps the finished `EbuildBuild` in a
/// `PackageMerge` (`Scheduler.py:1615-1621`), whose `_start` prints
/// `Installing (cur of max)` (`PackageMerge.py:32-54`) before
/// `create_install_task()` starts the `EbuildMerge` vdb merge -- i.e.
/// after the build phases (and the `EbuildBinpkg` packaging) succeed,
/// before the vdb merge begins. A failed build never reaches the hook
/// (real queues no merge for it), so callers that print the
/// `Installing` line from the hook also regain that gating for free.
/// `None` runs the fused build+merge exactly like [`run_merge`].
pub fn run_merge_with_hook(
    ebuild_path: &Path,
    root: &Path,
    portage_tmpdir: &Path,
    options: &MergeOptions,
    buildpkg: Option<&crate::ebuild_package::PackageOptions>,
    before_merge: Option<&dyn Fn()>,
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
        &options.build_env,
    )?;
    if status != 0 {
        return Ok(status);
    }
    if let Some(package_options) = buildpkg {
        // Real `Package.use.enabled` for this binpkg's own `Packages`
        // index `USE` field -- the same resolved flag set the `install`
        // phase above just ran with (`options.build_env`'s own `USE`
        // entry, if any flags are enabled at all).
        let use_flags = options
            .build_env
            .iter()
            .find(|(k, _)| k == "USE")
            .map(|(_, v)| v.as_str())
            .unwrap_or("");
        // Real per-package `FEATURES` (backlog #130): `options.features`
        // is already this call's fully resolved value -- the caller's
        // own per-entry fold when one matched (`entry_resolved_features`
        // + `MergeOptions::set_resolved_features`), the run-wide value
        // otherwise -- so re-deriving the binpkg-affecting `PackageOptions`
        // fields from it here keeps `binpkg-multi-instance`/
        // `buildpkg-live`/`binpkg-signing` in step with the same
        // `FEATURES` the `install` phase above just saw, instead of the
        // caller's single shared, run-wide `PackageOptions`.
        let mut per_entry_package_options = package_options.clone();
        per_entry_package_options.set_resolved_features(&options.features);
        // Real `EbuildBinpkg._start`'s per-package `BUILD_ID` gate
        // (backlog #147 S1 ruling (i)): `options.features` is
        // already this call's fully resolved value (per-entry fold
        // when one matched, run-wide otherwise), so the export
        // follows it while the layout stays run-wide.
        let per_entry_binpkg_multi_instance = options
            .features
            .split_whitespace()
            .any(|t| t == "binpkg-multi-instance");
        let status = crate::ebuild_package::package_after_install(
            ebuild_path,
            root,
            portage_tmpdir,
            &per_entry_package_options,
            use_flags,
            per_entry_binpkg_multi_instance,
        )?;
        if status != 0 {
            return Ok(status);
        }
    }
    let env = ebuild_phases::compute_environment(ebuild_path, portage_tmpdir)?;
    // Real's `PackageMerge._start` point (see `run_merge_with_hook`):
    // the build halves above all succeeded, the vdb merge below has not
    // started -- exactly where real prints `Installing (cur of max)`.
    if let Some(hook) = before_merge {
        hook();
    }
    let merge_status = merge_after_install(ebuild_path, root, portage_tmpdir, &env, options)?;
    // Real `dblink.merge()`'s tail (`dbapi/vartree.py:6183-6198`): after
    // the success hooks and `env_update`, the `clean` phase removes the
    // builddir unless `FEATURES=noclean` (and never after a postinst
    // failure -- bug #704866 -- which is exactly a non-zero
    // `merge_status`). The phase itself honors `keeptemp`/`keepwork`.
    // Backlog #42's post-merge half; the `emerge` scheduler's separate
    // build+merge split gets the same call from
    // `emerge_build::merge_one_built_entry`.
    if merge_status == 0 && !feature_enabled(options, "noclean") {
        ebuild_phases::run_clean(
            ebuild_path,
            root,
            portage_tmpdir,
            &options.build_env,
            options.debug,
            &options.config_root,
            options.shell,
            options.log_file.as_deref(),
        )?;
    }
    Ok(merge_status)
}

/// Real `doebuild()`'s own `mydo == "qmerge"` branch
/// (`lib/portage/package/ebuild/doebuild.py:1562-1591`): skips the
/// `install` phase entirely, assuming a prior real `install` (or `merge`,
/// which runs `install` first) already populated `${D}` -- gated on the
/// same real marker real `doebuild()` itself checks, `${PORTAGE_BUILDDIR}/
/// .installed` (see `Environment::installed_marker`'s own doc comment for
/// why portuale doesn't need to write it itself). Real portage doesn't
/// treat a missing marker as a hard failure (`writemsg(...); return 1`,
/// not a raised exception) -- portuale's own established idiom for
/// surfacing an internal message through `ebuild.rs`'s own `Err` ->
/// `eprintln!("ebuild: {e}")` path still produces the same real exit code
/// (1) either way (see `ebuild.rs`'s own `Ok(_) => ExitCode::from(1)`
/// fallback), so `Err` is used here for consistency with this module's
/// other "not in the expected state" checks (e.g. `run_unmerge`'s own
/// "not installed" case) rather than hand-rolling a second message-
/// printing path.
///
/// **No post-merge `clean` here.** Real `doebuild()`'s `qmerge` branch
/// adds `noclean` to `settings.features` before calling `merge()`
/// (`doebuild.py:1573-1575`: "qmerge is a special phase that implies
/// noclean"), so `dblink.merge()`'s tail skips it. The `emerge`
/// scheduler path also reaches `run_qmerge` (its build and merge are
/// separate tasks) but *does* post-clean like real's `dblink.merge()`;
/// that call lives in `emerge_build::merge_one_built_entry`, right after
/// this returns 0, keeping `ebuild <file> qmerge`'s builddir exactly the
/// way real keeps it (backlog #42).
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
/// Real `configdict["pkg"]["A"]` (`doebuild.py:585-594`): the
/// use-reduced, unique distfile basenames of the ebuild's `SRC_URI`,
/// reduced against the resolved `USE` the hook env carries. Real's
/// `config.environ()` exports it into every phase, so the postinst
/// vdb-env regeneration (see [`merge_after_install`]'s own
/// `PORTAGE_UPDATE_ENV` comment) sees it; the filtered `${T}/environment`
/// does not, which is why the merge path re-supplies it. `None` outside
/// a repo checkout, with no `SRC_URI`, or on an unparsable `SRC_URI` --
/// the same "can't tell, so don't set it" degrade `flat_field_on` uses.
fn source_distfiles(
    env: &ebuild_phases::Environment,
    hook_env: &[(String, String)],
) -> Option<String> {
    let repo_root = crate::ebuild_phases::repo_root_for(&env.pkg_dir)?;
    let metadata =
        portage_repo::repo_aux_metadata(&repo_root, &env.category, &env.split.pf).ok()?;
    // Real sets `A` unconditionally (`" ".join(uri_map)` is `""` for an
    // SRC_URI-less package like `virtual/pkgconfig`), so a missing or
    // empty `SRC_URI` yields `Some("")`, not `None` -- the vdb env must
    // carry `declare -x A=""` the way real's does.
    let src_uri = metadata.get("SRC_URI").map(String::as_str).unwrap_or("");
    if src_uri.trim().is_empty() {
        return Some(String::new());
    }
    let use_value = hook_env
        .iter()
        .rev()
        .find(|(k, _)| k == "USE")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    let use_flags: std::collections::HashSet<String> =
        use_value.split_whitespace().map(String::from).collect();
    let mut a: Vec<String> = Vec::new();
    for entry in
        portage_fetch::flatten_src_uri(src_uri, |negated, flag| use_flags.contains(flag) != negated)
            .ok()?
    {
        if !a.contains(&entry.filename) {
            a.push(entry.filename);
        }
    }
    Some(a.join(" "))
}

fn merge_after_install(
    ebuild_path: &Path,
    root: &Path,
    portage_tmpdir: &Path,
    env: &ebuild_phases::Environment,
    options: &MergeOptions,
) -> Result<i32, String> {
    // Real `dblink.treewalk()` starts with the `instprep` phase, before
    // anything below -- see `ebuild_phases::run_instprep`.
    let instprep_status = ebuild_phases::run_instprep(
        ebuild_path,
        None,
        false,
        root,
        portage_tmpdir,
        &options.build_env,
        options.debug,
        &options.config_root,
        options.shell,
        options.log_file.as_deref(),
    )?;
    if instprep_status != 0 {
        eprintln!("!!! instprep failed");
        return Ok(1);
    }

    // Real `dblink.treewalk()`'s own `self._installed_instance` slot
    // read (`vartree.py:4455-4498`): the **evaluated** slot comes from
    // `build-info/SLOT` (the install phase's `__dyn_install` wrote the
    // live `${SLOT}`, so a computed `SLOT="$(ver_cut 1)"` is already
    // evaluated there -- real `_eqawarn`s a divergence only), not from
    // parsing the raw ebuild text (which would hand back the literal
    // `$(ver_cut 1)` -- the #149 symptom). The full real chain:
    //
    // 1. read `build-info/SLOT`'s first line (empty/whitespace ~= read
    //    error; real tolerates a missing file as empty);
    // 2. empty -> fall back to the resolved `settings["SLOT"]` here
    //    (`options.build_env`'s `SLOT`, the emerge path's resolved
    //    metadata value) and `write_atomic` it back into the inforoot;
    // 3. still empty -> `!!! SLOT is undefined`, abort the merge;
    // 4. source builds only (`is_binpkg` is always false here -- the
    //    binary path reads `meta_get("SLOT")` instead): a *known*
    //    settings value that differs from the inforoot value gets a
    //    `_eqawarn` QA Notice. Guarded on the settings value being
    //    non-empty: real always carries a resolved `settings["SLOT"]`
    //    on a merge (`depend` has run), while portuale's fixture path
    //    can hand `merge_after_install` an empty `build_env` -- against
    //    that state an `Expected SLOT='', got '1'` notice would be noise.
    let info_root_slot = std::fs::read_to_string(env.build_info().join("SLOT"))
        .ok()
        .and_then(|text| text.lines().next().map(str::to_string))
        .map(|line| line.trim().to_string())
        .unwrap_or_default();
    let settings_slot = options
        .build_env
        .iter()
        .find(|(name, _)| name == "SLOT")
        .map(|(_, value)| value.clone())
        .unwrap_or_default();
    let full_slot = if info_root_slot.is_empty() {
        if settings_slot.is_empty() {
            eprintln!("!!! SLOT is undefined");
            return Ok(1);
        }
        let _ = std::fs::write(env.build_info().join("SLOT"), format!("{settings_slot}\n"));
        settings_slot
    } else {
        if !settings_slot.is_empty() && info_root_slot != settings_slot {
            eprintln!("QA Notice: Expected SLOT='{settings_slot}', got '{info_root_slot}'");
        }
        info_root_slot
    };
    let repository = repository_name_for(&env.pkg_dir).unwrap_or_else(|| "__unknown__".to_string());

    // Real `self._installed_instance` (`vartree.py:4409-4418`), computed
    // early -- before the vdb write below ever touches this exact
    // category/pf's own real `CONTENTS` -- see `installed_instance_pf`'s
    // own doc comment.
    // `installed_instance_pf`/`find_collisions`/`blocked_installed_
    // packages` all compare against `read_installed_slot`, which returns
    // the installed vdb `SLOT` file's *main* slot (`"0"` for a real
    // `"0/6"`): the merge must pass the main slot to them, while the vdb
    // write records the ebuild's full `slot/sub_slot`. Passing the full
    // slot here made every sub-slotted package (ncurses, glibc, ...)
    // look like it owned none of its own files, so `-e`/reinstall merges
    // aborted under `FEATURES=protect-owned` (L3 smoke finding).
    let main_slot = full_slot.split('/').next().unwrap_or("0");
    let installed_instance_pf =
        installed_instance_pf(root, &env.category, &env.split.pn, main_slot);
    // Real `dblink.treewalk()` sets `REPLACING_VERSIONS` right before
    // `pkg_preinst` (`vartree.py:4768-4771`) -- and real's general
    // `doebuild_environment` setter runs it for `postinst` too
    // (`doebuild.py:1317-1322`). See `replacing_versions`'s own doc.
    let replacing_versions = replacing_versions(&env.split.pn, installed_instance_pf.as_deref());
    let mut preinst_env = options.build_env.clone();
    if let Some(version) = &replacing_versions {
        preinst_env.push(("REPLACING_VERSIONS".to_string(), version.clone()));
    }

    // Real `dblink.treewalk()`'s `preinst_mask` + `install_mask_dir`
    // step, run before `_collision_protect` and before any file is
    // copied -- a masked path never reaches `${ROOT}` nor the vdb
    // `CONTENTS`. See `apply_install_mask`.
    apply_install_mask(&env.d(), &env.build_info(), options)?;

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
    let blocked =
        blocked_installed_packages(root, &options.config_root, env, main_slot, &repository);
    let (collisions, symlink_collisions, plib_collisions) = find_collisions(
        &env.d(),
        root,
        &env.category,
        &env.split.pn,
        main_slot,
        &options.config_protect,
        &options.config_protect_mask,
        &plib_inodes,
        &blocked,
    )?;
    // Real `dblink.merge()`'s own abort condition (`vartree.py:4830-
    // 4838`, Python operator precedence: `collision_protect or
    // (protect_owned and owners)`): a checked symlink-over-directory
    // violation (file-target/dangling symlink over a real directory;
    // directory-target symlinks never reach `symlink_collisions` --
    // see `find_collisions`)
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

    // Real `dblink.treewalk()`'s own order: the `-MERGING-<pf>` temporary
    // vdb entry is created before `pkg_preinst` runs (wiping a stale one
    // a killed previous merge left), populated after it, and only renamed
    // into place after every replaced same-slot version is unmerged --
    // `pkg_postinst` runs last and sees the live entry. See
    // `create_vdb_tmp`/`populate_vdb_tmp`/`publish_vdb_tmp`.
    // `run_single_phase` (not `run_commands`) since neither hook is part
    // of `install`'s own `actionmap_deps` chain (real `treewalk()` invokes
    // them directly, not through `doebuild()`).
    create_vdb_tmp(root, &env.category, &env.split.pf)?;
    let preinst_status = ebuild_phases::run_single_phase(
        ebuild_path,
        "preinst",
        root,
        portage_tmpdir,
        options.debug,
        &options.config_root,
        options.shell,
        &preinst_env,
        options.log_file.as_deref(),
    )?;
    if preinst_status != 0 {
        return Ok(preinst_status);
    }
    populate_vdb_tmp(
        root,
        &env.category,
        &env.split.pf,
        &env.build_info(),
        &full_slot,
        &repository,
    )?;

    let mut cfgfiledict = read_cfgfiledict(root);
    let mut contents = merge_tree(
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
    // Real `dblink.treewalk`'s own pre-replace-loop preserve-libs
    // block: the replaced same-slot instance's
    // still-needed libraries are selected now -- the new vdb entry sits
    // in its `-MERGING-<pf>` temporary (written by `populate_vdb_tmp`
    // above), so enumeration skips it and it reaches the linkage map
    // only through the explicit include feed inside
    // `find_preserve_paths_for_merge` (real `include_file=needed`) --
    // and their entries are carried
    // into the new package's own `CONTENTS`
    // (`_add_preserve_libs_to_contents`). The
    // record itself lands after the replace loop below
    // (`register_merge_preserved_libs`).
    let new_image_paths: BTreeSet<String> = contents
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1).map(String::from))
        .collect();
    let mut preserve_paths = BTreeSet::new();
    if let Some((paths, old_contents_text)) = find_preserve_paths_for_merge(
        root,
        &env.category,
        &env.split.pn,
        &env.split.pf,
        main_slot,
        &new_image_paths,
    ) {
        let (injected, surviving) =
            inject_preserved_libs_into_contents(&old_contents_text, &paths, &new_image_paths);
        contents.push_str(&injected);
        preserve_paths = surviving;
    }
    write_cfgfiledict(root, &cfgfiledict)?;
    write_vdb_tmp_contents(root, &env.category, &env.split.pf, &contents)?;

    if !plib_collisions.is_empty() {
        let cpv = format!("{}/{}", env.category, env.split.pf);
        unregister_preserved_libs(root, &cpv, plib_registry, &plib_collisions)?;
    }

    // Real `dblink.treewalk()`'s replace loop: unmerge every same-slot
    // version this merge replaced while the new version's vdb entry still
    // sits in its `-MERGING-<pf>` temporary -- see
    // `unmerge_replaced_same_slot`. Real `treewalk()` order: *after* the
    // file copy, *before* the rename into place and `pkg_postinst` /
    // `env_update`. A same-cpv `Reinstall` finds nothing to unmerge (its
    // own live entry is untouched until `publish_vdb_tmp` below),
    // matching the pre-replace-loop behaviour.
    // The merge-side just-preserved set travels with the replace loop
    // (real `preserve_paths` into `dblink.unmerge`) so the post-unmerge
    // prune can see files no `NEEDED.ELF.2` indexes yet -- see
    // `linkage_owner_entries`.
    let replacement_preserved: BTreeMap<String, Vec<String>> = if preserve_paths.is_empty() {
        BTreeMap::new()
    } else {
        BTreeMap::from([(
            format!("{}/{}", env.category, env.split.pf),
            preserve_paths.iter().cloned().collect(),
        )])
    };
    let replaced = unmerge_replaced_same_slot(
        root,
        &env.category,
        &env.split.pn,
        &env.split.pf,
        main_slot,
        &env.portage_builddir().join("unmerge-src"),
        portage_tmpdir,
        options,
        &replacement_preserved,
    )?;

    // Real `dblink.treewalk()`'s own post-replace-loop registration
    // (`vartree.py:5266-5272`) -- backlog #178: the one `cp:slot`
    // record is replaced by the merging package's own `(cpv, counter,
    // paths)`, so a second consecutive soname bump cannot keep a stale
    // path list.
    // After the rename into place, like real (backlog #183: the counter
    // is read back from the published entry). On a database backend the
    // publish, the deletion of the replaced entries and this registration
    // are one commit (see `publish_merged_entry`).
    publish_merged_entry(
        root,
        &env.category,
        &env.split.pn,
        &env.split.pf,
        main_slot,
        &preserve_paths,
        &replaced,
    )?;

    // Real `merge()`'s own ordering: `postinst` runs, but its own exit
    // status never gates anything after it ("It's stupid to bail out
    // here, so keep going regardless of phase return code") -- real
    // `env_update()` always runs next, as long as anything was actually
    // installed (real `if contents:`) or a replaced version was removed.
    //
    // Real `dblink.treewalk()` (`vartree.py:5334-5337`) sets
    // `PORTAGE_UPDATE_ENV=<dbpkgdir>/environment.bz2` before the
    // postinst phase of *every* merge, source included, so
    // `bin/phase-functions.sh:1072-1082` rewrites the vdb environment
    // from the hook's live environment through the filtered save
    // (`__save_ebuild_env --exclude-init-phases | __filter_readonly_variables
    // --filter-path --filter-sandbox --allow-extra-vars`). That rewrite is
    // what drops the install-time `build-info/environment` save's stray
    // globals (`f`, `x`) real never records (#45 P2a finding). The
    // filtered `${T}/environment` drops `A`, so it is re-supplied here
    // exactly as real `doebuild_environment` exports it
    // (`doebuild.py:585-594`: the use-reduced, unique distfile names;
    // real's `config.environ()` gives every phase that value).
    //
    // The path is the live entry's own file (`InstalledDb::entry_path`,
    // feat#157 N9; on `files` `<root>/var/db/pkg/<cat>/<pf>`, so bash
    // rewrites it in place exactly as before). A database backend has
    // no such path: a scratch copy in the build dir, stored back after
    // the phase (`update_env_target` / `absorb_update_env`, S4.1).
    let update_env = update_env_target(
        root,
        &env.category,
        &env.split.pf,
        &env.portage_builddir().join("vdb-update-env"),
    )?;
    let mut postinst_env = options.build_env.clone();
    if let Some(version) = &replacing_versions {
        postinst_env.push(("REPLACING_VERSIONS".to_string(), version.clone()));
    }
    postinst_env.push((
        "PORTAGE_UPDATE_ENV".to_string(),
        update_env.path.display().to_string(),
    ));
    if let Some(a) = source_distfiles(env, &postinst_env) {
        postinst_env.push(("A".to_string(), a));
    }
    let postinst_status = ebuild_phases::run_single_phase(
        ebuild_path,
        "postinst",
        root,
        portage_tmpdir,
        options.debug,
        &options.config_root,
        options.shell,
        &postinst_env,
        options.log_file.as_deref(),
    )?;
    absorb_update_env(root, &env.category, &env.split.pf, &update_env)?;

    if !contents.is_empty() || !replaced.is_empty() {
        env_update::run_env_update(root)?;
        // Real `treewalk()`: "For gcc upgrades, preserved libs have to be
        // removed after the library path has been updated" -- a preserved
        // lib whose last consumer this merge just rebuilt is now orphaned
        // and gets deleted + unregistered (real `_prune_plib_registry()`).
        // No include feed: the new entry is already renamed into place,
        // so its own lines are enumerated (backlog #224).
        prune_unused_preserved_libs(root, false, &|_| false, None, &BTreeMap::new(), &[])?;
    }

    // Real `dblink.merge()`: `self._elog_process()` runs here, after the
    // merge body and before the clean (which every caller runs once this
    // returns). Backlog #42: the post-merge clean removes `${T}`, so
    // this must not be deferred to a batch after the whole `emerge`
    // returns any more -- and real never deferred it either.
    process_merge_elog(env, root, options);

    Ok(postinst_status)
}

/// Real `dblink.treewalk()`'s replace loop (`vartree.py:5187-5219`):
/// while the *new* version's vdb entry still sits in its `-MERGING-<pf>`
/// temporary (it is renamed into place only after this returns), every
/// already-installed **same-slot** version of the same cp is removed --
/// `dblink.unmerge()` (`pkg_prerm` -> delete its files -> `pkg_postrm`)
/// then `dblink.delete()` (drop its vdb entry). Each `pkg_prerm`/
/// `pkg_postrm` runs from *that* version's own vdb-stored
/// `environment.bz2` + `<pf>.ebuild` (`ebuild_phases::
/// run_phase_from_saved_env`, gated on its recorded `DEFINED_PHASES`) --
/// a version merged before portuale kept those files, or via a bare
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
    replacement_preserved: &BTreeMap<String, Vec<String>>,
) -> Result<Vec<String>, String> {
    // Real merge-then-unmerge: `<package>-<digit...>` vdb-dir names, the
    // same shape `blocked_installed_packages` and `installed_instance_pf`
    // match; exclude the just-merged new entry. A `-MERGING-<pf>`
    // temporary can never match the `<package>-<digit...>` shape (real
    // package names never start with `-`), but skip it explicitly anyway:
    // readers never see in-progress entries, not even here.
    let mut replaced: Vec<String> = Vec::new();
    if let Ok(names) = portage_vdb::for_root(root).category_entries(category) {
        for name in names {
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
    // Real `dblink.unmerge(needed=...)` (backlog #224): the replacing
    // package's own `NEEDED.ELF.2` lines travel with the replace loop
    // into `_prune_plib_registry`'s `_linkmap_rebuild(include_file=...)`,
    // because the new entry still sits in its `-MERGING-<pf>` temporary
    // and enumeration skips it. Read once here (the temporary is live
    // for the whole loop) and thread through every replaced instance's
    // own post-unmerge prune below.
    let replacement_needed = replacement_needed_entries(root, category, new_pf);
    // feat#157 S4.1: on a database backend the replaced entries are
    // deleted by the publishing commit (`publish_merged_entry`), not here,
    // so the old instance stays installed until the new one is. A later
    // iteration must still treat the instances already unmerged as gone
    // (`others_in_slot`), as it does on `files` where their directories
    // are removed in this loop.
    let defer_delete = portage_vdb::for_root(root).replace_in_publish();
    for (i, old_pf) in replaced.iter().enumerate() {
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
            true,
            replacement_preserved,
            &replacement_needed,
            defer_delete.then(|| &replaced[..i]),
        )?;
    }

    // Real `dblink.unmerge()` -> `self._elog_process(phasefilter=("prerm",
    // "postrm"))`: the superseded version's `pkg_prerm`/`pkg_postrm`
    // `elog`/`ewarn`/`eerror` output reaches the `echo`/`save`/
    // `save_summary` modules, exactly as `emerge -C` already does through
    // `execute_unmerge`. Each old PF's `${T}` is `unmerge_one_installed`'s
    // own `run_phase_from_saved_env` builddir
    // (`<PORTAGE_TMPDIR>/portage/<cat>/<pf>/temp`), which here only ever
    // holds `prerm`/`postrm` logs. Colour is resolved the same way every
    // non-graph path does (`NO_COLOR` / not-a-tty aware).
    let items: Vec<(String, PathBuf)> = replaced
        .iter()
        .map(|old_pf| {
            (
                format!("{category}/{old_pf}"),
                portage_tmpdir
                    .join("portage")
                    .join(category)
                    .join(old_pf)
                    .join("temp"),
            )
        })
        .collect();
    crate::elog::process_batch(
        &crate::elog::logdir(root),
        &root.display().to_string(),
        &items,
        Some(&["prerm", "postrm"]),
        &crate::color::Colorizer::new(crate::color::resolve_havecolor(None)),
        // The merge path's resolved list when one was set (#37 S3); the
        // standalone `ebuild <file> merge` fallback is the raw env.
        options
            .features
            .split_whitespace()
            .any(|t| t == "split-elog"),
    );

    Ok(replaced)
}

/// Remove ONE already-installed version from the vdb -- real
/// `dblink.unmerge()` (`pkg_prerm` -> delete its files -> `pkg_postrm`)
/// then `dblink.delete()` (drop the vdb dir) -- for a single
/// `<category>/<pf>`. Both phase hooks run from *that version's own*
/// vdb-stored `environment.bz2` + `<pf>.ebuild`
/// (`ebuild_phases::run_phase_from_saved_env`, gated on its recorded
/// `DEFINED_PHASES`); a version installed before portuale kept those
/// files has neither and its rm hooks are skipped (documented degrade).
/// A phase failure is logged, never fatal -- real `treewalk()`'s replace
/// loop is a literal `# TODO: Check status and abort if necessary` that
/// doesn't, and real `emerge -C`'s own loop only aborts on the
/// file-removal core failing, not on a phase.
///
/// `also_keep` is folded into real `others_in_slot` so a path a
/// replacing version now owns is left in place -- empty for a standalone
/// `emerge -C`, `[new_pf]` for `treewalk()`'s replace loop.
/// `is_replacement` is real `unmerge_with_replacement` (see
/// `preserve_libs_on_unmerge`): set by `unmerge_replaced_same_slot`,
/// cleared by `pretend.rs`'s real `emerge -C`.
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
///
/// `replacement_preserved` is real `preserve_paths` (owner -> paths
/// just preserved merge-side): threaded from the merge caller through
/// `unmerge_replaced_same_slot` into the post-unmerge prune (see
/// `ebuild_unmerge::unmerge_pkgfiles`); empty on the standalone path.
/// `replacement_needed` is real `include_file` (the replacing
/// package's own `NEEDED.ELF.2` lines -- see
/// `unmerge_replaced_same_slot`): same threading, same empty-on-
/// standalone rule.
///
/// `deferred_delete` (feat#157 S4.1) is `None` everywhere except the
/// replace loop on a database backend: then the entry is **not** deleted
/// here (the merge's publishing commit deletes it, see
/// `publish_merged_entry`), and the slice names the instances this loop
/// already unmerged, which are still installed in the database but must
/// not count as `others_in_slot` (on `files` their directories are gone
/// by now).
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
    is_replacement: bool,
    replacement_preserved: &BTreeMap<String, Vec<String>>,
    replacement_needed: &[(String, Vec<crate::needed_elf::NeededEntry>)],
    deferred_delete: Option<&[String]>,
) -> Result<(), String> {
    let unmerge_options = crate::ebuild_unmerge::UnmergeOptions {
        debug: options.debug,
        shell: options.shell,
        config_protect: options.config_protect.clone(),
        config_protect_mask: options.config_protect_mask.clone(),
        config_root: options.config_root.clone(),
        already_unmerged: deferred_delete.map(<[String]>::to_vec).unwrap_or_default(),
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
        let defined = read_entry_text(root, category, pf, "DEFINED_PHASES").unwrap_or_default();
        if !entry_file_is_regular(root, category, pf, "environment.bz2")
            || !entry_file_is_regular(root, category, pf, &format!("{pf}.ebuild"))
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
    // feat#157 S4.2: a standalone unmerge on a database backend collects
    // its D4 writes and commits them with the row's deletion (below).
    let mut retire = (deferred_delete.is_none() && !is_replacement)
        .then(|| RetireWrites::for_root(root))
        .flatten();
    crate::ebuild_unmerge::unmerge_pkgfiles_into(
        root,
        category,
        package,
        pf,
        also_keep,
        &unmerge_options,
        is_replacement,
        replacement_preserved,
        replacement_needed,
        retire.as_mut(),
    )?;
    let postrm_status = run_hook("postrm")?;
    if postrm_status != 0 {
        eprintln!("{category}/{pf}: FAILED postrm ({postrm_status}) -- unmerge continues");
    }
    if deferred_delete.is_none() {
        crate::ebuild_unmerge::retire_entry(root, category, pf, retire)?;
    }
    Ok(())
}

/// Run one phase for an already-installed `<category>/<pf>` straight from
/// its own vdb-stored `environment.bz2` + `<pf>.ebuild`, copied into
/// `scratch_dir` as `<cat>/<pn>/<pf>.ebuild` so `compute_environment`'s
/// `<cat>/<pn>/<pf>.ebuild` path parse works (the vdb layout is
/// `<cat>/<pf>/<pf>.ebuild`). Errors if the vdb entry carries no saved
/// environment or ebuild (a package installed before portuale started
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
    // Both files go to bash / a copier by path (N9): the entry's own
    // directory on `files`; on a database backend a scratch copy of the
    // two files under `scratch_dir` (feat#157 S4.1; nothing is stored
    // back, the phase gets no `PORTAGE_UPDATE_ENV`).
    let vdb_dir = crate::ebuild_unmerge::entry_path_for_message(root, category, pf);
    let ebuild_name = format!("{pf}.ebuild");
    if !entry_file_is_regular(root, category, pf, "environment.bz2")
        || !entry_file_is_regular(root, category, pf, &ebuild_name)
    {
        return Err(format!(
            "{}: no saved build environment (installed before portuale kept one?)",
            vdb_dir.display()
        ));
    }
    let db = portage_vdb::for_root(root);
    let key = portage_vdb::EntryKey::new(category, pf);
    let vdb_dir = if db.entry_path(&key).is_some() {
        vdb_dir
    } else {
        let dir = scratch_dir.join("vdb-entry").join(category).join(pf);
        portage_vdb::materialize_files(
            db.as_ref(),
            &key,
            &["environment.bz2", ebuild_name.as_str()],
            &dir,
        )
        .map_err(|e| e.to_string())?;
        dir
    };
    let env = vdb_dir.join("environment.bz2");
    let ebuild = vdb_dir.join(&ebuild_name);
    let src_dir = scratch_dir.join(category).join(package);
    std::fs::create_dir_all(&src_dir).map_err(|e| format!("{}: {e}", src_dir.display()))?;
    let dst = src_dir.join(format!("{pf}.ebuild"));
    std::fs::copy(&ebuild, &dst).map_err(|e| format!("{}: {e}", ebuild.display()))?;
    crate::ebuild_phases::run_phase_from_saved_env(
        &dst,
        &env,
        // Each unmerge seeds its own (old version's) builddir once.
        true,
        phase,
        root,
        portage_tmpdir,
        options.debug,
        &options.config_root,
        options.shell,
        options.log_file.as_deref(),
        // An unmerge's `prerm`/`postrm` never rewrites a vdb env (the
        // entry is on its way out).
        None,
        None,
        // Real does not give an unmerge hook `REPLACING_VERSIONS`.
        None,
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
///     `<pf>.ebuild` -- older, or built before portuale kept them --
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
    // Real `config.environ()`'s `filter_calling_env` (`config.py:3275-
    // 3310`): once `${T}/environment` exists -- every binary-merge hook
    // has it, seeded from the archive -- the calling environment is
    // narrowed to `environ_whitelist` (bug #189417: a variable the
    // ebuild unsets must not leak back in), so a harness/portuale-only
    // var (`PORTAGE_RUNNING_ROOT`, `L1_SKIP_PORTAGE_UPGRADE`,
    // synthesized `GNUMAKEFLAGS`) never reaches the regenerated vdb
    // env. Programmatic per-phase keys (`EMERGE_FROM`, `MERGE_TYPE`,
    // `PORTAGE_UPDATE_ENV`, the `*_EXCLUDE` reads) are added separately
    // by `run_phase_from_saved_env` and are unaffected. L3 finding: the
    // L2 real-set control leg carries exactly those three vars without
    // this filter.
    let filtered_options = {
        let build_env: Vec<(String, String)> = options
            .build_env
            .iter()
            .filter(|(k, _)| ebuild_phases::environ_whitelisted(k))
            .cloned()
            .collect();
        if build_env.len() == options.build_env.len() {
            None
        } else {
            let mut filtered = options.clone();
            filtered.build_env = build_env;
            Some(filtered)
        }
    };
    let options = filtered_options.as_ref().unwrap_or(options);

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
    // `PF` -> `PN`: real `catpkgsplit` (`portage_dep::parse_candidate`'s
    // own regex, the canonical PMS `<pkg>-<version>[-r<rev>]` split).
    // The earlier hand-rolled `rsplit_once('-')` only recognised a
    // version-final `PF` (`foo-1.2`) or `foo-1.2-r3` where the split
    // happened to land a digit last -- it fell through to `package = pf`
    // for `libgcrypt-1.12.3-r1` (last token `r1`), which then made the
    // extracted-ebuild dir `ebuild-src/<cat>/libgcrypt-1.12.3-r1/` and
    // `split_package` reject `libgcrypt-1.12.3-r1.ebuild` ("filename
    // doesn't start with the parent directory's own name").
    let package = portage_dep::parse_candidate(&format!("{category}/{pf}"))
        .map(|c| c.package)
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
    crate::binpkg::extract_binpkg(binpkg_path, &image, &build_info, &options.gpg_verify)?;

    // Real `_emerge/Binpkg._start_task`: "Store the md5sum in the vdb."
    // It prefers the `MD5` field from the package index, else
    // `perform_md5(pkg_path)`. Neither xpak nor gpkg carries `BINPKGMD5`
    // inside itself -- it's the digest of the *whole* binpkg file, which
    // real records so a later `emerge -k` / index rebuild can tell a
    // still-current instance from a rebuilt one. Written into build-info
    // so `populate_vdb_tmp` copies it into the vdb like every
    // other build-info file.
    let binpkg_md5 = md5_hex(binpkg_path)?;
    std::fs::write(build_info.join("BINPKGMD5"), format!("{binpkg_md5}\n"))
        .map_err(|e| format!("{}: {e}", build_info.join("BINPKGMD5").display()))?;

    // Real `_emerge/Binpkg`: `pkg_setup`/`pkg_preinst`/`pkg_postinst`
    // run from the extracted `<pf>.ebuild` + the `bunzip2`'d
    // `environment.bz2` (see `ebuild_phases::run_phase_from_saved_env`).
    // Gated on `DEFINED_PHASES` (real `_defined_phases`) so a binpkg
    // that defines neither -- the common case -- spawns no shell at all.
    // Both files must be present (an older binpkg, or one built before
    // portuale kept them, gets no hooks -- a documented degrade).
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
    // `always`: run the phase even when the ebuild does not define it --
    // real portage's `postinst` `EbuildPhase` always starts (`pkg_postinst`
    // defined or not) so its post-hook `PORTAGE_UPDATE_ENV` block can run.
    //
    // Real `_emerge/BinpkgEnvExtractor` extracts `${T}/environment` once
    // per package; every hook then evolves that one file. Seed on the
    // first hook that actually runs, `false` afterwards -- a per-hook
    // re-seed would wipe `pkg_setup`'s own variable mutations before
    // `pkg_preinst`/`pkg_postinst` (and the vdb env regeneration) see
    // them (#30 finding `l3-binpkg-hook-env-reseed`).
    let seeded = std::cell::Cell::new(false);
    // Real `self._installed_instance` (`vartree.py:4409-4418`) is computed
    // before `treewalk()`'s `pkg_preinst`, and `REPLACING_VERSIONS` is set
    // from it right before that hook (`:4768-4771`). Computed here so the
    // hook closure below can thread it into `pkg_preinst`/`pkg_postinst`;
    // `merge_tree` reuses the same value.
    let installed_instance = installed_instance_pf(root, &category, &package, &main_slot);
    let replacing_versions = replacing_versions(&package, installed_instance.as_deref());
    let run_hook_ex =
        |phase: &str, always: bool, update_env: Option<&Path>| -> Result<i32, String> {
            match &extracted_ebuild {
                Some(ebuild) if always || phase_defined(phase) => {
                    let seed = !seeded.replace(true);
                    crate::ebuild_phases::run_phase_from_saved_env(
                        ebuild,
                        &saved_env,
                        seed,
                        phase,
                        root,
                        portage_tmpdir,
                        options.debug,
                        &options.config_root,
                        options.shell,
                        options.log_file.as_deref(),
                        update_env,
                        update_env.map(|_| options.features.as_str()),
                        replacing_versions.as_deref(),
                    )
                }
                _ => Ok(0),
            }
        };
    let run_hook = |phase: &str| run_hook_ex(phase, false, None);

    // Real `Scheduler._run_pkg_pretend` runs `pkg_pretend` for every
    // package in the merge list -- a binary package included: only the
    // `SRC_URI` fetch inside it is guarded by `if not x.built`, and the
    // status line even switches colour to `PKG_BINARY_MERGE` for the
    // built case. It skips EAPI 0-3 (where `pkg_pretend` does not
    // exist, so `DEFINED_PHASES` cannot list it anyway -- the
    // `phase_defined` gate in `run_hook` already covers that) and
    // anything not defining the phase. Real runs this as an up-front
    // pass over the whole list; portuale folds it into the per-package
    // flow here, exactly as the source path runs `pretend` inline
    // before each build rather than as a separate scheduler stage.
    let pretend_status = run_hook("pretend")?;
    if pretend_status != 0 {
        return Ok(pretend_status);
    }

    // Real `_emerge/Binpkg` order: `pkg_setup` (an `EbuildPhase`) runs
    // right after the metadata is extracted, before `unpack_contents` /
    // the merge.
    let setup_status = run_hook("setup")?;
    if setup_status != 0 {
        return Ok(setup_status);
    }

    // Real `EbuildMerge` -> `dblink.treewalk()`: the `instprep` phase
    // runs first, on the extracted image, from the binpkg's saved env
    // (see `ebuild_phases::run_instprep`). A binpkg without a saved env
    // gets no phase at all -- the same documented degrade as the hooks.
    if let Some(ebuild) = &extracted_ebuild {
        // `seeded` is false when no hook before this ran (a binpkg that
        // defines neither `pkg_pretend` nor `pkg_setup`): seed here so
        // `__dyn_instprep` still runs from the archive's env; otherwise
        // reuse the env `pkg_setup` evolved (see `run_instprep`'s own
        // `seed` doc comment).
        let seed_instprep = !seeded.replace(true);
        let instprep_status = ebuild_phases::run_instprep(
            ebuild,
            Some(&saved_env),
            seed_instprep,
            root,
            portage_tmpdir,
            &options.build_env,
            options.debug,
            &options.config_root,
            options.shell,
            options.log_file.as_deref(),
        )?;
        if instprep_status != 0 {
            eprintln!("!!! instprep failed");
            return Ok(1);
        }
    }

    // Real `dblink.treewalk()`: `preinst_mask` + `install_mask_dir` run
    // before `_collision_protect` and before any file is copied -- so a
    // masked path (e.g. `/usr/share/info` under `FEATURES=noinfo`) never
    // reaches `${ROOT}` and never shows up in the vdb `CONTENTS`. The
    // resolved `INSTALL_MASK` also lands in `build-info`, so
    // `populate_vdb_tmp` copies it into the vdb entry.
    apply_install_mask(&image, &build_info, options)?;

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

    // Real `dblink.treewalk()` order: the `-MERGING-<pf>` temporary vdb
    // entry is created before `pkg_preinst` runs, populated after it, and
    // only renamed into place after every replaced same-slot version is
    // unmerged -- identical to `merge_after_install`.
    create_vdb_tmp(root, &category, &pf)?;
    let preinst_status = run_hook("preinst")?;
    if preinst_status != 0 {
        return Ok(preinst_status);
    }
    populate_vdb_tmp(root, &category, &pf, &build_info, &full_slot, &repository)?;

    let installed_instance = installed_instance_pf(root, &category, &package, &main_slot);
    let mut cfgfiledict = read_cfgfiledict(root);
    let mut contents = merge_tree(
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
    // Real `dblink.treewalk`'s own pre-replace-loop preserve-libs
    // block (identical to `merge_after_install`): the replaced same-slot instance's
    // still-needed libraries are carried into this package's own
    // `CONTENTS`; the record itself lands after the replace loop below.
    let new_image_paths: BTreeSet<String> = contents
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1).map(String::from))
        .collect();
    let mut preserve_paths = BTreeSet::new();
    if let Some((paths, old_contents_text)) =
        find_preserve_paths_for_merge(root, &category, &package, &pf, &main_slot, &new_image_paths)
    {
        let (injected, surviving) =
            inject_preserved_libs_into_contents(&old_contents_text, &paths, &new_image_paths);
        contents.push_str(&injected);
        preserve_paths = surviving;
    }
    write_cfgfiledict(root, &cfgfiledict)?;
    write_vdb_tmp_contents(root, &category, &pf, &contents)?;

    // Real `treewalk()`: a preserved lib this new version now provides
    // itself is taken over from the `preserved_libs_registry` and
    // stripped from the previous owner's `CONTENTS` -- identical to
    // `merge_after_install`.
    if !plib_collisions.is_empty() {
        let cpv = format!("{category}/{pf}");
        unregister_preserved_libs(root, &cpv, plib_registry, &plib_collisions)?;
    }

    // Real merge-then-unmerge: drop every same-slot version the new one
    // replaced while its own vdb entry still sits in the `-MERGING-<pf>`
    // temporary (see `unmerge_replaced_same_slot`).
    // The merge-side just-preserved set travels with the replace loop
    // (real `preserve_paths` into `dblink.unmerge`) -- identical to
    // `merge_after_install`.
    let replacement_preserved: BTreeMap<String, Vec<String>> = if preserve_paths.is_empty() {
        BTreeMap::new()
    } else {
        BTreeMap::from([(
            format!("{category}/{pf}"),
            preserve_paths.iter().cloned().collect(),
        )])
    };
    let replaced_same_slot = unmerge_replaced_same_slot(
        root,
        &category,
        &package,
        &pf,
        &main_slot,
        &builddir.join("unmerge-src"),
        portage_tmpdir,
        options,
        &replacement_preserved,
    )?;
    // Publish, then real `dblink.treewalk()`'s own post-replace-loop
    // registration (`vartree.py:5266-5272`) -- identical to
    // `merge_after_install` (one commit on a database backend, see
    // `publish_merged_entry`).
    publish_merged_entry(
        root,
        &category,
        &package,
        &pf,
        &main_slot,
        &preserve_paths,
        &replaced_same_slot,
    )?;

    // Real `treewalk()` order: `pkg_postinst` runs after the vdb entry
    // is live *and* every replaced same-slot version is gone, but before
    // `env_update()`. Its own non-zero exit is logged, never fatal (real
    // `_postinst_failure` -- "It's stupid to bail out here").
    //
    // `PORTAGE_UPDATE_ENV` -> the vdb entry's own `environment.bz2` (just
    // written, verbatim from the binpkg): `phase-functions.sh`
    // regenerates it from the live, merge-time environment (real
    // `vartree.py:5334`), so the vdb env carries the resolved merge-time
    // `FEATURES` and drops stale build-host locals -- see
    // `ebuild_phases::run_phase_from_saved_env`. `always` so it runs even
    // with no `pkg_postinst`. No-op when the binpkg carries no saved env.
    //
    // The live entry's own path (`InstalledDb::entry_path`, N9), or a
    // scratch copy on a database backend (see `merge_after_install`).
    let update_env = update_env_target(root, &category, &pf, &builddir.join("vdb-update-env"))?;
    let vdb_env_bz2 = &update_env.path;
    let postinst_status = run_hook_ex(
        "postinst",
        true,
        vdb_env_bz2.is_file().then_some(vdb_env_bz2.as_path()),
    )?;
    absorb_update_env(root, &category, &pf, &update_env)?;
    if postinst_status != 0 {
        eprintln!(
            "{category}/{pf}: FAILED postinst ({postinst_status}) -- merge kept (real _postinst_failure)"
        );
    }

    if !contents.is_empty() || !replaced_same_slot.is_empty() {
        env_update::run_env_update(root)?;
        // Real `treewalk()`: prune preserved libs orphaned by this merge
        // (identical to `merge_after_install`). No include feed: the new
        // entry is already renamed into place, so its own lines are
        // enumerated (backlog #224).
        prune_unused_preserved_libs(root, false, &|_| false, None, &BTreeMap::new(), &[])?;
    }

    // Real `dblink.merge()`'s `_elog_process()`, before the builddir
    // removal below -- the binary merge's `pkg_preinst`/`pkg_postinst`
    // output lives in this builddir's `${T}/logging`, and the
    // unconditional `remove_dir_all` right after would take it with it
    // (the same #42 ordering as `merge_after_install`). Empty (and
    // silent) for a binpkg that carried no saved env.
    crate::elog::process_batch(
        &crate::elog::logdir(root),
        &root.display().to_string(),
        &[(format!("{category}/{pf}"), builddir.join("temp"))],
        None,
        &crate::color::Colorizer::new(crate::color::resolve_havecolor(None)),
        feature_enabled(options, "split-elog"),
    );

    let _ = std::fs::remove_dir_all(&builddir);
    Ok(postinst_status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;

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
    fn replacing_versions_is_the_installed_same_slot_version() {
        // #158: real's `REPLACING_VERSIONS` is the version of the
        // installed instance(s) this merge replaces (`vartree.py:4769`),
        // which is exactly the `{pn}-{version}` PF `installed_instance_pf`
        // selects.
        assert_eq!(
            replacing_versions("awk", Some("awk-4")),
            Some("4".to_string())
        );
        assert_eq!(
            replacing_versions("awk", Some("awk-3.1-r1")),
            Some("3.1-r1".to_string())
        );
        assert_eq!(
            replacing_versions("awk", None),
            None,
            "a first-ever install has no replaced version (real's empty string)"
        );
    }

    #[test]
    fn read_installed_slot_returns_0_for_a_slot_less_vdb_entry() {
        // #126 (O5 ruling 2026-09-23): real `aux_get` translates a
        // missing/empty SLOT to "0", so a SLOT-less entry groups with
        // slot-"0" entries instead of being skipped.
        let tmp = tempdir();
        let root = tmp.join("ROOT");
        let vdb_dir = root.join("var/db/pkg/dev-libs/slotlesspkg-1.0");
        std::fs::create_dir_all(&vdb_dir).unwrap();
        std::fs::write(vdb_dir.join("COUNTER"), "1").unwrap();

        assert_eq!(
            read_installed_slot(&root, "dev-libs", "slotlesspkg", "1.0"),
            Some("0".to_string())
        );
    }

    #[test]
    fn read_installed_slot_returns_0_for_an_empty_slot_file() {
        let tmp = tempdir();
        let root = tmp.join("ROOT");
        let vdb_dir = root.join("var/db/pkg/dev-libs/emptyslotpkg-1.0");
        std::fs::create_dir_all(&vdb_dir).unwrap();
        std::fs::write(vdb_dir.join("SLOT"), "").unwrap();

        assert_eq!(
            read_installed_slot(&root, "dev-libs", "emptyslotpkg", "1.0"),
            Some("0".to_string())
        );
    }

    #[test]
    fn read_installed_slot_collapses_a_multiline_slot_before_the_main_slot_projection() {
        // #126: through the seam a multi-line SLOT collapses
        // (`" ".join(myd.split())`, real `_aux_get`) where the old bare
        // `.trim()` kept the embedded newline.
        // #115 S1: the collapsed value (`"0 1"`) is itself invalid, so
        // the seam translates it on to `"0"` (real `aux_get`'s
        // `_get_slot_re` rule) -- the pre-S1 expectation is kept in git
        // history.
        let tmp = tempdir();
        let root = tmp.join("ROOT");
        let vdb_dir = root.join("var/db/pkg/dev-libs/multilinepkg-1.0");
        std::fs::create_dir_all(&vdb_dir).unwrap();
        std::fs::write(vdb_dir.join("SLOT"), "0\n1\n").unwrap();

        assert_eq!(
            read_installed_slot(&root, "dev-libs", "multilinepkg", "1.0"),
            Some("0".to_string())
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
        assert!(
            contents
                .lines()
                .any(|l| l.starts_with("obj /usr/share/x/hello.txt "))
        );
        assert!(contents.contains("sym /usr/share/x/link.txt -> hello.txt"));
    }

    /// Backlog #96's core invariant: replacing a file must **never write
    /// the existing inode**. The test holds the destination open and
    /// checks that its bytes are untouched after the merge, while the
    /// path itself carries the new content (a fresh inode). An in-place
    /// `std::fs::copy` fails this (the open fd would read the new bytes);
    /// the rename-based `replace_file_atomic` passes. This is the
    /// property that keeps an mmap'd `libc.so.6`/`libreadline.so.8` alive
    /// through its own merge, where `ETXTBSY` does **not** protect the
    /// running process.
    #[test]
    fn merge_tree_never_writes_the_existing_inode() {
        use std::io::Read;

        let tmp = tempdir();
        let d = tmp.join("D");
        let root = tmp.join("ROOT");
        std::fs::create_dir_all(d.join("lib")).unwrap();
        std::fs::create_dir_all(root.join("lib")).unwrap();
        std::fs::write(root.join("lib/libx.so.1"), b"old library bytes").unwrap();
        std::fs::write(d.join("lib/libx.so.1"), b"new library bytes, longer").unwrap();
        let old_inode = std::fs::metadata(root.join("lib/libx.so.1")).unwrap().ino();

        // Stand-in for a process that has the file open/mapped: the fd
        // must keep reading the old inode after the merge.
        let mut old_fd = std::fs::File::open(root.join("lib/libx.so.1")).unwrap();

        let mut cfgfiledict = BTreeMap::new();
        merge_tree(
            &d,
            &root,
            "sys-libs",
            None,
            false,
            "/etc",
            "/etc/env.d",
            false,
            &mut cfgfiledict,
        )
        .expect("merge_tree succeeds");

        let mut still_open = Vec::new();
        old_fd
            .read_to_end(&mut still_open)
            .expect("the old fd is still readable");
        assert_eq!(
            still_open, b"old library bytes",
            "the existing inode must never be written to"
        );
        assert_eq!(
            std::fs::read(root.join("lib/libx.so.1")).unwrap(),
            b"new library bytes, longer",
            "the path must carry the new content"
        );
        assert_ne!(
            std::fs::metadata(root.join("lib/libx.so.1")).unwrap().ino(),
            old_inode,
            "the destination must be a fresh inode"
        );
        let leftovers: Vec<String> = std::fs::read_dir(root.join("lib"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|n| n.contains("_portage_merge_"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "no merge temporary may remain: {leftovers:?}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// The live shape of backlog #96: a package upgrading a binary that
    /// is **currently executing** (`/bin/bash` on every shell upgrade).
    /// The old code's `std::fs::copy` hit `ETXTBSY` and aborted the merge
    /// mid-copy (leaving the new files unowned, the vdb unwritten); the
    /// rename-based replacement succeeds and the running process keeps
    /// its old inode. `/bin/sleep` stands in for the shell: it is the
    /// same execve-protected kernel path.
    #[test]
    fn merge_tree_replaces_a_running_executable_atomically() {
        let sleep = ["/bin/sleep", "/usr/bin/sleep"]
            .iter()
            .map(Path::new)
            .find(|p| p.is_file())
            .expect("a system sleep binary");
        let true_bin = ["/bin/true", "/usr/bin/true"]
            .iter()
            .map(Path::new)
            .find(|p| p.is_file())
            .expect("a system true binary");

        let tmp = tempdir();
        let d = tmp.join("D");
        let root = tmp.join("ROOT");
        std::fs::create_dir_all(d.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::copy(sleep, root.join("bin/prog")).unwrap();
        std::fs::copy(true_bin, d.join("bin/prog")).unwrap();
        let old_inode = std::fs::metadata(root.join("bin/prog")).unwrap().ino();

        let mut child = match std::process::Command::new(root.join("bin/prog"))
            .arg("60")
            .spawn()
        {
            Ok(child) => child,
            Err(first) => {
                // A fully loaded test machine (the whole-workspace run
                // executes every crate's tests) can transiently fail a
                // fork/exec with EAGAIN; retry once before giving up.
                std::thread::sleep(std::time::Duration::from_millis(500));
                std::process::Command::new(root.join("bin/prog"))
                    .arg("60")
                    .spawn()
                    .unwrap_or_else(|e| panic!("spawn the old binary ({first}, {e})"))
            }
        };
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(
            child.try_wait().expect("try_wait").is_none(),
            "the old binary must still be executing before the merge"
        );

        let mut cfgfiledict = BTreeMap::new();
        let outcome = merge_tree(
            &d,
            &root,
            "app-shells",
            None,
            false,
            "/etc",
            "/etc/env.d",
            false,
            &mut cfgfiledict,
        );

        let alive = child.try_wait().expect("try_wait").is_none();
        let _ = child.kill();
        let _ = child.wait();
        outcome.expect(
            "replacing a running executable must not write its inode (ETXTBSY would abort)",
        );
        assert!(alive, "the running process must survive the merge");
        assert_eq!(
            std::fs::read(root.join("bin/prog")).unwrap(),
            std::fs::read(true_bin).unwrap(),
            "the path must carry the new binary"
        );
        assert_ne!(
            std::fs::metadata(root.join("bin/prog")).unwrap().ino(),
            old_inode,
            "the destination must be a fresh inode"
        );
        let leftovers: Vec<String> = std::fs::read_dir(root.join("bin"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|n| n.contains("_portage_merge_"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "no merge temporary may remain: {leftovers:?}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// A symlink replacement uses the same rename-based atomic path (no
    /// remove-then-create window, no temporaries left behind), and the
    /// destination is still a symlink pointing at the new target.
    #[test]
    fn merge_tree_replaces_a_symlink_without_temporaries() {
        let tmp = tempdir();
        let d = tmp.join("D");
        let root = tmp.join("ROOT");
        std::fs::create_dir_all(d.join("usr/lib")).unwrap();
        std::fs::create_dir_all(root.join("usr/lib")).unwrap();
        std::fs::write(root.join("usr/lib/libx.so.1"), b"lib").unwrap();
        std::fs::write(d.join("usr/lib/libx.so.1"), b"lib").unwrap();
        std::os::unix::fs::symlink("libx.so.0", root.join("usr/lib/libx.so")).unwrap();
        std::os::unix::fs::symlink("libx.so.1", d.join("usr/lib/libx.so")).unwrap();

        let mut cfgfiledict = BTreeMap::new();
        merge_tree(
            &d,
            &root,
            "sys-libs",
            None,
            false,
            "/etc",
            "/etc/env.d",
            false,
            &mut cfgfiledict,
        )
        .expect("merge_tree succeeds");

        assert_eq!(
            std::fs::read_link(root.join("usr/lib/libx.so")).unwrap(),
            PathBuf::from("libx.so.1")
        );
        assert!(
            std::fs::symlink_metadata(root.join("usr/lib/libx.so"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let leftovers: Vec<String> = std::fs::read_dir(root.join("usr/lib"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|n| n.contains("_portage_merge_"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "no merge temporary may remain: {leftovers:?}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Directly `libc::lchown`/`libc::chown`s a test-source path to an
    /// arbitrary uid/gid, independent of `lchown_or_chown` itself (the
    /// function under test) -- test setup, not a call into production
    /// code.
    fn raw_chown(path: &Path, uid: u32, gid: u32, is_symlink: bool) {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        let ret = if is_symlink {
            // SAFETY: `c` is a CString that outlives the call; takes a
            // NUL-terminated path pointer plus plain uid/gid ints.
            unsafe { libc::lchown(c.as_ptr(), uid, gid) }
        } else {
            // SAFETY: `c` is a CString that outlives the call; takes a
            // NUL-terminated path pointer plus plain uid/gid ints.
            unsafe { libc::chown(c.as_ptr(), uid, gid) }
        };
        assert_eq!(
            ret,
            0,
            "{}: {}",
            path.display(),
            std::io::Error::last_os_error()
        );
    }

    #[test]
    fn merge_tree_preserves_ownership_from_the_source_when_root() {
        // Real movefile()'s lchown/chown (symlink, regular file) and
        // mergeme()'s own directory chown/chmod (a newly created
        // directory only): the merged destination's owner/group must
        // match the *source*'s own recorded uid/gid, not whoever ran
        // the merge. Only actually exercisable as root -- chowning to
        // an arbitrary uid needs privilege, the same real precondition
        // this whole feature has (see `lchown_or_chown`'s own doc
        // comment). Run under `sudo cargo test ...` to exercise it;
        // skipped (not failed) otherwise, matching that real
        // precondition rather than asserting something no local
        // environment could ever satisfy.
        // SAFETY: `geteuid` takes no arguments and returns a plain int.
        if unsafe { libc::geteuid() } != 0 {
            eprintln!(
                "skipping merge_tree_preserves_ownership_from_the_source_when_root: not root"
            );
            return;
        }
        let tmp = tempdir();
        let d = tmp.join("D");
        let root = tmp.join("ROOT");
        std::fs::create_dir_all(d.join("usr/share/x")).unwrap();
        std::fs::write(d.join("usr/share/x/hello.txt"), b"hello").unwrap();
        std::os::unix::fs::symlink("hello.txt", d.join("usr/share/x/link.txt")).unwrap();
        std::fs::create_dir_all(&root).unwrap();

        // A uid/gid that's certainly neither 0 nor this process's own.
        let (uid, gid) = (65534u32, 65534u32);
        for p in [
            d.join("usr"),
            d.join("usr/share"),
            d.join("usr/share/x"),
            d.join("usr/share/x/hello.txt"),
        ] {
            raw_chown(&p, uid, gid, false);
        }
        raw_chown(&d.join("usr/share/x/link.txt"), uid, gid, true);

        let mut cfgfiledict = BTreeMap::new();
        merge_tree(
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

        for p in [
            root.join("usr"),
            root.join("usr/share"),
            root.join("usr/share/x"),
            root.join("usr/share/x/hello.txt"),
            root.join("usr/share/x/link.txt"),
        ] {
            let meta = std::fs::symlink_metadata(&p).unwrap();
            assert_eq!(meta.uid(), uid, "{}: uid", p.display());
            assert_eq!(meta.gid(), gid, "{}: gid", p.display());
        }

        // An already-existing directory is left alone (real mergeme()'s
        // "kept as-is" branch, vartree.py:5808-5810) -- merging a
        // *second* time with a different source owner must not clobber
        // usr/share/x's now-installed ownership.
        raw_chown(&d.join("usr/share/x"), 0, 0, false);
        let mut cfgfiledict2 = BTreeMap::new();
        merge_tree(
            &d,
            &root,
            "dev-libs",
            None,
            false,
            "/etc",
            "/etc/env.d",
            false,
            &mut cfgfiledict2,
        )
        .expect("merge_tree succeeds");
        let meta = std::fs::symlink_metadata(root.join("usr/share/x")).unwrap();
        assert_eq!(meta.uid(), uid, "existing dir must keep its ownership");
        assert_eq!(meta.gid(), gid, "existing dir must keep its ownership");
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
        assert!(
            contents
                .lines()
                .any(|l| l.starts_with(&format!("obj /etc/foo.conf {new_md5} ")))
        );
        assert_eq!(cfgfiledict.get("/etc/foo.conf"), Some(&new_md5));
    }

    #[test]
    fn merge_tree_protect_if_modified_applies_directly_when_dest_still_matches_the_installed_instance()
     {
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
        assert!(
            contents
                .lines()
                .any(|l| l.starts_with(&format!("obj /etc/foo.conf {new_md5} ")))
        );
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
        assert!(
            contents
                .lines()
                .any(|l| l.starts_with("sym /etc/link.conf -> new-target"))
        );
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
        assert!(
            contents
                .lines()
                .any(|l| l.starts_with("sym /etc/thing.conf -> new-target"))
        );
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
        assert!(
            contents
                .lines()
                .any(|l| l.starts_with("obj /etc/thing.conf"))
        );
    }

    fn tempdir() -> std::path::PathBuf {
        TempDir::new("portuale-ebuild-merge-test").keep()
    }

    #[test]
    fn apply_install_mask_deletes_matched_paths_and_records_the_mask() {
        let tmp = tempdir();
        let image = tmp.join("image");
        let build_info = tmp.join("build-info");
        std::fs::create_dir_all(image.join("usr/share/info")).unwrap();
        std::fs::create_dir_all(image.join("usr/bin")).unwrap();
        std::fs::create_dir_all(&build_info).unwrap();
        std::fs::write(image.join("usr/share/info/foo.info"), b"x").unwrap();
        std::fs::write(image.join("usr/bin/foo"), b"x").unwrap();

        let options = MergeOptions {
            install_mask: "/usr/share/info".to_string(),
            install_mask_prunes_usr_share: true,
            ..MergeOptions::default()
        };
        apply_install_mask(&image, &build_info, &options).unwrap();

        assert!(!image.join("usr/share/info").exists(), "masked dir removed");
        assert!(
            !image.join("usr/share").exists(),
            "emptied usr/share pruned"
        );
        assert!(image.join("usr/bin/foo").is_file(), "unmasked file kept");
        assert_eq!(
            std::fs::read_to_string(build_info.join("INSTALL_MASK"))
                .unwrap()
                .trim(),
            "/usr/share/info"
        );

        // Empty mask: no-op, no INSTALL_MASK file.
        let bi2 = tmp.join("bi2");
        std::fs::create_dir_all(&bi2).unwrap();
        apply_install_mask(&image, &bi2, &MergeOptions::default()).unwrap();
        assert!(!bi2.join("INSTALL_MASK").exists());
        let _ = std::fs::remove_dir_all(&tmp);
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

        // The ordering markers `mergepkg`'s own hooks leave under `${T}`
        // are asserted below, so this merge keeps its build state the way
        // real `FEATURES=noclean` does -- the default post-merge clean is
        // pinned by `run_merge_post_cleans_the_builddir_unless_noclean`.
        let options = MergeOptions {
            features: "noclean".to_string(),
            ..MergeOptions::default()
        };
        let status =
            run_merge(&ebuild, &root, &portage_tmpdir, &options, None).expect("run_merge succeeds");
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
        assert!(
            contents
                .lines()
                .any(|l| l.starts_with("obj /usr/share/mergepkg/hello.txt "))
        );
        assert!(contents.contains("sym /usr/share/mergepkg/hello-link.txt -> hello.txt"));

        let counter: i64 = std::fs::read_to_string(vdb_dir.join("COUNTER"))
            .unwrap()
            .parse()
            .expect("COUNTER is a bare integer");
        assert!(counter >= 0);

        // Real dblink.merge()'s own atomic dbtmpdir-then-rename: no
        // MERGING_IDENTIFIER-prefixed temp directory should survive a
        // successful merge.
        assert!(
            !root
                .join("var/db/pkg/dev-libs/-MERGING-mergepkg-1.0")
                .exists()
        );

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

    /// A *computed* `SLOT="$(ver_cut 1)"` (task #149) is read back as its
    /// **evaluated** value -- the value real Portage's own metadata
    /// generation and `dblink.treewalk` both use. Expected value comes
    /// from real Portage: the mpdecimal probe on the container test bed
    /// (`4.0.1` -> `SLOT=4` in the real vdb, and `build-info/SLOT` = `4`
    /// from the install phase writing the evaluated `${SLOT}`); for this
    /// fixture `ver_cut` of `1.0` evaluates to `1`.
    ///
    /// Before the fix, `merge_after_install` derived `full_slot` by
    /// regex-parsing the *raw* ebuild text (`parse_slot`), so the literal
    /// `$(ver_cut 1)` itself landed in the vdb -- and every slot-keyed
    /// consumer (`installed_instance_pf`/`find_collisions`/
    /// `blocked_installed_packages`/`unmerge_replaced_same_slot`, all fed
    /// `full_slot.split('/')`) keyed off the same corrupt value. On a
    /// real image whose `dev-libs/mpdecimal-4.0.1` was installed by real
    /// portage (evaluated `SLOT=4`), that mismatch made `find_collisions`
    /// refuse to credit the installed instance, so a reinstall under
    /// `FEATURES=protect-owned` aborted with `NOT merged due to file
    /// collisions` -- the exact #149 symptom. Here the reinstall must not
    /// abort under `MergeOptions::default()` (`protect_owned: true`), and
    /// the vdb `SLOT` must record the evaluated `1`.
    #[test]
    fn computed_slot_is_evaluated_in_the_vdb_and_a_reinstall_does_not_abort_under_protect_owned() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/compuslotpkg/compuslotpkg-1.0.ebuild");

        // First install: lands the file and a vdb entry under the real
        // default FEATURES (protect_owned: true).
        let status = run_merge(
            &ebuild,
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
            None,
        )
        .expect("first merge succeeds");
        assert_eq!(status, 0);

        let vdb_dir = root.join("var/db/pkg/dev-libs/compuslotpkg-1.0");
        assert_eq!(
            std::fs::read_to_string(vdb_dir.join("SLOT"))
                .unwrap()
                .trim(),
            "1",
            "vdb SLOT must be the evaluated ver_cut value, not the raw $(ver_cut 1) text"
        );

        // Reinstall under the same real-default protect_owned: the
        // installed instance owns its own file, so this must not abort.
        let status = run_merge(
            &ebuild,
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
            None,
        )
        .expect("reinstall merge succeeds");
        assert_eq!(status, 0, "reinstall must not abort under protect-owned");
        assert_eq!(
            std::fs::read_to_string(vdb_dir.join("SLOT"))
                .unwrap()
                .trim(),
            "1"
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
            &[],
        )
        .expect("install phase succeeds");
        assert_eq!(install_status, 0);
        // Real bin/phase-functions.sh's own __dyn_install already leaves
        // this marker behind -- confirms the test's own setup is
        // faithful to what a real prior `ebuild <file> install` run
        // leaves for qmerge to find.
        assert!(
            portage_tmpdir
                .join("portage/dev-libs/mergepkg-1.0/.installed")
                .exists()
        );

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
        assert!(
            !root
                .join("var/db/pkg/dev-libs/-MERGING-mergepkg-1.0")
                .exists()
        );
    }

    /// Backlog #183: a stale `-MERGING-<pf>` entry left by a killed merge
    /// is invisible to every portuale vdb reader, and the next merge of
    /// the same package wipes it and publishes the real entry -- real
    /// `dblink.treewalk()`'s own `self.dbdir = self.dbtmpdir;
    /// self.delete(); ensure_dirs(self.dbtmpdir)` prologue plus the
    /// `vardbapi._excluded_dirs` reader skip (`vartree.py`). Ground truth:
    /// the `l32` C4 control cell (`findings/l5.md` Group 4) leaves exactly
    /// `-MERGING-slow-a-1.0` after the SIGKILL, no `slow-a-1.0`, and the
    /// resumed merge reinstalls from scratch.
    #[test]
    fn stale_merging_entry_is_invisible_and_replaced_by_the_next_merge() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        // A killed merge's leftovers: a half-written `-MERGING-` entry
        // for the package about to be merged, and nothing live.
        let stale = root.join("var/db/pkg/dev-libs/-MERGING-mergepkg-1.0");
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::write(stale.join("SLOT"), "0\n").unwrap();
        std::fs::write(stale.join("COUNTER"), "41\n").unwrap();
        std::fs::write(
            stale.join("CONTENTS"),
            "obj /usr/share/mergepkg/hello.txt deadbeef 123\n",
        )
        .unwrap();
        std::fs::write(stale.join("NEEDED.ELF.2"), "junk\n").unwrap();
        assert!(!root.join("var/db/pkg/dev-libs/mergepkg-1.0").exists());

        // Every reader skips the stale entry: it is not an installed
        // package, owns no path, and indexes no sonames.
        assert!(portage_repo::all_installed_packages(&root).is_empty());
        assert!(portage_repo::installed_versions(&root, "dev-libs", "mergepkg").is_empty());
        assert!(
            find_owners(&root, &["/usr/share/mergepkg/hello.txt".to_string()]).is_empty(),
            "a stale half-written CONTENTS claims nothing"
        );
        assert!(
            crate::needed_elf::read_all_needed_entries(&root).is_empty(),
            "a stale NEEDED.ELF.2 indexes nothing"
        );

        // The next merge behaves as real: it wipes the stale entry and
        // publishes the real one, leaving no temporary behind.
        let repo_root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/mergepkg/mergepkg-1.0.ebuild");
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
        let vdb_dir = root.join("var/db/pkg/dev-libs/mergepkg-1.0");
        let contents = std::fs::read_to_string(vdb_dir.join("CONTENTS")).unwrap();
        assert!(
            contents
                .lines()
                .any(|l| l.starts_with("obj /usr/share/mergepkg/hello.txt "))
        );
        assert!(
            !contents.contains("deadbeef"),
            "the stale CONTENTS is fully replaced, not merged"
        );
        assert!(!stale.exists(), "the stale temporary is wiped");
        assert_eq!(portage_repo::all_installed_packages(&root).len(), 1);
    }

    /// Backlog #183: interrupting a merge while its vdb entry is still
    /// being written leaves `-MERGING-<pf>` and nothing at `<pf>` -- real
    /// `dbtmpdir` visibility from its creation (before `pkg_preinst` and
    /// the `${ROOT}` copy) to the `_movefile` into place after the old
    /// instances are unmerged. Drives the staged writer directly:
    /// `create` + `populate` + `write contents` is the interrupt point
    /// (no `publish` ever runs), pinning that every pre-`publish` stage
    /// leaves the live entry untouched and the temporary invisible.
    #[test]
    fn interrupted_vdb_write_leaves_merging_and_no_final_entry() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let build_info = tmp.join("build-info");
        std::fs::create_dir_all(&build_info).unwrap();
        std::fs::write(build_info.join("SLOT"), "0\n").unwrap();

        create_vdb_tmp(&root, "dev-libs", "victimpkg-1.0").unwrap();
        // A kill here (during `pkg_preinst` / the `${ROOT}` copy) leaves
        // an empty temporary -- real's own C4 shape, where the kill lands
        // in the `pkg_preinst` sleep before any info file is copied.
        let tmp_dir = root.join("var/db/pkg/dev-libs/-MERGING-victimpkg-1.0");
        assert!(tmp_dir.is_dir());
        assert!(!root.join("var/db/pkg/dev-libs/victimpkg-1.0").exists());

        populate_vdb_tmp(
            &root,
            "dev-libs",
            "victimpkg-1.0",
            &build_info,
            "0",
            "testrepo",
        )
        .unwrap();
        write_vdb_tmp_contents(
            &root,
            "dev-libs",
            "victimpkg-1.0",
            "obj /usr/bin/victim abc 123\n",
        )
        .unwrap();
        // ... and a kill here leaves the fully-populated temporary, still
        // with nothing live and nothing enumerated.
        assert!(!root.join("var/db/pkg/dev-libs/victimpkg-1.0").exists());
        assert!(portage_repo::all_installed_packages(&root).is_empty());

        publish_vdb_tmp(&root, "dev-libs", "victimpkg-1.0").unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("var/db/pkg/dev-libs/victimpkg-1.0/CONTENTS"))
                .unwrap(),
            "obj /usr/bin/victim abc 123\n"
        );
        assert!(!tmp_dir.exists(), "no temporary survives a publish");
    }

    /// Backlog #183: an upgrade publishes the new version's vdb entry only
    /// after the replaced same-slot version is unmerged -- real
    /// `treewalk()`'s own `_movefile(self.dbtmpdir, self.dbpkgdir)`
    /// position, after the replace loop. Observable contract: the old
    /// entry is gone, the new one is live with the new content, and no
    /// temporary survives. (The in-between invisibility itself is what the
    /// `l32` C4 candidate run checks live: the kill lands while the new
    /// entry is still `-MERGING-*`.)
    #[test]
    fn upgrade_publishes_the_new_entry_after_unmerging_the_old() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let v1 = repo_root.join("dev-libs/othersinslotpkg/othersinslotpkg-1.0.ebuild");
        let v2 = repo_root.join("dev-libs/othersinslotpkg/othersinslotpkg-2.0.ebuild");

        assert_eq!(
            run_merge(&v1, &root, &portage_tmpdir, &MergeOptions::default(), None).unwrap(),
            0
        );
        assert!(
            root.join("var/db/pkg/dev-libs/othersinslotpkg-1.0")
                .is_dir()
        );

        assert_eq!(
            run_merge(&v2, &root, &portage_tmpdir, &MergeOptions::default(), None).unwrap(),
            0
        );
        let vdb_cat = root.join("var/db/pkg/dev-libs");
        let entries: Vec<String> = portage_util::read_dir_entries(&vdb_cat)
            .unwrap()
            .into_iter()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(entries, vec!["othersinslotpkg-2.0".to_string()]);
        assert_eq!(
            std::fs::read_to_string(root.join("usr/share/othersinslotpkg/shared.txt")).unwrap(),
            "shared, from 2.0\n"
        );
        assert!(
            root.join("usr/share/othersinslotpkg/only-in-v2.txt")
                .is_file()
        );
        assert!(
            !root
                .join("usr/share/othersinslotpkg/only-in-v1.txt")
                .exists()
        );
    }

    /// L3 smoke finding: a package whose ebuild `SLOT` carries a sub-slot
    /// (`SLOT="0/1"`) must not collide with its own installed copy on a
    /// reinstall / `-e` merge. `find_collisions` and
    /// `installed_instance_pf` compare against the vdb `SLOT` file's
    /// *main* slot, so passing the ebuild's full `slot/sub_slot` made
    /// every file look foreign and `FEATURES=protect-owned` (real's
    /// default) aborted the merge.
    #[test]
    fn re_merging_a_sub_slotted_package_does_not_collide_with_its_own_files() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();
        let ebuild = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/subslotfilepkg/subslotfilepkg-1.0.ebuild");
        let options = MergeOptions {
            features: "sandbox".to_string(),
            ..MergeOptions::default()
        };

        assert_eq!(
            run_merge(&ebuild, &root, &portage_tmpdir, &options, None).unwrap(),
            0
        );
        let slot =
            std::fs::read_to_string(root.join("var/db/pkg/dev-libs/subslotfilepkg-1.0/SLOT"))
                .unwrap();
        assert_eq!(slot.trim(), "0/1");

        // Same version, same slot: every file is owned by this exact
        // package, so the second merge must not abort.
        assert_eq!(
            run_merge(&ebuild, &root, &portage_tmpdir, &options, None).unwrap(),
            0
        );
        assert!(root.join("usr/share/subslotfilepkg/hello.txt").is_file());
    }

    /// Backlog #42: real `dblink.merge()`'s tail (`vartree.py:6183-6198`)
    /// runs the `clean` phase after a merge unless `FEATURES=noclean`.
    /// The `emerge` paths that reach `run_merge` therefore leave no
    /// `${PORTAGE_BUILDDIR}` behind (which is what stopped a later `-B`
    /// from repackaging an already-`instprep`ped image, #38 S4); with
    /// `noclean` the build state survives.
    #[test]
    fn run_merge_post_cleans_the_builddir_unless_noclean() {
        let repo_root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/mergepkg/mergepkg-1.0.ebuild");

        // Default (no noclean): the builddir is cleaned after the merge.
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();
        let options = MergeOptions {
            features: "sandbox".to_string(),
            ..MergeOptions::default()
        };
        let status =
            run_merge(&ebuild, &root, &portage_tmpdir, &options, None).expect("run_merge succeeds");
        assert_eq!(status, 0);
        assert!(root.join("usr/share/mergepkg/hello.txt").is_file());
        let builddir = portage_tmpdir.join("portage/dev-libs/mergepkg-1.0");
        assert!(
            !builddir.join(".installed").exists(),
            "post-merge clean must drop .installed"
        );
        assert!(
            !builddir.join("image").exists(),
            "post-merge clean must drop the image"
        );

        // `FEATURES=noclean`: real keeps the builddir (bug #704866's
        // sibling gate); `.installed` is still there for a later
        // `qmerge`/inspection.
        let tmp2 = tempdir();
        let root2 = tmp2.join("root");
        let portage_tmpdir2 = tmp2.join("tmp");
        std::fs::create_dir_all(&root2).unwrap();
        std::fs::create_dir_all(&portage_tmpdir2).unwrap();
        let options = MergeOptions {
            features: "noclean".to_string(),
            ..MergeOptions::default()
        };
        let status = run_merge(&ebuild, &root2, &portage_tmpdir2, &options, None)
            .expect("run_merge succeeds");
        assert_eq!(status, 0);
        assert!(root2.join("usr/share/mergepkg/hello.txt").is_file());
        assert!(
            portage_tmpdir2
                .join("portage/dev-libs/mergepkg-1.0/.installed")
                .exists(),
            "noclean must keep the builddir"
        );
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
        assert!(
            contents
                .lines()
                .any(|l| l.starts_with("obj /etc/configpkg.conf "))
        );
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
        assert!(
            contents
                .lines()
                .any(|l| l.starts_with("sym /etc/configsympkg.conf -> new-target"))
        );
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
    fn set_resolved_features_rederives_the_merge_tokens() {
        // #37 S3: the emerge paths replace `from_env`'s raw-FEATURES
        // fields with the resolved list, which is what makes a
        // make.conf `collision-protect` / `-protect-owned` /
        // `config-protect-if-modified` take effect; the GPG verify policy
        // follows the same list.
        let mut options = MergeOptions::default();
        options.set_resolved_features("collision-protect binpkg-request-signature");
        assert!(options.collision_protect);
        assert!(!options.protect_owned);
        assert!(!options.protect_if_modified);
        assert_eq!(
            options.features,
            "collision-protect binpkg-request-signature"
        );
        assert!(options.gpg_verify.request_signature);
        // Defaults-on tokens present in make.globals stay on.
        options.set_resolved_features("protect-owned config-protect-if-modified");
        assert!(options.protect_owned);
        assert!(options.protect_if_modified);
        assert!(!options.collision_protect);
        assert!(!options.gpg_verify.request_signature);
    }

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
    /// regardless of `FEATURES` -- `collisionpkg-b` installs a
    /// file-target (dangling `nowhere`) symlink exactly where
    /// `collisionpkg-a` already installed a real directory (`adir`),
    /// which aborts the merge even with `collision_protect: false`
    /// (`MergeOptions::default()`). Dangling/file-target symlinks are
    /// the ones real `os.walk` lists in `files` (hence in `linklist`);
    /// directory-target symlinks are in `dirs` and never checked -- see
    /// the next test.
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

    /// Real `os.walk` puts a symlink pointing to a directory into `dirs`,
    /// never into `filelist`/`linklist` (`vartree.py:4625-4681` comment) --
    /// so it is never collision-checked. `find_collisions` mirrors that:
    /// a directory-target symlink over an installed real directory (the
    /// linux-firmware `nvidia/ad10x` shape: new image symlinks, live
    /// filesystem directories) yields no `symlink_collisions` entry, and
    /// `merge_tree` lands the symlink at the first `dest.backup.NNNN`
    /// (real `mergeme()`'s own symlink-over-directory branch) keeping
    /// the directory, exiting 0 -- even with `protect_owned` on (the
    /// reporter's own `FEATURES`, which has `protect-owned` but not
    /// `collision-protect`) and an additional unclaimed ordinary
    /// collision present (which alone never aborts under `protect-owned`
    /// either).
    #[test]
    fn directory_target_symlink_over_a_real_directory_is_ignored_and_backed_up() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let image = tmp.join("image");
        std::fs::create_dir_all(root.join("lib/firmware/nvidia/ad103")).unwrap();
        std::fs::write(
            root.join("lib/firmware/nvidia/ad103/some.bin"),
            b"installed",
        )
        .unwrap();
        // An unclaimed ordinary collision alongside (a stray file the new
        // image also ships): must not abort under `protect_owned` alone.
        std::fs::create_dir_all(root.join("lib/firmware/brcm")).unwrap();
        std::fs::write(root.join("lib/firmware/brcm/stray.txt"), b"stray on disk").unwrap();

        std::fs::create_dir_all(image.join("lib/firmware/nvidia/real")).unwrap();
        std::fs::write(image.join("lib/firmware/nvidia/real/some.bin"), b"new").unwrap();
        std::os::unix::fs::symlink("real", image.join("lib/firmware/nvidia/ad103")).unwrap();
        std::fs::create_dir_all(image.join("lib/firmware/brcm")).unwrap();
        std::fs::write(image.join("lib/firmware/brcm/stray.txt"), b"new stray").unwrap();

        let empty_plib = HashMap::new();
        let empty_blocked = HashSet::new();
        let (collisions, symlink_collisions, _) = find_collisions(
            &image,
            &root,
            "sys-kernel",
            "linux-firmware",
            "0",
            "/etc",
            "/etc/env.d",
            &empty_plib,
            &empty_blocked,
        )
        .expect("find_collisions succeeds");
        assert!(
            symlink_collisions.is_empty(),
            "a directory-target symlink is not a PMS 13.4 collision: {symlink_collisions:?}"
        );
        // The ordinary stray-file collision is reported...
        assert_eq!(collisions, vec!["/lib/firmware/brcm/stray.txt".to_string()]);
        // ...but aborts under neither `collision-protect` (off) nor
        // `protect-owned` alone (no owner claims it).
        assert!(
            find_owners(&root, &collisions).is_empty(),
            "the stray file is unclaimed"
        );

        let mut cfgfiledict = BTreeMap::new();
        merge_tree(
            &image,
            &root,
            "sys-kernel",
            None,
            true,
            "/etc",
            "/etc/env.d",
            false,
            &mut cfgfiledict,
        )
        .expect("merge_tree succeeds");

        // The installed directory survives; the new symlink lands at the
        // first backup name, exactly like real `mergeme()`.
        assert!(root.join("lib/firmware/nvidia/ad103").is_dir());
        assert_eq!(
            std::fs::read_link(root.join("lib/firmware/nvidia/ad103.backup.0000")).unwrap(),
            PathBuf::from("real")
        );
        // The unclaimed ordinary file is merged over, like real's
        // "merged despite file collisions" path.
        assert_eq!(
            std::fs::read_to_string(root.join("lib/firmware/brcm/stray.txt")).unwrap(),
            "new stray"
        );
        let _ = std::fs::remove_dir_all(&tmp);
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
        // #116: `normalpkg` carries a sub-slot, so the re-point through
        // the vdb seam must preserve the `split_once('/')` post-processing
        // -- the expectations below are the pre-slice ones, unchanged.
        for (pf, slot) in [("blockedpkg-1.0", "0"), ("normalpkg-2.0", "3/3.1")] {
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

    /// `find_owners`/`owns_path` read each vdb entry's file list through
    /// the `mrg_director::PackagesDb` seam (`VdbReader::contents_files`),
    /// which keys by a split `(package, version)` and strips the
    /// `CONTENTS` record's one leading `/`. This pins the two conversions
    /// the production path now performs on a hand-built vdb entry: the
    /// directory name's `pf` splits back into that key, and the logical
    /// absolute collision path meets the stripped contents path while the
    /// returned owner key/values keep the old `category/pf` + absolute
    /// path shape exactly.
    #[test]
    fn find_owners_reads_contents_through_the_packages_db_seam() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let pkg_dir = root.join("var/db/pkg/dev-libs/findownerspkg-1.0");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("CONTENTS"),
            "dir /usr/share/findownerspkg\nobj /usr/share/findownerspkg/owned.txt abc 0\n",
        )
        .unwrap();

        assert!(owns_path(
            &root,
            "dev-libs",
            "findownerspkg",
            "1.0",
            "/usr/share/findownerspkg/owned.txt"
        ));
        assert!(!owns_path(
            &root,
            "dev-libs",
            "findownerspkg",
            "1.0",
            "/usr/share/findownerspkg/other.txt"
        ));

        let collisions = vec!["/usr/share/findownerspkg/owned.txt".to_string()];
        assert_eq!(
            find_owners(&root, &collisions),
            BTreeMap::from([(
                "dev-libs/findownerspkg-1.0".to_string(),
                vec!["/usr/share/findownerspkg/owned.txt".to_string()],
            )])
        );
        assert!(
            find_owners(&root, &["/usr/share/findownerspkg/stray.txt".to_string()]).is_empty(),
            "a path no installed entry recorded stays unclaimed"
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
                orig_entries: BTreeMap::new(),
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

    /// S0 for backlog #167(a): real `PreservedLibsRegistry.store()`
    /// (`3rdparty/portage/lib/portage/util/_dyn_libs/
    /// PreservedLibsRegistry.py:107-108`) returns without writing when the
    /// registry is unchanged (`self._data == self._data_orig`) -- a
    /// merge/unmerge that preserves nothing must leave a packaged 0-byte
    /// registry at 0 bytes (l32 C1/C3/C4 cells), not rewrite it.
    #[test]
    fn plib_registry_is_not_rewritten_when_nothing_is_preserved() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let reg_path = plib_registry_path(&root);
        std::fs::create_dir_all(reg_path.parent().unwrap()).unwrap();
        std::fs::write(&reg_path, b"").unwrap();
        let before = std::fs::metadata(&reg_path).unwrap();
        let before_mtime = before.modified().unwrap();

        std::fs::create_dir_all(root.join("usr/lib")).unwrap();
        std::fs::write(root.join("usr/lib/plain.so"), b"x").unwrap();
        let vdb = root.join("var/db/pkg/dev-libs/plain-1.0");
        std::fs::create_dir_all(&vdb).unwrap();
        std::fs::write(vdb.join("COUNTER"), "3\n").unwrap();
        let preserved = preserve_libs_on_unmerge(
            &root,
            "dev-libs",
            "plain",
            "plain-1.0",
            "0",
            "obj /usr/lib/plain.so abc 1\n",
            false,
        )
        .unwrap();
        assert!(preserved.is_empty());

        let registry = read_plib_registry(&root);
        unregister_preserved_libs(&root, "dev-libs/plain-1.0", registry, &BTreeMap::new()).unwrap();

        let removed =
            prune_unused_preserved_libs(&root, false, &|_| false, None, &BTreeMap::new(), &[])
                .unwrap();
        assert!(removed.is_empty());

        let after = std::fs::metadata(&reg_path).unwrap();
        assert_eq!(after.len(), 0, "a 0-byte registry must stay 0 bytes");
        assert_eq!(
            after.modified().unwrap(),
            before_mtime,
            "the registry file must not be rewritten when nothing changed"
        );
    }

    /// Fix round 1 for review item 2 (backlog #178): real `store()`
    /// returns early when the registry equals what was read
    /// (`PreservedLibsRegistry.store`) -- a merge/unmerge with no
    /// preserved-lib activity must leave an unrelated non-empty entry's
    /// bytes **and** mtime untouched, not just the empty-file case
    /// above. Pre-fix, every `preserve_libs_on_unmerge` /
    /// `prune_unused_preserved_libs` call rewrote the file even when
    /// `register()` was a no-op.
    #[test]
    fn plib_registry_with_an_unrelated_entry_is_not_rewritten_without_preserve_activity() {
        let tmp = tempdir();
        let root = tmp.join("root");
        // A live unrelated entry: the file exists (so `pruneNonExisting`
        // keeps it) and stays needed (an installed consumer links its
        // soname), so every step below is a genuine registry no-op.
        std::fs::create_dir_all(root.join("usr/lib")).unwrap();
        std::fs::create_dir_all(root.join("usr/bin")).unwrap();
        std::fs::write(root.join("usr/lib/other.so"), b"fake elf").unwrap();
        std::fs::write(root.join("usr/bin/cprog"), b"fake elf").unwrap();
        let vdb_consumer = root.join("var/db/pkg/dev-libs/consumer-1.0");
        std::fs::create_dir_all(&vdb_consumer).unwrap();
        std::fs::write(vdb_consumer.join("COUNTER"), "1\n").unwrap();
        std::fs::write(
            vdb_consumer.join("CONTENTS"),
            "obj /usr/bin/cprog abc 1\nobj /usr/lib/other.so def 1\n",
        )
        .unwrap();
        std::fs::write(
            vdb_consumer.join("NEEDED.ELF.2"),
            "X86_64;/usr/lib/other.so;libother.so.1;;\nX86_64;/usr/bin/cprog;;;libother.so.1\n",
        )
        .unwrap();
        let seed = "{\n\t\"dev-libs/other:0\": [\n\t\t\"dev-libs/other-1.0\",\n\t\t\"9\",\n\t\t[\n\t\t\t\"/usr/lib/other.so\"\n\t\t]\n\t]\n}";
        std::fs::create_dir_all(plib_registry_path(&root).parent().unwrap()).unwrap();
        std::fs::write(plib_registry_path(&root), seed).unwrap();
        // A distinctly old mtime, so any rewrite is observable even at
        // coarse filesystem timestamp granularity.
        let touch = std::process::Command::new("touch")
            .args([
                "-d",
                "2001-02-03 04:05:06",
                &plib_registry_path(&root).display().to_string(),
            ])
            .status()
            .expect("touch sets the seed mtime");
        assert!(touch.success());
        let before_bytes = std::fs::read(plib_registry_path(&root)).unwrap();
        let before_mtime = std::fs::metadata(plib_registry_path(&root))
            .unwrap()
            .modified()
            .unwrap();

        // A package with no preserved-lib activity at all.
        let vdb = root.join("var/db/pkg/dev-libs/plain-1.0");
        std::fs::create_dir_all(&vdb).unwrap();
        std::fs::write(vdb.join("COUNTER"), "3\n").unwrap();
        let preserved = preserve_libs_on_unmerge(
            &root,
            "dev-libs",
            "plain",
            "plain-1.0",
            "0",
            "obj /usr/lib/plain.so abc 1\n",
            false,
        )
        .unwrap();
        assert!(preserved.is_empty());
        let being: BTreeSet<String> = ["/usr/lib/plain.so".to_string()].into_iter().collect();
        let removed = prune_unused_preserved_libs(
            &root,
            true,
            &|p| being.contains(p),
            None,
            &BTreeMap::new(),
            &[],
        )
        .unwrap();
        assert!(
            removed.is_empty(),
            "the still-needed unrelated entry must survive the prune"
        );

        assert_eq!(
            std::fs::read(plib_registry_path(&root)).unwrap(),
            before_bytes,
            "the registry bytes must not change without preserve activity"
        );
        assert_eq!(
            std::fs::metadata(plib_registry_path(&root))
                .unwrap()
                .modified()
                .unwrap(),
            before_mtime,
            "the registry mtime must not change without preserve activity"
        );
    }

    /// S0 for backlog #167(b): an empty registry real *does* write is
    /// exactly `{}` (2 bytes, no trailing newline -- real
    /// `json.dumps({}, indent="\t", sort_keys=True)`).
    #[test]
    fn plib_registry_empty_serializes_to_exactly_empty_braces() {
        let tmp = tempdir();
        // A loaded non-empty registry whose entries were all
        // unregistered: snapshot `orig_entries` the way real `load()`
        // snapshots `_data_orig`, so the now-empty `entries` compare
        // changed and real `store()` rewrites.
        let mut registry = PlibRegistry {
            entries: BTreeMap::new(),
            orig_entries: BTreeMap::new(),
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
        registry.orig_entries = registry.entries.clone();
        register_preserved_libs(
            &mut registry,
            "dev-libs/foo-1.0",
            "dev-libs",
            "foo",
            "0",
            "5",
            &[],
        );
        assert!(registry.entries.is_empty());
        write_plib_registry(&tmp, &registry).expect("write succeeds");
        let bytes = std::fs::read(plib_registry_path(&tmp)).unwrap();
        assert_eq!(
            bytes, b"{}",
            "an empty registry is exactly `{{}}`, no trailing newline"
        );
    }

    /// S0 for backlog #167(c): a non-empty registry is serialized
    /// byte-identical to real `json.dumps(data, ensure_ascii=False,
    /// indent="\t", sort_keys=True)` (expected bytes below generated with
    /// `python3 -c 'import json; print(repr(json.dumps({"b:0":
    /// ("x-1.0", "1", []), "a:0": ("y-2.0", "3",
    /// ["/usr/lib/lib\u00e9.so.1", "a\"b\\\\c"])}, ensure_ascii=False,
    /// indent="\t", sort_keys=True)))'`).
    #[test]
    fn plib_registry_serialization_is_byte_identical_to_python_json_dumps() {
        let tmp = tempdir();
        let mut entries = BTreeMap::new();
        entries.insert(
            "b:0".to_string(),
            ("x-1.0".to_string(), "1".to_string(), Vec::new()),
        );
        entries.insert(
            "a:0".to_string(),
            (
                "y-2.0".to_string(),
                "3".to_string(),
                vec!["/usr/lib/lib\u{e9}.so.1".to_string(), "a\"b\\c".to_string()],
            ),
        );
        write_plib_registry(
            &tmp,
            &PlibRegistry {
                entries,
                orig_entries: BTreeMap::new(),
            },
        )
        .expect("write succeeds");
        let bytes = std::fs::read(plib_registry_path(&tmp)).unwrap();
        let expected = "{\n\t\"a:0\": [\n\t\t\"y-2.0\",\n\t\t\"3\",\n\t\t[\n\t\t\t\"/usr/lib/lib\u{e9}.so.1\",\n\t\t\t\"a\\\"b\\\\c\"\n\t\t]\n\t],\n\t\"b:0\": [\n\t\t\"x-1.0\",\n\t\t\"1\",\n\t\t[]\n\t]\n}";
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            expected,
            "registry bytes must match Python json.dumps exactly (no trailing newline)"
        );
    }

    /// S0 for backlog #167(e): real `load()` calls `pruneNonExisting()`
    /// (`PreservedLibsRegistry.py:96-97,180-219`), which rebuilds each
    /// entry's paths as regular files first (in stored order), then
    /// symlinks whose target is one of those files -- so after a reload
    /// the hardlink `.so.1.0.0` comes before the soname symlink `.so.1`
    /// even though `register()` stored `sorted()` order, and gone paths
    /// are dropped.
    #[test]
    fn plib_registry_reload_orders_regular_files_before_symlinks() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/lib")).unwrap();
        std::fs::write(root.join("usr/lib/libl32soname.so.1.0.0"), b"fake elf").unwrap();
        std::os::unix::fs::symlink(
            "libl32soname.so.1.0.0",
            root.join("usr/lib/libl32soname.so.1"),
        )
        .unwrap();
        let seed = "{\n\t\"l32/sonamelib:0\": [\n\t\t\"l32/sonamelib-1.0\",\n\t\t\"1\",\n\t\t[\n\t\t\t\"/usr/lib/libl32soname.so.1\",\n\t\t\t\"/usr/lib/libl32soname.so.1.0.0\",\n\t\t\t\"/usr/lib/libl32soname.so.gone\"\n\t\t]\n\t]\n}";
        std::fs::create_dir_all(plib_registry_path(&root).parent().unwrap()).unwrap();
        std::fs::write(plib_registry_path(&root), seed).unwrap();

        let registry = read_plib_registry(&root);
        let (_, _, paths) = registry
            .entries
            .get("l32/sonamelib:0")
            .expect("the entry survives reload");
        assert_eq!(
            paths,
            &vec![
                "/usr/lib/libl32soname.so.1.0.0".to_string(),
                "/usr/lib/libl32soname.so.1".to_string(),
            ],
            "regular files first, then symlinks; gone paths dropped"
        );
    }

    fn versioned_fixture(name: &str, version: &str) -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs")
            .join(name)
            .join(format!("{name}-{version}.ebuild"))
    }

    /// Backlog #178 (S0 oracle + S2): two consecutive soname bumps
    /// (`sonamebumplib` 1.0 -> 2.0 -> 3.0, each dropping the previous
    /// soname while `consumesonamebump` still links `.so.1`).
    /// Real `PreservedLibsRegistry.register(cpv, slot, counter, paths)`
    /// (`3rdparty/portage/lib/portage/util/_dyn_libs/
    /// PreservedLibsRegistry.py:142-169`) keyed `cp:slot` **replaces**
    /// the record with the merging package's `(cpv, counter, paths)` --
    /// real `dblink.treewalk()` calls it as `register(self.mycpv, slot,
    /// counter, sorted(preserve_paths))` (`lib/portage/dbapi/vartree.py:
    /// 5266-5272`), after copying the preserved entries into the new
    /// package's own `CONTENTS` (`_add_preserve_libs_to_contents`,
    /// `:3775-3826`). The S0 oracle (one `podman run ...
    /// localhost/test-portuale:latest` probe over the l32 C2 cell shape
    /// plus a `sonamelib-3.0` ebuild, real portage 3.0.82.2) holds after
    /// each step: `{"l32/sonamelib:0": ["l32/sonamelib-<N>.0",
    /// "<counter>", ["/usr/lib64/libl32soname.so.1.0.0",
    /// "/usr/lib64/libl32soname.so.1"]]}`, the `.so.2` files gone from
    /// disk, and the new package's `CONTENTS` listing the preserved
    /// `.so.1` entries with their original digest/mtime (see
    /// `differential-test-bed/findings/l5.md` "## Group 2").
    #[test]
    fn plib_registry_two_consecutive_soname_bumps_replace_the_record_under_the_merging_cpv() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        // A real Gentoo ROOT's own `/etc/ld.so.conf` lists `/usr/lib64`
        // (container-verified on the S0 probe image), so real
        // `getlibpaths()`'s own `defpath` covers the libdir and real
        // `findConsumers` matches the consumer. Portuale regenerates
        // `etc/ld.so.conf` from env.d `LDPATH` on every merge
        // (`env_update::run_env_update`), so seed that input the way a
        // real toolchain env.d entry does -- otherwise this synthetic
        // root's `defpath` is just `/usr/lib` + `/lib` and no
        // `/usr/lib64` consumer is ever found, on either implementation.
        std::fs::create_dir_all(root.join("etc/env.d")).unwrap();
        std::fs::write(
            root.join("etc/env.d/99sonamebump"),
            "LDPATH=\"/usr/lib64\"\n",
        )
        .unwrap();

        for (name, version) in [
            ("sonamebumplib", "1.0"),
            ("consumesonamebump", "1.0"),
            ("sonamebumplib", "2.0"),
        ] {
            let status = run_merge(
                &versioned_fixture(name, version),
                &root,
                &portage_tmpdir,
                &MergeOptions::default(),
                None,
            )
            .expect("run_merge succeeds");
            assert_eq!(status, 0, "{name}-{version} merges cleanly");
        }

        // Bump 1: the record is owned by the merging cpv with its own
        // vdb COUNTER, paths file-then-symlink (real `pruneNonExisting`
        // order), byte-exact.
        let counter_2 =
            std::fs::read_to_string(root.join("var/db/pkg/dev-libs/sonamebumplib-2.0/COUNTER"))
                .unwrap();
        let expected_bump1 = format!(
            "{{\n\t\"dev-libs/sonamebumplib:0\": [\n\t\t\"dev-libs/sonamebumplib-2.0\",\n\t\t\"{}\",\n\t\t[\n\t\t\t\"/usr/lib64/libsonamebump.so.1.0.0\",\n\t\t\t\"/usr/lib64/libsonamebump.so.1\"\n\t\t]\n\t]\n}}",
            counter_2.trim(),
        );
        assert_eq!(
            std::fs::read_to_string(plib_registry_path(&root)).unwrap(),
            expected_bump1,
            "after bump 1 the registry must match the S0 oracle shape"
        );
        let contents_2 =
            std::fs::read_to_string(root.join("var/db/pkg/dev-libs/sonamebumplib-2.0/CONTENTS"))
                .unwrap();
        assert!(
            contents_2.contains("obj /usr/lib64/libsonamebump.so.1.0.0"),
            "the new package owns the preserved hardlink: {contents_2}"
        );
        assert!(
            contents_2.contains("sym /usr/lib64/libsonamebump.so.1 "),
            "the new package owns the preserved soname symlink: {contents_2}"
        );

        let status = run_merge(
            &versioned_fixture("sonamebumplib", "3.0"),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
            None,
        )
        .expect("run_merge succeeds");
        assert_eq!(status, 0, "sonamebumplib-3.0 merges cleanly");

        // Bump 2: the SAME key is replaced -- owner flips to the new
        // merging cpv/counter while the still-needed `.so.1` path list
        // survives verbatim (real drops only the unneeded `.so.2` pair,
        // which nobody links).
        let counter_3 =
            std::fs::read_to_string(root.join("var/db/pkg/dev-libs/sonamebumplib-3.0/COUNTER"))
                .unwrap();
        let expected_bump2 = format!(
            "{{\n\t\"dev-libs/sonamebumplib:0\": [\n\t\t\"dev-libs/sonamebumplib-3.0\",\n\t\t\"{}\",\n\t\t[\n\t\t\t\"/usr/lib64/libsonamebump.so.1.0.0\",\n\t\t\t\"/usr/lib64/libsonamebump.so.1\"\n\t\t]\n\t]\n}}",
            counter_3.trim(),
        );
        assert_eq!(
            std::fs::read_to_string(plib_registry_path(&root)).unwrap(),
            expected_bump2,
            "after bump 2 the registry must match the S0 oracle shape"
        );
        assert!(
            root.join("usr/lib64/libsonamebump.so.1.0.0").is_file(),
            "the still-needed hardlink survives the second bump"
        );
        assert!(
            !root.join("usr/lib64/libsonamebump.so.2.0.0").exists(),
            "the unneeded .so.2 hardlink is unmerged at the second bump"
        );
        assert!(
            !root.join("usr/lib64/libsonamebump.so.2").exists(),
            "the unneeded .so.2 symlink is unmerged at the second bump"
        );
        let contents_3 =
            std::fs::read_to_string(root.join("var/db/pkg/dev-libs/sonamebumplib-3.0/CONTENTS"))
                .unwrap();
        assert!(
            contents_3.contains("obj /usr/lib64/libsonamebump.so.1.0.0"),
            "the newest package owns the preserved hardlink: {contents_3}"
        );
        assert!(
            !contents_3.contains("libsonamebump.so.2"),
            "the newest package must not claim the dropped soname: {contents_3}"
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
            orig_entries: BTreeMap::new(),
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
            orig_entries: BTreeMap::new(),
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

    /// Real `_prune_plib_registry`'s own unregister shape for a package
    /// that owns no files at all (empty `CONTENTS`): real
    /// `instance_owns_files` gates only the linkmap rebuild and the
    /// preserve/prune scans -- `unregister()` still runs. So portuale's
    /// own `preserve_libs_on_unmerge` returns an empty preserved set but
    /// still drops a matching stale entry; with no entry at all nothing
    /// is written (write-only-on-change).
    #[test]
    fn preserve_libs_on_unmerge_with_empty_contents_still_unregisters() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let vdb = root.join("var/db/pkg/dev-libs/foo-1.0");
        std::fs::create_dir_all(&vdb).unwrap();
        std::fs::write(vdb.join("COUNTER"), "5\n").unwrap();
        // The stale path exists, so the load-time `pruneNonExisting`
        // keeps the entry and only the `unregister()` below removes it.
        std::fs::create_dir_all(root.join("usr/lib")).unwrap();
        std::fs::write(root.join("usr/lib/stale.so"), b"fake elf").unwrap();
        let mut entries = BTreeMap::new();
        entries.insert(
            "dev-libs/foo:0".to_string(),
            (
                "dev-libs/foo-1.0".to_string(),
                "5".to_string(),
                vec!["/usr/lib/stale.so".to_string()],
            ),
        );
        write_plib_registry(
            &root,
            &PlibRegistry {
                entries,
                orig_entries: BTreeMap::new(),
            },
        )
        .expect("seeding the registry succeeds");

        let preserved =
            preserve_libs_on_unmerge(&root, "dev-libs", "foo", "foo-1.0", "0", "", false).unwrap();
        assert!(preserved.is_empty());
        assert_eq!(
            std::fs::read_to_string(plib_registry_path(&root)).unwrap(),
            "{}",
            "the matching stale entry must be unregistered even with empty CONTENTS"
        );

        // No entry at all: nothing to unregister, nothing written.
        let tmp2 = tempdir();
        let preserved =
            preserve_libs_on_unmerge(&tmp2, "dev-libs", "foo", "foo-1.0", "0", "", false).unwrap();
        assert!(preserved.is_empty());
        assert!(!plib_registry_path(&tmp2).exists());
    }

    /// Seeds a synthetic root where preserved lib `/usr/lib/keepA.so`
    /// (soname `libkeep.so.1`, indexed by `provider-1.0`'s hand-written
    /// `NEEDED.ELF.2`, registered under `dev-libs/provider:0`) has
    /// exactly one consumer, `/usr/bin/cprog`, owned by `oldapp-1.0`'s
    /// `CONTENTS`. Returns the root and the `being_unmerged` set an
    /// unmerge of the old instance would pass.
    fn seed_old_owned_consumer_root(tmp: &std::path::Path) -> (PathBuf, BTreeSet<String>) {
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/lib")).unwrap();
        std::fs::create_dir_all(root.join("usr/bin")).unwrap();
        std::fs::write(root.join("usr/lib/keepA.so"), b"fake elf").unwrap();
        std::fs::write(root.join("usr/bin/cprog"), b"fake elf").unwrap();
        for (pf, contents, needed) in [
            (
                "provider-1.0",
                "obj /usr/lib/keepA.so def 1\n",
                "X86_64;/usr/lib/keepA.so;libkeep.so.1;;\n",
            ),
            (
                "oldapp-1.0",
                "obj /usr/bin/cprog abc 1\n",
                "X86_64;/usr/bin/cprog;;;libkeep.so.1\n",
            ),
        ] {
            let vdb = root.join("var/db/pkg/dev-libs").join(pf);
            std::fs::create_dir_all(&vdb).unwrap();
            std::fs::write(vdb.join("CONTENTS"), contents).unwrap();
            std::fs::write(vdb.join("NEEDED.ELF.2"), needed).unwrap();
        }
        std::fs::write(root.join("var/db/pkg/dev-libs/oldapp-1.0/COUNTER"), "7\n").unwrap();
        let mut entries = BTreeMap::new();
        entries.insert(
            "dev-libs/provider:0".to_string(),
            (
                "dev-libs/provider-1.0".to_string(),
                "4".to_string(),
                vec!["/usr/lib/keepA.so".to_string()],
            ),
        );
        write_plib_registry(
            &root,
            &PlibRegistry {
                entries,
                orig_entries: BTreeMap::new(),
            },
        )
        .expect("seeding the registry succeeds");
        let being: BTreeSet<String> = ["/usr/bin/cprog".to_string()].into_iter().collect();
        (root, being)
    }

    /// Fix round 1 for review item 3 (backlog #178): real
    /// `unmerge_no_replacement = unmerge and not
    /// unmerge_with_replacement` (`dblink._prune_plib_registry`) -- in
    /// replacement mode a consumer owned by the old instance still
    /// keeps a preserved lib alive (it survives through the replacing
    /// package). Pre-fix, portuale always passed `true`, eliminating
    /// those consumers and wrongly pruning the lib.
    #[test]
    fn prune_keeps_a_preserved_lib_whose_only_consumer_is_old_owned_in_replacement_mode() {
        let tmp = tempdir();
        let (root, being) = seed_old_owned_consumer_root(&tmp);

        let unneeded = find_unused_preserved_libs(
            &root,
            true,
            &|p| being.contains(p),
            None,
            &BTreeMap::new(),
            &[],
        );
        assert_eq!(
            unneeded,
            BTreeMap::from([(
                "dev-libs/provider-1.0".to_string(),
                BTreeSet::from(["/usr/lib/keepA.so".to_string()]),
            )]),
            "standalone mode eliminates the about-to-be-unmerged consumer"
        );

        let unneeded = find_unused_preserved_libs(
            &root,
            false,
            &|p| being.contains(p),
            None,
            &BTreeMap::new(),
            &[],
        );
        assert!(
            unneeded.is_empty(),
            "replacement mode keeps the old-owned consumer, so the lib is needed: {unneeded:?}"
        );
    }

    /// Fix round 1 for review item 3, threading half: the replace loop
    /// (`unmerge_pkgfiles` with `is_replacement`) must reach the prune
    /// with `unmerge_no_replacement=false` and the old instance
    /// excluded, so the still-needed lib survives a real replacement
    /// unmerge on disk and in the registry. The replacing instance owns
    /// the consumer too (its `CONTENTS` + `NEEDED.ELF.2` are live --
    /// real `include_file`), which is what keeps the consumer visible
    /// past the old-instance exclusion.
    #[test]
    fn replacement_unmerge_keeps_a_preserved_lib_needed_by_the_replacing_consumer() {
        let tmp = tempdir();
        let (root, _) = seed_old_owned_consumer_root(&tmp);
        let vdb_new = root.join("var/db/pkg/dev-libs/newapp-1.0");
        std::fs::create_dir_all(&vdb_new).unwrap();
        std::fs::write(vdb_new.join("CONTENTS"), "obj /usr/bin/cprog abc 1\n").unwrap();
        std::fs::write(
            vdb_new.join("NEEDED.ELF.2"),
            "X86_64;/usr/bin/cprog;;;libkeep.so.1\n",
        )
        .unwrap();

        crate::ebuild_unmerge::unmerge_pkgfiles(
            &root,
            "dev-libs",
            "oldapp",
            "oldapp-1.0",
            &["newapp-1.0".to_string()],
            &crate::ebuild_unmerge::UnmergeOptions::default(),
            true,
            &BTreeMap::new(),
            &[],
        )
        .expect("replacement unmerge succeeds");

        assert!(
            root.join("usr/lib/keepA.so").is_file(),
            "the still-needed preserved lib must survive the replacement unmerge"
        );
        assert!(
            root.join("usr/bin/cprog").is_file(),
            "the consumer owned by the replacing package must survive"
        );
        let registry = read_plib_registry(&root);
        assert_eq!(
            registry.entries.get("dev-libs/provider:0"),
            Some(&(
                "dev-libs/provider-1.0".to_string(),
                "4".to_string(),
                vec!["/usr/lib/keepA.so".to_string()],
            )),
            "the registry entry must survive the replacement unmerge"
        );
    }

    /// Backlog #224: real `dblink.unmerge(needed=...)` feeds the
    /// replacing package's `NEEDED.ELF.2` lines into the replace-loop
    /// prune explicitly (`_prune_plib_registry` ->
    /// `_linkmap_rebuild(include_file=needed, ...)`), because the new
    /// entry still sits in its `-MERGING-<pf>` temporary and
    /// enumeration (`cpv_all()`) skips it. A preserved library whose
    /// only remaining consumer is the replacing package itself must
    /// therefore survive the replace-loop prune; without the feed it
    /// looks orphaned and is deleted + unregistered.
    ///
    /// Same seed shape as
    /// `replacement_unmerge_keeps_a_preserved_lib_needed_by_the_replacing_consumer`
    /// (hand-written `NEEDED.ELF.2` lines, per the brief's "or a
    /// hand-written NEEDED line"), except the replacing entry is
    /// `-MERGING-newapp-1.0` -- the real replace-loop state since #183,
    /// invisible to `read_all_needed_entries` -- and the call goes
    /// through the replace loop itself (`unmerge_replaced_same_slot`,
    /// whose signature is unchanged: it reads the feed from that
    /// temporary entry), so this test compiles and fails without the
    /// fix and passes with it.
    #[test]
    fn replace_loop_prune_sees_the_replacing_packages_merging_needed() {
        let tmp = tempdir();
        let (root, _) = seed_old_owned_consumer_root(&tmp);
        // The replacing package's vdb entry, exactly as the replace
        // loop sees it: still `-MERGING-`, so only the explicit
        // include feed (never enumeration) can show its consumer to
        // the prune. The `CONTENTS` entry is what the `also_keep`
        // ownership check sees (via the `-MERGING-` fallback).
        let vdb_new = root.join("var/db/pkg/dev-libs/-MERGING-newapp-1.0");
        std::fs::create_dir_all(&vdb_new).unwrap();
        std::fs::write(vdb_new.join("CONTENTS"), "obj /usr/bin/cprog abc 1\n").unwrap();
        std::fs::write(
            vdb_new.join("NEEDED.ELF.2"),
            "X86_64;/usr/bin/cprog;;;libkeep.so.1\n",
        )
        .unwrap();
        std::fs::write(root.join("var/db/pkg/dev-libs/oldapp-1.0/SLOT"), "0\n").unwrap();

        let scratch = tmp.join("scratch");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();
        let replaced = unmerge_replaced_same_slot(
            &root,
            "dev-libs",
            "oldapp",
            "newapp-1.0",
            "0",
            &scratch,
            &portage_tmpdir,
            &MergeOptions::default(),
            &BTreeMap::new(),
        )
        .expect("the replace loop succeeds");
        assert_eq!(
            replaced,
            vec!["oldapp-1.0".to_string()],
            "the loop must have unmerged the old instance"
        );

        assert!(
            root.join("usr/lib/keepA.so").is_file(),
            "the preserved lib consumed only by the replacing package must survive the replace-loop prune"
        );
        assert!(
            root.join("usr/bin/cprog").is_file(),
            "the consumer owned by the replacing package must survive"
        );
        let registry = read_plib_registry(&root);
        assert_eq!(
            registry.entries.get("dev-libs/provider:0"),
            Some(&(
                "dev-libs/provider-1.0".to_string(),
                "4".to_string(),
                vec!["/usr/lib/keepA.so".to_string()],
            )),
            "the registry entry must survive the replace-loop prune"
        );
        assert!(
            vdb_new.join("NEEDED.ELF.2").is_file(),
            "the replace loop must leave the `-MERGING-` entry alone"
        );
    }

    /// Backlog #229 (merge-side half of #224): real `dblink.treewalk`
    /// feeds the replacing package's build-info `NEEDED.ELF.2` into the
    /// merge-side preserve computation explicitly
    /// (`self._linkmap_rebuild(include_file=needed)` before
    /// `_find_libs_to_preserve()`), because the new entry still sits in
    /// its `-MERGING-<pf>` temporary and enumeration (`cpv_all()`)
    /// skips it. A library owned by the replaced instance whose only
    /// consumer is the replacing package itself must therefore be
    /// preserved merge-side; without the feed the consumer is invisible
    /// and the lib is never preserved (missed-preserve, so the file is
    /// unmerged with the old instance).
    ///
    /// Same hand-written-`NEEDED.ELF.2` seed shape as the #224 test,
    /// except the provider is the replaced same-slot instance itself
    /// (`oldapp-1.0`, live) and the only consumer (`/usr/bin/newtool`,
    /// shipped by the new image) is known solely to the
    /// `-MERGING-oldapp-2.0` temporary -- the real merge-side state
    /// since #183 (`populate_vdb_tmp` runs before the preserve block).
    /// The call goes through `find_preserve_paths_for_merge` itself
    /// (new `new_pf` parameter), so this test compiles and fails
    /// without the fix and passes with it.
    #[test]
    fn merge_side_preserve_sees_the_replacing_packages_merging_needed() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/lib")).unwrap();
        std::fs::create_dir_all(root.join("usr/bin")).unwrap();
        std::fs::write(root.join("usr/lib/libold.so.1"), b"fake elf").unwrap();
        std::fs::write(root.join("usr/bin/newtool"), b"fake elf").unwrap();
        // The replaced same-slot instance, live: owns the library.
        let vdb_old = root.join("var/db/pkg/dev-libs/oldapp-1.0");
        std::fs::create_dir_all(&vdb_old).unwrap();
        std::fs::write(vdb_old.join("CONTENTS"), "obj /usr/lib/libold.so.1 def 1\n").unwrap();
        std::fs::write(
            vdb_old.join("NEEDED.ELF.2"),
            "X86_64;/usr/lib/libold.so.1;libold.so.1;;\n",
        )
        .unwrap();
        std::fs::write(vdb_old.join("SLOT"), "0\n").unwrap();
        std::fs::write(vdb_old.join("COUNTER"), "7\n").unwrap();
        // The replacing package, exactly as the merge-side preserve
        // block sees it: still `-MERGING-`, so only the explicit
        // include feed (never enumeration) can show its consumer to
        // the preserve computation.
        let vdb_new = root.join("var/db/pkg/dev-libs/-MERGING-oldapp-2.0");
        std::fs::create_dir_all(&vdb_new).unwrap();
        std::fs::write(
            vdb_new.join("NEEDED.ELF.2"),
            "X86_64;/usr/bin/newtool;;;libold.so.1\n",
        )
        .unwrap();
        // The new image ships the consumer but not the library.
        let new_image_paths: BTreeSet<String> =
            ["/usr/bin/newtool".to_string()].into_iter().collect();

        let (paths, _) = find_preserve_paths_for_merge(
            &root,
            "dev-libs",
            "oldapp",
            "oldapp-2.0",
            "0",
            &new_image_paths,
        )
        .expect("a same-slot instance owning files preserves");
        assert!(
            paths.contains("/usr/lib/libold.so.1"),
            "the old-owned lib consumed only by the replacing package must be preserved: {paths:?}"
        );
    }

    /// Fix round 1 for review item 4, `exclude_pkgs` half (backlog
    /// #178): real `LinkageMap.rebuild(exclude_pkgs=...)` drops the
    /// unmerged instance's `NEEDED.ELF.2` lines, so a consumer only the
    /// old instance knows about cannot keep a preserved lib alive past
    /// the replacement. Without the exclusion the stale consumer data
    /// "would only serve to corrupt the `LinkageMap`".
    #[test]
    fn prune_excludes_the_replaced_instance_linkage_data() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/lib")).unwrap();
        std::fs::create_dir_all(root.join("usr/bin")).unwrap();
        std::fs::write(root.join("usr/lib/staleD.so"), b"fake elf").unwrap();
        std::fs::write(root.join("usr/bin/oldtool"), b"fake elf").unwrap();
        // The library is indexed (its provider is still installed); its
        // only consumer is known solely to the instance being replaced.
        let vdb_prov = root.join("var/db/pkg/dev-libs/provider-1.0");
        std::fs::create_dir_all(&vdb_prov).unwrap();
        std::fs::write(vdb_prov.join("CONTENTS"), "obj /usr/lib/staleD.so def 1\n").unwrap();
        std::fs::write(
            vdb_prov.join("NEEDED.ELF.2"),
            "X86_64;/usr/lib/staleD.so;libstale.so.1;;\n",
        )
        .unwrap();
        let vdb_old = root.join("var/db/pkg/dev-libs/oldapp-1.0");
        std::fs::create_dir_all(&vdb_old).unwrap();
        std::fs::write(vdb_old.join("CONTENTS"), "obj /usr/bin/oldtool abc 1\n").unwrap();
        std::fs::write(
            vdb_old.join("NEEDED.ELF.2"),
            "X86_64;/usr/bin/oldtool;;;libstale.so.1\n",
        )
        .unwrap();
        let mut entries = BTreeMap::new();
        entries.insert(
            "dev-libs/provider:0".to_string(),
            (
                "dev-libs/provider-1.0".to_string(),
                "4".to_string(),
                vec!["/usr/lib/staleD.so".to_string()],
            ),
        );
        write_plib_registry(
            &root,
            &PlibRegistry {
                entries,
                orig_entries: BTreeMap::new(),
            },
        )
        .expect("seeding the registry succeeds");

        let unneeded = find_unused_preserved_libs(
            &root,
            false,
            &|_| false,
            Some("dev-libs/oldapp-1.0"),
            &BTreeMap::new(),
            &[],
        );
        assert_eq!(
            unneeded,
            BTreeMap::from([(
                "dev-libs/provider-1.0".to_string(),
                BTreeSet::from(["/usr/lib/staleD.so".to_string()]),
            )]),
            "with the replaced instance excluded nothing consumes the lib"
        );

        let unneeded =
            find_unused_preserved_libs(&root, false, &|_| false, None, &BTreeMap::new(), &[]);
        assert!(
            unneeded.is_empty(),
            "without the exclusion the stale consumer keeps it (the corruption real excludes): {unneeded:?}"
        );
    }

    /// Compiles a real `libA` (soname `libA.so.1`) and a real `libB`
    /// (`DT_NEEDED libA.so.1`, soname `libB.so.1`) with the host `cc`
    /// and installs both under the synthetic root's `/usr/lib` --
    /// the same `gcc -shared -fPIC -Wl,-soname` shape the
    /// `sonamebumplib` fixtures use, so `scanelf` (hence the orphan
    /// branch under test) sees genuine ELF headers.
    fn compile_linked_pair(tmp: &std::path::Path, root: &std::path::Path) {
        let build = tmp.join("build");
        std::fs::create_dir_all(&build).unwrap();
        std::fs::write(build.join("a.c"), "int aval(void) { return 1; }\n").unwrap();
        std::fs::write(
            build.join("b.c"),
            "extern int aval(void);\nint bval(void) { return aval(); }\n",
        )
        .unwrap();
        let run = |args: &[&str]| {
            let status = std::process::Command::new("cc")
                .args(args)
                .current_dir(&build)
                .status()
                .expect("host cc must exist for the orphan-scan test");
            assert!(status.success(), "cc {args:?} must succeed");
        };
        run(&[
            "-shared",
            "-fPIC",
            "-Wl,-soname,libA.so.1",
            "-o",
            "libA.so.1.0.0",
            "a.c",
        ]);
        std::os::unix::fs::symlink("libA.so.1.0.0", build.join("libA.so")).unwrap();
        run(&[
            "-shared",
            "-fPIC",
            "-Wl,-soname,libB.so.1",
            "-o",
            "libB.so.1.0.0",
            "b.c",
            "-L.",
            "-lA",
        ]);
        std::fs::create_dir_all(root.join("usr/lib")).unwrap();
        std::fs::copy(
            build.join("libA.so.1.0.0"),
            root.join("usr/lib/libA.so.1.0.0"),
        )
        .unwrap();
        std::fs::copy(
            build.join("libB.so.1.0.0"),
            root.join("usr/lib/libB.so.1.0.0"),
        )
        .unwrap();
    }

    /// Fix round 1 for review item 4, `preserve_paths` half (backlog
    /// #178): real `LinkageMap.rebuild(preserve_paths=...)` scans the
    /// merge-side just-preserved set, so a preserved library whose only
    /// linkmap consumer is a just-preserved file -- indexed in no
    /// `NEEDED.ELF.2` anywhere -- is kept, not deleted as unneeded.
    /// Grounded in real `LinkageMap.rebuild` (the `preserve_paths`
    /// parameter): without the feed the consumer is invisible to the
    /// prune.
    #[test]
    fn prune_sees_just_preserved_files_missing_from_every_needed_elf2() {
        let tmp = tempdir();
        let root = tmp.join("root");
        compile_linked_pair(&tmp, &root);
        // `libA` is indexed (hand-written provider entry, the way any
        // still-installed owner's `NEEDED.ELF.2` would); `libB` appears
        // in no `NEEDED.ELF.2` -- only the `preserve_paths` feed (real
        // `LinkageMap.rebuild(preserve_paths=...)`) can show it to the
        // prune.
        let vdb_prov = root.join("var/db/pkg/dev-libs/provider-1.0");
        std::fs::create_dir_all(&vdb_prov).unwrap();
        std::fs::write(
            vdb_prov.join("CONTENTS"),
            "obj /usr/lib/libA.so.1.0.0 def 1\n",
        )
        .unwrap();
        std::fs::write(
            vdb_prov.join("NEEDED.ELF.2"),
            "X86_64;/usr/lib/libA.so.1.0.0;libA.so.1;;\n",
        )
        .unwrap();
        let mut entries = BTreeMap::new();
        entries.insert(
            "dev-libs/provider:0".to_string(),
            (
                "dev-libs/provider-1.0".to_string(),
                "4".to_string(),
                vec!["/usr/lib/libA.so.1.0.0".to_string()],
            ),
        );
        write_plib_registry(
            &root,
            &PlibRegistry {
                entries,
                orig_entries: BTreeMap::new(),
            },
        )
        .expect("seeding the registry succeeds");

        let unneeded =
            find_unused_preserved_libs(&root, false, &|_| false, None, &BTreeMap::new(), &[]);
        assert_eq!(
            unneeded,
            BTreeMap::from([(
                "dev-libs/provider-1.0".to_string(),
                BTreeSet::from(["/usr/lib/libA.so.1.0.0".to_string()]),
            )]),
            "without the just-preserved feed the consumer is invisible"
        );

        let feed: BTreeMap<String, Vec<String>> = BTreeMap::from([(
            "dev-libs/newapp-2.0".to_string(),
            vec!["/usr/lib/libB.so.1.0.0".to_string()],
        )]);
        let unneeded = find_unused_preserved_libs(&root, false, &|_| false, None, &feed, &[]);
        assert!(
            unneeded.is_empty(),
            "the just-preserved consumer keeps the lib alive: {unneeded:?}"
        );
        assert!(
            root.join("usr/lib/libA.so.1.0.0").is_file(),
            "nothing was deleted by the read-only probe above"
        );
    }

    /// Sanity baseline (portuale's own "fixtures must actually
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
    /// `preserved_libs_registry` (hand-seeded here -- portuale has no
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
        write_plib_registry(
            &root,
            &PlibRegistry {
                entries,
                orig_entries: BTreeMap::new(),
            },
        )
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
    /// portuale's own vdb entry is still removed as normal either way.
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
        // SAFETY: `c_path` is a CString that outlives the `mkfifo`
        // call; the call itself takes only the NUL-terminated pointer
        // plus a plain mode int.
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
    /// portuale's own unprivileged dev/test user. (A privilege-free
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
    /// portuale's own real subprocess invocation, the same "prove it with a
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
    /// `build-info` (see `populate_vdb_tmp`'s own doc comment for why
    /// portuale copies only this one build-info file, not the whole
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
        // #39: the install-phase `_post_src_install_soname_symlinks`
        // rewrite adds real's 6th multilib-category field, so the vdb
        // copy carries six `;`-separated fields.
        assert_eq!(entry.multilib_category.as_deref(), Some("x86_64"));
        assert!(
            needed.lines().all(|l| l.split(';').count() >= 6),
            "every rewritten line must carry the category field: {needed}"
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
    /// portuale's own `preserve_libs_on_unmerge` (see its own doc comment
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

        // Real `_prune_plib_registry()`'s tail: once the last consumer is
        // gone too, the preserved library is orphaned and the next
        // merge/unmerge deletes it and clears the registry (here: the
        // consumer's own unmerge does it, `unmerge_no_replacement=True`).
        let consumer_unmerge = crate::ebuild_unmerge::run_unmerge(
            &consumer_ebuild,
            &root,
            &portage_tmpdir,
            &crate::ebuild_unmerge::UnmergeOptions::default(),
        )
        .expect("run_unmerge (consumer) succeeds");
        assert_eq!(consumer_unmerge, 0);

        assert!(
            !lib_path.exists(),
            "the preserved library must be removed once nothing links it any more"
        );
        assert!(
            read_plib_registry(&root).entries.is_empty(),
            "the registry must be empty after the last consumer is unmerged"
        );
    }

    /// Real `_find_unneeded_preserved_nodes` cycle handling (bug 652382):
    /// two preserved libraries that only consume each other, with nothing
    /// outside the pair consuming either, are both unneeded.
    #[test]
    fn find_unneeded_preserved_drops_a_self_contained_cycle() {
        let tmp = tempdir();
        let root = tmp.join("root");
        for p in ["usr/lib/liba.so.1", "usr/lib/libb.so.1"] {
            std::fs::create_dir_all(root.join("usr/lib")).unwrap();
            std::fs::write(root.join(p), b"x").unwrap();
        }
        let preserved: BTreeSet<String> = ["/usr/lib/liba.so.1", "/usr/lib/libb.so.1"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        edges.insert(
            "/usr/lib/liba.so.1".into(),
            BTreeSet::from(["/usr/lib/libb.so.1".to_string()]),
        );
        edges.insert(
            "/usr/lib/libb.so.1".into(),
            BTreeSet::from(["/usr/lib/liba.so.1".to_string()]),
        );
        let unneeded = crate::needed_elf::find_unneeded_preserved(&root, &edges, &preserved);
        assert_eq!(unneeded, preserved);

        // But an outside (non-preserved) consumer of one keeps that one --
        // and, transitively, the other it depends on.
        std::fs::create_dir_all(root.join("usr/bin")).unwrap();
        std::fs::write(root.join("usr/bin/app"), b"x").unwrap();
        edges
            .get_mut("/usr/lib/liba.so.1")
            .unwrap()
            .insert("/usr/bin/app".to_string());
        let unneeded = crate::needed_elf::find_unneeded_preserved(&root, &edges, &preserved);
        assert!(unneeded.is_empty(), "{unneeded:?}");
    }

    fn fixtures_root() -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
    }

    type PublishHook = Box<dyn Fn(&Path)>;

    thread_local! {
        /// Test hook (feat#157 S4.1, S4.2): run inside `publish_merged_entry`
        /// and `RetireWrites::commit` on a database backend, after every call
        /// of the publishing / retiring transaction and right before its
        /// `commit`, on this thread.
        static PUBLISH_HOOK: std::cell::RefCell<Option<PublishHook>> =
            const { std::cell::RefCell::new(None) };
    }

    pub(super) fn publish_hook(root: &Path) {
        PUBLISH_HOOK.with(|hook| {
            if let Some(f) = hook.borrow().as_ref() {
                f(root);
            }
        });
    }

    /// Sanity baseline (portuale's own "fixtures must actually
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

    /// feat#157 S4.1: the merge on a database backend (sqlite).
    #[cfg(any(feature = "vdb-sqlite", feature = "vdb-redb"))]
    mod db_merge {
        use super::*;
        use portage_vdb::{EntryKey, InstalledDb};
        use std::cell::RefCell;
        use std::rc::Rc;
        use std::sync::Arc;

        type Tree = BTreeMap<String, (u32, Vec<u8>)>;

        fn binpkg(name: &str) -> PathBuf {
            fixtures_root().join("pkgdir/dev-libs").join(name)
        }

        /// Convert whatever `files` VDB `root` holds into a new sqlite
        /// database at `db_path` (as `portuale vdb convert` does) and make
        /// it `root`'s backend, as `mrg --vdb-backend=sqlite` does.
        #[cfg(feature = "vdb-sqlite")]
        fn use_sqlite(root: &Path, db_path: &Path) -> Arc<portage_vdb::SqliteDb> {
            let db = Arc::new(portage_vdb::SqliteDb::open(db_path).unwrap());
            portage_vdb::copy_all(&portage_vdb::FilesDb::new(root), db.as_ref(), false).unwrap();
            portage_vdb::register(root, db.clone());
            db
        }

        /// [`use_sqlite`] for any database backend: the same, with `kind`'s
        /// file (`mrg --vdb-backend=<kind>`).
        fn use_db(
            kind: portage_vdb::BackendKind,
            root: &Path,
            db_path: &Path,
        ) -> Arc<dyn InstalledDb> {
            let db: Arc<dyn InstalledDb> = match kind {
                #[cfg(feature = "vdb-sqlite")]
                portage_vdb::BackendKind::Sqlite => {
                    Arc::new(portage_vdb::SqliteDb::open(db_path).unwrap())
                }
                #[cfg(feature = "vdb-redb")]
                portage_vdb::BackendKind::Redb => {
                    Arc::new(portage_vdb::RedbDb::open(db_path).unwrap())
                }
                other => panic!("backend {other} is not built"),
            };
            portage_vdb::copy_all(&portage_vdb::FilesDb::new(root), db.as_ref(), false).unwrap();
            portage_vdb::register(root, db.clone());
            db
        }

        /// Every file of a live entry with its permission bits and bytes;
        /// the volatile `#dir_mtime=` line of `metadata` is dropped (the
        /// same thing `verify` ignores: directory mtimes).
        fn entry_files(db: &dyn InstalledDb, key: &EntryKey) -> Tree {
            let image = db.entry_image(key).unwrap().expect("entry is installed");
            image
                .files
                .into_iter()
                .map(|f| {
                    let data = if f.meta.name == "metadata" {
                        String::from_utf8(f.data)
                            .unwrap()
                            .lines()
                            .filter(|l| !l.starts_with("#dir_mtime="))
                            .map(|l| format!("{l}\n"))
                            .collect::<String>()
                            .into_bytes()
                    } else {
                        f.data
                    };
                    (f.meta.name, (f.meta.mode & 0o7777, data))
                })
                .collect()
        }

        /// Everything under `root` except the stores a backend keeps
        /// (`var/db/pkg`, `var/lib/portage`, `var/cache/edb`) and `etc`
        /// (env-update's generated files, whose modes follow the process
        /// umask; no fixture installs under `etc`): path ->
        /// (mode, bytes or symlink target). Directories carry no bytes.
        fn payload_tree(root: &Path) -> Tree {
            fn walk(root: &Path, dir: &Path, out: &mut Tree) {
                for entry in portage_util::read_dir_entries(dir).unwrap() {
                    let path = entry.path();
                    let rel = path.strip_prefix(root).unwrap().display().to_string();
                    if ["var/db/pkg", "var/lib/portage", "var/cache/edb", "etc"]
                        .contains(&rel.as_str())
                    {
                        continue;
                    }
                    let meta = std::fs::symlink_metadata(&path).unwrap();
                    let data = if meta.file_type().is_symlink() {
                        std::fs::read_link(&path)
                            .unwrap()
                            .display()
                            .to_string()
                            .into_bytes()
                    } else if meta.is_file() {
                        std::fs::read(&path).unwrap()
                    } else {
                        Vec::new()
                    };
                    out.insert(rel, (meta.mode(), data));
                    if meta.is_dir() {
                        walk(root, &path, out);
                    }
                }
            }
            let mut out = Tree::new();
            walk(root, root, &mut out);
            // A directory left with nothing in the tree only held a store
            // (`var/db`, `var/cache` exist on `files` alone).
            // Deepest first, so a parent of a dropped directory goes too.
            let mut keys: Vec<String> = out.keys().cloned().collect();
            keys.sort_by_key(|k| std::cmp::Reverse(k.matches('/').count()));
            for rel in keys {
                let is_dir = out[&rel].0 & 0o170000 == 0o040000;
                let prefix = format!("{rel}/");
                if is_dir && !out.keys().any(|k| k.starts_with(&prefix)) {
                    out.remove(&rel);
                }
            }
            out
        }

        /// [`payload_tree`] without the permission bits: another test's
        /// `umask` change (process-wide) can alter the modes of files a run
        /// creates, which is not what the unmerge tests compare.
        #[cfg(feature = "vdb-sqlite")]
        fn payload_bytes(root: &Path) -> BTreeMap<String, Vec<u8>> {
            payload_tree(root)
                .into_iter()
                .map(|(k, (_, data))| (k, data))
                .collect()
        }

        fn install_hook(f: impl Fn(&Path) + 'static) {
            PUBLISH_HOOK.with(|hook| *hook.borrow_mut() = Some(Box::new(f)));
        }

        fn clear_hook() {
            PUBLISH_HOOK.with(|hook| *hook.borrow_mut() = None);
        }

        /// What one merge leaves: the entry, the payload, the config memory.
        fn outcome(root: &Path, key: &EntryKey) -> (Tree, Tree, portage_vdb::ConfigMemory) {
            let db = portage_vdb::for_root(root);
            (
                entry_files(db.as_ref(), key),
                payload_tree(root),
                db.config_memory().unwrap(),
            )
        }

        /// (a) + (c): a binpkg merged on sqlite stores the same entry
        /// files (bytes and modes; `metadata` without its directory-mtime
        /// stamp) and lands the same payload as the same merge on `files`.
        /// Both runs use the same ROOT and PORTAGE_TMPDIR paths, so the
        /// `pkg_postinst` `PORTAGE_UPDATE_ENV` rewrite of `environment.bz2`
        /// (which records paths) is byte-identical; on sqlite it went
        /// through the scratch copy and `replace_file`, and the stored
        /// bytes differ from the binpkg's (the pending entry's) own.
        fn binpkg_merge_matches_files(kind: portage_vdb::BackendKind) {
            let tmp = tempdir();
            let root = tmp.join("root");
            let ptmp = tmp.join("ptmp");
            let key = EntryKey::new("dev-libs", "binpkgrmpkg-1.0");
            let merge = || {
                std::fs::create_dir_all(&root).unwrap();
                let status = merge_binpkg(
                    &binpkg("binpkgrmpkg-1.0.tbz2"),
                    &root,
                    &ptmp,
                    &MergeOptions::default(),
                )
                .expect("merge succeeds");
                assert_eq!(status, 0);
            };

            merge();
            let on_files = outcome(&root, &key);
            std::fs::remove_dir_all(&root).unwrap();
            let _ = std::fs::remove_dir_all(&ptmp);

            std::fs::create_dir_all(&root).unwrap();
            let db = use_db(kind, &root, &tmp.join("vdb.db"));
            let pending_env: Rc<RefCell<Option<Vec<u8>>>> = Rc::default();
            {
                let db = db.clone();
                let pending_env = pending_env.clone();
                let key = key.clone();
                install_hook(move |_| {
                    *pending_env.borrow_mut() =
                        db.read_pending_file(&key, "environment.bz2").unwrap();
                });
            }
            merge();
            clear_hook();
            let on_sqlite = outcome(&root, &key);

            assert!(
                !root.join("var/db/pkg").exists(),
                "a {kind} merge writes no var/db/pkg"
            );
            assert_eq!(on_sqlite.0, on_files.0, "entry files differ");
            assert_eq!(on_sqlite.1, on_files.1, "payload differs");
            assert_eq!(on_sqlite.2, on_files.2, "config memory differs");
            let pending_env = pending_env.borrow().clone().expect("pending env");
            assert_ne!(
                on_sqlite.0["environment.bz2"].1, pending_env,
                "pkg_postinst's PORTAGE_UPDATE_ENV rewrite reached the database"
            );
            assert!(db.pending_entries().unwrap().is_empty());
            let _ = std::fs::remove_dir_all(&tmp);
        }

        #[cfg(feature = "vdb-sqlite")]
        #[test]
        fn a_binpkg_merged_on_sqlite_matches_the_files_merge() {
            binpkg_merge_matches_files(portage_vdb::BackendKind::Sqlite);
        }

        /// feat#157 S5.4 (c): the same binpkg merge on redb. The fixture
        /// ebuilds (`binpkgrmpkg`) run no phase that calls `has_version` /
        /// `best_version` (grep of the fixture repo finds none), so this
        /// needs no parent pipe (S6.3); a phase that did would reach the
        /// held redb file only through that pipe.
        #[cfg(feature = "vdb-redb")]
        #[test]
        fn a_binpkg_merged_on_redb_matches_the_files_merge() {
            binpkg_merge_matches_files(portage_vdb::BackendKind::Redb);
        }

        /// (b): a same-slot upgrade on sqlite, over a converted VDB. Right
        /// before the publishing commit the old instance is still the
        /// installed one and the new one is pending; after it, only the
        /// new one is installed. The replaced version's `pkg_prerm` /
        /// `pkg_postrm` ran from its stored environment (scratch copy).
        #[cfg(feature = "vdb-sqlite")]
        #[test]
        fn a_same_slot_upgrade_on_sqlite_replaces_in_the_publishing_commit() {
            let tmp = tempdir();
            let root = tmp.join("root");
            let ptmp = tmp.join("ptmp");
            std::fs::create_dir_all(&root).unwrap();
            let old = EntryKey::new("dev-libs", "binpkgrmpkg-1.0");
            let new = EntryKey::new("dev-libs", "binpkgrmpkg-2.0");

            // 1.0 is installed on `files`, then the root is converted.
            merge_binpkg(
                &binpkg("binpkgrmpkg-1.0.tbz2"),
                &root,
                &ptmp,
                &MergeOptions::default(),
            )
            .expect("1.0 merges");
            let db = use_sqlite(&root, &tmp.join("vdb.sqlite"));
            assert!(db.has_entry(&old).unwrap());

            // (old live, new live, pending, generation) right before the commit.
            type Seen = (bool, bool, Vec<EntryKey>, u64);
            let seen: Rc<RefCell<Option<Seen>>> = Rc::default();
            {
                let db = db.clone();
                let seen = seen.clone();
                let (old, new) = (old.clone(), new.clone());
                install_hook(move |_| {
                    *seen.borrow_mut() = Some((
                        db.has_entry(&old).unwrap(),
                        db.has_entry(&new).unwrap(),
                        db.pending_entries().unwrap(),
                        db.generation().unwrap(),
                    ));
                });
            }
            let status = merge_binpkg(
                &binpkg("binpkgrmpkg-2.0.tbz2"),
                &root,
                &ptmp,
                &MergeOptions::default(),
            )
            .expect("2.0 merges");
            clear_hook();
            assert_eq!(status, 0);

            let (old_live, new_live, pending, generation) =
                seen.borrow().clone().expect("the publish hook ran");
            assert!(old_live, "the old instance is installed until the commit");
            assert!(!new_live, "the new instance is not live before the commit");
            assert_eq!(pending, vec![new.clone()]);

            assert!(!db.has_entry(&old).unwrap());
            assert!(db.has_entry(&new).unwrap());
            assert!(db.pending_entries().unwrap().is_empty());
            assert!(db.generation().unwrap() > generation);
            assert!(!root.join("var/db/pkg/dev-libs/binpkgrmpkg-2.0").exists());
            assert!(root.join("usr/share/binpkgrmpkg/payload-2.0.txt").is_file());
            assert!(!root.join("usr/share/binpkgrmpkg/payload-1.0.txt").exists());
            assert_eq!(
                std::fs::read_to_string(root.join("var/lib/binpkgrmpkg.log")).unwrap(),
                "setup-1.0\npreinst-1.0\npostinst-1.0\n\
                 setup-2.0\npreinst-2.0\nprerm-1.0\npostrm-1.0\npostinst-2.0\n"
            );
            // The counter ticked once on sqlite (1.0 took 0 on `files`).
            assert_eq!(
                db.read_file(&new, "COUNTER").unwrap().as_deref(),
                Some(&b"1"[..])
            );
            assert_eq!(db.counter().unwrap(), Some(portage_vdb::Counter(1)));
            let _ = std::fs::remove_dir_all(&tmp);
        }

        /// Clears the thread-local publish hook when dropped, so a hook that
        /// panics (a simulated crash) cannot leak into later tests that reuse
        /// this thread.
        #[cfg(feature = "vdb-sqlite")]
        struct HookGuard;
        #[cfg(feature = "vdb-sqlite")]
        impl Drop for HookGuard {
            fn drop(&mut self) {
                clear_hook();
            }
        }

        /// Run `f` (a merge or unmerge) with a hook that records the
        /// generation and then panics, i.e. "the process dies right before
        /// the final commit". Returns that generation; `f` must have panicked.
        #[cfg(feature = "vdb-sqlite")]
        fn crash_before_commit(db: &Arc<portage_vdb::SqliteDb>, f: impl FnOnce()) -> u64 {
            let at: Rc<RefCell<Option<u64>>> = Rc::default();
            let _guard = HookGuard;
            {
                let db = db.clone();
                let at = at.clone();
                install_hook(move |_| {
                    *at.borrow_mut() = Some(db.generation().unwrap());
                    panic!("simulated crash before the final commit");
                });
            }
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
            assert!(res.is_err(), "the hook crashed the operation");
            let generation = *at.borrow();
            generation.expect("the hook ran")
        }

        #[cfg(feature = "vdb-sqlite")]
        fn vdb_cli(args: &[&str]) -> (u8, String) {
            let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            let (mut o, mut e) = (Vec::new(), Vec::new());
            let code = crate::vdb_cmd::run_args(&args, &mut o, &mut e);
            (
                code,
                String::from_utf8_lossy(&o).into_owned() + &String::from_utf8_lossy(&e),
            )
        }

        /// feat#157 S4.5 (design 9.1): the process dies right before the
        /// publishing commit of a same-slot upgrade. After a restart (fresh
        /// handle) the old instance is still installed with its files, the
        /// new one is a pending orphan that `status` reports and `sweep`
        /// removes, the generation was not advanced by the aborted commit and
        /// the counter is not reused. The new payload files landed on disk
        /// before the crash and stay there (as in real Portage); that is only
        /// documented here, not asserted.
        #[cfg(feature = "vdb-sqlite")]
        #[test]
        fn a_crash_before_the_publishing_commit_leaves_the_old_instance_and_a_sweepable_orphan() {
            let tmp = tempdir();
            let root = tmp.join("root");
            let ptmp = tmp.join("ptmp");
            let dbp = tmp.join("vdb.sqlite");
            std::fs::create_dir_all(&root).unwrap();
            let old = EntryKey::new("dev-libs", "binpkgrmpkg-1.0");
            let new = EntryKey::new("dev-libs", "binpkgrmpkg-2.0");
            merge_binpkg(
                &binpkg("binpkgrmpkg-1.0.tbz2"),
                &root,
                &ptmp,
                &MergeOptions::default(),
            )
            .expect("1.0 merges");
            let db = use_sqlite(&root, &dbp);
            let old_files = entry_files(db.as_ref(), &old);

            let before_commit = crash_before_commit(&db, || {
                let _ = merge_binpkg(
                    &binpkg("binpkgrmpkg-2.0.tbz2"),
                    &root,
                    &ptmp,
                    &MergeOptions::default(),
                );
            });
            drop(db);

            // "Restart": a fresh handle on the same file.
            let fresh = Arc::new(portage_vdb::SqliteDb::open(&dbp).unwrap());
            portage_vdb::register(&root, fresh.clone());
            assert_eq!(fresh.entries().unwrap(), vec![old.clone()]);
            assert_eq!(entry_files(fresh.as_ref(), &old), old_files);
            assert!(!fresh.has_entry(&new).unwrap());
            assert_eq!(fresh.pending_entries().unwrap(), vec![new.clone()]);
            assert_eq!(
                fresh.generation().unwrap(),
                before_commit,
                "the aborted commit did not advance the generation"
            );
            let counter_after_crash = fresh.counter().unwrap().expect("a counter was taken");
            let warnings = crate::mrg::pending_entries_warnings(
                std::slice::from_ref(&new),
                &dbp,
                portage_vdb::BackendKind::Sqlite,
            );
            assert!(warnings[0].contains("binpkgrmpkg-2.0"), "{warnings:?}");
            assert!(
                crate::mrg::pending_entries_warnings(
                    &fresh.pending_entries().unwrap(),
                    &dbp,
                    portage_vdb::BackendKind::Sqlite,
                )
                .iter()
                .any(|w| w.contains("dev-libs/binpkgrmpkg-2.0"))
            );

            let spec = format!("sqlite:{}", dbp.display());
            let (code, out) = vdb_cli(&["status", &spec]);
            assert_eq!(code, 1, "{out}");
            assert!(out.contains("dev-libs/binpkgrmpkg-2.0"), "{out}");
            let (code, out) = vdb_cli(&["sweep", "--remove", "dev-libs/binpkgrmpkg-2.0", &spec]);
            assert_eq!(code, 0, "{out}");
            let (code, out) = vdb_cli(&["status", &spec]);
            assert_eq!(code, 0, "{out}");
            assert!(fresh.pending_entries().unwrap().is_empty());
            assert!(fresh.has_entry(&old).unwrap());
            assert_eq!(entry_files(fresh.as_ref(), &old), old_files);

            // The upgrade can now be redone.
            let status = merge_binpkg(
                &binpkg("binpkgrmpkg-2.0.tbz2"),
                &root,
                &ptmp,
                &MergeOptions::default(),
            )
            .expect("2.0 merges after the sweep");
            assert_eq!(status, 0);
            assert!(fresh.has_entry(&new).unwrap());
            assert!(!fresh.has_entry(&old).unwrap());
            let counter_file = fresh.read_file(&new, "COUNTER").unwrap().unwrap();
            let counter: i64 = String::from_utf8(counter_file)
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            assert!(
                counter > counter_after_crash.0,
                "{counter} reuses a counter taken before the crash ({counter_after_crash:?})"
            );
            assert!(fresh.generation().unwrap() > before_commit);
            let _ = std::fs::remove_dir_all(&tmp);
        }

        /// feat#157 S4.5: the process dies right before the retiring commit
        /// of `-C`. With a fresh handle the row is still installed and the
        /// generation is unchanged; a later `-C` succeeds. (The payload files
        /// were already removed before the crash; not asserted.)
        #[cfg(feature = "vdb-sqlite")]
        #[test]
        fn a_crash_before_the_retire_commit_leaves_the_entry_installed() {
            let tmp = tempdir();
            let root = tmp.join("root");
            let ptmp = tmp.join("ptmp");
            let dbp = tmp.join("vdb.sqlite");
            std::fs::create_dir_all(&root).unwrap();
            let key = EntryKey::new("dev-libs", "binpkgrmpkg-1.0");
            merge_binpkg(
                &binpkg("binpkgrmpkg-1.0.tbz2"),
                &root,
                &ptmp,
                &MergeOptions::default(),
            )
            .expect("1.0 merges");
            let db = use_sqlite(&root, &dbp);
            let files = entry_files(db.as_ref(), &key);

            let before_commit = crash_before_commit(&db, || {
                unmerge_standalone(&root, &ptmp, "binpkgrmpkg-1.0", "binpkgrmpkg");
            });
            drop(db);

            let fresh = Arc::new(portage_vdb::SqliteDb::open(&dbp).unwrap());
            portage_vdb::register(&root, fresh.clone());
            assert!(fresh.has_entry(&key).unwrap());
            assert_eq!(entry_files(fresh.as_ref(), &key), files);
            assert!(fresh.pending_entries().unwrap().is_empty());
            assert_eq!(fresh.generation().unwrap(), before_commit);

            unmerge_standalone(&root, &ptmp, "binpkgrmpkg-1.0", "binpkgrmpkg");
            assert!(!fresh.has_entry(&key).unwrap());
            assert!(fresh.generation().unwrap() > before_commit);
            let _ = std::fs::remove_dir_all(&tmp);
        }

        /// A source merge (`ebuild <file> merge`) on sqlite: same entry
        /// files and payload as on `files`, the `pkg_postinst` environment
        /// rewrite included.
        #[cfg(feature = "vdb-sqlite")]
        #[test]
        fn a_source_merge_on_sqlite_matches_the_files_merge() {
            let tmp = tempdir();
            let root = tmp.join("root");
            let ptmp = tmp.join("ptmp");
            let ebuild = fixtures_root().join("repo/dev-libs/mergepkg/mergepkg-1.0.ebuild");
            let key = EntryKey::new("dev-libs", "mergepkg-1.0");
            let merge = || {
                std::fs::create_dir_all(&root).unwrap();
                std::fs::create_dir_all(&ptmp).unwrap();
                let status = run_merge(&ebuild, &root, &ptmp, &MergeOptions::default(), None)
                    .expect("run_merge succeeds");
                assert_eq!(status, 0);
            };

            merge();
            let on_files = outcome(&root, &key);
            std::fs::remove_dir_all(&root).unwrap();
            let _ = std::fs::remove_dir_all(&ptmp);

            std::fs::create_dir_all(&root).unwrap();
            use_sqlite(&root, &tmp.join("vdb.sqlite"));
            merge();
            let on_sqlite = outcome(&root, &key);

            assert!(!root.join("var/db/pkg").exists());
            assert_eq!(
                on_sqlite.0.keys().collect::<Vec<_>>(),
                on_files.0.keys().collect::<Vec<_>>()
            );
            for (name, (mode, data)) in &on_files.0 {
                // A source build records its own build time.
                if name == "BUILD_TIME" || name == "metadata" {
                    continue;
                }
                assert_eq!(&on_sqlite.0[name].0, mode, "{name} mode");
                // `CONTENTS` records each file's mtime, which a fresh
                // build moves: compare the lines without their mtime.
                let strip = |b: &[u8]| -> Vec<String> {
                    String::from_utf8_lossy(b)
                        .lines()
                        .map(|l| match l.split_whitespace().next() {
                            Some("obj" | "sym") => l.rsplit_once(' ').map_or(l, |(a, _)| a),
                            _ => l,
                        })
                        .map(String::from)
                        .collect()
                };
                if name == "CONTENTS" {
                    assert_eq!(strip(&on_sqlite.0[name].1), strip(data), "CONTENTS lines");
                    continue;
                }
                assert_eq!(&on_sqlite.0[name].1, data, "{name} bytes");
            }
            assert_eq!(on_sqlite.1, on_files.1, "payload differs");
            let _ = std::fs::remove_dir_all(&tmp);
        }
        fn unmerge_standalone(root: &Path, ptmp: &Path, pf: &str, pn: &str) {
            unmerge_one_installed(
                root,
                "dev-libs",
                pn,
                pf,
                &[],
                &ptmp.join("scratch"),
                ptmp,
                &MergeOptions::default(),
                None,
                false,
                &BTreeMap::new(),
                &[],
                None,
            )
            .expect("unmerge succeeds");
        }

        /// feat#157 S4.2 (a): `mrg -C` of an installed package on sqlite.
        /// The payload goes as on `files`, prerm/postrm ran from the
        /// scratch copy of the stored environment, and the row goes in ONE
        /// commit: right before it the entry is still installed (and its
        /// payload already gone); the generation moved by exactly one.
        #[cfg(feature = "vdb-sqlite")]
        #[test]
        fn a_standalone_unmerge_on_sqlite_retires_the_row_in_one_commit() {
            let tmp = tempdir();
            let root = tmp.join("root");
            let ptmp = tmp.join("ptmp");
            let key = EntryKey::new("dev-libs", "binpkgrmpkg-1.0");
            let run = |sqlite: bool| {
                std::fs::create_dir_all(&root).unwrap();
                let db = sqlite.then(|| use_sqlite(&root, &tmp.join("vdb.sqlite")));
                merge_binpkg(
                    &binpkg("binpkgrmpkg-1.0.tbz2"),
                    &root,
                    &ptmp,
                    &MergeOptions::default(),
                )
                .expect("merge succeeds");
                assert!(portage_vdb::for_root(&root).has_entry(&key).unwrap());
                let seen: Rc<RefCell<Option<(bool, bool, u64)>>> = Rc::default();
                let before = db.as_ref().map(|db| db.generation().unwrap());
                if let Some(db) = &db {
                    let db = db.clone();
                    let seen = seen.clone();
                    let key = key.clone();
                    let root = root.clone();
                    install_hook(move |_| {
                        *seen.borrow_mut() = Some((
                            db.has_entry(&key).unwrap(),
                            root.join("usr/share/binpkgrmpkg/payload-1.0.txt").exists(),
                            db.generation().unwrap(),
                        ));
                    });
                }
                unmerge_standalone(&root, &ptmp, "binpkgrmpkg-1.0", "binpkgrmpkg");
                clear_hook();
                assert!(!portage_vdb::for_root(&root).has_entry(&key).unwrap());
                let log = std::fs::read_to_string(root.join("var/lib/binpkgrmpkg.log")).unwrap();
                let tree = payload_bytes(&root);
                if let Some(db) = &db {
                    let (live, payload, generation) = seen.borrow().expect("retire hook ran");
                    assert!(live, "the row is installed until the retire commit");
                    assert!(!payload, "the payload was removed before it");
                    assert_eq!(generation, before.unwrap());
                    assert_eq!(db.generation().unwrap(), before.unwrap() + 1);
                    assert!(!root.join("var/db/pkg").exists());
                }
                (log, tree)
            };
            let on_files = run(false);
            std::fs::remove_dir_all(&root).unwrap();
            let _ = std::fs::remove_dir_all(&ptmp);
            let on_sqlite = run(true);
            assert!(
                on_files.0.ends_with("prerm-1.0\npostrm-1.0\n"),
                "{}",
                on_files.0
            );
            assert_eq!(on_sqlite.0, on_files.0, "phase log differs");
            assert_eq!(on_sqlite.1, on_files.1, "payload differs");
            let _ = std::fs::remove_dir_all(&tmp);
        }

        /// feat#157 S4.2 (b): the soname-bump scenario of S1.5. After
        /// `sonamebumplib` 1.0 -> 2.0 (the old soname is preserved under
        /// 2.0), unmerging the last consumer prunes the preserved library:
        /// it is removed, the registry is cleared, and the W4 rewrites of
        /// `sonamebumplib-2.0`'s `CONTENTS` / `NEEDED.ELF.2` happen. On
        /// sqlite they all land in the commit that deletes the consumer's
        /// row (one generation step); the end state equals the `files` run.
        #[cfg(feature = "vdb-sqlite")]
        #[test]
        fn a_standalone_unmerge_prunes_preserved_libs_in_its_retire_commit() {
            let tmp = tempdir();
            let ptmp = tmp.join("ptmp");
            let run = |name: &str, sqlite: bool| {
                let root = tmp.join(name);
                std::fs::create_dir_all(&root).unwrap();
                // As the S1.5 soname-bump test: `/usr/lib64` must be in the
                // linker path for the consumer to be found.
                std::fs::create_dir_all(root.join("etc/env.d")).unwrap();
                std::fs::write(
                    root.join("etc/env.d/99sonamebump"),
                    "LDPATH=\"/usr/lib64\"\n",
                )
                .unwrap();
                let db = sqlite.then(|| use_sqlite(&root, &tmp.join(format!("{name}.sqlite"))));
                for (pn, version) in [
                    ("sonamebumplib", "1.0"),
                    ("consumesonamebump", "1.0"),
                    ("sonamebumplib", "2.0"),
                ] {
                    let status = run_merge(
                        &versioned_fixture(pn, version),
                        &root,
                        &ptmp,
                        &MergeOptions::default(),
                        None,
                    )
                    .expect("merge succeeds");
                    assert_eq!(status, 0);
                }
                let lib = EntryKey::new("dev-libs", "sonamebumplib-2.0");
                let consumer = EntryKey::new("dev-libs", "consumesonamebump-1.0");
                let live = portage_vdb::for_root(&root);
                let registry_before = read_plib_registry(&root).preserved_libs();
                assert!(
                    registry_before.contains_key("dev-libs/sonamebumplib-2.0"),
                    "sanity: the old soname is preserved: {registry_before:?}"
                );
                let contents_before = live.read_file(&lib, "CONTENTS").unwrap().unwrap();

                type Seen = (Vec<u8>, BTreeMap<String, Vec<String>>);
                let seen: Rc<RefCell<Option<Seen>>> = Rc::default();
                let before = db.as_ref().map(|db| db.generation().unwrap());
                if let Some(db) = &db {
                    let db = db.clone();
                    let seen = seen.clone();
                    let lib = lib.clone();
                    install_hook(move |_| {
                        *seen.borrow_mut() = Some((
                            db.read_file(&lib, "CONTENTS").unwrap().unwrap(),
                            // The stored registry (`read_plib_registry` would
                            // prune the paths the unmerge already deleted).
                            db.preserved_libs()
                                .unwrap()
                                .entries
                                .into_values()
                                .map(|e| (e.cpv, e.paths))
                                .collect(),
                        ));
                    });
                }
                unmerge_standalone(&root, &ptmp, "consumesonamebump-1.0", "consumesonamebump");
                clear_hook();

                assert!(!live.has_entry(&consumer).unwrap());
                assert!(read_plib_registry(&root).entries.is_empty());
                let contents_after = live.read_file(&lib, "CONTENTS").unwrap().unwrap();
                assert_ne!(contents_after, contents_before, "W4 rewrote CONTENTS");
                if let Some(db) = &db {
                    let (contents_seen, registry_seen) = seen.borrow().clone().expect("hook ran");
                    assert_eq!(contents_seen, contents_before, "W4 not visible before");
                    assert_eq!(
                        registry_seen, registry_before,
                        "registry not visible before"
                    );
                    assert_eq!(db.generation().unwrap(), before.unwrap() + 1);
                }
                // Each run builds its own files: compare `CONTENTS` without mtimes.
                let contents_after: Vec<String> = String::from_utf8(contents_after)
                    .unwrap()
                    .lines()
                    .map(|l| match l.split_whitespace().next() {
                        Some("obj" | "sym") => l.rsplit_once(' ').map_or(l, |(a, _)| a).to_string(),
                        _ => l.to_string(),
                    })
                    .collect();
                (
                    contents_after,
                    live.read_file(&lib, "NEEDED.ELF.2").unwrap(),
                    payload_bytes(&root),
                )
            };
            let on_files = run("files-root", false);
            let on_sqlite = run("sqlite-root", true);
            assert_eq!(on_sqlite.0, on_files.0, "CONTENTS differs");
            assert_eq!(on_sqlite.1, on_files.1, "NEEDED.ELF.2 differs");
            let differing: Vec<&String> = on_files
                .2
                .keys()
                .chain(on_sqlite.2.keys())
                .filter(|k| on_files.2.get(*k) != on_sqlite.2.get(*k))
                .collect();
            assert!(differing.is_empty(), "payload differs at {differing:?}");
            let _ = std::fs::remove_dir_all(&tmp);
        }

        /// What a VDB holds after a sequence, with the run-dependent values
        /// taken out (see [`a_merge_upgrade_unmerge_sequence_is_equivalent_on_sqlite_and_files`]).
        #[derive(Debug, PartialEq)]
        struct Snap {
            /// key -> (files: name -> (mode, bytes), stamp state).
            entries: BTreeMap<EntryKey, (Tree, portage_vdb::MetadataStamp)>,
            world: portage_vdb::World,
            world_sets: portage_vdb::WorldSets,
            preserved_libs: BTreeMap<String, portage_vdb::PreservedLibsEntry>,
            config_memory: portage_vdb::ConfigMemory,
            counter: Option<portage_vdb::Counter>,
        }

        /// `CONTENTS` / `metadata` / `BUILD_TIME` reduced to their stable
        /// parts; every other file is returned as stored.
        fn stable_bytes(name: &str, data: Vec<u8>) -> Vec<u8> {
            let lines = |data: &[u8], keep: &dyn Fn(&str) -> Option<String>| -> Vec<u8> {
                String::from_utf8_lossy(data)
                    .lines()
                    .filter_map(keep)
                    .map(|l| format!("{l}\n"))
                    .collect::<String>()
                    .into_bytes()
            };
            match name {
                // The mtime is the last field of `obj` / `sym` lines.
                "CONTENTS" => lines(&data, &|l| {
                    Some(match l.split_whitespace().next() {
                        Some("obj" | "sym") => l.rsplit_once(' ').map_or(l, |(a, _)| a).to_string(),
                        _ => l.to_string(),
                    })
                }),
                // The stamp line and the BUILD_TIME field; the other
                // `metadata` lines are the stable fields.
                "metadata" => lines(&data, &|l| {
                    (!l.starts_with("#dir_mtime=") && !l.starts_with("BUILD_TIME="))
                        .then(|| l.to_string())
                }),
                _ => data,
            }
        }

        fn snapshot(db: &dyn InstalledDb) -> Snap {
            let mut entries = BTreeMap::new();
            for key in db.entries().unwrap() {
                let image = db.entry_image(&key).unwrap().expect("live entry");
                let files: Tree = image
                    .files
                    .into_iter()
                    // A source build records its own build time.
                    .filter(|f| f.meta.name != "BUILD_TIME")
                    .map(|f| {
                        let data = stable_bytes(&f.meta.name, f.data);
                        // Group/other write bits depend on the process umask,
                        // which another test may have changed.
                        (f.meta.name, (f.meta.mode & 0o7755, data))
                    })
                    .collect();
                entries.insert(key, (files, image.metadata_stamp));
            }
            Snap {
                entries,
                world: db.world().unwrap(),
                world_sets: db.world_sets().unwrap(),
                preserved_libs: db.preserved_libs().unwrap().entries,
                config_memory: db.config_memory().unwrap(),
                counter: db.counter().unwrap(),
            }
        }

        /// feat#157 S4.4: the same sequence on a `files` ROOT and on a
        /// sqlite ROOT ends in the same installed state. Sequence: (1) merge
        /// binpkgrmpkg-1.0 (binary), (2) merge mergepkg (source), (3)
        /// same-slot upgrade binpkgrmpkg 1.0 -> 2.0, (4) the soname bump
        /// sonamebumplib 1.0 -> consumesonamebump -> sonamebumplib 2.0
        /// (preserves the old soname), (5) unmerge consumesonamebump (prunes
        /// the preserved library, W4), (6) unmerge mergepkg.
        ///
        /// Both runs use the same ROOT and PORTAGE_TMPDIR paths one after
        /// the other, so `environment.bz2` (which records paths) is
        /// comparable byte for byte. The sqlite result is also converted to
        /// a temporary `files` root with `copy_all` and compared as well;
        /// `verify` runs on that pair too.
        ///
        /// Compared per live entry: the file set, every file's bytes and
        /// permission bits (group/other write masked, see below) and the
        /// `metadata` stamp state; plus the same
        /// live-entry set, `world`, `world_sets`, `preserved_libs`,
        /// `config_memory`, counter, and the payload trees under ROOT
        /// (paths, bytes, symlink targets; no modes, see below).
        ///
        /// Ignored, because they are run-dependent by nature (and `verify`
        /// compares them, so its reported differences are only checked to
        /// be among these, the modes only by the umask bits): all file mtimes and the entry directory mtime
        /// (each run builds and writes its files at another instant); the
        /// `#dir_mtime=` stamp line of `metadata` (derived from the entry
        /// directory mtime; its Valid/Stale/Absent state is compared); the
        /// mtime field of `obj` / `sym` lines of `CONTENTS` (the payload
        /// files' mtimes); the `BUILD_TIME` file and the `BUILD_TIME=` line of
        /// `metadata` (a source merge records the time of its build); the entry
        /// directory's mode (`files` records the merge's `mkdir` under the
        /// process umask, so 0o40755, or 0o40775 after another test changed
        /// the umask; a sqlite merge stores the schema default 0o755 with no
        /// type bits); the group and other write bits of entry file modes
        /// (a file written under another umask: 0o664 vs 0o644); the modes
        /// of the payload tree (the same umask race,
        /// as `payload_bytes` notes). Nothing else is dropped: not
        /// `environment.bz2`, not `COUNTER`, not the rest of `metadata`.
        fn sequence_equivalence(kind: portage_vdb::BackendKind) {
            let tmp = tempdir();
            let root = tmp.join("root");
            let ptmp = tmp.join("ptmp");
            let ebuild = fixtures_root().join("repo/dev-libs/mergepkg/mergepkg-1.0.ebuild");

            let sequence = || {
                for dir in [&root, &ptmp] {
                    let _ = std::fs::remove_dir_all(dir);
                    std::fs::create_dir_all(dir).unwrap();
                }
                // As the S1.5 soname-bump test: `/usr/lib64` must be in the
                // linker path for the consumer to be found.
                std::fs::create_dir_all(root.join("etc/env.d")).unwrap();
                std::fs::write(
                    root.join("etc/env.d/99sonamebump"),
                    "LDPATH=\"/usr/lib64\"\n",
                )
                .unwrap();
            };
            // Runs the steps on whatever backend `root` has.
            let steps = || {
                let opts = MergeOptions::default();
                let binpkg_1 = binpkg("binpkgrmpkg-1.0.tbz2");
                assert_eq!(merge_binpkg(&binpkg_1, &root, &ptmp, &opts).unwrap(), 0);
                assert_eq!(run_merge(&ebuild, &root, &ptmp, &opts, None).unwrap(), 0);
                assert_eq!(
                    merge_binpkg(&binpkg("binpkgrmpkg-2.0.tbz2"), &root, &ptmp, &opts).unwrap(),
                    0
                );
                for (pn, version) in [
                    ("sonamebumplib", "1.0"),
                    ("consumesonamebump", "1.0"),
                    ("sonamebumplib", "2.0"),
                ] {
                    let fixture = versioned_fixture(pn, version);
                    assert_eq!(run_merge(&fixture, &root, &ptmp, &opts, None).unwrap(), 0);
                }
                let preserved = portage_vdb::for_root(&root).preserved_libs().unwrap();
                assert!(
                    preserved.entries.contains_key("dev-libs/sonamebumplib:0"),
                    "sanity: the old soname is preserved: {:?}",
                    preserved.entries
                );
                unmerge_standalone(&root, &ptmp, "consumesonamebump-1.0", "consumesonamebump");
                assert!(
                    portage_vdb::for_root(&root)
                        .preserved_libs()
                        .unwrap()
                        .entries
                        .is_empty()
                );
                unmerge_standalone(&root, &ptmp, "mergepkg-1.0", "mergepkg");
            };

            // The files run; then its ROOT moves aside (rename keeps mtimes).
            sequence();
            steps();
            let files_db = portage_vdb::FilesDb::new(&root);
            let files_snap = snapshot(&files_db);
            let files_payload = payload_tree(&root);
            let files_final = tmp.join("files-final");
            std::fs::rename(&root, &files_final).unwrap();

            // The sqlite run, in the same paths.
            sequence();
            let db = use_db(kind, &root, &tmp.join("vdb.db"));
            steps();
            let sqlite_snap = snapshot(db.as_ref());
            let sqlite_payload = payload_tree(&root);
            assert!(!root.join("var/db/pkg").exists());
            assert!(db.pending_entries().unwrap().is_empty());

            // Convert the sqlite result to a temporary files root.
            let converted = tmp.join("converted");
            std::fs::create_dir_all(&converted).unwrap();
            let converted_db = portage_vdb::FilesDb::new(&converted);
            portage_vdb::copy_all(db.as_ref(), &converted_db, false).unwrap();
            let converted_snap = snapshot(&converted_db);

            // The sequence left something to compare (1 + 2 + upgrade +
            // soname bump - consumer - mergepkg).
            let live: Vec<String> = files_snap.entries.keys().map(|k| k.to_string()).collect();
            assert_eq!(
                live,
                ["dev-libs/binpkgrmpkg-2.0", "dev-libs/sonamebumplib-2.0"],
                "unexpected end state"
            );
            assert!(files_snap.counter.is_some());

            // Field by field, so a failure names what differs.
            let same = |got: &Snap, what: &str| {
                assert_eq!(
                    got.entries.keys().collect::<Vec<_>>(),
                    files_snap.entries.keys().collect::<Vec<_>>(),
                    "{what}: live entries"
                );
                for (key, (files, stamp)) in &files_snap.entries {
                    let (g_files, g_stamp) = &got.entries[key];
                    assert_eq!(g_stamp, stamp, "{what}: {key} metadata stamp state");
                    assert_eq!(
                        g_files.keys().collect::<Vec<_>>(),
                        files.keys().collect::<Vec<_>>(),
                        "{what}: {key} file set"
                    );
                    for (name, (mode, data)) in files {
                        let (g_mode, g_data) = &g_files[name];
                        assert_eq!(g_mode, mode, "{what}: {key}/{name} mode");
                        assert_eq!(
                            String::from_utf8_lossy(g_data),
                            String::from_utf8_lossy(data),
                            "{what}: {key}/{name} bytes"
                        );
                    }
                }
                assert_eq!(got.world, files_snap.world, "{what}: world");
                assert_eq!(got.world_sets, files_snap.world_sets, "{what}: world_sets");
                assert_eq!(
                    got.preserved_libs, files_snap.preserved_libs,
                    "{what}: plibs"
                );
                assert_eq!(
                    got.config_memory, files_snap.config_memory,
                    "{what}: config"
                );
                assert_eq!(got.counter, files_snap.counter, "{what}: counter");
            };
            same(&sqlite_snap, &format!("{kind} vs files"));
            same(&converted_snap, &format!("{kind} converted vs files"));
            let bytes = |tree: Tree| -> BTreeMap<String, Vec<u8>> {
                tree.into_iter().map(|(k, (_, data))| (k, data)).collect()
            };
            assert_eq!(bytes(sqlite_payload), bytes(files_payload), "payload tree");

            // `verify` on the converted pair: every difference it reports
            // must be one of the ignored, run-dependent kinds.
            let rep = portage_vdb::verify(&portage_vdb::FilesDb::new(&files_final), &converted_db)
                .unwrap();
            assert_eq!(rep.entries_compared, 2);
            for line in &rep.differences {
                let (name, what) = line.split_once(": ").unwrap_or((line, ""));
                let file = name.rsplit_once('/').map_or("", |(_, f)| f);
                // "mode <a> != <b>" (octal): only the umask bits may differ.
                let mode_ok = what.strip_prefix("mode ").is_some_and(|m| {
                    let v: Vec<u32> = m
                        .split(" != ")
                        .filter_map(|x| u32::from_str_radix(x, 8).ok())
                        .collect();
                    v.len() == 2 && (v[0] ^ v[1]) & !0o022 == 0
                });
                let ok = what.starts_with("mtime")
                    || what.starts_with("directory mtime")
                    || what.starts_with("directory mode")
                    || mode_ok
                    || (what.starts_with("bytes differ")
                        && ["CONTENTS", "metadata", "BUILD_TIME"].contains(&file));
                assert!(ok, "verify reports a non-volatile difference: {line}");
            }
            let _ = std::fs::remove_dir_all(&tmp);
        }

        #[cfg(feature = "vdb-sqlite")]
        #[test]
        fn a_merge_upgrade_unmerge_sequence_is_equivalent_on_sqlite_and_files() {
            sequence_equivalence(portage_vdb::BackendKind::Sqlite);
        }

        /// feat#157 S5.4: the same sequence on redb (see
        /// [`sequence_equivalence`] for what is compared and ignored).
        #[cfg(feature = "vdb-redb")]
        #[test]
        fn a_merge_upgrade_unmerge_sequence_is_equivalent_on_redb_and_files() {
            sequence_equivalence(portage_vdb::BackendKind::Redb);
        }

        /// The database backends compiled in, as `BackendKind`s.
        fn db_kinds() -> Vec<portage_vdb::BackendKind> {
            vec![
                #[cfg(feature = "vdb-sqlite")]
                portage_vdb::BackendKind::Sqlite,
                #[cfg(feature = "vdb-redb")]
                portage_vdb::BackendKind::Redb,
            ]
        }

        /// feat#157 S8.2: the collision scenario on a database backend
        /// (`find_owners` answered from the `owner` index): `collisionpkg-c`
        /// would overwrite `collisionpkg-a`'s `shared.txt`, the merge
        /// aborts under `collision-protect` and names `collisionpkg-a` as
        /// the owner, and `find_owners` agrees with the files answer.
        #[test]
        fn collision_protect_names_the_owner_from_the_index_on_every_backend() {
            for kind in db_kinds() {
                let tmp = tempdir();
                let root = tmp.join("root");
                let portage_tmpdir = tmp.join("tmp");
                std::fs::create_dir_all(&root).unwrap();
                std::fs::create_dir_all(&portage_tmpdir).unwrap();
                let db = use_db(kind, &root, &tmp.join("vdb.db"));
                run_merge(
                    &collision_fixture("collisionpkg-a"),
                    &root,
                    &portage_tmpdir,
                    &MergeOptions::default(),
                    None,
                )
                .expect("collisionpkg-a merges cleanly");
                assert_eq!(db.kind(), kind);
                let collisions = vec![
                    "/usr/share/collisiontest/shared.txt".to_string(),
                    "/usr/share/collisiontest/stray.txt".to_string(),
                ];
                assert_eq!(
                    find_owners(&root, &collisions),
                    BTreeMap::from([(
                        "dev-libs/collisionpkg-a-1.0".to_string(),
                        vec!["/usr/share/collisiontest/shared.txt".to_string()],
                    )]),
                    "{kind}"
                );
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
                assert!(err.contains("dev-libs/collisionpkg-a-1.0"), "{kind}: {err}");
                assert!(
                    err.contains("/usr/share/collisiontest/shared.txt"),
                    "{kind}: {err}"
                );
                assert!(
                    err.contains("NOT merged due to file collisions"),
                    "{kind}: {err}"
                );
                assert_eq!(
                    std::fs::read_to_string(root.join("usr/share/collisiontest/shared.txt"))
                        .unwrap(),
                    "hello from collisionpkg-a\n",
                    "{kind}"
                );
                let _ = std::fs::remove_dir_all(&tmp);
            }
        }

        /// One hand-written entry: `(category, pf, files)`.
        type HandEntry<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);

        /// A `files` VDB written by hand.
        fn write_vdb(root: &Path, entries: &[HandEntry]) {
            for (cat, pf, files) in entries {
                let dir = root.join("var/db/pkg").join(cat).join(pf);
                std::fs::create_dir_all(&dir).unwrap();
                for (name, data) in *files {
                    std::fs::write(dir.join(name), data).unwrap();
                }
            }
        }

        /// feat#157 S8.1 / S8.2: on a hand-built VDB whose dependency
        /// strings carry every kind of token (USE conditional, `||` group,
        /// slot operator, blocker, version bounds, a wildcard and bare
        /// versioned names the index cannot classify), plus `CONTENTS`
        /// lines with odd path spellings, `installed_reverse_dependents`
        /// and `find_owners` give the same answer on `files` (the scan)
        /// and on every database backend (the index), and the answer is
        /// the right one.
        #[test]
        fn reverse_dependents_and_owners_from_the_index_equal_the_scan() {
            let tmp = tempdir();
            let root = tmp.join("root");
            let e = |pf: &'static str, files: &'static [(&'static str, &'static str)]| {
                ("dev-libs", pf, files)
            };
            write_vdb(
                &root,
                &[
                    e(
                        "consumer-1.0",
                        &[
                            ("SLOT", "0\n"),
                            ("CONTENTS", "dir /usr/lib\nobj /usr/lib/libc.so abc 1\n"),
                        ],
                    ),
                    e(
                        "cond-1.0",
                        &[
                            ("SLOT", "0\n"),
                            ("USE", "ssl\n"),
                            (
                                "RDEPEND",
                                "ssl? ( dev-libs/consumer ) !ssl? ( dev-libs/o )\n",
                            ),
                        ],
                    ),
                    e(
                        "condoff-1.0",
                        &[
                            ("SLOT", "0\n"),
                            ("USE", "x\n"),
                            ("RDEPEND", "ssl? ( dev-libs/consumer )\n"),
                        ],
                    ),
                    e(
                        "alt-1.0",
                        &[
                            ("SLOT", "0\n"),
                            ("RDEPEND", "|| ( dev-libs/zzz >=dev-libs/consumer-0.5 )\n"),
                        ],
                    ),
                    e(
                        "slotted-1.0",
                        &[("SLOT", "0\n"), ("DEPEND", "dev-libs/consumer:0=[x(+)]\n")],
                    ),
                    e(
                        "pdep-1.0",
                        &[("SLOT", "0\n"), ("PDEPEND", "=dev-libs/consumer-1*\n")],
                    ),
                    e(
                        "blocker-1.0",
                        &[
                            ("SLOT", "0\n"),
                            ("RDEPEND", "!dev-libs/consumer !!<dev-libs/consumer-3\n"),
                        ],
                    ),
                    e(
                        "toonew-1.0",
                        &[("SLOT", "0\n"), ("RDEPEND", ">=dev-libs/consumer-2\n")],
                    ),
                    e("wild-1.0", &[("SLOT", "0\n"), ("RDEPEND", "dev-libs/*\n")]),
                    e(
                        "bare-1.0",
                        &[("SLOT", "0\n"), ("RDEPEND", "dev-libs/consumer-1.0\n")],
                    ),
                    e(
                        "junk-1.0",
                        &[("SLOT", "0\n"), ("RDEPEND", "ssl? ( ( dev-libs/consumer\n")],
                    ),
                    e(
                        "other-1.0",
                        &[
                            ("SLOT", "0\n"),
                            ("RDEPEND", "dev-libs/other dev-libs/consumer2\n"),
                        ],
                    ),
                    e("nodeps-1.0", &[("SLOT", "0\n")]),
                    e(
                        "bidx-1.0",
                        &[
                            ("SLOT", "0\n"),
                            ("BDEPEND", "dev-libs/consumer\n"),
                            ("CONTENTS", "obj usr/lib/libc.so d 1\nobj //x d 2\n"),
                        ],
                    ),
                ],
            );
            let consumers = [
                ("dev-libs", "consumer", "1.0"),
                ("dev-libs", "other", "1.0"),
            ];
            let want_rdeps =
                |c: (&str, &str, &str)| installed_reverse_dependents_for(&root, c.0, c.1, c.2);
            let scan: Vec<Vec<String>> = consumers.iter().map(|&c| want_rdeps(c)).collect();
            let wanted: Vec<String> =
                ["alt-1.0", "bidx-1.0", "cond-1.0", "pdep-1.0", "slotted-1.0"]
                    .iter()
                    .map(|pf| format!("dev-libs/{pf}"))
                    .collect();
            for d in &wanted {
                assert!(scan[0].contains(d), "the scan misses {d}: {:?}", scan[0]);
            }
            for no in [
                "condoff-1.0",
                "blocker-1.0",
                "toonew-1.0",
                "other-1.0",
                "nodeps-1.0",
            ] {
                assert!(!scan[0].contains(&format!("dev-libs/{no}")), "{no}");
            }
            let paths: Vec<String> = ["/usr/lib", "/usr/lib/libc.so", "/x", "/nope"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let scan_owners = find_owners(&root, &paths);
            assert_eq!(scan_owners.len(), 2, "{scan_owners:?}");
            for (n, kind) in db_kinds().into_iter().enumerate() {
                // Another spelling of the same root per backend, so the
                // registry keeps `root` on `files`.
                let mut alias = root.clone();
                for _ in 0..=n {
                    alias = alias.join("../root");
                }
                let db = use_db(kind, &alias, &tmp.join(format!("vdb{n}.db")));
                assert_eq!(db.kind(), kind);
                // `use_db` converted `alias`'s files VDB (the same tree).
                for (&c, want) in consumers.iter().zip(&scan) {
                    assert_eq!(
                        &installed_reverse_dependents_for(&alias, c.0, c.1, c.2),
                        want,
                        "{kind}: {c:?}"
                    );
                }
                assert_eq!(find_owners(&alias, &paths), scan_owners, "{kind}");
                // The index is really in use: the entry without any dep
                // token is not even read.
                let recs = db
                    .reverse_dependents("dev-libs/consumer", &[portage_vdb::DepClass::Rdepend])
                    .unwrap();
                assert!(!recs.iter().any(|r| r.key.pf == "nodeps-1.0"), "{kind}");
                assert!(recs.iter().any(|r| r.key.pf == "wild-1.0"), "{kind}");
            }
            let _ = std::fs::remove_dir_all(&tmp);
        }

        fn installed_reverse_dependents_for(
            root: &Path,
            category: &str,
            package: &str,
            version: &str,
        ) -> Vec<String> {
            portage_repo::installed_reverse_dependents(root, category, package, version)
        }

        /// feat#157 S8.1 on the fixture root: every installed package gets
        /// the same reverse dependents from the scan (`files`) and from
        /// each database backend converted from it.
        #[test]
        fn fixture_reverse_dependents_are_identical_on_every_backend() {
            let fixtures = fixtures_root();
            let tmp = tempdir();
            let pkgs = portage_repo::all_installed_packages(&fixtures);
            assert!(pkgs.len() > 5);
            let want: Vec<Vec<String>> = pkgs
                .iter()
                .map(|p| {
                    portage_repo::installed_reverse_dependents(
                        &fixtures,
                        &p.category,
                        &p.package,
                        &p.version,
                    )
                })
                .collect();
            assert!(want.iter().any(|w| !w.is_empty()), "the fixture has edges");
            for (n, kind) in db_kinds().into_iter().enumerate() {
                let mut alias = fixtures.clone();
                for _ in 0..=n {
                    alias = alias.join("../fixtures");
                }
                use_db(kind, &alias, &tmp.join(format!("vdb{n}.db")));
                for (p, w) in pkgs.iter().zip(&want) {
                    assert_eq!(
                        &portage_repo::installed_reverse_dependents(
                            &alias,
                            &p.category,
                            &p.package,
                            &p.version
                        ),
                        w,
                        "{kind}: {}",
                        p.cpv()
                    );
                }
            }
            let _ = std::fs::remove_dir_all(&tmp);
        }
    }
}
