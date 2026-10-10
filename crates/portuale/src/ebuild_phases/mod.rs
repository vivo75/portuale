// Real ebuild phase execution (task #54, LLM/agent-context.md's own
// "Real ebuild phase execution" section) -- the first slice: proving real
// phase functions run and real files land under a real `${D}`, without
// touching the vdb/CONTENTS/merge machinery at all (task #55, a
// separately-scoped, much bigger piece: `dblink.merge()`/`treewalk()`/
// `mergeme()` in `lib/portage/dbapi/vartree.py`, ~6500 lines).
//
// Bash-execution backend: by default a genuine `bash` subprocess (real
// portage's own `_doebuild_spawn()` shape); optionally an embedded
// `brush_core::Shell` via `--shell brush` (see `portuale/Cargo.toml`'s
// own doc comment for the pinned commit, and `ShellBackend`'s doc
// comment for why bash is the default). Either way it drives the REAL,
// unmodified `bin/ebuild.sh` and the phase
// functions it sources (`bin/phase-functions.sh`/`bin/phase-helpers.sh`/
// `bin/isolated-functions.sh`/`bin/bashrc-functions.sh`/
// `bin/save-ebuild-env.sh`) -- none of that bash is reimplemented in Rust;
// this module only computes the environment `doebuild_environment()`
// would (category/version splitting, the real directory layout) and
// drives the same per-command phase sequencing `doebuild()` itself does
// (see `phase_prerequisites`'s own doc comment), leaving every actual
// phase's own execution (including real EAPI-default-phase dispatch --
// `default_src_install` etc. are themselves real bash functions in
// `phase-functions.sh`, ported here for free by sourcing that file, not
// reimplemented) entirely to that real bash.
//
// Deliberately real, but real bin/ebuild.sh has machinery this slice
// doesn't reach yet: `bin/ebuild.sh` is sourced directly (not spawned as
// its own process the way real `doebuild()`'s `spawnebuild()` does),
// deliberately without ever setting `EBUILD_SH_ARGS` -- its own trailing
// `if [[ -n ${EBUILD_SH_ARGS} ]]` block ends with a bare `exit $?`, which
// would kill the embedding Rust process rather than just return control.
// `__check_bash_version` (called unconditionally at ebuild.sh's own
// top level) checks `BASH_VERSINFO` against the EAPI's own minimum real
// bash version -- brush reports one high enough to satisfy every EAPI
// portuale's own `portage_dep`/`portage_profile` crates already
// recognize, confirmed empirically, not by reading brush's own
// version-reporting code. Real `bin/ebuild.sh`'s own top-level code
// ALSO already sources the ebuild file itself unconditionally
// (`bin/ebuild.sh:681`'s own `source "${EBUILD}" || die`, not gated on
// `EBUILD_SH_ARGS` at all) -- confirmed empirically after an earlier
// version of this module sourced the ebuild file a *second* time
// itself, which failed ("cannot mutate readonly variable") against
// variables ebuild.sh's own tail had already made `readonly` from the
// first, automatic pass. `run_one_phase` sources `bin/ebuild.sh` alone;
// it never sources the ebuild file directly.
//
// ONE FRESH SHELL PER PHASE, not one shared across a whole invocation:
// real `bin/ebuild.sh`'s own tail makes `EBUILD_PHASE` (among other
// variables) `readonly`, so a *second* phase in the same shell can't
// `export EBUILD_PHASE=<next>` at all -- confirmed empirically the same
// way. A fresh `brush_core::Shell` per phase mirrors what real
// `doebuild()` itself does (a fresh `bin/ebuild.sh` *process* per phase,
// via `spawnebuild()`) far more literally than sharing one shell ever
// would have; real `PORTAGE_BUILDDIR`-relative resume markers
// (`.pretended`/`.setuped`/`.unpacked`/etc., written by `__dyn_*`
// themselves) are what make re-"running" an already-done prerequisite
// phase from a fresh shell cheap, exactly like real portage's own
// separate `spawnebuild()` calls rely on. See `run_one_phase`'s own doc
// comment.
//
// MUST use a multi-threaded tokio runtime (`run_commands`'s own doc
// comment): a single-threaded one deadlocks partway through a real
// multi-phase run -- confirmed empirically, and consistent with
// brush-core's own `Cargo.toml` requiring tokio's `rt-multi-thread`
// feature under unix, not just `rt`. One process-wide runtime
// (`shared_runtime`), not one per phase -- real `_emerge/Scheduler.py`
// itself runs a whole `emerge` invocation on a single `asyncio` event
// loop, and every one of this module's own sync entry points used to
// pay a fresh thread-pool setup/teardown cost per phase per package
// instead.
//
// KNOWN, DOCUMENTED GAPS (v1 scope, matching portuale's own
// "narrow v1, document the cut" pattern):
//   - This module itself only runs the `actionmap_deps`-chained phases
//     for real: `pretend`, `setup`, `unpack`, `prepare`, `configure`,
//     `compile`, `test`, `install` (see `phase_prerequisites`'s own doc
//     comment). `merge`/`qmerge`/`unmerge`/`package` are real too, but
//     live in their own modules (`ebuild_merge`/`ebuild_unmerge`/
//     `ebuild_package`, each routed to directly by `ebuild.rs`, not
//     through this module's own `run_commands`). `config`/`info`/
//     `prerm`/`postrm` *are* real too, through this module's own
//     `run_single_phase` (see `is_standalone_phase_command`'s own
//     doc comment) -- routed to directly by `ebuild.rs`, the same way
//     `merge`/`qmerge`/`unmerge`/`package` are. Every other real
//     `ebuild` command (`preinst`/`postinst`/`nofetch`/`depend`/`fetch`/
//     `fetchall`/`digest`/`manifest`/`rpm`/`instprep`/`clean`/
//     `cleanrm`) still falls through to `ebuild.rs`'s own pre-existing
//     dry-run stub message unchanged (`preinst`/`postinst` *are* run
//     for real, but only internally, as part of `merge` -- see
//     `is_standalone_phase_command`'s own doc comment for why they,
//     unlike `prerm`/`postrm`, stay internal-only).
//   - The `FEATURES` build-isolation set **is** modelled (SCOPE_BACKLOG
//     Part 2.D): `sandbox`/`usersandbox`, `network-sandbox`,
//     `ipc-sandbox`, `mount-sandbox`, `pid-sandbox`. All apply to the
//     same six real `src_*` phases (`unpack`/`prepare`/`configure`/
//     `compile`/`test`/`install` -- real `_doebuild_spawn` sandboxes
//     every phase not in `_unsandboxed_phases` and unshares every phase
//     not in `_ipc_phases`; for the phases portuale runs as real bash
//     both come out to this set, `SANDBOXED_SRC_PHASES`). Any one of
//     them forces the `Bash` backend for those phases: neither an
//     `unshare(2)` namespace nor an LD_PRELOAD `libsandbox.so` can
//     confine the in-process `Brush` interpreter (the same constraint
//     the scheduler's captured builds accept). The wrappers compose --
//     `unshare <flags> --map-root-user -- sh -c '<config>; exec "$@"' _
//     [sandbox] bash bin/ebuild.sh <phase>` -- see `Isolation` /
//     `phase_isolation` / `sandbox_wrapped_command`.
//     * `FEATURES=sandbox` (or `usersandbox`): `sandbox bash …` (real
//       `spawn_sandbox`) when `/usr/bin/sandbox` exists (real
//       `sandbox_capable`). `phase_env_vars` sets
//       `SANDBOX_LOG=${T}/sandbox.log` (real `doebuild.py:526`) and
//       `SANDBOX_DISABLED=0` for the wrapped phase, so `bin/ebuild.sh`
//       does its own real `SANDBOX_ON=1` + `addread /` + `addwrite
//       "${PORTAGE_TMPDIR}/portage"` setup; the `sandbox` binary logs
//       any write outside the build tree and exits non-zero, failing
//       the phase. The `bin/misc-functions.sh` calls (`install_qa_check`
//       post-`install`, `__dyn_package`) are `sandbox`-wrapped too, with
//       a separate `SANDBOX_LOG=${T}/sandbox-misc.log` (real
//       `MiscFunctionsProcess._spawn`). A missing binary degrades to an
//       unsandboxed run with a one-shot warning (real `_spawn`'s own
//       silent `free = True` fallback).
//     * `FEATURES=network-sandbox` -> `unshare --net` + `ip link set lo
//       up` plus the real `10.0.0.1/8` + `fd::1/8`
//       `AI_ADDRCONFIG`-workaround addresses (bug #690758) inside (real
//       `_configure_loopback_interface`). `FEATURES=ipc-sandbox` ->
//       `unshare --ipc`.
//       `FEATURES=mount-sandbox` -> `unshare --mount` + `mount
//       --make-rslave /` inside (real `_exec2`). `FEATURES=pid-sandbox`
//       -> `unshare --pid --fork --mount-proc` (real `CLONE_NEWPID` +
//       `pid-ns-init`; `--fork` stands in for the full init).
//     * `RESTRICT=network-sandbox` / `PROPERTIES=live` (unpack) /
//       `PROPERTIES=test_network` (test) exemptions ARE real now
//       (`phase_isolation`'s own doc comment) -- the phase env carries
//       USE-reduced `PORTAGE_RESTRICT`/`PORTAGE_PROPERTIES` too
//       (`restrict_and_properties`), the same real `doebuild_environment`
//       always sets, for real bash's own direct consumption
//       (`RESTRICT=test`/`RESTRICT=nostrip`/`RESTRICT=strip` skips).
//     * Cuts: SELinux sandbox (a kernel LSM feature with no meaning
//       outside a real SELinux-enabled host -- unlike the rest of this
//       set, there is no reasonable degrade to model, only a real
//       `libselinux`/policy dependency this scope has no use for);
//       `userpriv` / `fakeroot` -- these exist in real portage
//       specifically to drop privileges *from* an already-root process
//       (a build should not itself run as root); portuale's merge code
//       does now reproduce real's own `os.lchown`/`os.chown` calls
//       (`ebuild_merge.rs`'s `lchown_or_chown`), but that's a *merge*
//       (into `${ROOT}`) concern, not a *build* (`src_compile` etc.,
//       still never run as root) one -- there's still no privilege to
//       drop at build time and nothing for either feature to do here;
//       and, unlike real portage (which only unshares when
//       `uid == 0`), portuale always uses `--map-root-user` so it
//       works non-root -- an unavailable user namespace degrades with a
//       warning (real "Unable to unshare").
//   - `PORTAGE_PYM_PATH` (real portage's own Python-package import path)
//     is always set (see `phase_env_vars`'s own comment and
//     `pym_path_value`): `<checkout>/lib` when the portage checkout
//     exists, else `/` so `__dyn_clean`'s unconditional
//     `cd "${PORTAGE_PYM_PATH}"` still has a valid cwd. It was originally
//     left unset --
//     `create_directories` pre-creates `${PORTAGE_BUILDDIR}/empty` so
//     `bin/ebuild.sh`'s own "safe cwd" logic (EAPI 8's own comment:
//     "requires us to use an empty directory here") takes *that* branch
//     rather than `cd "${PORTAGE_PYM_PATH}" || die`, and it still does --
//     but upstream's `bin/` helper script that `import`s portage
//     (`ebuild-pyhelper`, reached in `real` oracle mode) `cd`s
//     `"${PORTAGE_PYM_PATH}" || exit 1` unconditionally, so with it
//     unset every `has_version`/`best_version` an eclass runs failed.
//   - `__source_all_bashrcs` (real per-profile/package bashrc hook
//     support, `/etc/portage/bashrc` and friends) is left unimplemented
//     -- also observed as a non-fatal "command not found" warning, not a
//     phase failure. Portuale has no profile/make.conf-driven bashrc
//     concept anywhere yet.
//   - Starting from the third phase in any given `ebuild_phases::
//     run_commands` call, sourcing `bin/ebuild.sh` prints six additional
//     "cannot mutate readonly variable" warnings (real `bin/ebuild.sh`'s
//     own `readonly SANDBOX_{ALLOW,ACTIVE,DENY,DEBUG,ON,PREDICT,READ,
//     WRITE}` declaration) -- observed, not yet root-caused (each phase
//     gets a genuinely fresh `Shell`, so this isn't the same
//     shared-shell issue `run_one_phase`'s own doc comment describes;
//     plausibly brush's own environment-variable inheritance across
//     `Shell` instances within one OS process, still unconfirmed).
//     Cosmetic: every phase still completes and returns the correct
//     exit status regardless.
//   - `EAPI` is read via the real PMS 7.3.1 rule directly from the
//     ebuild's own text (see `parse_eapi`) rather than through this
//     portuale's own md5-cache-reading machinery (`portage-repo`'s own
//     `read_md5_cache`) -- real `ebuild <file> <command>` operates on an
//     arbitrary standalone ebuild file, not necessarily one that's part
//     of a configured, md5-cache-indexed repo, so this mirrors real
//     `_parse_eapi_ebuild_head` instead.
//   - `PORTAGE_TMPDIR` defaults to `/var/tmp` (real portage's own
//     `make.globals` default; the builddir adds `portage/<cat>/<pf>`
//     itself) but is overridable via the `PORTAGE_TMPDIR`
//     environment variable -- portuale has no make.conf-reading path
//     into `ebuild.rs` at all yet, so an env var is the only override
//     mechanism, the same "env var, not full config resolution"
//     simplification `ROOT`/`PORTAGE_CONFIGROOT` already established for
//     `emerge`.
//   - `FILESDIR` (real `${PORTAGE_BUILDDIR}/files`, itself populated by
//     `prepare_build_dirs()` copying the repo's own `<category>/
//     <package>/files/` into it) is created here empty and never
//     populated -- this slice's own fixture ebuild deliberately avoids
//     `FILESDIR` entirely (writes its own scratch file under `${T}`
//     instead) rather than needing that copy step ported too.

