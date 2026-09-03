"""Black-box test for the emerge/ebuild portuale skeleton (see
docs/agent-context.md, "emerge/ebuild binary shape"). Tests the real compiled
CLI via symlinks in a PATH, exactly as it would be invoked in practice --
not by importing anything from the binary.

Also covers `ebuild`'s CLI-surface-recognition follow-up (see
rust/portuale/src/ebuild.rs/ebuild_options.rs): real ebuild
options (bin/ebuild's own argparse setup) and real ebuild commands
(doebuild()'s own validcommands list) are recognized and accepted as a
still-a-no-op dry-run stub, while genuinely invalid input (an
unrecognized option, a bad filename, an unrecognized command, or
missing required args) is now rejected with a specific message and a
real exit code -- unlike `emerge --pretend`, `ebuild` has no Python
reference implementation to contract-test against, since it has no
real behavior to keep in sync between two implementations; this file
is the only test surface for it.
"""

import os
import subprocess
import tarfile
from pathlib import Path

import pytest

FIXTURES_ROOT = str(Path(__file__).resolve().parents[1] / "fixtures")


def _fixture_env():
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = FIXTURES_ROOT
    return env


def test_dispatch_via_symlink_emerge(portuale_binary, tmp_path):
    """Exercises real `emerge --pretend` resolution (see
    test_emerge_pretend_contract.py for full coverage of the outcomes);
    here the point is that the symlink-dispatched binary reaches it at
    all."""
    emerge_link = tmp_path / "emerge"
    emerge_link.symlink_to(portuale_binary)
    result = subprocess.run(
        [str(emerge_link), "--pretend", "dev-libs/newpkg"],
        capture_output=True,
        text=True,
        check=True,
        env=_fixture_env(),
    )
    assert result.stdout.strip() == "[ebuild  N     ] dev-libs/newpkg-1.0"


def test_dispatch_via_symlink_ebuild(portuale_binary, tmp_path):
    ebuild_link = tmp_path / "ebuild"
    ebuild_link.symlink_to(portuale_binary)
    result = subprocess.run(
        [str(ebuild_link), "foo-1.0.ebuild", "clean"],
        capture_output=True,
        text=True,
        check=True,
    )
    assert "ebuild: dry run" in result.stdout


def test_dispatch_via_path_lookup_by_bare_name(portuale_binary, tmp_path):
    """The real-world usage pattern: PATH contains a directory of applet
    symlinks, and the applet is invoked by bare name. Proves the binary is
    a drop-in for tooling that calls `emerge`/`ebuild` directly."""
    (tmp_path / "emerge").symlink_to(portuale_binary)
    (tmp_path / "ebuild").symlink_to(portuale_binary)
    env = _fixture_env()
    env["PATH"] = f"{tmp_path}{os.pathsep}{env.get('PATH', '')}"

    result = subprocess.run(
        ["emerge", "--pretend", "dev-libs/newpkg"],
        capture_output=True,
        text=True,
        check=True,
        env=env,
    )
    assert result.stdout.strip() == "[ebuild  N     ] dev-libs/newpkg-1.0"


def test_explicit_arg_fallback_dispatch(portuale_binary):
    """Invoked under its own name (no symlink), the binary still dispatches
    via an explicit first argument, busybox-style, so it's testable and
    usable without setting up symlinks."""
    result = subprocess.run(
        [str(portuale_binary), "ebuild", "foo-1.0.ebuild", "clean"],
        capture_output=True,
        text=True,
        check=True,
    )
    assert "ebuild: dry run" in result.stdout


def test_no_applet_prints_the_applet_list(portuale_binary):
    """A bare `portuale` (no symlink, no applet name) lists the applets
    and exits 0 -- busybox-style -- rather than erroring."""
    result = subprocess.run(
        [str(portuale_binary)], capture_output=True, text=True, check=False
    )
    assert result.returncode == 0
    assert result.stderr == ""
    assert result.stdout.startswith("portuale: a multicall binary")
    assert "Applets:" in result.stdout
    stripped = [line.strip() for line in result.stdout.splitlines()]
    for name in ("emerge", "ebuild"):
        row = next(line for line in stripped if line.startswith(name + " "))
        description = row.split(None, 1)[1]
        assert len(description) < 120, (name, len(description))


@pytest.mark.parametrize("flag", ["-h", "--help"])
def test_help_flag_prints_the_applet_list(portuale_binary, flag):
    result = subprocess.run(
        [str(portuale_binary), flag], capture_output=True, text=True, check=False
    )
    assert result.returncode == 0
    assert result.stdout.startswith("portuale: a multicall binary")
    assert "   emerge   " in result.stdout
    assert "   ebuild   " in result.stdout


def test_unrecognized_applet_fails_clearly(portuale_binary):
    result = subprocess.run(
        [str(portuale_binary), "frobnicate"], capture_output=True, text=True, check=False
    )
    assert result.returncode != 0
    assert 'unrecognized applet "frobnicate"' in result.stderr


def test_ebuild_accepts_multiple_real_commands(ebuild_binary):
    """Real ebuild invocations commonly chain several phases in one call
    (e.g. "clean compile install") -- all still just recognized, still a
    no-op stub."""
    result = subprocess.run(
        [str(ebuild_binary), "foo-1.0.ebuild", "clean", "compile", "install"],
        capture_output=True,
        text=True,
        check=True,
    )
    assert "ebuild: dry run" in result.stdout


def test_ebuild_accepts_a_real_value_option_without_misreading_its_value(ebuild_binary):
    """--color is a real ebuild option that takes a value (see
    bin/ebuild's own argparse setup) -- its value ("y") must not be
    misinterpreted as the ebuild file or an extra command."""
    result = subprocess.run(
        [str(ebuild_binary), "--color", "y", "foo-1.0.ebuild", "clean"],
        capture_output=True,
        text=True,
        check=True,
    )
    assert "ebuild: dry run" in result.stdout
    assert 'ebuild file: "foo-1.0.ebuild"' in result.stdout
    assert 'commands: ["clean"]' in result.stdout


def test_ebuild_accepts_the_inline_equals_form_of_a_value_option(ebuild_binary):
    result = subprocess.run(
        [str(ebuild_binary), "--color=y", "foo-1.0.ebuild", "clean"],
        capture_output=True,
        text=True,
        check=True,
    )
    assert "ebuild: dry run" in result.stdout


