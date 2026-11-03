"""S0 #649 Step 2 gate check: run the FIXED binary against scratch hermetic
worlds mirroring playground N0/N1, and record which portuale gate fires.

Scratch dirs live under /tmp/s649 (UNCOMMITTED, never under fixtures/;
rebuilt by every run). Hermeticity: PORTAGE_CONFIGROOT == ROOT == the
scratch dir (the _t341_configroot precedent), `make.profile` symlinked at
the fixture default profile, repos.conf holding ONLY the scratch repo, and
`PORTAGE_REPOSITORIES` pinned to that same text (the #645 Task-2 host-gentoo
leak must not recur -- a host repos.conf must never re-scope the run).
`PORTAGE_RUNNING_ROOT` == ROOT (the fixture_env precedent: BDEPENDs must
not consult the host vdb). The suite's `_ENV_CONFIG_VARS` are stripped from
the child env so host USE/ACCEPT_KEYWORDS/FEATURES cannot leak in.
`seed_updates_record` is skipped deliberately: the scratch repo ships no
`profiles/updates` dir, so there is nothing to apply (stated; re-check the
stdout for a global-updates block on every run).

Worlds (ebuild/vdb text mirrors probe649_mutations.py N0/N1 dict-for-dict,
except the `!>` atoms keep the probe spelling per that file's ARMS note):
- N0 live-only: perl-5.42.2 + both 5.42-arm installed virtuals, bare
  `dev-lang/perl` request, NO 5.44.0 in tree. Portage oracle (this probe's
  companion run): MERGELIST ['dev-lang/perl-5.42.2'].
- N1 N0 + perl-5.44.0 in tree (identical PDEPEND). Portage oracle:
  MERGELIST ['dev-lang/perl-5.42.2'] -- the guard (dep_check.py:634-635)
  transiently demotes `=perl-5.42*` while 5.44.0 is co-scheduled, but the
  final evaluation re-resolves to the installed arm.

Deltas vs the real cell (stated, all outcome-neutral for the verdict):
the `=perl-5.40*` leg dangles (no perl-5.40.2 in the scratch tree; the real
tree has it, but the arm is guard-demoted there and unavailable here --
non-winning on both sides); no other installed packages exist, so no
`dev-lang/perl:0/5.42=` reverse edges (they argue against 5.44.0, never
for the fall-through); the non-interacting PDEPEND conjuncts
(perl-cleaner/CPAN/File-Temp/Data-Dumper/Math-BigInt) are omitted, like
the M4 probe's Carp/Exporter/Except omission.

Gate identification (no product debug logging exists in resolve/, so the
inference is structural + the companion trace):
- Consumer-side `expand_consumer_virtual_arms` CANNOT fire on either world:
  no merge-bound consumer string contains a top-level `||` with a bare
  virtual arm (perl's PDEPEND is plain conjuncts; the virtuals' own `||`
  arms name dev-lang/perl + perl-core, never category `virtual`, so
  `expand_arm` returns None for every arm). Any extra is therefore the
  `virtual_expansion_seeds` feed (feedback/mod.rs:318).
- The winning depstring is read off the merged pins: 3.210.0 / 3.500.0
  exist ONLY in the installed 5.42-arm conjunctions (no tree ebuilds carry
  those pins -- the N1 companion trace's losing side).
- The missing Portage-side counterpart is the final re-resolution: Portage
  re-evaluates the kept virtual's `||` after the slot-conflict skip drops
  5.44.0 from the graph (companion bins: `preferred_in_graph` for
  `=perl-5.42*`, `preferred_non_installed`/unavailable for `~perl-core`);
  portuale's seed is a merge commitment with no demotion vehicle
  (rev_dep_or.rs:681-683), computed once against pass.entries that still
  hold the transient 5.44.0.

Run: python3 probe649_gate.py [N0|N1]  (default: both).
Expects `target/release/portuale` to be the fixed binary (Task-2 BASE
4faa021b; `cargo build --release` is a no-op when fresh).
"""

import os
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
PM = HERE.parents[2]
BINARY = PM / "target" / "release" / "portuale"
FIXTURES = Path(os.environ.get("PORTUALE_FIXTURES", PM / "fixtures"))
SCRATCH_BASE = Path("/tmp/s649")