use crate::fetch::{self, FetchOptions};

mod commands;
mod depend_phase;
mod environment;
mod fetch_sources;
mod misc_functions;
mod package_env;
mod phase_env;
mod post_install_meta;
mod run_phase;
mod runtime_paths;
mod sandbox;
mod scheduler_registry;
mod single_phase;
mod standalone_config;

#[cfg(test)]
mod tests;

pub use commands::*;
pub(crate) use depend_phase::*;
pub(crate) use environment::*;
pub(crate) use fetch_sources::*;
pub(crate) use misc_functions::*;
pub(crate) use package_env::*;
pub(crate) use phase_env::*;
pub(crate) use post_install_meta::*;
pub(crate) use run_phase::*;
pub(crate) use runtime_paths::*;
pub use sandbox::*;
pub(crate) use scheduler_registry::*;
pub(crate) use single_phase::*;
pub(crate) use standalone_config::*;

use brush_builtins::ShellBuilderExt as _;

use regex::Regex;

use std::os::unix::io::FromRawFd as _;

use std::path::{Path, PathBuf};

/// Real PMS 7.3.1: the ebuild's own EAPI is the value of an `EAPI=...`
/// assignment (optionally single- or double-quoted, an optional trailing
/// comment) on the first non-blank, non-comment line -- and *only* that
/// line; an `EAPI=` assignment anywhere else in the file is not the
/// ebuild's own EAPI at all (PMS's own rationale: the EAPI must be
/// knowable without evaluating any bash). No match at all (including an
/// ebuild whose first real line isn't an EAPI assignment) means EAPI "0",
/// matching real `doebuild.py`'s own "eapi = None" fallback. Mirrors real
/// `lib/portage/__init__.py`'s own `_parse_eapi_ebuild_head`/
/// `_pms_eapi_re` exactly.
fn parse_eapi(ebuild_text: &str) -> String {
    // Real PMS's own `\1` backreference (matching whichever quote char, if
    // any, opened the value) isn't expressible in Rust's `regex` crate
    // (deliberately no backreference support, for guaranteed linear-time
    // matching) -- expanded into three explicit alternatives instead
    // (double-quoted / single-quoted / bare), semantically identical.
    let eapi_re = Regex::new(
        r#"^[ \t]*EAPI=(?:"([A-Za-z0-9+_.-]*)"|'([A-Za-z0-9+_.-]*)'|([A-Za-z0-9+_.-]*))[ \t]*([ \t]#.*)?$"#,
    )
    .expect("static regex is valid");
    let comment_or_blank = Regex::new(r"^\s*(#.*)?$").expect("static regex is valid");

    for line in ebuild_text.lines() {
        if comment_or_blank.is_match(line) {
            continue;
        }
        return match eapi_re.captures(line) {
            Some(caps) => caps
                .get(1)
                .or_else(|| caps.get(2))
                .or_else(|| caps.get(3))
                .map(|m| m.as_str())
                .unwrap_or("")
                .to_string(),
            None => "0".to_string(),
        };
    }
    "0".to_string()
}