def test_ebuild_rejects_an_unrecognized_option(ebuild_binary):
    """Distinct from a real-but-unimplemented option: a token that isn't
    in bin/ebuild's own option surface at all is rejected immediately
    and specifically, unlike real bin/ebuild's own argparse (which uses
    parse_known_args and would silently swallow it into the positional
    args instead -- see ebuild.rs's doc comment for why portuale
    deviates)."""
    result = subprocess.run(
        [str(ebuild_binary), "--not-a-real-option", "foo-1.0.ebuild", "clean"],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 1
    assert result.stderr.strip() == 'ebuild: unrecognized option "--not-a-real-option"'


def test_ebuild_rejects_a_filename_not_ending_in_dot_ebuild(ebuild_binary):
    result = subprocess.run(
        [str(ebuild_binary), "foo-1.0.tar.gz", "clean"],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 1
    assert result.stderr.strip() == 'ebuild: "foo-1.0.tar.gz": does not end with ".ebuild"'


def test_ebuild_rejects_an_unrecognized_command(ebuild_binary):
    """"not-a-real-phase" isn't in doebuild()'s own validcommands list,
    so it must be rejected the same way real doebuild() itself would
    (exit 1), not silently accepted as if it were a real, merely
    unimplemented phase."""
    result = subprocess.run(
        [str(ebuild_binary), "foo-1.0.ebuild", "not-a-real-phase"],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 1
    assert (
        result.stderr.strip()
        == 'ebuild: "not-a-real-phase" is not one of the valid ebuild commands'
    )


def test_ebuild_rejects_missing_required_args(ebuild_binary):
    """Mirrors real bin/ebuild's own argparse parser.error() exit code
    (2) for "missing required args", distinct from the exit-1 "invalid
    input" cases above."""
    no_args = subprocess.run(
        [str(ebuild_binary)], capture_output=True, text=True, check=False
    )
    assert no_args.returncode == 2
    assert no_args.stderr.strip() == "ebuild: missing required args"

    no_command = subprocess.run(
        [str(ebuild_binary), "foo-1.0.ebuild"], capture_output=True, text=True, check=False
    )
    assert no_command.returncode == 2
    assert no_command.stderr.strip() == "ebuild: missing required args"


def test_ebuild_help_is_implemented(ebuild_binary):
    """-h/--help are real and implemented now (real bin/ebuild's own
    argparse auto-adds them) -- previously neither was even in
    ebuild_options.rs's own OPTIONS table at all, so a bare "--help"
    invocation used to be rejected as an unrecognized option instead of
    printing help and exiting 0."""
    result = subprocess.run(
        [str(ebuild_binary), "--help"], capture_output=True, text=True, check=False
    )
    assert result.returncode == 0
    assert result.stderr == ""
    assert result.stdout.startswith(
        "ebuild: command-line interface to the Portuale package manager"
    )


def test_ebuild_short_help_alias_is_implemented(ebuild_binary):
    result = subprocess.run(
        [str(ebuild_binary), "-h"], capture_output=True, text=True, check=False
    )
    assert result.returncode == 0
    assert result.stdout.startswith(
        "ebuild: command-line interface to the Portuale package manager"
    )


def test_ebuild_help_wins_unconditionally_regardless_of_position_or_other_args(
    ebuild_binary,
):
    """Matches real bin/ebuild's own behavior: argparse's own -h/--help
    action is checked during parsing itself, so it wins no matter where
    it appears or what else (valid or not) accompanies it -- same
    precedent emerge's own --help already set."""
    for args in (
        ["foo-1.0.ebuild", "clean", "--help"],
        ["--not-a-real-option", "--help"],
        ["-h", "foo-1.0.ebuild"],
    ):
        result = subprocess.run(
            [str(ebuild_binary), *args], capture_output=True, text=True, check=False
        )
        assert result.returncode == 0, args
        assert result.stdout.startswith(
            "ebuild: command-line interface to the Portuale package manager"
        ), args


def test_ebuild_version_is_recognized_but_not_specially_implemented(ebuild_binary):
    """--version is deliberately NOT specially implemented -- see
    ebuild_options.rs's own doc comment: real bin/ebuild's own
    portage.VERSION is derived live via "git describe" for a
    from-source checkout (exactly what this repo is), not a static
    string, so printing a real version was ruled out. It's still a real,
    recognized ebuild option like the other five, though -- ebuild.rs's
    own stub treats every declared option uniformly (accepted, no
    special behavior), unlike emerge's own CLI, which explicitly rejects
    every merely-recognized-but-unimplemented option by name."""
    result = subprocess.run(
        [str(ebuild_binary), "--version", "foo-1.0.ebuild", "clean"],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0
    assert "ebuild: dry run" in result.stdout


def test_ebuild_debug_flag_enables_real_set_x_tracing(ebuild_binary, tmp_path):
    """`--debug` is real, not a no-op: unlike every other `Kind::Boolean`
    ebuild option (still a pure no-op -- see the module docstring above),
    ebuild.rs sets real `PORTAGE_DEBUG=1` in the phase environment, which
    triggers real `bin/ebuild.sh`'s own `set -x` guard (`bin/ebuild.sh`
    lines 479/680, `bin/phase-functions.sh`'s own phase-dispatch case
    branches) during real phase execution (task #54). This exercises
    task #54's real `pretend` phase (a fast, side-effect-free real phase)
    against the `mergepkg` fixture, not the dry-run stub -- so unlike
    every other test in this file, it needs its own writable ROOT/
    PORTAGE_TMPDIR rather than the shared, read-only fixture ROOT."""
    ebuild_path = str(
        Path(FIXTURES_ROOT) / "repo/dev-libs/mergepkg/mergepkg-1.0.ebuild"
    )
    env = dict(os.environ)
    env["ROOT"] = str(tmp_path / "root")
    env["PORTAGE_TMPDIR"] = str(tmp_path / "portage-tmpdir")

    without_debug = subprocess.run(
        [str(ebuild_binary), ebuild_path, "pretend"],
        capture_output=True,
        text=True,
        check=True,
        env=env,
    )
    assert not any(
        line.startswith("+ ") for line in without_debug.stderr.splitlines()
    )

    with_debug = subprocess.run(
        [str(ebuild_binary), "--debug", ebuild_path, "pretend"],
        capture_output=True,
        text=True,
        check=True,
        env=env,
    )
    assert any(line.startswith("+ ") for line in with_debug.stderr.splitlines())


def _real_build_env(tmp_path):
    """`ROOT` stays the real, read-only fixture tree (`run_package`'s own
    real chain never writes under `ROOT` at all -- only `${D}`/`PKGDIR`,
    both `tmp_path`-relative here), matching how portuale's own manual
    verification of `ebuild <file> package` and `emerge --buildpkgonly`
    already proved this is safe."""
    env = _fixture_env()
    env["PORTAGE_TMPDIR"] = str(tmp_path / "portage-tmpdir")
    env["PKGDIR"] = str(tmp_path / "pkgdir")
    return env


def test_emerge_buildpkgonly_without_pretend_really_builds_a_binary_package(
    emerge_binary, tmp_path
):
    """The feature this whole slice is about: `emerge --buildpkgonly
    <atom>` -- deliberately WITHOUT `--pretend` -- is the one real,
    non-dry-run execution path portuale implements for `emerge`
    itself (see emerge_build.rs's own module doc comment). `packagepkg`
    RDEPENDs on `samepkg`, which the shared fixture ROOT already has an
    installed vdb entry for, so --buildpkgonly's own real depgraph gate
    (see the dry-run contract tests) has nothing to object to."""
    env = _real_build_env(tmp_path)
    result = subprocess.run(
        [str(emerge_binary), "--buildpkgonly", "dev-libs/packagepkg"],
        capture_output=True,
        text=True,
        check=True,
        env=env,
    )
    assert "[ebuild  N     ] dev-libs/packagepkg-1.0" in result.stdout
    assert ">>> Building binary for dev-libs/packagepkg-1.0..." in result.stdout

    tbz2 = Path(env["PKGDIR"]) / "dev-libs/packagepkg-1.0.tbz2"
    assert tbz2.is_file()
    assert b"XPAKPACK" in tbz2.read_bytes()

    packages = (Path(env["PKGDIR"]) / "Packages").read_text()
    assert "CPV: dev-libs/packagepkg-1.0" in packages
    assert "RDEPEND: dev-libs/samepkg" in packages


def test_emerge_buildpkgonly_with_binpkg_format_gpkg_builds_a_real_gpkg_tar(
    emerge_binary, tmp_path
):
    """`BINPKG_FORMAT=gpkg` routes real, unmodified `bin/misc-functions.sh
    __dyn_package` to real, unmodified `bin/gpkg-helper.py compress`
    (real `portage.gpkg.gpkg().compress()`) instead of the xpak
    `xpak-helper.py` path -- producing a genuine `<cat>/<pf>.gpkg.tar`
    (an outer tar of `gpkg-1` / `metadata.tar.<comp>` / `image.tar.<comp>`
    / `Manifest`). `BINPKG_COMPRESS=gzip` keeps this off `zstd` (the real
    default) so the test doesn't need it installed."""
    env = _real_build_env(tmp_path)
    env["BINPKG_FORMAT"] = "gpkg"
    env["BINPKG_COMPRESS"] = "gzip"
    result = subprocess.run(
        [str(emerge_binary), "--buildpkgonly", "dev-libs/packagepkg"],
        capture_output=True,
        text=True,
        check=True,
        env=env,
    )
    assert ">>> Building binary for dev-libs/packagepkg-1.0..." in result.stdout

    gpkg = Path(env["PKGDIR"]) / "dev-libs/packagepkg-1.0.gpkg.tar"
    assert gpkg.is_file()
    assert not (Path(env["PKGDIR"]) / "dev-libs/packagepkg-1.0.tbz2").exists()

    with tarfile.open(gpkg, "r") as container:
        names = {Path(n).name for n in container.getnames()}
    assert "gpkg-1" in names
    assert "metadata.tar.gz" in names
    assert "image.tar.gz" in names
    assert "Manifest" in names

    packages = (Path(env["PKGDIR"]) / "Packages").read_text()
    assert "CPV: dev-libs/packagepkg-1.0" in packages
    assert "PATH: dev-libs/packagepkg-1.0.gpkg.tar" in packages


def test_emerge_buildpkgonly_with_pretend_stays_dry_run(emerge_binary, tmp_path):
    """The exact same atom as the real-build test above, but with
    `--pretend` also given -- must stay a pure dry-run report, matching
    real portage's own `--pretend` always suppressing every real action
    regardless of what else is requested alongside it."""
    env = _real_build_env(tmp_path)
    result = subprocess.run(
        [str(emerge_binary), "--pretend", "--buildpkgonly", "dev-libs/packagepkg"],
        capture_output=True,
        text=True,
        check=True,
        env=env,
    )
    assert result.stdout.strip() == "[ebuild  N     ] dev-libs/packagepkg-1.0"
    assert "Building binary" not in result.stdout
    assert not (Path(env["PKGDIR"]) / "dev-libs").exists()


def test_emerge_regen_regenerates_md5_cache_from_the_depend_phase(
    emerge_binary, tmp_path
):
    """`emerge --regen` (real action_regen -> MetadataRegen): run every
    ebuild's `depend` phase and (re)write `metadata/md5-cache/<cat>/<pf>`
    in real portage.cache.flat_hash format -- sorted keys, empty values
    omitted, `_md5_=<md5 of the ebuild>` last. Exercised on a tiny
    standalone repo so it stays fast. Then a resolve against the freshly
    generated cache must work."""
    import hashlib

    repo = tmp_path / "repo"
    (repo / "dev-libs" / "regenpkg").mkdir(parents=True)
    (repo / "profiles").mkdir(parents=True)
    (repo / "profiles" / "repo_name").write_text("regentest\n")
    ebuild = repo / "dev-libs" / "regenpkg" / "regenpkg-1.0.ebuild"
    ebuild_text = (
        'EAPI=8\n'
        'DESCRIPTION="regen test"\n'
        'SLOT="0"\n'
        'KEYWORDS="amd64"\n'
        'IUSE="foo"\n'
        'RDEPEND="dev-libs/other"\n'
    )
    ebuild.write_text(ebuild_text)

    cfg = tmp_path / "cfg"
    (cfg / "etc" / "portage").mkdir(parents=True)
    (cfg / "etc" / "portage" / "repos.conf").write_text(
        f"[DEFAULT]\nmain-repo = regentest\n\n[regentest]\nlocation = {repo}\n"
    )

    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = str(cfg)
    env["ROOT"] = str(cfg)
    env["PORTAGE_RUNNING_ROOT"] = "/"
    env["PORTAGE_TMPDIR"] = str(tmp_path / "pt")

    result = subprocess.run(
        [str(emerge_binary), "--regen"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )
    assert result.returncode == 0, result.stderr
    assert "Regenerating cache entries..." in result.stdout
    assert "Processing dev-libs/regenpkg" in result.stdout
    assert result.stdout.rstrip().endswith("done!")

    entry = repo / "metadata" / "md5-cache" / "dev-libs" / "regenpkg-1.0"
    md5 = hashlib.md5(ebuild_text.encode()).hexdigest()
    assert entry.read_text() == (
        "DEFINED_PHASES=-\n"
        "DESCRIPTION=regen test\n"
        "EAPI=8\n"
        "IUSE=foo\n"
        "KEYWORDS=amd64\n"
        "RDEPEND=dev-libs/other\n"
        "SLOT=0\n"
        f"_md5_={md5}\n"
    )

    # `--pretend` is rejected outright (real actions.py:4106).
    rej = subprocess.run(
        [str(emerge_binary), "-p", "--regen"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )
    assert rej.returncode == 1
    assert rej.stderr.strip() == (
        "emerge: The 'regen' action does not support '--pretend'."
    )


def test_emerge_buildpkgonly_refuses_a_real_src_uri_with_no_manifest_entry(
    emerge_binary, tmp_path
):
    """`fetchpkg` has a real, nonempty `SRC_URI` but no `Manifest` entry
    at all -- refused before any network access is even attempted (see
    `crate::fetch::fetch_src_uri`'s own doc comment: unverifiable
    content is worse than a loud failure)."""
    env = _real_build_env(tmp_path)
    result = subprocess.run(
        [str(emerge_binary), "--buildpkgonly", "dev-libs/fetchpkg"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )
    assert result.returncode == 1
    assert "dev-libs/fetchpkg-1.0" in result.stderr
    assert "no Manifest entry" in result.stderr
    assert not (Path(env["PKGDIR"]) / "dev-libs").exists()


def test_emerge_atom_without_pretend_really_builds_and_merges_from_source(
    emerge_binary, tmp_path
):
    """A plain `emerge <atom>` (no --pretend, no --buildpkgonly/-G) is the
    portuale's first real source build-and-merge for `emerge` itself
    (emerge_build::run_source_merge): resolve the graph, then for each New
    source entry run the full `install` phase chain + the vdb merge
    (ebuild_merge::run_merge). `dev-libs/packagepkg` RDEPENDs on
    `samepkg`, already in the fixture vdb, so the graph resolves cleanly;
    its `src_install` writes `/usr/share/packagepkg/hello.txt`."""
    root = tmp_path / "root"
    # A writable ROOT that still has the fixture vdb (so `samepkg` reads
    # as installed and the resolve doesn't try to pull it in).
    import shutil

    shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(root)
    env["DISTDIR"] = str(Path(FIXTURES_ROOT) / "distfiles")
    env["PORTAGE_TMPDIR"] = str(tmp_path / "portage-tmpdir")

    result = subprocess.run(
        [str(emerge_binary), "dev-libs/packagepkg"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )
    assert result.returncode == 0, result.stderr
    assert ">>> dev-libs/packagepkg-1.0 merged." in result.stdout

    assert (root / "usr/share/packagepkg/hello.txt").read_text().strip() == (
        "hello from packagepkg"
    )
    vdb = root / "var/db/pkg/dev-libs/packagepkg-1.0"
    assert (vdb / "CONTENTS").is_file()
    assert "/usr/share/packagepkg/hello.txt" in (vdb / "CONTENTS").read_text()
    assert (vdb / "RDEPEND").read_text().strip() == "dev-libs/samepkg"

    # Real Scheduler._world_atom: the target was recorded in the world
    # file (sorted, existing entries preserved; the RDEPEND `samepkg`,
    # only a dependency, is NOT added).
    assert ">>> Recording dev-libs/packagepkg in \"world\" favorites file..." in result.stdout
    world_lines = (root / "var/lib/portage/world").read_text().split()
    assert "dev-libs/packagepkg" in world_lines
    assert "dev-libs/samepkg" not in world_lines
    assert world_lines == sorted(world_lines)


def test_emerge_atom_source_build_sees_the_resolved_use_and_build_flags(
    emerge_binary, tmp_path
):
    """`emerge <atom>` (source) now passes the resolved `USE` and the
    resolved compiler/make flags into every ebuild phase
    (`MergeOptions::build_env` ← `build_config_env` + per-entry USE), so
    `bin/ebuild.sh`'s `use()` works and `${CFLAGS}`/`${MAKEOPTS}` are
    real. `dev-libs/usebuildpkg` has `IUSE="buildflag"` (enabled for it
    in `fixtures/etc/portage/package.use`); its `src_install` records
    `use buildflag` and the two flag vars into merged files. Before this
    the phase env left `USE=""`/`CFLAGS=""`."""
    root = tmp_path / "root"
    import shutil

    shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(root)
    env["DISTDIR"] = str(Path(FIXTURES_ROOT) / "distfiles")
    env["PORTAGE_TMPDIR"] = str(tmp_path / "portage-tmpdir")
    # The `env` USE_ORDER layer folds these into Config::other_vars,
    # which build_config_env reads.
    env["CFLAGS"] = "-O2 -pipe"
    env["MAKEOPTS"] = "-j3"

    result = subprocess.run(
        [str(emerge_binary), "dev-libs/usebuildpkg"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )
    assert result.returncode == 0, result.stderr
    assert ">>> dev-libs/usebuildpkg-1.0 merged." in result.stdout
    assert (root / "usr/share/usebuildpkg/state").read_text().strip() == "on"
    assert (root / "usr/share/usebuildpkg/flags").read_text() == (
        "CFLAGS=-O2 -pipe\nMAKEOPTS=-j3\n"
    )


def test_emerge_atom_source_build_package_env_overrides_the_build_flags(
    emerge_binary, tmp_path
):
    """The non-`USE` half of `package.env` (real `_grab_pkg_env` into
    `configdict["pkg"]`): `fixtures/etc/portage/package.env` maps
    `dev-libs/penvbuildpkg` to the env file `penv-buildflags`, which sets
    `CFLAGS`/`MAKEOPTS`. Those override the run-wide (env-layer)
    `CFLAGS`/`MAKEOPTS` in that package's build phase env only --
    `MergeOptions::package_env_vars` ← `Config::package_env_vars`, layered
    by `emerge_build::entry_package_env_vars` after `build_config_env`."""
    root = tmp_path / "root"
    import shutil

    shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(root)
    env["DISTDIR"] = str(Path(FIXTURES_ROOT) / "distfiles")
    env["PORTAGE_TMPDIR"] = str(tmp_path / "portage-tmpdir")
    # The run-wide values -- package.env must win over these for this pkg.
    env["CFLAGS"] = "-O2 -pipe"
    env["MAKEOPTS"] = "-j3"

    result = subprocess.run(
        [str(emerge_binary), "dev-libs/penvbuildpkg"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )
    assert result.returncode == 0, result.stderr
    assert ">>> dev-libs/penvbuildpkg-1.0 merged." in result.stdout
    assert (root / "usr/share/penvbuildpkg/flags").read_text() == (
        "CFLAGS=-Os -march=fixturepkgenv\nMAKEOPTS=-j7\n"
    )


def test_emerge_atom_with_buildpkg_writes_a_binpkg_and_still_merges(
    emerge_binary, tmp_path
):
    """`FEATURES=buildpkg` / `--buildpkg`/`-b` (real _emerge/EbuildBinpkg):
    a source `emerge <atom>` also writes a binpkg into $PKGDIR (before the
    vdb merge), then merges normally. `--buildpkg=n` wins over the
    FEATURE."""
    import shutil
    import tarfile as _tarfile

    def _fresh_root():
        root = tmp_path / f"root{_fresh_root.n}"
        _fresh_root.n += 1
        shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
        env = dict(os.environ)
        env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
        env["ROOT"] = str(root)
        env["DISTDIR"] = str(Path(FIXTURES_ROOT) / "distfiles")
        env["PORTAGE_TMPDIR"] = str(root / "portage-tmpdir")
        env["PKGDIR"] = str(root / "pkgdir")
        env.pop("FEATURES", None)
        return root, env

    _fresh_root.n = 0

    # FEATURES=buildpkg: binpkg built + merged.
    root, env = _fresh_root()
    env["FEATURES"] = "buildpkg"
    r = subprocess.run(
        [str(emerge_binary), "dev-libs/packagepkg"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert r.returncode == 0, r.stderr
    assert ">>> Building package for dev-libs/packagepkg-1.0..." in r.stdout
    tbz2 = root / "pkgdir/dev-libs/packagepkg-1.0.tbz2"
    assert tbz2.is_file()
    assert b"XPAKSTOP" in tbz2.read_bytes()[-4096:]
    with _tarfile.open(tbz2, "r|*") as tf:
        names = [m.name.lstrip("./") for m in tf]
    assert "usr/share/packagepkg/hello.txt" in names
    assert "CPV: dev-libs/packagepkg-1.0" in (root / "pkgdir/Packages").read_text()
    assert (root / "var/db/pkg/dev-libs/packagepkg-1.0/CONTENTS").is_file()

    # -b flag, no FEATURE: same.
    root, env = _fresh_root()
    r = subprocess.run(
        [str(emerge_binary), "-b", "dev-libs/packagepkg"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert r.returncode == 0, r.stderr
    assert (root / "pkgdir/dev-libs/packagepkg-1.0.tbz2").is_file()

    # --buildpkg=n wins over FEATURES=buildpkg: no binpkg, still merges.
    root, env = _fresh_root()
    env["FEATURES"] = "buildpkg"
    r = subprocess.run(
        [str(emerge_binary), "--buildpkg=n", "dev-libs/packagepkg"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert r.returncode == 0, r.stderr
    assert not (root / "pkgdir/dev-libs/packagepkg-1.0.tbz2").exists()
    assert (root / "var/db/pkg/dev-libs/packagepkg-1.0/CONTENTS").is_file()

    # --buildpkg-exclude skips the binpkg for a matching entry (still merged);
    # a non-matching atom leaves the binpkg on.
    root, env = _fresh_root()
    r = subprocess.run(
        [str(emerge_binary), "-b", "--buildpkg-exclude", "dev-libs/packagepkg",
         "dev-libs/packagepkg"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert r.returncode == 0, r.stderr
    assert not (root / "pkgdir/dev-libs/packagepkg-1.0.tbz2").exists()
    assert (root / "var/db/pkg/dev-libs/packagepkg-1.0/CONTENTS").is_file()

    root, env = _fresh_root()
    r = subprocess.run(
        [str(emerge_binary), "-b", "--buildpkg-exclude", "dev-libs/other",
         "dev-libs/packagepkg"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert r.returncode == 0, r.stderr
    assert (root / "pkgdir/dev-libs/packagepkg-1.0.tbz2").is_file()


def test_emerge_atom_oneshot_does_not_touch_the_world_file(emerge_binary, tmp_path):
    """--oneshot/-1 (real Scheduler._world_atom's own suppression set):
    the package still merges, but its atom is NOT recorded in world."""
    import shutil

    root = tmp_path / "root"
    shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(root)
    env["DISTDIR"] = str(Path(FIXTURES_ROOT) / "distfiles")
    env["PORTAGE_TMPDIR"] = str(tmp_path / "portage-tmpdir")

    world_before = (root / "var/lib/portage/world").read_bytes()
    result = subprocess.run(
        [str(emerge_binary), "--oneshot", "dev-libs/packagepkg"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )
    assert result.returncode == 0, result.stderr
    assert (root / "var/db/pkg/dev-libs/packagepkg-1.0/CONTENTS").is_file()
    assert "Recording" not in result.stdout
    assert (root / "var/lib/portage/world").read_bytes() == world_before


def test_emerge_slotted_atom_is_recorded_slot_qualified_in_world(emerge_binary, tmp_path):
    """Real create_world_atom: "If the argument atom is precise enough to
    identify a specific slot then a slot atom will be returned." A
    `cat/pkg:slot` argument for a genuinely slotted cp (dev-libs/dualslotpkg
    has SLOT=1 and SLOT=2, neither "0") records `dev-libs/dualslotpkg:1`.
    An unslotted cp (dev-libs/packagepkg, SLOT="0" only) records the plain
    `cat/pkg` even when the arg carried `:0`."""
    import shutil

    def _root(n):
        root = tmp_path / f"root{n}"
        shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
        env = dict(os.environ)
        env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
        env["ROOT"] = str(root)
        env["DISTDIR"] = str(Path(FIXTURES_ROOT) / "distfiles")
        env["PORTAGE_TMPDIR"] = str(tmp_path / f"portage-tmpdir{n}")
        return root, env

    # Slotted cp + slot-specific arg -> slot atom in world.
    root, env = _root(0)
    r = subprocess.run(
        [str(emerge_binary), "dev-libs/dualslotpkg:1"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert r.returncode == 0, r.stderr
    assert '>>> Recording dev-libs/dualslotpkg:1 in "world" favorites file...' in r.stdout
    assert "dev-libs/dualslotpkg:1" in (root / "var/lib/portage/world").read_text().split()

    # Bare arg for the same slotted cp -> plain cat/pkg (arg isn't slot-specific).
    root, env = _root(1)
    r = subprocess.run(
        [str(emerge_binary), "dev-libs/dualslotpkg"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert r.returncode == 0, r.stderr
    world = (root / "var/lib/portage/world").read_text().split()
    assert "dev-libs/dualslotpkg" in world
    assert not any(w.startswith("dev-libs/dualslotpkg:") for w in world)

    # Unslotted cp (SLOT="0" only) + `:0` arg -> still plain cat/pkg.
    root, env = _root(2)
    r = subprocess.run(
        [str(emerge_binary), "dev-libs/packagepkg:0"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert r.returncode == 0, r.stderr
    world = (root / "var/lib/portage/world").read_text().split()
    assert world.count("dev-libs/packagepkg") == 1
    assert "dev-libs/packagepkg:0" not in world


def test_emerge_custom_set_is_recorded_in_world_sets(emerge_binary, tmp_path):
    """Real depgraph.saveNomergeFavorites's @set half: `emerge @name` for
    a user-defined set records `@name` in var/lib/portage/world_sets (NOT
    the plain world file -- its member packages aren't directly-named
    atoms). `--oneshot` suppresses it, same as the world file."""
    import shutil

    def _root(n):
        root = tmp_path / f"root{n}"
        shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
        env = dict(os.environ)
        env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
        env["ROOT"] = str(root)
        env["DISTDIR"] = str(Path(FIXTURES_ROOT) / "distfiles")
        env["PORTAGE_TMPDIR"] = str(tmp_path / f"portage-tmpdir{n}")
        return root, env

    root, env = _root(0)
    world_before = (root / "var/lib/portage/world").read_text()
    r = subprocess.run(
        [str(emerge_binary), "@innernestedset"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert r.returncode == 0, r.stderr
    assert '>>> Recording @innernestedset in "world_sets" favorites file...' in r.stdout
    world_sets = (root / "var/lib/portage/world_sets").read_text().split()
    assert "@innernestedset" in world_sets
    assert world_sets == sorted(world_sets)
    # The set's member packages are NOT added to the plain world file.
    assert (root / "var/lib/portage/world").read_text() == world_before

    # --oneshot: still merges, but records nothing in world_sets.
    root, env = _root(1)
    ws_before = (root / "var/lib/portage/world_sets").read_bytes()
    r = subprocess.run(
        [str(emerge_binary), "--oneshot", "@innernestedset"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert r.returncode == 0, r.stderr
    assert "Recording" not in r.stdout
    assert (root / "var/lib/portage/world_sets").read_bytes() == ws_before


def test_emerge_jobs_builds_independent_packages_in_parallel(emerge_binary, tmp_path):
    """`emerge -jN <atom>` (real `_emerge/Scheduler.py`'s `_max_jobs`):
    run up to N `install` phases concurrently, dispatching a build only
    once every dependency it has is merged, and serializing the vdb merge.
    `dev-libs/schedparent` RDEPENDs the two independent leaves
    `schedleaf-a`/`schedleaf-b`; under `-j2` both leaves start building
    before either merges, then schedparent builds last. All three end up
    installed."""
    import shutil

    root = tmp_path / "root"
    shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(root)
    env["DISTDIR"] = str(Path(FIXTURES_ROOT) / "distfiles")
    env["PORTAGE_TMPDIR"] = str(tmp_path / "portage-tmpdir")

    result = subprocess.run(
        [str(emerge_binary), "-j2", "dev-libs/schedparent"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )
    assert result.returncode == 0, result.stderr
    out = result.stdout

    for pkg in ("schedleaf-a-1.0", "schedleaf-b-1.0", "schedparent-1.0"):
        assert f">>> dev-libs/{pkg} merged." in out
        assert (root / f"var/db/pkg/dev-libs/{pkg}/CONTENTS").is_file()

    # Both leaves' builds start before either one merges -- the mark of
    # real parallel dispatch, not a serial build+merge per package.
    emerge_a = out.index(">>> Emerging (dev-libs/schedleaf-a-1.0)")
    emerge_b = out.index(">>> Emerging (dev-libs/schedleaf-b-1.0)")
    first_leaf_merge = min(
        out.index(">>> dev-libs/schedleaf-a-1.0 merged."),
        out.index(">>> dev-libs/schedleaf-b-1.0 merged."),
    )
    assert max(emerge_a, emerge_b) < first_leaf_merge

    # schedparent (the dependent) only starts building after both leaves
    # have merged.
    assert out.index(">>> Emerging (dev-libs/schedparent-1.0)") > max(
        out.index(">>> dev-libs/schedleaf-a-1.0 merged."),
        out.index(">>> dev-libs/schedleaf-b-1.0 merged."),
    )

    # Real `Scheduler.JobStatusDisplay`: a running "X of Y complete" line.
    assert ">>> Jobs: 1 of 3 complete" in out
    assert ">>> Jobs: 3 of 3 complete" in out

    # Real `--quiet-build` (on by default under `--jobs`): each build's
    # own phase output is captured to `${T}/build.log`, NOT interleaved on
    # the parsable stdout. Every stdout line is a portuale-emitted `>>>` /
    # `[ebuild` line, never a stray phase / shell diagnostic.
    for line in out.splitlines():
        assert line.startswith((">>>", "[ebuild", "[blocks", "[nomerge")), repr(line)
    assert (
        tmp_path / "portage-tmpdir/portage/dev-libs/schedleaf-a-1.0/temp/build.log"
    ).is_file()


def test_emerge_quiet_build_redirects_a_single_job_build_to_the_log(
    emerge_binary, tmp_path
):
    """`--quiet-build[=y|n]` (real `Scheduler._background_mode`): at the
    default `-j1`, `--quiet-build=y` redirects a package's phase output to
    `${T}/build.log` instead of the terminal -- the same capture a `-j` >1
    run always does. Without it (the default) the output streams and no
    build.log is written. Either way the package builds and merges."""
    import shutil

    def _root(n):
        root = tmp_path / f"root{n}"
        shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
        env = dict(os.environ)
        env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
        env["ROOT"] = str(root)
        env["DISTDIR"] = str(Path(FIXTURES_ROOT) / "distfiles")
        env["PORTAGE_TMPDIR"] = str(tmp_path / f"pt{n}")
        return root, env

    log_rel = "portage/dev-libs/packagepkg-1.0/temp/build.log"

    # --quiet-build=y: stdout carries only portuale's own `>>>` /
    # `[ebuild` lines; the phase output landed in build.log.
    root, env = _root(0)
    r = subprocess.run(
        [str(emerge_binary), "--quiet-build=y", "dev-libs/packagepkg"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert r.returncode == 0, r.stderr
    assert ">>> dev-libs/packagepkg-1.0 merged." in r.stdout
    for line in r.stdout.splitlines():
        assert line.startswith((">>>", "[ebuild", "[blocks", "[nomerge")), repr(line)
    log = tmp_path / "pt0" / log_rel
    assert log.is_file() and log.stat().st_size > 0

    # Default: streamed, no build.log written.
    root, env = _root(1)
    r = subprocess.run(
        [str(emerge_binary), "dev-libs/packagepkg"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert r.returncode == 0, r.stderr
    assert ">>> dev-libs/packagepkg-1.0 merged." in r.stdout
    assert not (tmp_path / "pt1" / log_rel).is_file()


def test_emerge_jobs_with_load_average_still_builds_everything(emerge_binary, tmp_path):
    """`emerge -j4 --load-average <LA>` (real `main.py` `type=float`): the
    scheduler holds off on an *additional* build while the 1-minute system
    load exceeds LA, but never blocks the first -- so with a huge LA the
    throttle is a no-op and all three sched packages still build+merge,
    and with a tiny LA the run still completes (serialized, not stalled).
    Also proves `--load-average` / `-l` is now parsed, not reported as an
    unimplemented real option."""
    import shutil

    for la in ("999", "0.01"):
        root = tmp_path / f"root_{la}"
        shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
        env = dict(os.environ)
        env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
        env["ROOT"] = str(root)
        env["DISTDIR"] = str(Path(FIXTURES_ROOT) / "distfiles")
        env["PORTAGE_TMPDIR"] = str(tmp_path / f"ptmp_{la}")
        result = subprocess.run(
            [str(emerge_binary), "-j4", "--load-average", la, "dev-libs/schedparent"],
            capture_output=True,
            text=True,
            check=False,
            env=env,
        )
        assert result.returncode == 0, result.stderr
        for pkg in ("schedleaf-a-1.0", "schedleaf-b-1.0", "schedparent-1.0"):
            assert (root / f"var/db/pkg/dev-libs/{pkg}/CONTENTS").is_file()


def test_emerge_jobs_keep_going_skips_a_failed_builds_dependents(emerge_binary, tmp_path):
    """`emerge -j2 --keep-going`: a failed build (`schedbad`'s src_install
    dies) drops its transitive dependents (`schedbaddep`) from the merge
    set (real `Scheduler._calc_resume_list`), the independent `schedok`
    still merges, and the run exits non-zero listing what failed and what
    was skipped."""
    import shutil

    root = tmp_path / "root"
    shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(root)
    env["DISTDIR"] = str(Path(FIXTURES_ROOT) / "distfiles")
    env["PORTAGE_TMPDIR"] = str(tmp_path / "portage-tmpdir")

    result = subprocess.run(
        [str(emerge_binary), "-j2", "--keep-going", "dev-libs/schedbaddep", "dev-libs/schedok"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )
    assert result.returncode == 1
    assert (root / "var/db/pkg/dev-libs/schedok-1.0/CONTENTS").is_file()
    assert not (root / "var/db/pkg/dev-libs/schedbaddep-1.0").exists()
    assert "dev-libs/schedbad-1.0" in result.stderr
    assert "dev-libs/schedbaddep" in result.stderr
    # The failed build's captured log tail is folded into the report so
    # the user can see why it failed without hunting for build.log.
    assert "last lines of" in result.stderr
    assert "build.log" in result.stderr
    assert "deliberate fixture build failure" in result.stderr


def test_emerge_atom_upgrade_replaces_the_installed_version(emerge_binary, tmp_path):
    """`emerge <atom>` handles an Upgrade too now: merge the new version,
    then unmerge the replaced same-slot version (real
    `dblink.treewalk()`'s merge-then-unmerge). `dev-libs/binpkgrmpkg`'s
    two versions define all five `pkg_*` hooks, each appending
    `<phase>-<PVR>` to a `${ROOT}` log -- so the full real interleave is
    checkable end to end."""
    import shutil

    root = tmp_path / "root"
    shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(root)
    env["PORTAGE_TMPDIR"] = str(tmp_path / "portage-tmpdir")

    # `emerge dev-libs/binpkgrmpkg` resolves the highest version (2.0);
    # pre-seed the 1.0 install by merging its ebuild directly first.
    v1 = str(
        Path(FIXTURES_ROOT) / "repo/dev-libs/binpkgrmpkg/binpkgrmpkg-1.0.ebuild"
    )
    ebuild_link = tmp_path / "ebuild"
    ebuild_link.symlink_to(Path(emerge_binary).resolve())
    r1 = subprocess.run(
        [str(ebuild_link), v1, "merge"], capture_output=True, text=True, check=False, env=env
    )
    assert r1.returncode == 0, r1.stderr
    assert (root / "var/db/pkg/dev-libs/binpkgrmpkg-1.0").is_dir()

    result = subprocess.run(
        [str(emerge_binary), "dev-libs/binpkgrmpkg"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )
    assert result.returncode == 0, result.stderr

    assert (root / "var/db/pkg/dev-libs/binpkgrmpkg-2.0/CONTENTS").is_file()
    assert not (root / "var/db/pkg/dev-libs/binpkgrmpkg-1.0").exists()
    assert (root / "usr/share/binpkgrmpkg/payload-2.0.txt").is_file()
    assert not (root / "usr/share/binpkgrmpkg/payload-1.0.txt").exists()
    assert (root / "var/lib/binpkgrmpkg.log").read_text() == (
        "setup-1.0\npreinst-1.0\npostinst-1.0\n"
        "setup-2.0\npreinst-2.0\nprerm-1.0\npostrm-1.0\npostinst-2.0\n"
    )


def test_emerge_unmerge_without_pretend_really_removes_and_deselects(
    emerge_binary, tmp_path
):
    """`emerge -C <atom>` WITHOUT `--pretend` is a real removal now (real
    `_emerge/unmerge.py::unmerge`'s own loop): after the `_unmerge_display`
    preview, each selected package's `pkg_prerm` (from its own vdb-saved
    env) runs, its files and vdb entry go, its `pkg_postrm` runs, and it's
    deselected from the world file (real
    `WorldSelectedPackagesSet.cleanPackage`). `dev-libs/binpkgrmpkg`'s
    five hooks each append `<phase>-<PVR>` to a `${ROOT}` log."""
    import shutil

    root = tmp_path / "root"
    shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(root)
    env["PORTAGE_TMPDIR"] = str(tmp_path / "portage-tmpdir")

    # Seed an installed 1.0 via a direct `ebuild <file> merge`, and record
    # it in the world file as a directly-selected package.
    v1 = str(
        Path(FIXTURES_ROOT) / "repo/dev-libs/binpkgrmpkg/binpkgrmpkg-1.0.ebuild"
    )
    ebuild_link = tmp_path / "ebuild"
    ebuild_link.symlink_to(Path(emerge_binary).resolve())
    r1 = subprocess.run(
        [str(ebuild_link), v1, "merge"], capture_output=True, text=True, check=False, env=env
    )
    assert r1.returncode == 0, r1.stderr
    assert (root / "var/db/pkg/dev-libs/binpkgrmpkg-1.0").is_dir()
    assert (root / "usr/share/binpkgrmpkg/payload-1.0.txt").is_file()
    (root / "var/lib/portage").mkdir(parents=True, exist_ok=True)
    (root / "var/lib/portage/world").write_text(
        "dev-libs/binpkgrmpkg\ndev-libs/keepme\n"
    )
    (root / "var/lib/binpkgrmpkg.log").write_text("")

    result = subprocess.run(
        [str(emerge_binary), "-C", "dev-libs/binpkgrmpkg"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )
    assert result.returncode == 0, result.stderr
    # The `--pretend`/`--ask`-only header is NOT printed for a real run.
    assert ">>> These are the packages that would be unmerged:" not in result.stdout
    assert ">>> Unmerging (1 of 1) dev-libs/binpkgrmpkg-1.0..." in result.stdout

    # Really gone: vdb entry, payload file.
    assert not (root / "var/db/pkg/dev-libs/binpkgrmpkg-1.0").exists()
    assert not (root / "usr/share/binpkgrmpkg/payload-1.0.txt").exists()
    # prerm then postrm ran, from the vdb-saved env.
    assert (root / "var/lib/binpkgrmpkg.log").read_text() == "prerm-1.0\npostrm-1.0\n"
    # Deselected from world; the unrelated atom is untouched.
    assert (root / "var/lib/portage/world").read_text() == "dev-libs/keepme\n"


def test_emerge_resume_replays_the_saved_mergelist(emerge_binary, tmp_path):
    """Real `_emerge/Scheduler.py::_save_resume_list` + `--resume`: a
    failed `emerge <atoms>` writes the still-unmerged packages to
    `mtimedb["resume"]`; `emerge --resume` replays them in order,
    `emerge --resume --skipfirst` drops the first (the one that failed).
    `dev-libs/schedbad`'s src_install dies; `dev-libs/schedok` is
    independent."""
    import json
    import shutil

    root = tmp_path / "root"
    shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(root)
    env["DISTDIR"] = str(Path(FIXTURES_ROOT) / "distfiles")
    env["PORTAGE_TMPDIR"] = str(tmp_path / "pt")

    r = subprocess.run(
        [str(emerge_binary), "dev-libs/schedbad", "dev-libs/schedok"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert r.returncode == 1
    assert "emerge --resume" in r.stderr
    mtimedb = root / "var/cache/edb/mtimedb"
    saved = json.loads(mtimedb.read_text())
    cpvs = [x[2] for x in saved["resume"]["mergelist"]]
    assert cpvs == ["dev-libs/schedbad-1.0", "dev-libs/schedok-1.0"]
    assert saved["resume"]["favorites"] == ["dev-libs/schedbad", "dev-libs/schedok"]

    # --resume --pretend: show the saved list, merge nothing, leave the
    # resume list intact (real `_emerge/actions.py`: display + return 0).
    r = subprocess.run(
        [str(emerge_binary), "--resume", "--pretend"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert r.returncode == 0, r.stderr
    assert "dev-libs/schedbad-1.0" in r.stdout
    assert "dev-libs/schedok-1.0" in r.stdout
    assert r.stdout.startswith("[ebuild")
    assert "merged." not in r.stdout
    assert mtimedb.is_file()
    assert not (root / "var/db/pkg/dev-libs/schedok-1.0").exists()

    # --resume alone: retries schedbad, which fails again; list is re-saved.
    r = subprocess.run(
        [str(emerge_binary), "--resume"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert r.returncode == 1
    assert mtimedb.is_file()
    assert not (root / "var/db/pkg/dev-libs/schedok-1.0").exists()

    # --resume --skipfirst: drops schedbad, merges schedok, clears the list.
    r = subprocess.run(
        [str(emerge_binary), "--resume", "--skipfirst"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert r.returncode == 0, r.stderr
    assert ">>> dev-libs/schedok-1.0 merged." in r.stdout
    assert (root / "var/db/pkg/dev-libs/schedok-1.0/CONTENTS").is_file()
    assert not mtimedb.exists()  # resume list cleared on success


def test_emerge_resume_carries_the_oneshot_flag(emerge_binary, tmp_path):
    """Real `mtimedb["resume"]["myopts"]`: a failed `emerge --oneshot
    <atoms>` records `--oneshot`, so `emerge --resume` replays without
    adding the recovered packages to the world file (before this the
    resume always world-recorded its favorites)."""
    import json
    import shutil

    root = tmp_path / "root"
    shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
    (root / "var/lib/portage").mkdir(parents=True, exist_ok=True)
    (root / "var/lib/portage/world").write_text("")
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(root)
    env["DISTDIR"] = str(Path(FIXTURES_ROOT) / "distfiles")
    env["PORTAGE_TMPDIR"] = str(tmp_path / "pt")

    r = subprocess.run(
        [str(emerge_binary), "--oneshot", "dev-libs/schedbad", "dev-libs/schedok"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert r.returncode == 1
    saved = json.loads((root / "var/cache/edb/mtimedb").read_text())
    assert saved["resume"]["myopts"] == {"--oneshot": True}

    r = subprocess.run(
        [str(emerge_binary), "--resume", "--skipfirst"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert r.returncode == 0, r.stderr
    assert (root / "var/db/pkg/dev-libs/schedok-1.0/CONTENTS").is_file()
    # --oneshot carried through: schedok is NOT in world.
    assert (root / "var/lib/portage/world").read_text() == ""


def test_emerge_elog_echo_prints_a_message_summary(emerge_binary, tmp_path):
    """Real `elog_process` / `mod_echo` (default-on via `make.globals`
    `PORTAGE_ELOG_SYSTEM`): after the merge, the `elog`/`ewarn` messages an
    ebuild emitted (routed to `${T}/logging/<phase>` by
    `bin/isolated-functions.sh`) are echoed as a
    `* Messages for package <cpv>:` block, filtered by
    `PORTAGE_ELOG_CLASSES` (default `log warn error` -- so `einfo` is
    NOT echoed). `PORTAGE_ELOG_SYSTEM=` disables it."""
    import shutil

    def _emerge(env_extra):
        root = tmp_path / f"root{len(env_extra)}_{sum(len(v) for v in env_extra.values())}"
        shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
        env = dict(os.environ)
        env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
        env["ROOT"] = str(root)
        env["DISTDIR"] = str(Path(FIXTURES_ROOT) / "distfiles")
        env["PORTAGE_TMPDIR"] = str(root / "pt")
        env.update(env_extra)
        return subprocess.run(
            [str(emerge_binary), "dev-libs/elogmsgpkg"],
            capture_output=True, text=True, check=False, env=env,
        )

    r = _emerge({})
    assert r.returncode == 0, r.stderr
    out = r.stdout
    assert " * Messages for package dev-libs/elogmsgpkg-1.0 merged to " in out
    assert " * this package needs manual configuration" in out  # elog (LOG)
    assert " * see /usr/share/doc for details" in out            # elog (LOG)
    assert " * a deprecated feature is still enabled" in out      # ewarn (WARN)
    # einfo is INFO-class -> not in the default `log warn error` set.
    assert "purely informational note" not in out

    r = _emerge({"PORTAGE_ELOG_SYSTEM": ""})
    assert r.returncode == 0
    assert "Messages for package" not in r.stdout


def test_emerge_elog_save_and_save_summary_write_log_files(emerge_binary, tmp_path):
    """Real `mod_save` / `mod_save_summary` (the latter ON by default via
    `make.globals` `PORTAGE_ELOG_SYSTEM`): after the merge, each package's
    class-filtered elog messages are written to
    `<logdir>/elog/<cat>:<pf>:<stamp>.log` (`save`) and appended to
    `<logdir>/elog/summary.log` (`save_summary`), in real
    `_combine_logentries` format. `mail`/`mail_summary` print an
    "unsupported" notice and are skipped."""
    import re
    import shutil

    root = tmp_path / "root"
    shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
    logdir = tmp_path / "logs"
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(root)
    env["DISTDIR"] = str(Path(FIXTURES_ROOT) / "distfiles")
    env["PORTAGE_TMPDIR"] = str(root / "pt")
    env["PORTAGE_LOGDIR"] = str(logdir)
    env["PORTAGE_ELOG_SYSTEM"] = "save save_summary:log,warn,error,qa mail echo"

    r = subprocess.run(
        [str(emerge_binary), "dev-libs/elogmsgpkg"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert r.returncode == 0, r.stderr
    assert "elog `mail`/`mail_summary` is not supported" in r.stderr

    elog = logdir / "elog"
    per_pkg = [p for p in elog.iterdir() if re.fullmatch(
        r"dev-libs:elogmsgpkg-1\.0:\d{8}-\d{6}\.log", p.name)]
    assert len(per_pkg) == 1
    # `save` uses the default PORTAGE_ELOG_CLASSES (log warn error).
    assert per_pkg[0].read_text() == (
        "LOG: install\n"
        "this package needs manual configuration\n"
        "see /usr/share/doc for details\n"
        "WARN: postinst\n"
        "a deprecated feature is still enabled\n"
    )

    summary = (elog / "summary.log").read_text()
    assert re.search(
        r">>> Messages generated by process \d+ on \d{8}-\d{6} UTC "
        r"for package dev-libs/elogmsgpkg-1\.0:\n\n", summary)
    assert (
        "LOG: install\n"
        "this package needs manual configuration\n"
        "see /usr/share/doc for details\n"
        "WARN: postinst\n"
        "a deprecated feature is still enabled\n"
    ) in summary


def test_emerge_unmerge_processes_prerm_postrm_elog(emerge_binary, tmp_path):
    """Real `dblink.unmerge()` -> `self._elog_process(phasefilter=("prerm",
    "postrm"))`: the `elog`/`ewarn` a removed package's `pkg_prerm` /
    `pkg_postrm` emit are echoed (`* Messages for package <cpv>:`) and
    written to `<logdir>/elog/summary.log`, exactly like a merge --
    `execute_unmerge` re-scans each removed package's `${T}/logging/`
    via `elog::process_batch` with a prerm/postrm phase filter."""
    import re
    import shutil

    root = tmp_path / "root"
    shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
    logdir = tmp_path / "logs"
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(root)
    env["DISTDIR"] = str(Path(FIXTURES_ROOT) / "distfiles")
    env["PORTAGE_TMPDIR"] = str(tmp_path / "pt")
    env["PORTAGE_LOGDIR"] = str(logdir)

    # Seed an installed 1.0 via a direct `ebuild <file> merge` (full vdb
    # entry incl. environment.bz2 + <pf>.ebuild, so the removal hooks run).
    v1 = str(Path(FIXTURES_ROOT) / "repo/dev-libs/elogrmpkg/elogrmpkg-1.0.ebuild")
    ebuild_link = tmp_path / "ebuild"
    ebuild_link.symlink_to(Path(emerge_binary).resolve())
    r1 = subprocess.run(
        [str(ebuild_link), v1, "merge"], capture_output=True, text=True, check=False, env=env
    )
    assert r1.returncode == 0, r1.stderr

    result = subprocess.run(
        [str(emerge_binary), "-C", "dev-libs/elogrmpkg"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert result.returncode == 0, result.stderr
    out = result.stdout
    assert " * Messages for package dev-libs/elogrmpkg-1.0" in out
    assert " * config files in /etc/elogrmpkg are left behind" in out   # ewarn (prerm)
    assert " * run revdep-rebuild after removing this package" in out    # elog (postrm)

    summary = (logdir / "elog" / "summary.log").read_text()
    assert "for package dev-libs/elogrmpkg-1.0:" in summary
    assert re.search(
        r"WARN: prerm\nconfig files in /etc/elogrmpkg are left behind\n"
        r"LOG: postrm\nrun revdep-rebuild after removing this package\n",
        summary,
    )


def test_emerge_upgrade_in_place_processes_the_old_versions_rm_elog(
    emerge_binary, tmp_path
):
    """Real `dblink.unmerge()` runs `_elog_process(phasefilter=("prerm",
    "postrm"))` for the SUPERSEDED version during an in-place replace too,
    not just under `emerge -C`. `elogrmpkg-1.0`'s `pkg_prerm`/`pkg_postrm`
    emit `ewarn`/`elog`; upgrading it to 2.0 (same SLOT) must echo and
    log those -- `unmerge_replaced_same_slot` now calls `process_batch`
    after the replace loop."""
    import re
    import shutil

    root = tmp_path / "root"
    shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
    logdir = tmp_path / "logs"
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(root)
    env["DISTDIR"] = str(Path(FIXTURES_ROOT) / "distfiles")
    env["PORTAGE_TMPDIR"] = str(tmp_path / "pt")
    env["PORTAGE_LOGDIR"] = str(logdir)

    # Seed an installed 1.0 (full vdb entry so its rm hooks run on replace).
    v1 = str(Path(FIXTURES_ROOT) / "repo/dev-libs/elogrmpkg/elogrmpkg-1.0.ebuild")
    ebuild_link = tmp_path / "ebuild"
    ebuild_link.symlink_to(Path(emerge_binary).resolve())
    r1 = subprocess.run(
        [str(ebuild_link), v1, "merge"], capture_output=True, text=True, check=False, env=env
    )
    assert r1.returncode == 0, r1.stderr

    result = subprocess.run(
        [str(emerge_binary), "dev-libs/elogrmpkg"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert result.returncode == 0, result.stderr
    assert (root / "var/db/pkg/dev-libs/elogrmpkg-2.0/CONTENTS").is_file()
    assert not (root / "var/db/pkg/dev-libs/elogrmpkg-1.0").exists()

    out = result.stdout
    assert " * Messages for package dev-libs/elogrmpkg-1.0" in out
    assert " * config files in /etc/elogrmpkg are left behind" in out   # ewarn (prerm)
    assert " * run revdep-rebuild after removing this package" in out    # elog (postrm)

    summary = (logdir / "elog" / "summary.log").read_text()
    assert "for package dev-libs/elogrmpkg-1.0:" in summary
    assert re.search(
        r"WARN: prerm\nconfig files in /etc/elogrmpkg are left behind\n"
        r"LOG: postrm\nrun revdep-rebuild after removing this package\n",
        summary,
    )


def test_emerge_applies_portage_niceness_and_ionice(emerge_binary, fixture_env):
    """Real `_emerge/actions.py::apply_priorities` (via `run_action`):
    `PORTAGE_NICENESS` -> `renice -n <n> <pid>`, `PORTAGE_IONICE_COMMAND`
    -> spawned with `${PID}` expanded. A failing renice / ionice prints
    an eerror-style line and the run continues; unset -> nothing."""
    base = [str(emerge_binary), "--pretend", "dev-libs/newpkg"]

    # Baseline: neither var -> no scheduling-policy output at all.
    clean = dict(fixture_env)
    clean.pop("PORTAGE_NICENESS", None)
    clean.pop("PORTAGE_IONICE_COMMAND", None)
    r = subprocess.run(base, capture_output=True, text=True, check=False, env=clean)
    assert r.returncode == 0
    assert "renice" not in r.stderr
    assert "PORTAGE_IONICE_COMMAND" not in r.stderr

    # A non-integer PORTAGE_NICENESS makes `renice` fail -> eerror line,
    # run still proceeds.
    env = dict(clean, PORTAGE_NICENESS="notanumber")
    r = subprocess.run(base, capture_output=True, text=True, check=False, env=env)
    assert r.returncode == 0
    assert "renice command returned" in r.stderr
    assert r.stdout.startswith("[ebuild")

    # A failing PORTAGE_IONICE_COMMAND -> its own eerror lines.
    env = dict(clean, PORTAGE_IONICE_COMMAND="false ${PID}")
    r = subprocess.run(base, capture_output=True, text=True, check=False, env=env)
    assert r.returncode == 0
    assert "PORTAGE_IONICE_COMMAND returned" in r.stderr
    assert "make.conf(5)" in r.stderr

    # PORTAGE_IONICE_COMMAND is shlex-split now (real `shlex.split`): a
    # quoted argument stays one word rather than splitting on its space.
    env = dict(clean, PORTAGE_IONICE_COMMAND="/bin/sh -c 'exit 7' ${PID}")
    r = subprocess.run(base, capture_output=True, text=True, check=False, env=env)
    assert r.returncode == 0
    assert "PORTAGE_IONICE_COMMAND returned 7" in r.stderr


def test_emerge_applies_portage_scheduling_policy(emerge_binary, fixture_env):
    """`PORTAGE_SCHEDULING_POLICY` -> real `os.sched_setscheduler` (real
    `_emerge/actions.py::set_scheduling_policy`). `batch` maps to
    `SCHED_BATCH`; an unknown name prints the real "Invalid policy" eerror
    pair. Either way the action still proceeds."""
    base = [str(emerge_binary), "--pretend", "dev-libs/newpkg"]
    clean = dict(fixture_env)
    clean.pop("PORTAGE_SCHEDULING_POLICY", None)
    clean.pop("PORTAGE_SCHEDULING_PRIORITY", None)

    r = subprocess.run(base, capture_output=True, text=True, check=False, env=clean)
    assert r.returncode == 0
    assert "PORTAGE_SCHEDULING_POLICY" not in r.stderr

    # A recognized policy: no "Invalid policy", run proceeds. (A stricter
    # environment could still fail the syscall with EPERM -- tolerated;
    # the point is the name was mapped, not rejected.)
    env = dict(clean, PORTAGE_SCHEDULING_POLICY="batch")
    r = subprocess.run(base, capture_output=True, text=True, check=False, env=env)
    assert r.returncode == 0
    assert "Invalid policy" not in r.stderr
    assert r.stdout.strip() == "[ebuild  N     ] dev-libs/newpkg-1.0"

    # An unknown policy name -> the real eerror pair, run still proceeds.
    env = dict(clean, PORTAGE_SCHEDULING_POLICY="turbo")
    r = subprocess.run(base, capture_output=True, text=True, check=False, env=env)
    assert r.returncode == 0
    assert "Invalid policy in PORTAGE_SCHEDULING_POLICY." in r.stderr
    assert "make.conf(5)" in r.stderr
    assert r.stdout.strip() == "[ebuild  N     ] dev-libs/newpkg-1.0"

    # An out-of-range PORTAGE_SCHEDULING_PRIORITY -> its own eerror pair.
    env = dict(clean, PORTAGE_SCHEDULING_POLICY="batch",
               PORTAGE_SCHEDULING_PRIORITY="999999")
    r = subprocess.run(base, capture_output=True, text=True, check=False, env=env)
    assert r.returncode == 0
    assert "Invalid priority in PORTAGE_SCHEDULING_PRIORITY." in r.stderr


def test_emerge_ask_prompts_before_a_real_merge_and_honours_the_answer(
    emerge_binary, tmp_path
):
    """`emerge --ask <atom>` (real `_emerge/actions.py:525`): after the
    merge list, prompt `Would you like to merge these packages? [Yes/No]`.
    A `No` prints `Quitting.` and exits 130 without building; a `Yes` (or
    bare Enter) proceeds. Ignored under `--pretend`."""
    import shutil

    counter = [0]

    def _run(answer, extra):
        counter[0] += 1
        root = tmp_path / f"root{counter[0]}"
        shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
        env = dict(os.environ)
        env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
        env["ROOT"] = str(root)
        env["DISTDIR"] = str(Path(FIXTURES_ROOT) / "distfiles")
        env["PORTAGE_TMPDIR"] = str(root / "pt")
        r = subprocess.run(
            [str(emerge_binary), *extra, "--ask", "dev-libs/schedok"],
            input=answer,
            capture_output=True,
            text=True,
            check=False,
            env=env,
        )
        return root, r

    root, r = _run("n\n", [])
    assert r.returncode == 130
    assert "Would you like to merge these packages? [Yes/No]" in r.stdout
    assert "Quitting." in r.stdout
    assert not (root / "var/db/pkg/dev-libs/schedok-1.0").exists()

    root, r = _run("\n", [])  # bare Enter == Yes
    assert r.returncode == 0
    assert (root / "var/db/pkg/dev-libs/schedok-1.0/CONTENTS").is_file()

    # Under --pretend the prompt never appears (nothing executes anyway).
    root, r = _run("", ["--pretend"])
    assert r.returncode == 0
    assert "Would you like to merge" not in r.stdout


def test_emerge_ask_prompts_before_a_real_unmerge(emerge_binary, tmp_path):
    """`emerge -C --ask <atom>`: prompt `Would you like to unmerge these
    packages?`; `No` -> exit 130, package left installed."""
    import shutil

    root = tmp_path / "root"
    shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(root)
    env["DISTDIR"] = str(Path(FIXTURES_ROOT) / "distfiles")
    env["PORTAGE_TMPDIR"] = str(root / "pt")
    subprocess.run(
        [str(emerge_binary), "dev-libs/schedok"],
        input="", capture_output=True, text=True, check=False, env=env,
    )
    assert (root / "var/db/pkg/dev-libs/schedok-1.0/CONTENTS").is_file()

    r = subprocess.run(
        [str(emerge_binary), "-C", "--ask", "dev-libs/schedok"],
        input="n\n", capture_output=True, text=True, check=False, env=env,
    )
    assert r.returncode == 130
    assert "Would you like to unmerge these packages? [Yes/No]" in r.stdout
    assert (root / "var/db/pkg/dev-libs/schedok-1.0/CONTENTS").is_file()


def test_emerge_unmerge_with_pretend_still_only_previews(emerge_binary, tmp_path):
    """`emerge -pC <atom>` keeps the old preview-only behaviour: the
    `>>> These are the packages that would be unmerged:` header prints and
    nothing is removed."""
    import shutil

    root = tmp_path / "root"
    shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(root)
    env["PORTAGE_TMPDIR"] = str(tmp_path / "portage-tmpdir")

    v1 = str(
        Path(FIXTURES_ROOT) / "repo/dev-libs/binpkgrmpkg/binpkgrmpkg-1.0.ebuild"
    )
    ebuild_link = tmp_path / "ebuild"
    ebuild_link.symlink_to(Path(emerge_binary).resolve())
    r1 = subprocess.run(
        [str(ebuild_link), v1, "merge"], capture_output=True, text=True, check=False, env=env
    )
    assert r1.returncode == 0, r1.stderr

    result = subprocess.run(
        [str(emerge_binary), "-pC", "dev-libs/binpkgrmpkg"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )
    assert result.returncode == 0, result.stderr
    assert ">>> These are the packages that would be unmerged:" in result.stdout
    assert ">>> Unmerging (" not in result.stdout
    assert (root / "var/db/pkg/dev-libs/binpkgrmpkg-1.0").is_dir()
    assert (root / "usr/share/binpkgrmpkg/payload-1.0.txt").is_file()


def test_emerge_deselect_without_pretend_rewrites_world_and_world_sets(
    emerge_binary, tmp_path
):
    """`emerge --deselect` WITHOUT `--pretend` is a real write now (real
    action_deselect's `world_set.replace(remaining)`): the matching atoms
    leave var/lib/portage/world and matching @sets leave world_sets, both
    files rewritten sorted with comment lines dropped; `--pretend` still
    only previews."""
    wl = tmp_path / "var" / "lib" / "portage"
    wl.mkdir(parents=True)
    (wl / "world").write_text(
        "# my favorites\ndev-libs/zkeep\ndev-libs/adrop\ndev-libs/slotted:2\n"
    )
    (wl / "world_sets").write_text("@keepset\n@dropset\n")
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(tmp_path)

    # --pretend leaves both files untouched.
    before_world = (wl / "world").read_text()
    before_sets = (wl / "world_sets").read_text()
    p = subprocess.run(
        [str(emerge_binary), "--pretend", "--deselect", "dev-libs/adrop", "@dropset"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert p.returncode == 0, p.stderr
    assert '>>> Would remove @dropset from "world_sets" favorites file...' in p.stdout
    assert (wl / "world").read_text() == before_world
    assert (wl / "world_sets").read_text() == before_sets

    # The real thing: rewrites both, sorted, comment dropped.
    r = subprocess.run(
        [str(emerge_binary), "--deselect", "dev-libs/adrop", "@dropset"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert r.returncode == 0, r.stderr
    assert '>>> Removing dev-libs/adrop from "world" favorites file...' in r.stdout
    assert '>>> Removing @dropset from "world_sets" favorites file...' in r.stdout
    assert (wl / "world").read_text() == "dev-libs/slotted:2\ndev-libs/zkeep\n"
    assert (wl / "world_sets").read_text() == "@keepset\n"


def test_emerge_deselect_ask_prompts_before_rewriting_world(emerge_binary, tmp_path):
    """`emerge --deselect --ask` (real `action_deselect`): after the
    `>>> Removing ...` lines, prompt `Would you like to remove these
    packages from your world favorites? [Yes/No]`; `n` aborts (exit 130)
    with the world file untouched, empty answer proceeds."""
    wl = tmp_path / "var" / "lib" / "portage"
    wl.mkdir(parents=True)
    (wl / "world").write_text("dev-libs/zkeep\ndev-libs/adrop\n")
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(tmp_path)

    no = subprocess.run(
        [str(emerge_binary), "--ask", "--deselect", "dev-libs/adrop"],
        input="n\n", capture_output=True, text=True, check=False, env=env,
    )
    assert no.returncode == 130
    assert "Would you like to remove these packages from your world favorites? [Yes/No]" in no.stdout
    assert (wl / "world").read_text() == "dev-libs/zkeep\ndev-libs/adrop\n"

    yes = subprocess.run(
        [str(emerge_binary), "--ask", "--deselect", "dev-libs/adrop"],
        input="\n", capture_output=True, text=True, check=False, env=env,
    )
    assert yes.returncode == 0, yes.stderr
    assert (wl / "world").read_text() == "dev-libs/zkeep\n"


def _seed_binpkgrmpkg(emerge_binary, root, env, version):
    """Merge dev-libs/binpkgrmpkg-<version> into `root` via a direct
    `ebuild <file> merge` (full vdb entry: CONTENTS, environment.bz2,
    <pf>.ebuild, DEFINED_PHASES) so the removal paths have a real
    installed package to work on."""
    ebuild = str(
        Path(FIXTURES_ROOT)
        / f"repo/dev-libs/binpkgrmpkg/binpkgrmpkg-{version}.ebuild"
    )
    link = root.parent / "ebuild"
    if not link.exists():
        link.symlink_to(Path(emerge_binary).resolve())
    r = subprocess.run(
        [str(link), ebuild, "merge"], capture_output=True, text=True, check=False, env=env
    )
    assert r.returncode == 0, r.stderr
    assert (root / f"var/db/pkg/dev-libs/binpkgrmpkg-{version}").is_dir()


def test_emerge_depclean_without_pretend_really_removes_orphans(emerge_binary, tmp_path):
    """`emerge --depclean` (no args) WITHOUT `--pretend` really removes
    the cleanlist now (real `action_depclean` -> `unmerge(..., "unmerge",
    cleanlist, ordered=True)`). A lone installed `binpkgrmpkg-1.0` that no
    world/@system member needs is an orphan -> removed, its
    prerm/postrm run, and the stats block reads `Number removed:`."""
    root = tmp_path / "root"
    (root / "var/lib/portage").mkdir(parents=True)
    (root / "var/lib/portage/world").write_text("")
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(root)
    env["PORTAGE_TMPDIR"] = str(tmp_path / "portage-tmpdir")

    _seed_binpkgrmpkg(emerge_binary, root, env, "1.0")
    (root / "var/lib/binpkgrmpkg.log").write_text("")

    result = subprocess.run(
        [str(emerge_binary), "--depclean"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )
    assert result.returncode == 0, result.stderr
    assert " * Always study the list of packages to be cleaned" in result.stdout
    assert ">>> Calculating removal order..." in result.stdout
    assert ">>> These are the packages that would be unmerged:" not in result.stdout
    assert ">>> Unmerging (1 of 1) dev-libs/binpkgrmpkg-1.0..." in result.stdout
    assert "Number removed:       1" in result.stdout

    assert not (root / "var/db/pkg/dev-libs/binpkgrmpkg-1.0").exists()
    assert not (root / "usr/share/binpkgrmpkg/payload-1.0.txt").exists()
    assert (root / "var/lib/binpkgrmpkg.log").read_text() == "prerm-1.0\npostrm-1.0\n"


def test_emerge_prune_without_pretend_really_removes_lower_versions(emerge_binary, tmp_path):
    """`emerge --prune` WITHOUT `--pretend` really removes every installed
    version of a multi-version cp except the highest (real
    `action_depclean` `action="prune"` -> `unmerge(...)`). Seed 1.0 + 2.0,
    prune -> 1.0 gone, 2.0 kept."""
    root = tmp_path / "root"
    (root / "var/lib/portage").mkdir(parents=True)
    (root / "var/lib/portage/world").write_text("")
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(root)
    env["PORTAGE_TMPDIR"] = str(tmp_path / "portage-tmpdir")

    # Merge 1.0 for real (full vdb entry so its files/hooks are real), then
    # hand-place a minimal 2.0 vdb entry -- merging 2.0 via `ebuild merge`
    # would same-slot-replace 1.0 (both SLOT=0), leaving nothing to prune.
    _seed_binpkgrmpkg(emerge_binary, root, env, "1.0")
    v2 = root / "var/db/pkg/dev-libs/binpkgrmpkg-2.0"
    v2.mkdir(parents=True)
    (v2 / "CATEGORY").write_text("dev-libs\n")
    (v2 / "SLOT").write_text("0\n")
    (v2 / "CONTENTS").write_text("")
    (root / "var/lib/binpkgrmpkg.log").write_text("")

    result = subprocess.run(
        [str(emerge_binary), "--prune"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )
    assert result.returncode == 0, result.stderr
    assert ">>> Unmerging (1 of 1) dev-libs/binpkgrmpkg-1.0..." in result.stdout

    assert not (root / "var/db/pkg/dev-libs/binpkgrmpkg-1.0").exists()
    assert (root / "var/db/pkg/dev-libs/binpkgrmpkg-2.0").is_dir()
    assert not (root / "usr/share/binpkgrmpkg/payload-1.0.txt").exists()
    assert (root / "var/lib/binpkgrmpkg.log").read_text() == "prerm-1.0\npostrm-1.0\n"


def test_emerge_config_runs_pkg_config_from_the_vdb(emerge_binary, tmp_path):
    """`emerge --config <atom>` (real `action_config`): resolve the single
    installed match, print `Configuring pkg...`, run its `pkg_config` from
    the vdb-stored environment.bz2 + <pf>.ebuild. `dev-libs/emergeconfigpkg`'s
    pkg_config writes `${EROOT}/var/lib/emergeconfigpkg.configured`."""
    root = tmp_path / "root"
    (root / "var/lib").mkdir(parents=True)
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(root)
    env["PORTAGE_TMPDIR"] = str(tmp_path / "portage-tmpdir")

    ebuild = str(
        Path(FIXTURES_ROOT)
        / "repo/dev-libs/emergeconfigpkg/emergeconfigpkg-1.0.ebuild"
    )
    link = tmp_path / "ebuild"
    link.symlink_to(Path(emerge_binary).resolve())
    r = subprocess.run(
        [str(link), ebuild, "merge"], capture_output=True, text=True, check=False, env=env
    )
    assert r.returncode == 0, r.stderr
    assert (root / "var/db/pkg/dev-libs/emergeconfigpkg-1.0/environment.bz2").is_file()

    result = subprocess.run(
        [str(emerge_binary), "--config", "dev-libs/emergeconfigpkg"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )
    assert result.returncode == 0, result.stderr
    assert "Configuring pkg..." in result.stdout
    assert (root / "var/lib/emergeconfigpkg.configured").read_text() == "configured 1.0\n"

    # `--config --ask`: prompt `Ready to configure <cpv>?` instead of the
    # `Configuring pkg...` line; `n` aborts (exit 130) before pkg_config.
    (root / "var/lib/emergeconfigpkg.configured").unlink()
    no = subprocess.run(
        [str(emerge_binary), "--ask", "--config", "dev-libs/emergeconfigpkg"],
        input="n\n", capture_output=True, text=True, check=False, env=env,
    )
    assert no.returncode == 130
    assert "Ready to configure dev-libs/emergeconfigpkg-1.0? [Yes/No]" in no.stdout
    assert "Configuring pkg..." not in no.stdout
    assert not (root / "var/lib/emergeconfigpkg.configured").exists()

    yes = subprocess.run(
        [str(emerge_binary), "--ask", "--config", "dev-libs/emergeconfigpkg"],
        input="\n", capture_output=True, text=True, check=False, env=env,
    )
    assert yes.returncode == 0, yes.stderr
    assert (root / "var/lib/emergeconfigpkg.configured").read_text() == "configured 1.0\n"


def test_emerge_config_rejects_multiple_atoms_and_reports_missing(emerge_binary, tmp_path):
    """Real `action_config`: `len(myfiles) != 1` -> the red one-liner,
    exit 1; a valid atom matching nothing installed -> `No packages
    found.` exit 0."""
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(tmp_path / "root")

    multi = subprocess.run(
        [str(emerge_binary), "--config", "dev-libs/a", "dev-libs/b"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert multi.returncode == 1
    assert "config can only take a single package atom" in multi.stdout

    missing = subprocess.run(
        [str(emerge_binary), "--config", "dev-libs/nonexistent"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert missing.returncode == 0
    assert "No packages found." in missing.stdout


def test_emerge_config_shell_flag_selects_the_pkg_config_backend(
    emerge_binary, tmp_path
):
    """`emerge --config --shell bash|brush <atom>`: the portuale-only
    `--shell` flag now reaches the `pkg_config` phase too (real
    `action_config` -> `doebuild(ebuildpath, "config", ...)`), not just a
    real merge. Both backends run `dev-libs/emergeconfigpkg`'s own
    `pkg_config` and must produce the identical real marker file."""
    ebuild = str(
        Path(FIXTURES_ROOT)
        / "repo/dev-libs/emergeconfigpkg/emergeconfigpkg-1.0.ebuild"
    )
    for shell in ("brush", "bash"):
        root = tmp_path / f"root-{shell}"
        (root / "var/lib").mkdir(parents=True)
        env = dict(os.environ)
        env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
        env["ROOT"] = str(root)
        env["PORTAGE_TMPDIR"] = str(tmp_path / f"portage-tmpdir-{shell}")

        bindir = tmp_path / f"bin-{shell}"
        bindir.mkdir()
        link = bindir / "ebuild"
        link.symlink_to(Path(emerge_binary).resolve())
        merged = subprocess.run(
            [str(link), "--shell", shell, ebuild, "merge"],
            capture_output=True, text=True, check=False, env=env,
        )
        assert merged.returncode == 0, (shell, merged.stderr)

        result = subprocess.run(
            [str(emerge_binary), "--config", "--shell", shell, "dev-libs/emergeconfigpkg"],
            capture_output=True, text=True, check=False, env=env,
        )
        assert result.returncode == 0, (shell, result.stderr)
        assert "Configuring pkg..." in result.stdout
        assert (
            root / "var/lib/emergeconfigpkg.configured"
        ).read_text() == "configured 1.0\n"


def test_emerge_unmerge_shell_flag_reaches_prerm_postrm(emerge_binary, tmp_path):
    """`emerge -C --shell bash|brush <atom>`: `--shell` now threads into
    the removal-hook path too (`ebuild_merge::unmerge_one_installed`'s own
    `pkg_prerm`/`pkg_postrm` via `MergeOptions::from_env(shell, ...)`), not
    just a real merge. Both backends run `dev-libs/binpkgrmpkg`'s hooks
    and append the identical `<phase>-<PVR>` lines to the `${ROOT}` log."""
    import shutil

    v1 = str(
        Path(FIXTURES_ROOT) / "repo/dev-libs/binpkgrmpkg/binpkgrmpkg-1.0.ebuild"
    )
    for shell in ("brush", "bash"):
        root = tmp_path / f"root-{shell}"
        shutil.copytree(Path(FIXTURES_ROOT) / "var", root / "var")
        env = dict(os.environ)
        env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
        env["ROOT"] = str(root)
        env["PORTAGE_TMPDIR"] = str(tmp_path / f"portage-tmpdir-{shell}")

        bindir = tmp_path / f"bin-{shell}"
        bindir.mkdir()
        ebuild_link = bindir / "ebuild"
        ebuild_link.symlink_to(Path(emerge_binary).resolve())
        r1 = subprocess.run(
            [str(ebuild_link), "--shell", shell, v1, "merge"],
            capture_output=True, text=True, check=False, env=env,
        )
        assert r1.returncode == 0, (shell, r1.stderr)
        (root / "var/lib/binpkgrmpkg.log").write_text("")

        result = subprocess.run(
            [str(emerge_binary), "-C", "--shell", shell, "dev-libs/binpkgrmpkg"],
            capture_output=True, text=True, check=False, env=env,
        )
        assert result.returncode == 0, (shell, result.stderr)
        assert not (root / "var/db/pkg/dev-libs/binpkgrmpkg-1.0").exists()
        assert (
            root / "var/lib/binpkgrmpkg.log"
        ).read_text() == "prerm-1.0\npostrm-1.0\n"


def test_emerge_unmerge_backup_quickpkgs_before_removing(emerge_binary, tmp_path):
    """`FEATURES=unmerge-backup` (real `dblink._pre_unmerge_backup` ->
    quickpkg): `emerge -C` builds a binpkg of the *installed* package into
    $PKGDIR from its CONTENTS files, THEN removes it. Reuses
    `dev-libs/emergeconfigpkg` (installs
    /usr/share/emergeconfigpkg/emergeconfigpkg.txt)."""
    import tarfile as _tarfile

    root = tmp_path / "root"
    (root / "var/lib/portage").mkdir(parents=True)
    pkgdir = tmp_path / "pkgdir"
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(root)
    env["PORTAGE_TMPDIR"] = str(tmp_path / "portage-tmpdir")
    env["PKGDIR"] = str(pkgdir)

    ebuild = str(
        Path(FIXTURES_ROOT)
        / "repo/dev-libs/emergeconfigpkg/emergeconfigpkg-1.0.ebuild"
    )
    link = tmp_path / "ebuild"
    link.symlink_to(Path(emerge_binary).resolve())
    r = subprocess.run(
        [str(link), ebuild, "merge"], capture_output=True, text=True, check=False, env=env
    )
    assert r.returncode == 0, r.stderr
    assert (root / "usr/share/emergeconfigpkg/emergeconfigpkg.txt").is_file()

    env["FEATURES"] = "unmerge-backup"
    result = subprocess.run(
        [str(emerge_binary), "-C", "dev-libs/emergeconfigpkg"],
        capture_output=True, text=True, check=False, env=env,
    )
    assert result.returncode == 0, result.stderr
    assert ">>> Building backup package for dev-libs/emergeconfigpkg-1.0" in result.stdout

    # The backup binpkg exists, is a valid xpak (image tar + XPAK trailer),
    # holds the installed file, and got a Packages index entry.
    tbz2 = pkgdir / "dev-libs/emergeconfigpkg-1.0.tbz2"
    assert tbz2.is_file()
    assert b"XPAKSTOP" in tbz2.read_bytes()[-4096:]
    with _tarfile.open(tbz2, "r|*") as tf:
        names = [m.name.lstrip("./") for m in tf]
    assert "usr/share/emergeconfigpkg/emergeconfigpkg.txt" in names
    packages = (pkgdir / "Packages").read_text()
    assert "CPV: dev-libs/emergeconfigpkg-1.0" in packages

    # ...and only then is the package actually gone.
    assert not (root / "var/db/pkg/dev-libs/emergeconfigpkg-1.0").exists()
    assert not (root / "usr/share/emergeconfigpkg/emergeconfigpkg.txt").exists()


def _merge_slotopdepspkg(emerge_binary, root, env):
    ebuild = str(
        Path(FIXTURES_ROOT)
        / "repo/dev-libs/slotopdepspkg/slotopdepspkg-1.0.ebuild"
    )
    link = root.parent / "ebuild"
    if not link.exists():
        link.symlink_to(Path(emerge_binary).resolve())
    r = subprocess.run(
        [str(link), ebuild, "merge"], capture_output=True, text=True, check=False, env=env
    )
    assert r.returncode == 0, r.stderr


def test_merge_binds_the_slot_operator_in_stored_dep_metadata(emerge_binary, tmp_path):
    """Real `_post_src_install_write_metadata` ->
    `evaluate_slot_operator_equal_deps`: a merged package records its `:=`
    deps bound to the installed dependency's `<slot>/<sub-slot>=`.
    `dev-libs/slotopdepspkg` RDEPENDs `dev-libs/slotoptarget:=`."""
    root = tmp_path / "root"
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(root)
    env["PORTAGE_TMPDIR"] = str(tmp_path / "portage-tmpdir")

    # dev-libs/slotoptarget installed in slot 2 (SLOT="2" -> sub-slot 2).
    tgt = root / "var/db/pkg/dev-libs/slotoptarget-1.0"
    tgt.mkdir(parents=True)
    (tgt / "CATEGORY").write_text("dev-libs\n")
    (tgt / "SLOT").write_text("2\n")

    _merge_slotopdepspkg(emerge_binary, root, env)
    rdepend = (root / "var/db/pkg/dev-libs/slotopdepspkg-1.0/RDEPEND").read_text().strip()
    assert rdepend == "dev-libs/slotoptarget:2/2="


def test_merge_leaves_an_unresolvable_slot_operator_bare(emerge_binary, tmp_path):
    """Real `_eval_deps`: a `:=` dep with nothing installed to satisfy it
    is left as-is (`dev-libs/slotoptarget:=`)."""
    root = tmp_path / "root"
    root.mkdir()
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = str(root)
    env["PORTAGE_TMPDIR"] = str(tmp_path / "portage-tmpdir")

    _merge_slotopdepspkg(emerge_binary, root, env)
    rdepend = (root / "var/db/pkg/dev-libs/slotopdepspkg-1.0/RDEPEND").read_text().strip()
    assert rdepend == "dev-libs/slotoptarget:="


def test_ebuild_install_really_fetches_via_the_already_verified_skip_path(
    ebuild_binary, tmp_path
):
    """`dev-libs/verifiedfetchpkg` has a real SRC_URI using the full real
    grammar this slice implements: an arrow-rename and a `test?`
    USE-conditional group. Pre-seeding DISTDIR with a real, correctly-
    digested payload (the same real BLAKE2b-512/SHA-512 values the
    fixture's own checked-in Manifest records) exercises the real
    already-verified skip-fetch path end-to-end through the compiled
    CLI, with no live network access needed at all -- the fixture's own
    `src_install` records the real `A`/`AA` it observed, proving the
    conditional group is excluded from `A` but still present in `AA`."""
    ebuild_path = str(
        Path(FIXTURES_ROOT)
        / "repo/dev-libs/verifiedfetchpkg/verifiedfetchpkg-1.0.ebuild"
    )
    env = dict(os.environ)
    portage_tmpdir = tmp_path / "portage-tmpdir"
    distdir = tmp_path / "distdir"
    env["PORTAGE_TMPDIR"] = str(portage_tmpdir)
    env["DISTDIR"] = str(distdir)
    distdir.mkdir()
    (distdir / "verifiedfetchpkg-1.0.tar.gz").write_bytes(
        b"hello from verifiedfetchpkg\n"
    )

    result = subprocess.run(
        [str(ebuild_binary), ebuild_path, "install"],
        capture_output=True,
        text=True,
        check=True,
        env=env,
    )
    assert result.returncode == 0, result.stderr

    marker = (
        portage_tmpdir
        / "portage/dev-libs/verifiedfetchpkg-1.0/temp/fetch-vars.txt"
    )
    assert marker.read_text() == (
        "A=verifiedfetchpkg-1.0.tar.gz\n"
        "AA=verifiedfetchpkg-1.0.tar.gz verifiedfetchpkg-tests-1.0.tar.gz\n"
    )


def test_ebuild_install_restrict_fetch_never_downloads_the_plain_uri(
    ebuild_binary, tmp_path
):
    """`dev-libs/fetchrestrictpkg` has `RESTRICT="fetch"` and a plain
    `https://example.invalid/...` SRC_URI. Real `fetch.py:1167`: a plain
    URI is barred from the fetchable-candidate list under RESTRICT=fetch,
    and the public mirrors too -- so with the distfile ABSENT from
    DISTDIR the install fails, first running the ebuild's own pkg_nofetch
    phase (its `elog` "download it from ..." lines print above the error,
    real fetch.py's spawn_nofetch), and crucially never tries to reach
    example.invalid. With the distfile PRESENT (user-placed) and
    Manifest-verified, the install succeeds via the already-verified
    skip path."""
    ebuild_path = str(
        Path(FIXTURES_ROOT)
        / "repo/dev-libs/fetchrestrictpkg/fetchrestrictpkg-1.0.ebuild"
    )
    env = dict(os.environ)
    env["PORTAGE_TMPDIR"] = str(tmp_path / "portage-tmpdir")
    distdir = tmp_path / "distdir"
    env["DISTDIR"] = str(distdir)
    distdir.mkdir()

    # ABSENT -> fails fast (no network), with the RESTRICT=fetch message.
    absent = subprocess.run(
        [str(ebuild_binary), ebuild_path, "install"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
        timeout=30,
    )
    assert absent.returncode == 1
    assert "RESTRICT=fetch" in absent.stderr
    assert "example.invalid" not in absent.stderr or "bars downloading" in absent.stderr
    # The ebuild's own pkg_nofetch phase ran and printed its instructions.
    assert "Please download fetchrestrictpkg-1.0.tar.gz from https://example.org/" in (
        absent.stdout + absent.stderr
    )

    # PRESENT + verified -> installs.
    (distdir / "fetchrestrictpkg-1.0.tar.gz").write_bytes(
        b"fetchrestrictpkg fixture distfile\n"
    )
    present = subprocess.run(
        [str(ebuild_binary), ebuild_path, "install"],
        capture_output=True,
        text=True,
        check=True,
        env=env,
    )
    assert present.returncode == 0, present.stderr
    marker = (
        tmp_path
        / "portage-tmpdir/portage/dev-libs/fetchrestrictpkg-1.0/temp/fetch-vars.txt"
    )
    assert marker.read_text() == "A=fetchrestrictpkg-1.0.tar.gz\n"


def test_ebuild_install_really_inherits_a_real_eclass(ebuild_binary, tmp_path):
    """`dev-libs/eclasspkg` really `inherit`s a real (if fixture-only)
    eclass, `pilotcheck.eclass`, via real, unmodified `bin/ebuild.sh`'s
    own `inherit()` function -- previously portuale never populated
    `PORTAGE_ECLASS_LOCATIONS` at all, so this would have `die`d
    immediately with `"pilotcheck.eclass could not be found by
    inherit()"`. Confirmed live against a real system before this fix:
    `sys-fs/fuse`, `app-editors/nano`, and `app-arch/xz-utils` all
    failed here, each on a different real eclass. `src_install` calls a
    real function the eclass defines, proving the eclass's own content
    -- not just its own existence -- is really usable afterward."""
    ebuild_path = str(
        Path(FIXTURES_ROOT) / "repo/dev-libs/eclasspkg/eclasspkg-1.0.ebuild"
    )
    env = dict(os.environ)
    portage_tmpdir = tmp_path / "portage-tmpdir"
    env["PORTAGE_TMPDIR"] = str(portage_tmpdir)

    result = subprocess.run(
        [str(ebuild_binary), ebuild_path, "install"],
        capture_output=True,
        text=True,
        check=True,
        env=env,
    )
    assert result.returncode == 0, result.stderr
    # No spurious "inherited illegally" QA notice: `phase_env_vars`
    # exports `INHERITED` (from the ebuild's md5-cache) so `bin/ebuild.sh`
    # snapshots it into `__INHERITED_QA_CACHE` before a non-`depend`
    # phase re-sources the ebuild -- exactly as real portage does.
    assert "inherited illegally" not in (result.stdout + result.stderr), (
        result.stdout + result.stderr
    )


def test_ebuild_install_does_not_deadlock_on_a_large_eclass_scope(
    ebuild_binary, tmp_path
):
    """Regression test for a real upstream `brush` bug, since fixed in
    the pinned fork (see docs/what-this-proves.md's eclass section for the full
    root-cause writeup): a shell function used as a non-last pipeline
    stage used to run inline rather than as a background task, so real
    `bin/phase-functions.sh`'s own post-phase `__save_ebuild_env |
    __filter_readonly_variables` pipe (both sides real shell functions)
    deadlocked once `__save_ebuild_env`'s own `declare -f` dump exceeded
    the OS pipe buffer (~64KiB on Linux) before `__filter_readonly_
    variables` was even spawned to drain it. `bigfixture.eclass` defines
    ~400 functions specifically to exceed that threshold, the same way
    the real `multilib` eclass family did when this was first found live
    against real `app-arch/xz-utils`/`sys-fs/fuse`. An explicit
    `timeout=` below makes a regression here fail this test outright
    instead of hanging the whole suite."""
    ebuild_path = str(
        Path(FIXTURES_ROOT) / "repo/dev-libs/bigeclasspkg/bigeclasspkg-1.0.ebuild"
    )
    env = dict(os.environ)
    portage_tmpdir = tmp_path / "portage-tmpdir"
    env["PORTAGE_TMPDIR"] = str(portage_tmpdir)

    result = subprocess.run(
        [str(ebuild_binary), ebuild_path, "install"],
        capture_output=True,
        text=True,
        check=True,
        env=env,
        timeout=30,
    )
    assert result.returncode == 0, result.stderr

    marker = (
        portage_tmpdir / "portage/dev-libs/bigeclasspkg-1.0/temp/bigfixture-marker.txt"
    )
    assert marker.read_text() == "hello from bigfixture.eclass\n"


def test_ebuild_shell_bash_and_brush_produce_the_same_real_result(
    ebuild_binary, tmp_path
):
    """`--shell bash|brush` (default `bash`) selects which real shell
    backend executes every phase: a genuine `bash <bin_dir>/ebuild.sh
    <phase>` subprocess (the default -- matching real portage's own
    `_doebuild_spawn()` invocation shape), or the embedded
    `brush_core::Shell` (see `ebuild_phases::ShellBackend`'s own doc
    comment and docs/what-this-proves.md's eclass section for the full
    writeup, including why the default is `bash`). Both backends run the
    same real `dev-libs/phasepkg` fixture's own `src_install`, so this
    asserts they produce an identical real file, not just a zero exit
    code each."""
    ebuild_path = str(Path(FIXTURES_ROOT) / "repo/dev-libs/phasepkg/phasepkg-1.0.ebuild")
    installed_relative = "portage/dev-libs/phasepkg-1.0/image/usr/share/phasepkg/hello.txt"

    for shell, subdir in [("brush", "brush-run"), ("bash", "bash-run")]:
        env = dict(os.environ)
        portage_tmpdir = tmp_path / subdir
        env["PORTAGE_TMPDIR"] = str(portage_tmpdir)

        result = subprocess.run(
            [str(ebuild_binary), "--shell", shell, ebuild_path, "install"],
            capture_output=True,
            text=True,
            env=env,
        )
        assert result.returncode == 0, (shell, result.stderr)
        assert (portage_tmpdir / installed_relative).read_text() == "hello from phasepkg\n"


def test_ebuild_shell_accepts_the_inline_equals_form(ebuild_binary, tmp_path):
    """`--shell=bash`, not just `--shell bash` -- same inline-`=` form
    every real `Kind::Value` ebuild option already accepts (see
    `ebuild.rs`'s own CLI-parsing loop)."""
    ebuild_path = str(Path(FIXTURES_ROOT) / "repo/dev-libs/phasepkg/phasepkg-1.0.ebuild")
    env = dict(os.environ)
    portage_tmpdir = tmp_path / "portage-tmpdir"
    env["PORTAGE_TMPDIR"] = str(portage_tmpdir)

    result = subprocess.run(
        [str(ebuild_binary), "--shell=bash", ebuild_path, "install"],
        capture_output=True,
        text=True,
        env=env,
    )
    assert result.returncode == 0, result.stderr


def test_ebuild_shell_rejects_an_invalid_value(ebuild_binary):
    """A portuale-only flag, not a real `bin/ebuild` option -- so unlike
    every real `Kind::Value` option (which accepts any string, unchecked)
    `--shell` validates its own value against exactly `"bash"`/
    `"brush"`."""
    result = subprocess.run(
        [str(ebuild_binary), "--shell", "zsh", "foo-1.0.ebuild", "pretend"],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 1
    assert result.stderr.strip() == (
        'ebuild: --shell: "zsh" is not "bash" or "brush"'
    )


def test_ebuild_shell_requires_a_value(ebuild_binary):
    result = subprocess.run(
        [str(ebuild_binary), "--shell"],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 2
    assert result.stderr.strip() == "ebuild: option '--shell' requires a value"


def _unshare_usable(*flags: str) -> bool:
    """Whether `unshare <flags> --map-root-user` works here (the same
    check `ebuild_phases::unshare_combo_usable` caches)."""
    try:
        return (
            subprocess.run(
                ["unshare", *flags, "--map-root-user", "--", "true"],
                capture_output=True,
            ).returncode
            == 0
        )
    except FileNotFoundError:
        return False


def _unshare_net_usable() -> bool:
    return _unshare_usable("--net")


def _netsandbox_probe(portage_tmpdir: Path) -> dict[str, str]:
    text = (
        portage_tmpdir
        / "portage/dev-libs/netsandboxpkg-1.0/image/usr/share/netsandboxpkg/netsandbox-probe"
    ).read_text()
    out: dict[str, str] = {}
    for line in text.splitlines():
        if "=" in line and not line.startswith(("bash:", " ")):
            k, _, v = line.partition("=")
            out.setdefault(k, v)
    return out


def test_ebuild_network_sandbox_gives_src_phases_a_fresh_network_namespace(
    ebuild_binary, tmp_path
):
    """FEATURES=network-sandbox (SCOPE_BACKLOG Part 2.D): the six real
    `src_*` phases run inside a fresh network namespace via
    `unshare --net --map-root-user` (real portage's own
    `unshare(CLONE_NEWNET)`). `dev-libs/netsandboxpkg`'s src_compile
    records `readlink /proc/self/ns/net`, its visible interfaces (from
    the per-netns `/proc/net/dev`), and an outbound-connect result. Run
    twice -- with and without the feature -- and compare: the feature
    must put the phase in a *different* netns whose only interface is
    `lo` and where a non-loopback connect is unreachable."""
    if not _unshare_net_usable():
        pytest.skip("unprivileged network-namespace unshare unavailable here")

    ebuild_path = str(
        Path(FIXTURES_ROOT) / "repo/dev-libs/netsandboxpkg/netsandboxpkg-1.0.ebuild"
    )

    base_env = dict(os.environ)
    base_env["ROOT"] = str(tmp_path / "root")

    plain_env = dict(base_env)
    plain_env["FEATURES"] = ""
    plain_env["PORTAGE_TMPDIR"] = str(tmp_path / "plain")
    plain = subprocess.run(
        [str(ebuild_binary), ebuild_path, "install"],
        capture_output=True,
        text=True,
        env=plain_env,
    )
    assert plain.returncode == 0, plain.stderr
    plain_probe = _netsandbox_probe(tmp_path / "plain")

    sb_env = dict(base_env)
    sb_env["FEATURES"] = "network-sandbox"
    sb_env["PORTAGE_TMPDIR"] = str(tmp_path / "sandboxed")
    sandboxed = subprocess.run(
        [str(ebuild_binary), ebuild_path, "install"],
        capture_output=True,
        text=True,
        env=sb_env,
    )
    assert sandboxed.returncode == 0, sandboxed.stderr
    sb_probe = _netsandbox_probe(tmp_path / "sandboxed")

    # The sandboxed src_compile ran in its own network namespace...
    assert sb_probe["netns"] != plain_probe["netns"], (plain_probe, sb_probe)
    # ...with only loopback...
    assert sb_probe["ifaces"] == "lo", sb_probe
    # ...and no route off it.
    assert "Network is unreachable" in sb_probe["connect"], sb_probe


def test_ebuild_without_network_sandbox_shares_the_host_network_namespace(
    ebuild_binary, tmp_path
):
    """Regression guard: with no FEATURES=network-sandbox, src_compile
    runs in the host network namespace exactly as before -- same netns as
    the portuale process, more than just `lo` visible (unless the host
    itself only has `lo`, hence the tolerant interface check)."""
    ebuild_path = str(
        Path(FIXTURES_ROOT) / "repo/dev-libs/netsandboxpkg/netsandboxpkg-1.0.ebuild"
    )
    env = dict(os.environ)
    env["FEATURES"] = ""
    env["ROOT"] = str(tmp_path / "root")
    env["PORTAGE_TMPDIR"] = str(tmp_path / "pt")
    result = subprocess.run(
        [str(ebuild_binary), ebuild_path, "install"],
        capture_output=True,
        text=True,
        env=env,
    )
    assert result.returncode == 0, result.stderr
    probe = _netsandbox_probe(tmp_path / "pt")
    assert probe["netns"] == os.readlink("/proc/self/ns/net")


@pytest.mark.parametrize(
    "feature, unshare_flag, probe_key",
    [
        ("ipc-sandbox", "--ipc", "ipcns"),
        ("mount-sandbox", "--mount", "mntns"),
        ("pid-sandbox", "--pid", "pidns"),
    ],
)
def test_ebuild_sandbox_features_each_unshare_their_own_namespace(
    ebuild_binary, tmp_path, feature, unshare_flag, probe_key
):
    """FEATURES=ipc-sandbox / mount-sandbox / pid-sandbox (SCOPE_BACKLOG
    Part 2.D): each puts the src_* phases in its own namespace via the
    matching `unshare(1)` flag (real _doebuild_spawn's
    unshare_{ipc,mount,pid}). `dev-libs/netsandboxpkg` records
    `readlink /proc/self/ns/{ipc,mnt,pid}`; with the feature on that id
    must differ from the portuale process's own."""
    if not _unshare_usable(unshare_flag):
        pytest.skip(f"unshare {unshare_flag} unavailable here")

    ebuild_path = str(
        Path(FIXTURES_ROOT) / "repo/dev-libs/netsandboxpkg/netsandboxpkg-1.0.ebuild"
    )
    env = dict(os.environ)
    env["FEATURES"] = feature
    env["ROOT"] = str(tmp_path / "root")
    env["PORTAGE_TMPDIR"] = str(tmp_path / "pt")
    result = subprocess.run(
        [str(ebuild_binary), ebuild_path, "install"],
        capture_output=True,
        text=True,
        env=env,
    )
    assert result.returncode == 0, result.stderr
    probe = _netsandbox_probe(tmp_path / "pt")

    host_ns = os.readlink(f"/proc/self/ns/{probe_key[:-2]}")
    assert probe[probe_key] != host_ns, probe
    if feature == "pid-sandbox":
        # A fresh PID namespace hides every host process.
        assert int(probe["procs"]) < 20, probe


_FSSANDBOX_EBUILD = "repo/dev-libs/fssandboxpkg/fssandboxpkg-1.0.ebuild"
_FSSANDBOX_SANDBOX_LOG = (
    "portage/dev-libs/fssandboxpkg-1.0/temp/sandbox.log"
)
_FSSANDBOX_IMAGE_FILE = (
    "portage/dev-libs/fssandboxpkg-1.0/image/usr/share/fssandboxpkg/hello.txt"
)


def test_ebuild_sandbox_denies_and_fails_on_a_write_outside_the_build_tree(
    ebuild_binary, tmp_path
):
    """FEATURES=sandbox (SCOPE_BACKLOG Part 2.D): the six real src_* phases
    run wrapped in the sys-apps/sandbox binary (real portage's own
    spawn_sandbox). `dev-libs/fssandboxpkg`'s src_install writes a legit
    file into ${D} and also tries to write /var/lib/portage-portuale-sandbox-
    probe; with the feature on, `sandbox` denies the stray write, records
    it in ${T}/sandbox.log, and exits non-zero -- so `ebuild install`
    fails."""
    if not os.access("/usr/bin/sandbox", os.X_OK):
        pytest.skip("/usr/bin/sandbox not installed")

    env = dict(os.environ)
    env["FEATURES"] = "sandbox"
    env["ROOT"] = str(tmp_path / "root")
    env["PORTAGE_TMPDIR"] = str(tmp_path / "pt")
    result = subprocess.run(
        [str(ebuild_binary), str(Path(FIXTURES_ROOT) / _FSSANDBOX_EBUILD), "install"],
        capture_output=True,
        text=True,
        env=env,
    )
    assert result.returncode != 0
    assert "ACCESS DENIED" in result.stderr
    log = tmp_path / "pt" / _FSSANDBOX_SANDBOX_LOG
    assert log.is_file() and log.stat().st_size > 0
    assert "/var/lib/portage-portuale-sandbox-probe" in log.read_text()


def test_ebuild_without_sandbox_tolerates_the_same_stray_write(
    ebuild_binary, tmp_path
):
    """Regression guard: with no FEATURES=sandbox, the stray
    /var/lib write just fails with EACCES (it is not fatal on its own),
    the legit file still lands in ${D}, and `ebuild install` exits 0 --
    exactly as before this slice. No sandbox.log is written."""
    env = dict(os.environ)
    env["FEATURES"] = ""
    env["ROOT"] = str(tmp_path / "root")
    env["PORTAGE_TMPDIR"] = str(tmp_path / "pt")
    result = subprocess.run(
        [str(ebuild_binary), str(Path(FIXTURES_ROOT) / _FSSANDBOX_EBUILD), "install"],
        capture_output=True,
        text=True,
        env=env,
    )
    assert result.returncode == 0, result.stderr
    assert (tmp_path / "pt" / _FSSANDBOX_IMAGE_FILE).read_text().strip() == (
        "hello from fssandboxpkg"
    )
    log = tmp_path / "pt" / _FSSANDBOX_SANDBOX_LOG
    assert not log.exists() or log.stat().st_size == 0
