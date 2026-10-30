//! The phase shell backend, the calling-environment filter, and sandbox/isolation.

use crate::ebuild_phases::*;

/// Which shell executes a phase, and every `bin/*.sh` sourced along with it.
///
/// `Bash` (**the default**) is a genuine `bash <bin_dir>/ebuild.sh <phase>`
/// subprocess — Portage's `_doebuild_spawn` shape (`EBUILD_SH_ARGS="$*"` picks up
/// `<phase>`, and the tail really calls `__ebuild_main ${EBUILD_SH_ARGS}` and exits).
///
/// `Brush` is an embedded `brush_core::Shell` (see `Cargo.toml` for the pinned commit).
/// It never sets `EBUILD_SH_ARGS` — a bare `exit` inside an embedded shell would kill
/// the host process — so it uses "source, then separately `invoke_function`" instead.
///
/// `Bash` is the default because brush's `declare -f` serializer corrupts function
/// bodies containing redirected here-documents; `__save_ebuild_env` runs `declare -f`
/// between phases, so essentially every compiled package broke. `Brush` stays via
/// `--shell brush` (a portuale-only flag, not in `ebuild_options::OPTIONS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShellBackend {
    #[default]
    Bash,
    Brush,
}

/// The `src_*` phases sandboxing applies to. Portage sandboxes every phase outside
/// `_unsandboxed_phases` and unshares every phase outside `_ipc_phases`; for the
/// phases run here both come out to this list. `nofetch` is excluded (its own
/// private tmpdir, neither unshared nor wrapped).
/// Portage: doebuild.py, _doebuild_spawn
pub(crate) const SANDBOXED_SRC_PHASES: &str = include_str!("sandboxed_src_phases.txt");

/// The calling-env vars Portage's `config.environ()` carries into a phase (plus
/// `CCACHE_*`/`DISTCC_*`/`QEMU_*`). Phases spawn with the inherited env plus `phase_env_vars`
/// overrides; without this filter CLI-only vars (e.g. `EMERGE_DEFAULT_OPTS`) would
/// leak into the regenerated vdb `environment`.
/// Portage: special_env_vars.py. One name per line.
pub(crate) const ENVIRON_WHITELIST: &str = include_str!("environ_whitelist.txt");

/// Whether `name` is a line of the one-name-per-line `table`.
pub(crate) fn listed(table: &str, name: &str) -> bool {
    table.lines().any(|line| line == name)
}

/// Whether `key` survives Portage's `config.environ()` calling-env filter.
pub(crate) fn environ_whitelisted(key: &str) -> bool {
    listed(ENVIRON_WHITELIST, key)
        || key.starts_with("CCACHE_")
        || key.starts_with("DISTCC_")
        || key.starts_with("QEMU_")
}

/// Settings the native `gpkg-helper compress` reads from its env. The native helper
/// has no config loader, so `invoke_dyn_package` exports the resolved values and they
/// must survive the calling-env filter in the `package` phase — the one phase running
/// `__dyn_package`. They never reach any other phase.
/// Portage: gpkg-helper.py
pub(crate) fn gpkg_helper_setting(phase: &str, key: &str) -> bool {
    phase == "package"
        && (key == "BINPKG_COMPRESS"
            || key == "BINPKG_COMPRESS_FLAGS"
            || key.starts_with("BINPKG_COMPRESS_FLAGS_")
            || key.starts_with("BINPKG_GPG_SIGNING_"))
}

/// Portage's `filter_calling_env`: once `${T}/environment` exists, only whitelisted
/// calling-env keys pass; the saved environment supplies the rest, so an ebuild's
/// `unset` sticks. Exempts the init/fetch phases and standalone-`ebuild` `setup`.
/// Portage: config.py
pub(crate) fn filter_calling_env(
    phase: &str,
    t_dir: &Path,
    extra_env: &[(String, String)],
) -> bool {
    if matches!(phase, "clean" | "cleanrm" | "depend" | "fetch") {
        return false;
    }
    // `EMERGE_FROM` rides in `extra_env` for the binary/saved-env paths.
    let emerge_from = extra_env
        .iter()
        .rev()
        .find(|(k, _)| k == "EMERGE_FROM")
        .map(|(_, v)| v.as_str())
        .unwrap_or("ebuild");
    if phase == "setup" && emerge_from == "ebuild" {
        return false;
    }
    t_dir.join("environment").is_file()
}