/// Real `doebuild.py`'s own `_pkgsplit`-derived `P`/`PN`/`PV`/`PR`/`PVR`.
/// Real `_pkgsplit` derives `PN` from the version-shaped *suffix* of a
/// bare `PF` string with no other information -- portuale's own
/// `portage-versions` crate doesn't port that algorithm (it only has
/// `ververify`/`vercmp`, not a name/version splitter), so this reuses the
/// same shortcut `portage-repo`'s own `strip_version_prefix` already
/// relies on instead: `PN` is simply the ebuild's own *parent directory*
/// name (real convention -- `<category>/<package>/<package>-<version>.
/// ebuild` -- that `doebuild_environment`'s own `os.path.basename(pkg_dir)
/// in (mysplit[0], mypv)` assertion actually checks holds, rather than
/// derives from scratch). `PR` is the trailing `-r<digits>` suffix of
/// what's left after stripping `PN-` (default `"r0"`, real portage's own
/// "no explicit revision" default) with `PV` being everything before it.
pub(crate) struct PackageSplit {
    pub(crate) p: String,
    pub(crate) pn: String,
    /// Real `PV`: the version *without* any `-r<digits>` revision suffix.
    pub(crate) pv: String,
    /// Real `PR`: `"r0"` when no explicit `-r<digits>` suffix was present
    /// (real portage's own "no explicit revision" default), otherwise the
    /// suffix itself.
    pub(crate) pr: String,
    /// Real `PVR`: `PV` alone when `PR` is `"r0"`, otherwise `PV-PR` --
    /// real portage's own "omit r0 from display" convention.
    pub(crate) pvr: String,
    pub(crate) pf: String,
}

