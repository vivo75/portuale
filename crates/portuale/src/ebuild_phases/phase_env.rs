//! The environment variables and PATH a phase runs with.

use crate::ebuild_phases::*;

/// The real environment-variable block every real phase and every real
/// `bin/misc-functions.sh` `__dyn_*` command alike needs, as raw
/// `(name, value)` pairs -- shared by both shell backends (`Brush`
/// formats these into `export NAME=value` bash source text, see
/// `phase_setup_script` below; `Bash` passes them directly as real
/// subprocess environment variables, `std::process::Command::envs`,
/// with no shell-quoting step -- and so no `$`/backtick-expansion risk
/// -- at all) and by `run_one_phase`/`run_misc_functions` so the two
/// don't duplicate this. `extra_env` is appended verbatim, for anything
/// specific to one call site (e.g. `ebuild_package`'s own `PKGDIR`/
/// `PORTAGE_BINPKG_TMPFILE`).
/// Real `bin/ebuild.sh`'s own `eval "PORTAGE_ECLASS_LOCATIONS=(${{PORTAGE_ECLASS_LOCATIONS}})"`
/// (line ~611, run once, unconditionally, right after being sourced)
/// expects this env var's own raw string value to already be a
/// sequence of shell-single-quoted path tokens -- real `doebuild.py`'s
/// own `repo.eclass_db.eclass_locations_string` (`shlex.join(...)`)
/// builds it the same way. `inherit()` itself (also real, unmodified
/// bash) then walks that array looking for `<location>/eclass/
/// <name>.eclass` for every eclass named on an ebuild's own top-level
/// `inherit ...` line -- previously portuale never populated this
/// var at all, so `inherit()` always `die`d immediately for ANY
/// eclass, confirmed live against a real system: `sys-fs/fuse`,
/// `app-editors/nano`, and `app-arch/xz-utils` all failed here before
/// this fix, each on a different real eclass.
///
/// Real masters-chain resolution (`config.py:1256-1266`, `RepoConfigLoader.
/// __init__`): `eclass_locations = [master.location for master in
/// repo.masters] + ([repo.location] if repo.location not already in
/// there)`, then `eclass_db.eclass_locations_string` exports it
/// `reversed()` (`eclass_cache.py:177-179`) -- the ebuild's own
/// containing repo searched *first*, its masters after, in real
/// declared order. `repo.masters` itself (`RepoConfig::masters`, the
/// same real, already-resolved chain `ebuild_merge::
/// blocked_installed_packages` and `pretend.rs` already consult for
/// profile/USE config stacking) defaults to the main repo alone when no
/// explicit `masters =` key is present, empty for the main repo itself.
/// `repos.conf`/`config_root` resolution failure of any kind (missing
/// `repos.conf`, the containing repo not listed in it, etc.) degrades
/// to the previous v1 behavior -- the ebuild's own containing repo
/// alone, no masters chain -- the same graceful-degrade precedent
/// `blocked_installed_packages`'s own doc comment already established
/// for this exact `find_repos(config_root).ok()?` pattern: never a
/// false negative in the direction that could break an eclass lookup
/// that used to work. `None` (a standalone ebuild file outside any repo
/// checkout) exports an empty value either way -- `inherit()` still
/// `die`s for any eclass in that case, the same honest "nothing to
/// look in" real behavior a truly master-less, repo-less ebuild file
/// would hit too.
pub(crate) fn eclass_locations_value(pkg_dir: &Path, config_root: &Path) -> String {
    let Some(repo_root) = repo_root_for(pkg_dir) else {
        return String::new();
    };

    let masters: Vec<PathBuf> = (|| -> Option<Vec<PathBuf>> {
        let repos = portage_repo::find_repos(config_root).ok()?;
        let repo = repos.iter().find(|r| r.location == repo_root)?;
        Some(repo.masters.clone())
    })()
    .unwrap_or_default();

    let mut locations = masters;
    if !locations.contains(&repo_root) {
        locations.push(repo_root);
    }
    locations.reverse();

    locations
        .iter()
        .map(|p| format!("'{}'", p.display().to_string().replace('\'', r"'\''")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Absolute path of the running binary, exported as `PORTUALE_BIN` for
/// the vendored `bin/portageq-wrapper` shim and the `__helper` shims.
/// Under `cfg(test)` this resolves to the real `portuale` binary next
/// to `deps/` (#326 D8: never the libtest harness, whose stdout banner
/// would otherwise become the shim's answer).
pub(crate) fn portuale_bin_value() -> String {
    #[cfg(test)]
    {
        let mut exe = std::env::current_exe().expect("current test exe");
        exe.pop();
        if exe.ends_with("deps") {
            exe.pop();
        }
        exe.push("portuale");
        exe.display().to_string()
    }
    // #331: resolved once at startup, never `<path> (deleted)`.
    #[cfg(not(test))]
    {
        crate::self_exe::self_exe().display().to_string()
    }
}

/// Value for `PORTAGE_PYTHON` in a phase (#326 S2.3/D2): the
/// `portuale-python` shim next to the phase runtime in native mode,
/// `/usr/bin/python` with `PORTUALE_PYTHON_HELPERS=real`.
pub(crate) fn portage_python_value(bin_dir: &Path) -> String {
    if std::env::var("PORTUALE_PYTHON_HELPERS").as_deref() == Ok("real") {
        "/usr/bin/python".to_string()
    } else {
        bin_dir.join("portuale-python").display().to_string()
    }
}

#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn phase_env_vars(
    env: &Environment,
    root: &Path,
    ebuild_phase_value: &str,
    debug: bool,
    bin_dir: &Path,
    helpers_dir: &Path,
    config_root: &Path,
    extra_env: &[(String, String)],
) -> Vec<(String, String)> {
    // Real EPREFIX support is never enabled here (`EPREFIX=""`
    // unconditionally below), so real `ED="${D}"` (no prefix-relative
    // adjustment) collapses to exactly `D`'s own value -- no separate
    // computation needed.
    let d = eapi_path_var(&env.eapi, &format!("{}/", env.d().display()));
    let root_value = eapi_path_var(&env.eapi, &format!("{}/", root.display()));
    let resolved_features = features_string(extra_env);
    let path = phase_path(helpers_dir, extra_env, &resolved_features);
    // Config-derived base env for standalone runs (USE + the whole
    // resolved config env), computed once here so the single config load
    // serves both the `USE` entry below and the base pairs seeding
    // `vars`. `("", [])` for merge builds (their `extra_env` carries
    // everything) and the `depend` phase -- see the helper.
    let standalone_base_env =
        phase_standalone_base_env(env, config_root, root, ebuild_phase_value, extra_env);
    // #160: real `config.environ()`'s `filter_calling_env` drops every
    // config/calling-env variable not on `environ_whitelist` once this
    // phase's own `${T}/environment` exists, leaving the saved
    // environment to supply it -- so an earlier phase's `unset` (ninja's
    // `unset CFLAGS`) sticks. See `filter_calling_env`'s own doc comment.
    let calling_env_filtered = filter_calling_env(ebuild_phase_value, &env.t(), extra_env);
    // Real `doebuild_environment()` assigns its computed values *after*
    // the config is built, so they win over a same-named config key:
    // the resolved-config base pairs seed `vars` first and the computed
    // literal below overrides them (downstream `cmd.envs` is last-wins),
    // while the merge path's `extra_env` still overrides both at the end.
    let mut vars: Vec<(String, String)> = if calling_env_filtered {
        standalone_base_env
            .1
            .into_iter()
            .filter(|(k, _)| environ_whitelisted(k))
            .collect()
    } else {
        standalone_base_env.1
    };
    vars.extend(vec![
        ("EAPI".to_string(), env.eapi.clone()),
        ("PN".to_string(), env.split.pn.clone()),
        ("PV".to_string(), env.split.pv.clone()),
        ("PR".to_string(), env.split.pr.clone()),
        ("PVR".to_string(), env.split.pvr.clone()),
        ("P".to_string(), env.split.p.clone()),
        ("PF".to_string(), env.split.pf.clone()),
        ("CATEGORY".to_string(), env.category.clone()),
        ("EBUILD".to_string(), env.ebuild_abs.display().to_string()),
        ("ROOT".to_string(), root_value.clone()),
        ("EROOT".to_string(), root_value),
        (
            "PORTAGE_BUILDDIR".to_string(),
            env.portage_builddir.display().to_string(),
        ),
        ("WORKDIR".to_string(), env.workdir().display().to_string()),
        ("S".to_string(), env.s().display().to_string()),
        ("D".to_string(), d.clone()),
        ("ED".to_string(), d),
        ("T".to_string(), env.t().display().to_string()),
        ("HOME".to_string(), env.home().display().to_string()),
        ("FILESDIR".to_string(), env.filesdir().display().to_string()),
        (
            "PORTAGE_BIN_PATH".to_string(),
            bin_dir.display().to_string(),
        ),
        // feat#157 S6: the tracked `bin/portageq-wrapper` shim execs
        // `$PORTUALE_BIN portageq ...`; an absolute path so the shim never
        // depends on `PATH`.
        ("PORTUALE_BIN".to_string(), portuale_bin_value()),
        (
            "PORTAGE_ECLASS_LOCATIONS".to_string(),
            eclass_locations_value(&env.pkg_dir, config_root),
        ),
        ("PORTAGE_PYTHON".to_string(), portage_python_value(bin_dir)),
        // Real `doebuild.py:543`: `PORTAGE_COLORMAP = colormap()` -- bash
        // source `bin/isolated-functions.sh` `eval`s so an ebuild's
        // `elog`/`einfo` use the same (`color.map`-aware) colours.
        (
            "PORTAGE_COLORMAP".to_string(),
            crate::color::phase_colormap_export(),
        ),
        ("PATH".to_string(), path),
        (
            "PORTAGE_TMPDIR".to_string(),
            env.portage_tmpdir().display().to_string(),
        ),
        // Real `doebuild.py:526`: always set for a non-`depend` phase.
        // `bin/phase-functions.sh` deletes a stale one before each phase
        // and (for `FEATURES=sandbox`) `sandbox` appends violations to it.
        (
            "SANDBOX_LOG".to_string(),
            env.t().join("sandbox.log").display().to_string(),
        ),
        // `bin/phase-functions.sh`: `[[ ${SANDBOX_DISABLED:-0} = 0 ]] &&
        // export SANDBOX_ON=1` for the `src_*` phases. `"0"` only when
        // this phase is actually `sandbox`-wrapped (see
        // `fs_sandbox_for_phase` / `run_one_phase`), so the unwrapped
        // default stays exactly as before.
        (
            "SANDBOX_DISABLED".to_string(),
            if fs_sandbox_for_phase(ebuild_phase_value, &resolved_features) {
                "0"
            } else {
                "1"
            }
            .to_string(),
        ),
        // Real `doebuild_environment()` exports the package's effective
        // `USE` into every phase. Merge builds override this base value
        // downstream via `extra_env` (see `run_commands_async`); a
        // standalone `ebuild <file> <phase>` has no `extra_env` flags, so
        // the config-derived base is what its `use()` calls see.
        // `phase_standalone_base_env` returns `("", [])` for the
        // `depend` phase and whenever `extra_env` already carries `USE`.
        ("USE".to_string(), standalone_base_env.0.clone()),
        ("EPREFIX".to_string(), String::new()),
        ("EMERGE_FROM".to_string(), "ebuild".to_string()),
        ("PORTAGE_QUIET".to_string(), "1".to_string()),
        (
            "PORTAGE_DEBUG".to_string(),
            if debug { "1" } else { "0" }.to_string(),
        ),
        ("EBUILD_PHASE".to_string(), ebuild_phase_value.to_string()),
    ]);

    // Real `FEATURES` for the phase's own bash environment: the resolved
    // list whenever one is already in `vars` — the merge path's
    // `extra_env`-independent base is empty so the process value below
    // stands (and `extra_env` still overrides it downstream), while the
    // standalone `phase_environ` base already folds the run-wide list
    // with the per-entry package.env value and the calling env
    // (#98/#100), which a raw process-env literal must not clobber.
    // (No-config standalone runs keep the process value, as before.)
    if !vars.iter().any(|(k, _)| k == "FEATURES") {
        vars.push(("FEATURES".to_string(), phase_features_value()));
    }

    // Real `doebuild_environment()`: `PORTAGE_RESTRICT`/
    // `PORTAGE_PROPERTIES` are set for every phase, always, from the
    // package's own already-USE-reduced `RESTRICT`/`PROPERTIES` (real
    // `str(self._pkg.restrict)`/`str(self._pkg.properties)`). Real bash
    // consults these directly -- `phase-functions.sh:549`'s own
    // `RESTRICT=test` skip, `:777`'s `RESTRICT=nostrip`/
    // `RESTRICT=strip` -- rather than re-deriving them from `RESTRICT`/
    // `PROPERTIES` itself, which portuale's own phase env never exports
    // at all (only the ebuild's own bash sees the raw, unreduced values,
    // via its own sourced metadata). The `depend` phase reduces on the
    // config `USE` set (real `doebuild(mydo="depend")`'s own
    // `PORTAGE_USE`); every other phase reduces on the empty set -- see
    // `restrict_and_properties`'s own doc comment.
    let restrict_use_set = if ebuild_phase_value == "depend" {
        depend_use_set(env, config_root, root)
    } else {
        std::collections::HashSet::new()
    };
    let (restrict, properties) = restrict_and_properties(env, &restrict_use_set);
    vars.push(("PORTAGE_RESTRICT".to_string(), restrict));
    vars.push(("PORTAGE_PROPERTIES".to_string(), properties));

    // Real `INHERITED` (`porttree.py:872`): exported into every phase so
    // that when a non-`depend` phase re-sources the ebuild,
    // `bin/ebuild.sh`'s `__INHERITED_QA_CACHE=${INHERITED}` snapshot (then
    // `unset INHERITED`, then `source "${EBUILD}"`) lets the re-run
    // `inherit` calls find every eclass already known -- suppressing the
    // spurious `QA Notice: Eclass '…' inherited illegally in … <phase>`
    // real portage never emits here. See `Environment::inherited`.
    if let Some(inherited) = &env.inherited {
        vars.push(("INHERITED".to_string(), inherited.clone()));
    }

    // Real portage's `PORTAGE_PYM_PATH`: the `lib/` dir of the portage
    // checkout, where the `portage` python package lives. Upstream's
    // `bin/` helper scripts that import portage -- `ebuild-pyhelper`
    // (and its `chmod-lite`/`ebuild-ipc` symlinks) -- begin with
    // `cd "${PORTAGE_PYM_PATH}" || exit 1`, so
    // leaving it unset makes every one of them abort, which breaks
    // eclass `has_version` / `best_version` (they shell out to
    // `portageq`) for any real ebuild (e.g. `autotools.eclass`'s
    // automake probe). (The vendored `bin/portageq-wrapper` shim is
    // native since feat#157 S6 and never touches this variable; the
    // `cd` above is what upstream's scripts do in `real` oracle mode.)
    // `bin/ebuild.sh`'s own
    // cwd choice still prefers `${PORTAGE_BUILDDIR}/empty` (pre-created
    // by `create_directories`), so setting this does not regress the
    // "safe cwd for bug #469338" branch it was originally left unset for.
    // Always set: `<checkout>/lib` with a checkout (see `pym_path_value`),
    // else `/` so `__dyn_clean`'s unconditional
    // `cd "${PORTAGE_PYM_PATH}"` still has a valid cwd.
    vars.push((
        "PORTAGE_PYM_PATH".to_string(),
        pym_path_value(&portage_checkout()),
    ));

    // The phase process is spawned with a cleared environment, so the
    // database selection `mrg` exported for its children
    // (`PORTUALE_VDB_*`, see `portageq.rs`) must be forwarded explicitly or
    // `has_version`/`best_version` would read the files tree even on a
    // sqlite/redb ROOT. `PORTUALE_VDB_IPC` is the parent pipe of a redb run
    // (S6.3, `vdb_ipc.rs`). `bin/phase-functions.sh` keeps all of them out
    // of the saved environment (`PORTUALE_.*`).
    //
    // `PORTUALE_PYTHON_HELPERS` rides along so the `chmod-lite` shim
    // sees the D2 `real` mode inside the phase too.
    for k in [
        "PORTUALE_VDB_BACKEND",
        "PORTUALE_VDB_PATH",
        "PORTUALE_VDB_ROOT",
        "PORTUALE_PYTHON_HELPERS",
        crate::vdb_ipc::IPC_VAR,
    ] {
        if let Some(v) = std::env::var_os(k).filter(|v| !v.is_empty()) {
            vars.push((k.to_string(), v.to_string_lossy().into_owned()));
        }
    }
    // #318: the phase's `portageq` never resolves make.conf for the
    // backend itself; with no choice exported (an in-process caller that
    // ran no selection) it is the files tree, the default.
    if !vars.iter().any(|(k, _)| k == "PORTUALE_VDB_BACKEND") {
        vars.push(("PORTUALE_VDB_BACKEND".to_string(), "files".to_string()));
    }

    // `PATH` is consumed above as the base behind the helper dirs; a
    // verbatim `extra_env` pair would drop them again.
    vars.extend(
        extra_env
            .iter()
            .filter(|(k, _)| {
                k != "PATH"
                    && (!calling_env_filtered
                        || environ_whitelisted(k)
                        || gpkg_helper_setting(ebuild_phase_value, k))
            })
            .cloned(),
    );

    // Real `config.environ()` (`config.py:3403-3409`): when both
    // `PORTAGE_BUILDDIR` and `DISTDIR` are set, the real distfiles
    // directory moves to `PORTAGE_ACTUAL_DISTDIR` and `DISTDIR` becomes the
    // per-package fake `${PORTAGE_BUILDDIR}/distdir` (filled by
    // `prepare_fake_distdir`). Applied to every phase environment, after
    // the pairs above so the remapped values are the last ones set (#330).
    // No `DISTDIR` at all: real's `KeyError` guard leaves both untouched.
    if let Some(actual) = vars
        .iter()
        .rev()
        .find(|(k, _)| k == "DISTDIR")
        .map(|(_, v)| v.clone())
    {
        vars.retain(|(k, _)| k != "DISTDIR" && k != "PORTAGE_ACTUAL_DISTDIR");
        vars.push(("PORTAGE_ACTUAL_DISTDIR".to_string(), actual));
        vars.push((
            "DISTDIR".to_string(),
            env.fake_distdir().display().to_string(),
        ));
    }

    // Real `EbuildPhase._start` (`EbuildPhase.py:52-56`) calls
    // `split_LC_ALL(settings)` (`portage/util/locale.py:160`) before
    // spawning any phase: a set `LC_ALL` is copied to every
    // `locale_categories` entry and itself blanked (then deleted by
    // `config.environ()`; for EAPI 5+'s `posixish_locale` real even
    // asserts it is absent, `config.py:3374-3385`). The phase therefore
    // sees `LC_*`, never `LC_ALL`. The resolved `extra_env` value wins
    // over the calling process env, and any `LC_ALL` pair is filtered
    // out so it cannot leak back in. (The `--shell brush` path still
    // inherits the hosting process's own `LC_ALL` in its embedded
    // shell; the category exports below override it for every
    // subprocess the phase spawns.)
    vars.retain(|(k, _)| k != "LC_ALL");
    // #326 D6: no IPC daemon. `PORTAGE_IPC_DAEMON` is whitelisted by
    // real's `environ_whitelist` (mirrored above), so a value in the
    // calling environment or `extra_env` would otherwise reach the phase
    // and `has_version` would take the `ebuild-ipc` branch. Drop it from
    // the computed vars here; the two spawn sites filter it as well.
    vars.retain(|(k, _)| k != "PORTAGE_IPC_DAEMON");
    let lc_all = extra_env
        .iter()
        .rev()
        .find(|(k, _)| k == "LC_ALL")
        .map(|(_, v)| v.clone())
        .or_else(|| std::env::var("LC_ALL").ok())
        .unwrap_or_default();
    if !lc_all.is_empty() {
        for category in [
            "LC_COLLATE",
            "LC_CTYPE",
            "LC_MONETARY",
            "LC_MESSAGES",
            "LC_NUMERIC",
            "LC_TIME",
            "LC_ADDRESS",
            "LC_IDENTIFICATION",
            "LC_MEASUREMENT",
            "LC_NAME",
            "LC_PAPER",
            "LC_TELEPHONE",
        ] {
            vars.push((category.to_string(), lc_all.clone()));
        }
    }

    vars
}

/// Real `config.environ()`'s path-variable shape (`config.py:3392-3395`):
/// `D`/`ED`/`ROOT`/`EROOT` end with a trailing `/` only for EAPI <= 6
/// (`eapi.py:307`, `path_variables_end_with_trailing_slash`); later
/// EAPIs `rstrip("/")` them, so `ROOT=/` is exported as `""` and
/// `${ROOT}/usr/src/linux` (`linux-info.eclass`) stays `/usr/src/linux`.
/// `value` is passed with its trailing slash.
pub(crate) fn eapi_path_var(eapi: &str, value: &str) -> String {
    let trailing = matches!(eapi, "0" | "1" | "2" | "3" | "4" | "5" | "6");
    let base = value.trim_end_matches('/');
    if trailing {
        format!("{base}/")
    } else {
        base.to_string()
    }
}

/// Real `_doebuild_path` (`doebuild.py:332-378`), narrowed to the
/// `ebuild-helpers` prefix portuale's runner needs: the helper dir first,
/// then every entry of the base `PATH` not already listed. Under
/// `FEATURES=xattr` (on by default: `make.globals` appends it), real puts
/// `<bin>/ebuild-helpers/xattr` in front of the helper dir
/// (`doebuild.py:344-346`, #329). That directory holds the `install`
/// wrapper, so every `install` a phase runs (`doexe`, `dobin`, ebuild
/// code) goes through `install-xattr`, or `install.py` (portuale's
/// native one via `portuale-python`), and keeps xattrs. Neither can loop
/// back into the wrapper: `install-xattr` runs `/usr/bin/install` (or
/// `$REAL_INSTALL`), and `install.py`'s `Which("install", exclude=
/// __PORTAGE_HELPER_PATH)` skips the wrapper itself. The base is
/// the resolved config's `PATH` when the caller threaded one (the last
/// `extra_env` pair -- `portage_profile::phase_environ` exports it only
/// when `env.d` sets `PATH`, real's "ignore PATH from the calling
/// environment" rule), else the calling env's.
pub(crate) fn phase_path(
    helpers_dir: &Path,
    extra_env: &[(String, String)],
    features: &str,
) -> String {
    let base = extra_env
        .iter()
        .rev()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| std::env::var("PATH").unwrap_or_default());
    let helpers = helpers_dir.display().to_string();
    let xattr = helpers_dir.join("xattr").display().to_string();
    let mut parts = Vec::new();
    if feature_token_present(features, "xattr") {
        parts.push(xattr.as_str());
    }
    parts.push(helpers.as_str());
    let mut seen: std::collections::HashSet<&str> = parts.iter().copied().collect();
    for p in base.split(':') {
        if seen.insert(p) {
            parts.push(p);
        }
    }
    parts.join(":")
}

/// `Brush`-backend-only: `phase_env_vars` formatted as real `export
/// NAME=value` bash source text. Values are **single-quote escaped**
/// (`shell_single_quote`, G6 of the #37 plan): since the resolved
/// config env reaches `extra_env`, a value can now be arbitrary
/// config/ebuild text (`CFLAGS` with `$(…)`, `PORTAGE_COMPRESS_
/// EXCLUDE_SUFFIXES` globs, descriptions with quotes), and the previous
/// `{:?}`-formatted double-quoted export would have let the brush shell
/// word-split/expand/command-substitute it. `Bash` backend needs no such
/// text at all -- `phase_env_vars`'s own pairs are passed directly as
/// real subprocess environment variables instead, see
/// `run_one_phase_bash`/`run_misc_functions_bash`.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn phase_setup_script(
    env: &Environment,
    root: &Path,
    ebuild_phase_value: &str,
    debug: bool,
    bin_dir: &Path,
    helpers_dir: &Path,
    config_root: &Path,
    extra_env: &[(String, String)],
) -> String {
    let vars = phase_env_vars(
        env,
        root,
        ebuild_phase_value,
        debug,
        bin_dir,
        helpers_dir,
        config_root,
        extra_env,
    );
    let mut script = String::new();
    // The embedded brush shell starts from the inherited process env
    // (like the bash backend's subprocess). Real `doebuild` never
    // inherits wholesale -- drop every calling-env var real's
    // `environ_whitelist` wouldn't keep, before the `phase_env_vars`
    // exports set what portuale needs (L1-f, mirrors `run_one_phase_bash`).
    let set_here: std::collections::HashSet<&str> = vars.iter().map(|(k, _)| k.as_str()).collect();
    for (name, _) in std::env::vars() {
        if !environ_whitelisted(&name)
            && !set_here.contains(name.as_str())
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            && !name.is_empty()
            && !name.as_bytes()[0].is_ascii_digit()
        {
            script.push_str(&format!("unset {name}\n"));
        }
    }
    for (name, value) in vars {
        script.push_str(&format!("export {name}={}\n", shell_single_quote(&value)));
    }
    script
}

/// A POSIX single-quoted shell word: everything between `'…'` is
/// literal, and an embedded `'` is the usual `'\''` close/escape/reopen
/// dance. `$`, backticks, `\`, `"`, whitespace and newlines are all
/// inert inside it. Used by `phase_setup_script` for every brush
/// `export` value (G6 of the #37 plan).
pub(crate) fn shell_single_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for c in value.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}