/// The `FEATURES` string phase decisions consult: the last `extra_env` pair when
/// present (the resolved list), else the process env (standalone runs).
/// Portage: config.py, settings.features
pub(crate) fn features_string(extra_env: &[(String, String)]) -> String {
    extra_env
        .iter()
        .rev()
        .find(|(k, _)| k == "FEATURES")
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| std::env::var("FEATURES").unwrap_or_default())
}

/// A `FEATURES` token check over `features_string`'s output. CLI fallbacks elsewhere
/// keep their own process-env reads.
pub(crate) fn feature_token_present(features: &str, token: &str) -> bool {
    features.split_whitespace().any(|t| t == token)
}

/// `PORTAGE_CONFIGROOT` with Portage's guaranteed trailing slash
/// (`LocationsManager` normalizes `config_root.rstrip(sep) + sep`, so
/// `/` stays `/` and anything else gains one).
// Portage: LocationsManager.py
pub(crate) fn config_root_with_slash(config_root: &Path) -> String {
    let s = config_root.display().to_string();
    if s.ends_with('/') { s } else { format!("{s}/") }
}

/// `FEATURES=network-sandbox` present?
pub(crate) fn network_sandbox_requested(features: &str) -> bool {
    feature_token_present(features, "network-sandbox")
}

/// The non-`FEATURES` half of the net-unshare decision (pure, so unit-testable):
/// unpack of a `live` package, `test` of a `test_network` package, or
/// `RESTRICT=network-sandbox`. Inputs are already USE-reduced token strings.
/// Portage: doebuild.py, _doebuild_spawn
pub(crate) fn network_sandbox_exempt(phase: &str, restrict: &str, properties: &str) -> bool {
    (phase == "unpack" && flat_field_has_token(properties, &["live"]))
        || (phase == "test" && flat_field_has_token(properties, &["test_network"]))
        || flat_field_has_token(restrict, &["network-sandbox"])
}

/// Portage's `_spawn_actionmap` `droppriv` gate plus its per-phase
/// actionmap table: only `unpack`/`prepare`/`configure`/`compile`/`test`
/// demote, and only with `FEATURES=userpriv`, neither `RESTRICT` opt-out,
/// and `secpass >= 2` (effectively root here, per `is_root`).
// Portage: doebuild.py, _spawn_actionmap (:1780-1795) and actionmap (:1813-1870)
pub(crate) fn phase_droppriv(phase: &str, features: &str, restrict: &str, is_root: bool) -> bool {
    matches!(
        phase,
        "unpack" | "prepare" | "configure" | "compile" | "test"
    ) && feature_token_present(features, "userpriv")
        // Deviation: `nouserpriv` too, which `droppriv` (doebuild.py:1795) never
        // reads; brief-mandated, mirroring the `nosandbox` check at doebuild.py:1787.
        && !flat_field_has_token(restrict, &["userpriv", "nouserpriv"])
        && is_root
}

/// The identity a demoted phase runs as: Portage's `portage_uid`/`portage_gid`
/// plus its `userpriv_groups` list. The phase spawn demotes to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PhaseIds {
    pub(crate) uid: u32,
    pub(crate) gid: u32,
    pub(crate) groups: Vec<u32>,
}