fn split_package(ebuild_path: &Path, package_dir_name: &str) -> Result<PackageSplit, String> {
    let pf = ebuild_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("{}: not a valid file path", ebuild_path.display()))?
        .to_string();
    let pvr = pf
        .strip_prefix(package_dir_name)
        .and_then(|rest| rest.strip_prefix('-'))
        .ok_or_else(|| {
            format!(
                "{}: filename doesn't start with the parent directory's own name ({package_dir_name:?})",
                ebuild_path.display()
            )
        })?
        .to_string();
    let revision_re = Regex::new(r"^(.*)-(r\d+)$").expect("static regex is valid");
    let (pv, pr) = match revision_re.captures(&pvr) {
        Some(caps) => (caps[1].to_string(), caps[2].to_string()),
        None => (pvr.clone(), "r0".to_string()),
    };
    let pn = package_dir_name.to_string();
    let p = format!("{pn}-{pv}");
    Ok(PackageSplit {
        p,
        pn,
        pv,
        pr,
        pvr,
        pf,
    })
}

/// Real `doebuild.py:874-884`'s own `actionmap_deps`: the prerequisite
/// chain for `mydo`, run in order before `mydo` itself -- ported here as
/// the Rust-side driver loop (see this module's own doc comment for why
/// everything else stays real bash). Only the phase-only subset is
/// covered (see this module's own "KNOWN, DOCUMENTED GAPS"); an
/// unrecognized `mydo` returns just itself, letting the caller decide
/// whether that's valid for its own purposes.
fn phase_prerequisites(mydo: &str) -> Vec<&'static str> {
    const CHAIN: &[&str] = &[
        "pretend",
        "setup",
        "unpack",
        "prepare",
        "configure",
        "compile",
        "test",
        "install",
    ];
    match CHAIN.iter().position(|&p| p == mydo) {
        Some(idx) => CHAIN[..=idx].to_vec(),
        None => Vec::new(),
    }
}