ENC_RDEPEND_542 = (
    "|| ( =dev-lang/perl-5.42* =dev-lang/perl-5.40*"
    " ~perl-core/Encode-3.210.0 ) dev-lang/perl:= !<perl-core/Encode-3.210.0"
    " !>perl-core/Encode-3.210.0-r999"
)
ENC_RDEPEND_544 = (
    "|| ( =dev-lang/perl-5.44* ~perl-core/Encode-3.240.0 )"
    " dev-lang/perl:= !<perl-core/Encode-3.240.0 !>perl-core/Encode-3.240.0-r999"
)
TH_RDEPEND_542 = (
    "|| ( =dev-lang/perl-5.42*"
    " ~perl-core/Test-Harness-3.500.0 ) dev-lang/perl:="
    " !<perl-core/Test-Harness-3.500.0 !>perl-core/Test-Harness-3.500.0-r999"
)
TH_RDEPEND_544 = (
    "|| ( =dev-lang/perl-5.44* ~perl-core/Test-Harness-3.520.0 )"
    " dev-lang/perl:= !<perl-core/Test-Harness-3.520.0"
    " !>perl-core/Test-Harness-3.520.0-r999"
)
PERL_PDEPEND = "!minimal? ( >=virtual/perl-Encode-3.120.0 virtual/perl-Test-Harness )"


def _perl_ebuild(slot):
    return (
        'EAPI=8\nDESCRIPTION="s649 perl"\nSLOT="0/%s"\nKEYWORDS="amd64"\n'
        'IUSE="minimal"\nPDEPEND="%s"\n' % (slot, PERL_PDEPEND)
    )


def _virt_ebuild(rdepend):
    return 'EAPI=8\nDESCRIPTION="s649 virtual"\nSLOT="0"\nKEYWORDS="amd64"\nRDEPEND="%s"\n' % rdepend


def _core_ebuild():
    return 'EAPI=8\nDESCRIPTION="s649 core"\nSLOT="0"\nKEYWORDS="amd64"\n'


def _world_ebuilds(with_544):
    ebuilds = {
        "dev-lang/perl/perl-5.42.2.ebuild": _perl_ebuild("5.42"),
        "virtual/perl-Encode/perl-Encode-3.210.0-r2.ebuild": _virt_ebuild(ENC_RDEPEND_542),
        "virtual/perl-Encode/perl-Encode-3.240.0.ebuild": _virt_ebuild(ENC_RDEPEND_544),
        "virtual/perl-Test-Harness/perl-Test-Harness-3.500.0-r1.ebuild": _virt_ebuild(
            TH_RDEPEND_542
        ),
        "virtual/perl-Test-Harness/perl-Test-Harness-3.520.0.ebuild": _virt_ebuild(
            TH_RDEPEND_544
        ),
        "perl-core/Encode/Encode-3.190.0.ebuild": _core_ebuild(),
        "perl-core/Encode/Encode-3.210.0.ebuild": _core_ebuild(),
        "perl-core/Test-Harness/Test-Harness-3.500.0.ebuild": _core_ebuild(),
    }
    if with_544:
        ebuilds["dev-lang/perl/perl-5.44.0.ebuild"] = _perl_ebuild("5.44")
    return ebuilds


def _vdb_entries(with_pin):
    # (vdb RDEPEND mirrors the tree arms: the Task-1 substitution, revision
    # equality at the pin. The virtuals' RDEPEND is what
    # live_metadata_for_installed feeds the seed gate. The N2 pin-holder
    # carries the cell's `dev-lang/perl:0/5.42=` reverse edge; installed
    # only, no tree ebuild -- background, never merged.)
    entries = {
        "dev-lang/perl-5.42.2": {"SLOT": "0/5.42", "RDEPEND": ""},
        "virtual/perl-Encode-3.210.0-r2": {"SLOT": "0", "RDEPEND": ENC_RDEPEND_542},
        "virtual/perl-Test-Harness-3.500.0-r1": {"SLOT": "0", "RDEPEND": TH_RDEPEND_542},
    }
    if with_pin:
        entries["dev-perl/s649pin-1"] = {"SLOT": "0", "RDEPEND": "dev-lang/perl:0/5.42="}
    return entries


# The suite's _ENV_CONFIG_VARS (conftest.py): stripped so the host config
# cannot leak into the scratch run.
_ENV_STRIP = (
    "USE ACCEPT_KEYWORDS USE_EXPAND USE_EXPAND_UNPREFIXED USE_EXPAND_IMPLICIT "
    "USE_EXPAND_HIDDEN IUSE_IMPLICIT ACCEPT_LICENSE ACCEPT_PROPERTIES "
    "ACCEPT_RESTRICT PKGDIR PORTAGE_LOGDIR PORTAGE_BINHOST PORTAGE_NICENESS "
    "PORTAGE_IONICE_COMMAND PORTAGE_SCHEDULING_POLICY PORTAGE_SCHEDULING_PRIORITY "
    "PORTAGE_ELOG_SYSTEM PORTAGE_ELOG_CLASSES PORTAGE_ELOG_MAILURI FEATURES CHOST "
    "CBUILD CTARGET CFLAGS CXXFLAGS CPPFLAGS LDFLAGS FFLAGS FCFLAGS MAKEOPTS "
    "EMERGE_DEFAULT_OPTS PORTAGE_RSYNC_EXTRA_OPTS GENTOO_MIRRORS VIDEO_CARDS "
    "PYTHON_TARGETS PYTHON_SINGLE_TARGET LINGUAS L10N CPU_FLAGS_X86 ELIBC KERNEL "
    "USERLAND ABI_X86 BINPKG_FORMAT BINPKG_COMPRESS BINPKG_COMPRESS_FLAGS "
    "PORTAGE_BZIP2_COMMAND PORTAGE_BUNZIP2_COMMAND CONFIG_PROTECT "
    "CONFIG_PROTECT_MASK ENV_UNSET PORTAGE_REPOSITORIES"
).split()