/// Resolve [`PhaseIds`] for the configured build user: `username` when given,
/// else `PORTAGE_USERNAME`, else `portage` (Portage's `_portage_username`
/// default). `None` on any lookup failure so the caller degrades to no
/// demotion — the same shape as the unusable-unshare path.
// Portage: data.py, _portage_username (:249-265, :336) and portage_uid/portage_gid (:169);
// userpriv_groups (:224-247)
pub(crate) fn portage_phase_ids(username: Option<&str>) -> Option<PhaseIds> {
    use std::ffi::CString;

    let name = username.map(str::to_owned).unwrap_or_else(|| {
        std::env::var("PORTAGE_USERNAME").unwrap_or_else(|_| "portage".to_string())
    });
    let c_name = CString::new(name).ok()?;

    // SAFETY: `c_name` is a valid NUL-terminated string; `getpwnam` takes no
    // other arguments. The result points at a static buffer, so the fields
    // are copied out before the next NSS call can reuse it.
    let passwd = unsafe { libc::getpwnam(c_name.as_ptr()) };
    if passwd.is_null() {
        return None;
    }
    let (uid, gid) = unsafe { ((*passwd).pw_uid, (*passwd).pw_gid) };

    // `getgrouplist` reports the required count through `ngroups` and returns
    // -1 when the buffer is short; retry once with the reported size.
    let mut ngroups: libc::c_int = 16;
    let mut groups: Vec<libc::gid_t> = vec![0; ngroups as usize];
    // SAFETY: the buffer has `ngroups` entries, which `getgrouplist` respects.
    let mut rc = unsafe {
        libc::getgrouplist(
            c_name.as_ptr(),
            gid as libc::gid_t,
            groups.as_mut_ptr(),
            &mut ngroups,
        )
    };
    if rc < 0 {
        if ngroups <= 0 {
            return None;
        }
        groups.resize(ngroups as usize, 0);
        // SAFETY: as above, with the buffer resized to the required count.
        rc = unsafe {
            libc::getgrouplist(
                c_name.as_ptr(),
                gid as libc::gid_t,
                groups.as_mut_ptr(),
                &mut ngroups,
            )
        };
    }
    if rc < 0 {
        return None;
    }
    groups.truncate(ngroups.max(0) as usize);

    Some(PhaseIds {
        uid,
        gid,
        // `gid_t` is `u32` on Linux, the only target this backend supports.
        groups,
    })
}

/// Post-phase ownership fixup: a privileged phase may have left `$T` and the
/// homedir root-owned, so hand both back to the build user (`dirmode 0o700`,
/// `filemode 0o600`, masks `0`). Per-entry failures are warnings, never a
/// failed phase.
/// Portage: doebuild.py, _post_phase_userpriv_perms (:2485-2498)
pub(crate) fn apply_userpriv_post_perms(temp: &Path, home: &Path, ids: &PhaseIds) {
    if let Err(e) = std::fs::create_dir_all(home) {
        eprintln!("warning: could not create {}: {e}", home.display());
    }
    for top in [home, temp] {
        apply_recursive_perms(top, ids);
    }
}

/// `apply_recursive_permissions` (util/__init__.py:1266-1340): the top entry first,
/// then each directory's children before descending into any of them, never
/// following symlinks. Entries that vanish mid-walk are skipped silently.
// Portage: util/__init__.py, apply_recursive_permissions (:1266-1340)
fn apply_recursive_perms(top: &Path, ids: &PhaseIds) {
    let meta = match std::fs::symlink_metadata(top) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            eprintln!("warning: could not stat {}: {e}", top.display());
            return;
        }
    };
    apply_secpass_perms(top, ids, &meta);
    if meta.is_dir() {
        walk_perms(top, ids);
    }
}

fn walk_perms(dir: &Path, ids: &PhaseIds) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            eprintln!("warning: could not read {}: {e}", dir.display());
            return;
        }
    };
    let mut children = Vec::new();
    for entry in entries {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(_) => continue,
        };
        match std::fs::symlink_metadata(&path) {
            Ok(meta) => children.push((path, meta)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => eprintln!("warning: could not stat {}: {e}", path.display()),
        }
    }
    for (path, meta) in &children {
        apply_secpass_perms(path, ids, meta);
    }
    for (path, meta) in &children {
        if meta.is_dir() {
            walk_perms(path, ids);
        }
    }
}