/// Whether `command` is one this module can actually execute for real
/// (the `actionmap_deps`-chained phase subset) -- `ebuild.rs` checks this
/// before routing to `run_commands`, falling back to its own pre-existing
/// dry-run stub for everything else.
pub fn is_phase_command(command: &str) -> bool {
    phase_prerequisites(command).last() == Some(&command)
}

/// Whether `command` is a real, standalone single-phase `ebuild`
/// command with no `actionmap_deps` prerequisite chain at all -- real
/// `doebuild()`'s own `mydo in ("config", "help", "info", "postinst",
/// "preinst", "pretend", "postrm", "prerm")` early-return branch
/// (`lib/portage/package/ebuild/doebuild.py:1326-1351`, "running them
/// out of the sandbox -- and stop now"), narrowed to the four of those a
/// real admin/user actually invokes directly by name:
/// `config`/`info`/`prerm`/`postrm` -- `preinst`/`postinst` are real
/// too, but only ever reached internally, as part of `merge` (see
/// `ebuild_merge::run_merge`'s own doc comment: real `dblink.
/// treewalk()` invokes them directly around the actual file-copy step,
/// a real ordering constraint no standalone top-level invocation could
/// reproduce -- `preinst` must run *before* anything is merged,
/// `postinst` only *after*), so they stay internal-only; `pretend` is
/// already part of the `actionmap_deps` chain above; `help` this
/// portuale's own CLI already handles separately (`wants_help`).
/// `prerm`/`postrm` have no equivalent ordering constraint tying them to
/// `unmerge`'s own file-removal step the way `preinst`/`postinst` do to
/// `merge`'s -- real portage itself allows invoking them completely
/// standalone (e.g. to test a `pkg_prerm`/`pkg_postrm` function without
/// actually removing the package), so `unmerge`'s own internal use
/// (`ebuild_unmerge::run_unmerge`'s own doc comment) and this new
/// standalone path are simply two independent, real ways to reach the
/// same real phase function. `run_single_phase` (already used
/// internally for exactly this reason) is a direct fit for all four:
/// real `bin/phase-functions.sh`'s own `__ebuild_main` already accepts
/// them as literal phase arguments (`run_single_phase`'s own doc
/// comment), so no new phase-execution machinery is needed at all --
/// this is purely a CLI-routing addition.
pub fn is_standalone_phase_command(command: &str) -> bool {
    matches!(command, "config" | "info" | "prerm" | "postrm")
}