def build_world(name, with_544, with_pin=False):
    root = SCRATCH_BASE / name
    if root.exists():
        shutil.rmtree(root)
    repo = root / "repo"
    (repo / "profiles").mkdir(parents=True)
    (repo / "metadata").mkdir(parents=True)
    (repo / "profiles/repo_name").write_text("s649repo\n")
    (repo / "metadata/layout.conf").write_text("masters =\n")
    cats = set()
    for rel, text in _world_ebuilds(with_544).items():
        p = repo / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text)
        cats.add(rel.split("/")[0])
    (repo / "profiles/categories").write_text("".join(c + "\n" for c in sorted(cats)))
    # Minimal standalone profile (NOT the fixture default: its parent chain
    # pulls `overlay:crossrepo-parent`, which a scratch repos.conf cannot see).
    prof = repo / "profiles" / "s649"
    prof.mkdir(parents=True, exist_ok=True)
    (prof / "eapi").write_text("8\n")
    (prof / "make.defaults").write_text('ARCH="amd64"\nACCEPT_KEYWORDS="${ARCH}"\n')
    for vdb_rel, fields in _vdb_entries(with_pin).items():
        d = root / "var/db/pkg" / vdb_rel
        d.mkdir(parents=True)
        cat = vdb_rel.split("/")[0]
        (d / "CATEGORY").write_text(cat + "\n")
        (d / "EAPI").write_text("8\n")
        (d / "SLOT").write_text(fields["SLOT"] + "\n")
        (d / "RDEPEND").write_text(fields["RDEPEND"] + "\n")
        (d / "repository").write_text("s649repo\n")
    (root / "var/lib/portage").mkdir(parents=True, exist_ok=True)
    (root / "var/lib/portage/world").write_text("dev-lang/perl\n")
    (root / "distfiles").mkdir(parents=True, exist_ok=True)
    etc = root / "etc/portage"
    etc.mkdir(parents=True)
    repos_conf = (
        "[DEFAULT]\nmain-repo = s649repo\n\n[s649repo]\nlocation = %s\n" % (repo)
    )
    (etc / "repos.conf").write_text(repos_conf)
    (etc / "make.conf").write_text('PKGDIR="%s"\nFEATURES="-news"\n' % (root / "pkgdir"))
    (etc / "make.profile").symlink_to(repo / "profiles/s649")
    env = dict(os.environ)
    for k in _ENV_STRIP:
        env.pop(k, None)
    env["PORTAGE_CONFIGROOT"] = str(root)
    env["ROOT"] = str(root)
    env["PORTAGE_RUNNING_ROOT"] = str(root)
    env["DISTDIR"] = str(root / "distfiles")
    env["PORTAGE_REPOSITORIES"] = repos_conf
    return root, env


def run_world(name, with_544, with_pin=False, atoms=("dev-lang/perl",)):
    root, env = build_world(name, with_544, with_pin)
    assert BINARY.is_file(), f"fixed binary missing: {BINARY}"
    proc = subprocess.run(
        [str(BINARY), "emerge", "--pretend", *atoms],
        capture_output=True,
        text=True,
        env=env,
        check=False,
    )
    print("=" * 70)
    print(f"WORLD {name} (5.44.0 in tree: {with_544}) root={root} atoms={list(atoms)}")
    print(f"--- rc={proc.returncode} stdout ---")
    print(proc.stdout, end="")
    print(f"--- stderr ({len(proc.stderr)} bytes) ---")
    print(proc.stderr, end="")
    return proc


names = sys.argv[1:] or ["N0", "N1", "N2"]
PINS = {"N1pinned": ("N1", True, False, ("=dev-lang/perl-5.42.2",))}
for name in names:
    if name in PINS:
        base, with_544, with_pin, atoms = PINS[name]
        run_world(name, with_544, with_pin, atoms)
    else:
        run_world(name, with_544=(name != "N0"), with_pin=(name == "N2"))