/// `apply_secpass_permissions` with `follow_links=False` (util/__init__.py:1348)
/// and `apply_permissions` (:1161): `lchown` to `ids` when the owner differs,
/// then OR the owner mode into the entry (mask 0 adds bits, never strips them).
/// Symlinks get the owner change only.
// Portage: util/__init__.py, apply_secpass_permissions (:1348), apply_permissions (:1161)
fn apply_secpass_perms(path: &Path, ids: &PhaseIds, meta: &std::fs::Metadata) {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    let is_link = meta.file_type().is_symlink();
    let chowned = meta.uid() != ids.uid || meta.gid() != ids.gid;
    if chowned && let Err(e) = lchown(path, ids.uid, ids.gid) {
        eprintln!("warning: could not chown {}: {e}", path.display());
    }
    if is_link {
        return;
    }
    let want = if meta.is_dir() { 0o700 } else { 0o600 };
    let st_mode = meta.mode() & 0o7777;
    // `chown` clears setuid/setgid, so a chowned entry with those bits must be
    // re-chmodded even when the owner mode is already present.
    let keeps_special = chowned && st_mode & 0o6000 != 0;
    let target = st_mode | want;
    if (target != st_mode || keeps_special)
        && let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(target))
    {
        eprintln!("warning: could not chmod {}: {e}", path.display());
    }
}

fn lchown(path: &Path, uid: u32, gid: u32) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt as _;

    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    // SAFETY: `c_path` is a valid NUL-terminated string; `lchown` does not
    // follow a final symlink component.
    if unsafe { libc::lchown(c_path.as_ptr(), uid, gid) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Pre-phase ownership handoff: a demoted child must find a writable tree on
/// entry, so ensure the builddir spine, hand each entry to `ids`, and OR in
/// owner `rwx` (mask 0 adds bits, never strips — the same OR semantics as the
/// post-phase fixup, via [`apply_secpass_perms`]). Top-level only, symlinks
/// never followed. Per-entry failures are warnings, never a failed phase.
/// Portage: prepare_build_dirs.py, prepare_build_dirs (:96-106) and _prepare_workdir (:329-368)
pub(crate) fn apply_userpriv_pre_perms(ids: &PhaseIds, env: &Environment) {
    let builddir = env.portage_builddir().to_path_buf();
    let mut dirs: Vec<PathBuf> = Vec::with_capacity(6);
    if let Some(parent) = builddir.parent() {
        dirs.push(parent.to_path_buf());
    }
    dirs.extend([
        builddir,
        env.workdir(),
        env.t(),
        env.home(),
        env.t().join("logging"),
    ]);
    // A skipped (symlinked) entry also skips everything beneath it, so the
    // spine is never created through or followed — the same no-follow rule
    // `create_directories` applies to filesdir.
    let mut skipped: Vec<&PathBuf> = Vec::new();
    for dir in &dirs {
        if skipped.iter().any(|s| dir.starts_with(s)) {
            continue;
        }
        if std::fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink()) {
            skipped.push(dir);
            continue;
        }
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("warning: could not create {}: {e}", dir.display());
            continue;
        }
        match std::fs::symlink_metadata(dir) {
            Ok(meta) => apply_secpass_perms(dir, ids, &meta),
            Err(e) => eprintln!("warning: could not stat {}: {e}", dir.display()),
        }
    }
}

/// `FEATURES=sandbox` or `usersandbox` present (equivalent here: no `userpriv`).
/// Portage: process.py, _spawn
pub(crate) fn fs_sandbox_requested(features: &str) -> bool {
    feature_token_present(features, "sandbox") || feature_token_present(features, "usersandbox")
}

/// `/usr/bin/sandbox` when executable (Portage's `sandbox_capable`). `None`
/// degrades to an unsandboxed run.
/// Portage: const.py, process.py
pub(crate) fn sandbox_binary() -> Option<&'static Path> {
    use std::sync::OnceLock;
    static BIN: OnceLock<Option<PathBuf>> = OnceLock::new();
    BIN.get_or_init(|| {
        let p = PathBuf::from("/usr/bin/sandbox");
        // Matches Portage's `os.access(_, os.X_OK)`.
        if p.is_file()
            && std::fs::metadata(&p)
                .map(|m| {
                    use std::os::unix::fs::PermissionsExt as _;
                    m.permissions().mode() & 0o111 != 0
                })
                .unwrap_or(false)
        {
            Some(p)
        } else {
            None
        }
    })
    .as_deref()
}

