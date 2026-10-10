// Real merge/filesystem mutation (task #55, `LLM/agent-context.md`'s own
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
//     real now -- see `LLM/what-this-proves.md`'s own "`preserve-libs`" sections for
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
//     `LLM/scope-backlog.md` for that separate, still-correct
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

mod after_install;
mod collisions;
mod from_binpkg;
mod installed_instance;
mod options;
mod plib_merge;
mod plib_registry;
mod plib_unmerge;
mod protect;
mod tree;
mod vdb_entry;

#[cfg(test)]
mod tests;

pub(crate) use after_install::*;
pub(crate) use collisions::*;
pub use from_binpkg::*;
pub(crate) use installed_instance::*;
pub use options::*;
pub(crate) use plib_merge::*;
pub(crate) use plib_registry::*;
pub(crate) use plib_unmerge::*;
pub(crate) use protect::*;
pub(crate) use tree::*;
pub(crate) use vdb_entry::*;

use crate::env_update;

use md5::{Digest, Md5};

use mrg_director::PackagesDb as _;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use std::os::unix::fs::{FileTypeExt, MetadataExt};

use std::path::{Path, PathBuf};

/// Whether `command` is the one real merge command this module implements
/// -- `ebuild.rs` checks this alongside `ebuild_phases::
/// is_phase_command` before routing to real execution.
pub fn is_merge_command(command: &str) -> bool {
    command == "merge"
}

/// Whether `command` is real `qmerge` -- checked separately from
/// `is_merge_command` since `ebuild.rs` routes it to `run_qmerge`,
/// not `run_merge` (real `qmerge` skips the `install` phase entirely,
/// see `run_qmerge`'s own doc comment).
pub fn is_qmerge_command(command: &str) -> bool {
    command == "qmerge"
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
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
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
