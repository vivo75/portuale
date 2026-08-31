"""Black-box test for the emerge/ebuild portuale skeleton (see
PORTING/PROMPT.md, "emerge/ebuild binary shape"). Tests the real compiled
CLI via symlinks in a PATH, exactly as it would be invoked in practice --
not by importing anything from the binary.

Also covers `ebuild`'s CLI-surface-recognition follow-up (see
PORTING/rust/portuale/src/ebuild.rs/ebuild_options.rs): real ebuild
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
    assert "ebuild (pilot stub)" in result.stdout


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
    assert "ebuild (pilot stub)" in result.stdout


def test_unrecognized_applet_fails_clearly(portuale_binary):
    result = subprocess.run(
        [str(portuale_binary)], capture_output=True, text=True, check=False
    )
    assert result.returncode != 0
    assert "unrecognized applet" in result.stderr


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
    assert "ebuild (pilot stub)" in result.stdout


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
    assert "ebuild (pilot stub)" in result.stdout
    assert 'ebuild file: "foo-1.0.ebuild"' in result.stdout
    assert 'commands: ["clean"]' in result.stdout


def test_ebuild_accepts_the_inline_equals_form_of_a_value_option(ebuild_binary):
    result = subprocess.run(
        [str(ebuild_binary), "--color=y", "foo-1.0.ebuild", "clean"],
        capture_output=True,
        text=True,
        check=True,
    )
    assert "ebuild (pilot stub)" in result.stdout


def test_ebuild_rejects_an_unrecognized_option(ebuild_binary):
    """Distinct from a real-but-unimplemented option: a token that isn't
    in bin/ebuild's own option surface at all is rejected immediately
    and specifically, unlike real bin/ebuild's own argparse (which uses
    parse_known_args and would silently swallow it into the positional
    args instead -- see ebuild.rs's doc comment for why this pilot
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
        "ebuild (pilot stub): command-line interface to the Rust porting pilot"
    )


def test_ebuild_short_help_alias_is_implemented(ebuild_binary):
    result = subprocess.run(
        [str(ebuild_binary), "-h"], capture_output=True, text=True, check=False
    )
    assert result.returncode == 0
    assert result.stdout.startswith(
        "ebuild (pilot stub): command-line interface to the Rust porting pilot"
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
            "ebuild (pilot stub): command-line interface to the Rust porting pilot"
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
    assert "ebuild (pilot stub)" in result.stdout


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
    both `tmp_path`-relative here), matching how this pilot's own manual
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
    non-dry-run execution path this pilot implements for `emerge`
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
    pilot's first real source build-and-merge for `emerge` itself
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
    DISTDIR the install fails (this pilot doesn't run the ebuild's own
    pkg_nofetch phase, a documented cut -- it fails with a "place it in
    DISTDIR by hand" pointer), and crucially never tries to reach
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
    own `inherit()` function -- previously this pilot never populated
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


def test_ebuild_install_does_not_deadlock_on_a_large_eclass_scope(
    ebuild_binary, tmp_path
):
    """Regression test for a real upstream `brush` bug, since fixed in
    the pinned fork (see README.md's own eclass section for the full
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


def test_ebuild_shell_bash_produces_the_same_real_result_as_the_brush_default(
    ebuild_binary, tmp_path
):
    """`--shell bash|brush` (default `brush`) selects which real shell
    backend executes every phase: the default embedded `brush_core::
    Shell`, or a genuine `bash <bin_dir>/ebuild.sh <phase>` subprocess
    (matching real portage's own `_doebuild_spawn()` invocation shape --
    see `ebuild_phases::ShellBackend`'s own doc comment and README.md's
    own eclass section for the full writeup). Both backends run the same
    real `dev-libs/phasepkg` fixture's own `src_install`, so this
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
    """A pilot-only flag, not a real `bin/ebuild` option -- so unlike
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