/// True when sandboxing is requested, the phase is a `src_*` phase, and the binary
/// exists (else a one-shot warning and an unsandboxed run).
pub(crate) fn fs_sandbox_for_phase(phase: &str, features: &str) -> bool {
    use std::sync::OnceLock;
    if !fs_sandbox_requested(features) || !listed(SANDBOXED_SRC_PHASES, phase) {
        return false;
    }
    if sandbox_binary().is_some() {
        return true;
    }
    static WARNED: OnceLock<()> = OnceLock::new();
    WARNED.get_or_init(|| {
        eprintln!(
            "!!! /usr/bin/sandbox not found (for FEATURES=\"sandbox\"); \
             src_* phases run without filesystem confinement"
        );
    });
    false
}

/// Namespace/confinement wrappers for one phase's bash subprocess.
/// Portage: doebuild.py, _doebuild_spawn
#[derive(Clone, Copy, Default)]
pub(crate) struct Isolation {
    /// `FEATURES=network-sandbox` (`CLONE_NEWNET`).
    pub(crate) net: bool,
    /// `FEATURES=ipc-sandbox` (`CLONE_NEWIPC`).
    pub(crate) ipc: bool,
    /// `FEATURES=mount-sandbox` (`CLONE_NEWNS` + `mount --make-rslave /`).
    pub(crate) mount: bool,
    /// `FEATURES=pid-sandbox` (`CLONE_NEWPID`; `--fork` for the init, `--mount-proc`
    /// for the new `/proc` mount).
    pub(crate) pid: bool,
    /// `FEATURES=sandbox`/`usersandbox` (Portage's `spawn_sandbox`).
    pub(crate) fs_sandbox: bool,
}

impl Isolation {
    pub(crate) fn any_unshare(&self) -> bool {
        self.net || self.ipc || self.mount || self.pid
    }
    pub(crate) fn any(&self) -> bool {
        self.any_unshare() || self.fs_sandbox
    }
    /// `unshare(1)` flags. Non-root gets `--map-root-user` first so it can
    /// unshare at all. Root omits it: Portage unshares as root via `unshare(2)`
    /// with no new userns (process.py:1061), and a userns would map only the
    /// caller, leaving portage-owned build paths as overflow uid.
    pub(crate) fn unshare_flags(&self, is_root: bool) -> Vec<&'static str> {
        let mut f = if is_root {
            Vec::new()
        } else {
            vec!["--map-root-user"]
        };
        if self.net {
            f.push("--net");
        }
        if self.ipc {
            f.push("--ipc");
        }
        if self.mount {
            f.push("--mount");
        }
        if self.pid {
            f.extend(["--pid", "--fork", "--mount-proc"]);
        }
        f
    }
}

/// Whether `unshare <flags> -- true` succeeds here (cached per flag set; Portage
/// validates the same way via `_unshare_validator`). `false` drops the wrappers
/// with a warning — Portage's non-fatal "Unable to unshare".
pub(crate) fn unshare_combo_usable(flags: &[&str]) -> bool {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    let key = flags.join(" ");
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(&v) = cache.lock().unwrap().get(&key) {
        return v;
    }
    let ok = std::process::Command::new("unshare")
        .args(flags)
        .args(["--", "true"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    cache.lock().unwrap().insert(key, ok);
    ok
}

/// Raw `unshare(2)` flags for the demoted `pre_exec` path: the child is still
/// root there (demotion runs after), so no `CLONE_NEWUSER` is ever set —
/// exactly Portage's root `libc.unshare` with no userns. Pure and
/// unit-testable without root.
/// Portage: process.py, _exec (:693-714) — net adds `NEWNET|NEWUTS`, ipc
/// adds `NEWIPC`, mount adds `NEWNS`, pid adds `NEWPID|NEWNS`.
#[cfg(target_os = "linux")]
pub(crate) fn clone_flags_for(iso: &Isolation) -> libc::c_int {
    let mut flags = 0;
    if iso.net {
        flags |= libc::CLONE_NEWNET | libc::CLONE_NEWUTS;
    }
    if iso.ipc {
        flags |= libc::CLONE_NEWIPC;
    }
    if iso.mount {
        flags |= libc::CLONE_NEWNS;
    }
    if iso.pid {
        flags |= libc::CLONE_NEWPID | libc::CLONE_NEWNS;
    }
    flags
}

/// Non-Linux targets have no namespace APIs: the demoted spawn demotes
/// exactly as today, and `pre_exec` (whose unshare block is
/// `#[cfg(target_os = "linux")]`-gated) skips namespace setup.
#[cfg(not(target_os = "linux"))]
pub(crate) fn clone_flags_for(_: &Isolation) -> libc::c_int {
    0
}

/// Message for the once-per-process degrade warning: names the requested FEATURES and
/// the degraded phase; when demoted, states DAC confinement survives (the
/// phase still demotes to `portage:portage`, only namespace confinement is
/// lost). Pure so it is unit-testable without namespaces. `pre_exec` itself
/// stays print-free.
pub(crate) fn unshare_degrade_message(features: &str, phase: &str, demoted: bool) -> String {
    if demoted {
        format!(
            "!!! Unable to unshare (for FEATURES=\"{features}\"); \
             {phase} runs without namespace isolation but still as portage:portage"
        )
    } else {
        format!(
            "!!! Unable to unshare (for FEATURES=\"{features}\"); \
             {phase} runs without namespace isolation"
        )
    }
}

/// Wrappers for one phase from `FEATURES`: only `src_*` phases; an unusable unshare
/// combination degrades with one warning. `net` also carries the `networked`
/// exemption (live-`unpack`, `test_network`-`test`, `RESTRICT` opt-out).
/// `demoted` is [`run_one_phase`]'s already-computed gate outcome (`None`
/// ids, e.g. an unknown build user, arrive as `false`, leaving the
/// isolation undisturbed like the unusable-unshare path below).
/// Portage: doebuild.py, _doebuild_spawn
pub(crate) fn phase_isolation(
    env: &Environment,
    phase: &str,
    features: &str,
    demoted: bool,
    is_root: bool,
) -> Isolation {
    use std::sync::OnceLock;
    if !listed(SANDBOXED_SRC_PHASES, phase) {
        return Isolation::default();
    }
    let mut net = network_sandbox_requested(features);
    // Exemptions run for sandboxable phases only, so the empty-set reduction suffices.
    let (restrict, properties) = restrict_and_properties(env, &std::collections::HashSet::new());
    if net && network_sandbox_exempt(phase, &restrict, &properties) {
        net = false;
    }
    // A demoted phase keeps its namespaces: the child unshares raw via
    // `unshare(2)` while still root in `pre_exec`, then demotes to the
    // portage ids — Portage's `_exec` shape (process.py:1061-1084), where no
    // userns appears because no uid mapping is needed. (The `unshare(1)`
    // shim's `--map-root-user` maps only the caller, which is why the shim
    // never runs on the demoted path.) Only the `usersandbox` fs wrapper
    // rule below stays demotion-dependent, per `free = droppriv and not
    // usersandbox` (doebuild.py:2233-2240).
    let mut iso = Isolation {
        net,
        ipc: feature_token_present(features, "ipc-sandbox"),
        mount: feature_token_present(features, "mount-sandbox"),
        pid: feature_token_present(features, "pid-sandbox"),
        fs_sandbox: if demoted {
            feature_token_present(features, "usersandbox")
        } else {
            fs_sandbox_for_phase(phase, features)
        },
    };
    if iso.any_unshare() && !unshare_combo_usable(&iso.unshare_flags(is_root)) {
        static WARNED: OnceLock<()> = OnceLock::new();
        // Portage names the involved features (process.py:1097-1104).
        let mut involved: Vec<&str> = Vec::new();
        if iso.ipc {
            involved.push("ipc-sandbox");
        }
        if iso.mount {
            involved.push("mount-sandbox");
        }
        if iso.net {
            involved.push("network-sandbox");
        }
        if iso.pid {
            involved.push("pid-sandbox");
        }
        let msg = unshare_degrade_message(&involved.join(" "), phase, demoted);
        WARNED.get_or_init(|| {
            eprintln!("{msg}");
        });
        iso.net = false;
        iso.ipc = false;
        iso.mount = false;
        iso.pid = false;
    }
    iso
}

/// Portage's `spawn` `opt_name`: `[<cat>/<pf>]` plus ` bash` when the
/// phase runs free, ` sandbox` when sandboxed. Portage emits ` fakeroot`
/// under its fakeroot wrapper; portuale has none, so it never appears.
/// Empty `CATEGORY`/`PF` stays empty, like Portage's `mycpv is None`
/// `[{CATEGORY}/{PF}]` fallback.
/// Portage: doebuild.py, spawn (:2243-2261)
pub(crate) fn phase_opt_name(category: &str, pf: &str, sandboxed: bool) -> String {
    let suffix = if sandboxed { " sandbox" } else { " bash" };
    format!("[{category}/{pf}]{suffix}")
}

/// Single-quote `s` for embedding in the shell shim below.
fn sh_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The fully-built phase argv: the `libsandbox` prefix (when present — its
/// presence was already confirmed before `fs_sandbox` was set) plus
/// `bash <script> <arg>`. Single source: both the shim branch below and the
/// demoted direct spawn take this argv, never re-derived, so the Task 2
/// usersandbox control (namespaces + `libsandbox` compose) cannot regress.
pub(crate) fn phase_argv(script: &Path, arg: &str, iso: &Isolation) -> Vec<std::ffi::OsString> {
    use std::ffi::OsString;
    let mut argv: Vec<OsString> = Vec::new();
    if iso.fs_sandbox
        && let Some(bin) = sandbox_binary()
    {
        argv.push(bin.into());
    }
    argv.push("bash".into());
    argv.push(script.into());
    argv.push(arg.into());
    argv
}

/// A direct `Command` from the fully-built `argv` with `argv[0]` renamed to
/// `opt_name`: the demoted spawn (namespaces happen in its own `pre_exec`,
/// so the `unshare(1)` shim — which would demote the wrapper process instead
/// of the phase — never runs there) and the unisolated fast path below.
pub(crate) fn direct_command(argv: &[std::ffi::OsString], opt_name: &str) -> std::process::Command {
    use std::os::unix::process::CommandExt as _;
    debug_assert!(!argv.is_empty(), "phase argv always holds bash at least");
    let mut c = std::process::Command::new(&argv[0]);
    c.args(&argv[1..]);
    c.arg0(opt_name);
    c
}

/// A blocking `Command` running `bash <script> <arg>` (Portage's `EBUILD_SH_BINARY`
/// shape) wrapped per `iso`, running under `opt_name` (Portage's `spawn`
/// `opt_name`, via [`phase_opt_name`]):
/// `unshare <flags> -- bash -c '<config>; exec -a '<opt>' "$@"' portuale-sandbox [sandbox] bash <script> <arg>`.
/// The `bash -c` shim configures inside each namespace what Portage's `_exec2` does
/// (loopback + workaround addresses for `--net`, `--make-rslave` for `--mount`);
/// `bin/*.sh` itself does the `SANDBOX_ON`/`addread`/`addwrite` setup from the env.
/// `bash` (not `sh`) interprets the shim because only it provides `exec -a`;
/// `sh` may be dash. The outer `unshare` keeps its own name: Portage has no
/// such wrapper process (it unshares via `unshare(2)` in the child). The
/// in-process `--shell brush` backend has no child at all (a cut, not a
/// divergence). `arg0` renames only `argv[0]`; `get_args` still starts at
/// the real binary.
/// Portage: doebuild.py, spawn (:2243-2261); process.py, _exec (:1027-1036)
pub(crate) fn sandbox_wrapped_command(
    script: &Path,
    arg: &str,
    iso: Isolation,
    opt_name: &str,
    is_root: bool,
) -> std::process::Command {
    let argv = phase_argv(script, arg, &iso);

    if !iso.any_unshare() {
        return direct_command(&argv, opt_name);
    }

    let mut shim = String::new();
    if iso.net {
        shim.push_str(
            "ip link set lo up 2>/dev/null; \
             ip addr add 10.0.0.1/8 dev lo 2>/dev/null; \
             ip -6 addr add fd::1/8 dev lo 2>/dev/null; ",
        );
    }
    if iso.mount {
        shim.push_str("mount --make-rslave / 2>/dev/null; ");
    }
    shim.push_str(&format!("exec -a {} \"$@\"", sh_single_quote(opt_name)));

    let mut c = std::process::Command::new("unshare");
    c.args(iso.unshare_flags(is_root))
        .arg("--")
        .arg("bash")
        .arg("-c")
        .arg(shim)
        .arg("portuale-sandbox")
        .args(&argv);
    c
}
