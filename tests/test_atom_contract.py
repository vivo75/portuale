"""Black-box contract suite for the atom-matching pilot (see
PORTING/PROMPT.md and PORTING/rust/portage-dep/src/lib.rs). Drives the
Python harness (a thin wrapper around the real portage.dep.Atom /
match_from_list) and the Rust harness identically via subprocess, and
asserts their outputs are byte-for-byte the same. Neither harness's
internals are imported directly.
"""

import subprocess

import pytest

PARSE_VALID_ATOMS = [
    "dev-libs/foo",
    "sys-apps/portage",
    "=dev-libs/foo-1.2.3",
    "=dev-libs/foo-1.2.3-r1",
    ">=dev-libs/foo-1.2.3-r1",
    ">dev-libs/foo-1.0",
    "<=dev-libs/foo-2.0",
    "<dev-libs/foo-2.0",
    "~dev-libs/foo-1.2.3",
    "!dev-libs/foo",
    "!!dev-libs/foo",
    "!>=dev-libs/foo-1.0",
    "dev-libs/foo:2",
    "dev-libs/foo:2/2.1",
    ">=dev-libs/foo-1.0:0",
    "dev-libs/foo-bar",  # package name itself containing a hyphen
    "dev-libs/foo-bar:1.0-2",  # hyphenated package name plus a dotted slot
    "=dev-libs/foo-1.0b",  # letter suffix
    "=dev-libs/foo-1.0_pre2",  # underscore suffix
    "=dev-libs/foo-1.0_alpha3-r4",
    "dev-libs/foo:=",  # slot operator, no explicit slot: any slot acceptable
    "dev-libs/foo:*",  # same, "*" form
    "dev-libs/foo:0=",  # slot operator with an explicit slot ("slot=" form)
    "dev-libs/foo:0/1=",  # slot operator with an explicit slot AND sub-slot
    ">=dev-libs/foo-1.0:0=",  # slot operator combined with a version operator
    "dev-libs/foo[bar]",  # USE dep, "must be enabled" form
    "dev-libs/foo[-bar]",  # "must be disabled" form
    "dev-libs/foo[bar?]",  # "enabled if enabled on the owner" form
    "dev-libs/foo[!bar?]",  # "disabled if disabled on the owner" form
    "dev-libs/foo[bar=]",  # "same as the owner" form
    "dev-libs/foo[!bar=]",  # "opposite of the owner" form
    "dev-libs/foo[bar(+)]",  # 4-style default: assume enabled if not in IUSE
    "dev-libs/foo[bar(-)]",  # 4-style default: assume disabled if not in IUSE
    "dev-libs/foo[bar(+)=]",  # default combined with an operator form
    "dev-libs/foo[bar,-baz,qux?]",  # multiple comma-separated requirements
    "dev-libs/foo[bar(+),bar(+)]",  # same flag repeated with a CONSISTENT default: fine
    "dev-libs/foo:0[bar]",  # USE dep combined with a plain slot
    "dev-libs/foo:0=[bar]",  # USE dep combined with a slot operator
    ">=dev-libs/foo-1.0[bar]",  # USE dep combined with a version operator
    "=dev-libs/foo-1.2.3*",  # "=*" glob version operator (PMS 8.3.1)
    "=dev-libs/foo-1.2.3-r1*",  # glob combined with an explicit revision
    "=dev-libs/foo-1*",  # single version component before the glob
]

# Not valid at all, or valid PMS atoms that use a feature outside the v1
# grammar this pilot ports (see portage-dep/src/lib.rs's module doc
# comment).
PARSE_INVALID_ATOMS = [
    "not-an-atom",
    "dev-libs/",
    "/foo",
    "dev-libs/foo-1.2.3",  # bare version with no operator: not a valid atom
    "dev-libs/foo::gentoo",  # repo constraint: out of v1 scope
    ">=dev-libs/foo-1.2.3*",  # PMS 8.3.1: "*" is illegal with any operator
    "<dev-libs/foo-1.2.3*",   # other than "=" -- not silently truncated
    "~dev-libs/foo-1.2.3*",   # or accepted under the wrong operator
    "*/foo-1",  # extended/wildcard syntax: out of v1 scope
    "dev-libs/foo-1.0@2",  # build id: out of v1 scope
    "dev-libs/foo-bar-2",  # bare atom whose package name would end in
    "dev-libs/foo-2",  # something version-like: ambiguous under PMS, rejected
    "dev-libs/foo:",  # bare trailing ":" with nothing after it: invalid
    "dev-libs/foo:0*",  # explicit slot combined with "*": invalid ("*" means
                         # "any slot", contradictory with a specific one)
    "dev-libs/foo[]",  # empty USE-dep brackets: invalid
    "dev-libs/foo[-bar=]",  # "-" prefix combined with "=" suffix: not a real
    "dev-libs/foo[-bar?]",  # operator, even though the per-token regex alone
                             # would syntactically match it
    "dev-libs/foo[bar(+),bar(-)]",  # same flag, conflicting 4-style defaults
    "dev-libs/foo[bar(+),-bar]",  # same flag, default vs. no-default conflict
    "dev-libs/foo[bar][baz]",  # two bracket groups: only one is ever allowed
    "dev-libs/foo[ bar]",  # whitespace inside the brackets: invalid
]


def _run(cmd: list[str], *args: str) -> str:
    result = subprocess.run(
        [*cmd, *args], capture_output=True, text=True, check=True
    )
    return result.stdout.strip()


@pytest.mark.parametrize("atom", PARSE_VALID_ATOMS)
def test_parse_valid_atom_matches_between_implementations(
    atom, atom_harness_python, atom_harness_rust
):
    python_result = _run(atom_harness_python, "parse", atom)
    rust_result = _run([str(atom_harness_rust)], "parse", atom)
    assert rust_result == python_result
    assert python_result != "INVALID"


@pytest.mark.parametrize("atom", PARSE_INVALID_ATOMS)
def test_parse_invalid_atom_matches_between_implementations(
    atom, atom_harness_python, atom_harness_rust
):
    python_result = _run(atom_harness_python, "parse", atom)
    rust_result = _run([str(atom_harness_rust)], "parse", atom)
    assert python_result == "INVALID"
    assert rust_result == python_result


CANDIDATES = [
    "dev-libs/foo-1.0",
    "dev-libs/foo-1.2.3",
    "dev-libs/foo-1.2.3-r1",
    "dev-libs/foo-1.2.3-r1:2/2.1",
    "dev-libs/foo-2.0:3",
    "dev-libs/foo-2.0:3/1.0",
    "dev-libs/foo-0.9:2",
    "dev-libs/bar-1.0",
    "dev-libs/foobar-1.0",  # same prefix, different package: must not match
]

MATCH_CASES = [
    # (atom, candidates)
    ("dev-libs/foo", CANDIDATES),
    ("=dev-libs/foo-1.2.3", CANDIDATES),
    ("=dev-libs/foo-1.2.3-r1", CANDIDATES),
    ("=dev-libs/foo-1.2.3-r0", CANDIDATES),  # r0 must equal the un-revisioned "1.2.3"
    ("~dev-libs/foo-1.2.3", CANDIDATES),
    (">=dev-libs/foo-1.2.3", CANDIDATES),
    (">dev-libs/foo-1.2.3", CANDIDATES),
    ("<=dev-libs/foo-1.2.3", CANDIDATES),
    ("<dev-libs/foo-1.2.3", CANDIDATES),
    ("dev-libs/foo:2", CANDIDATES),
    ("dev-libs/foo:2/2.1", CANDIDATES),
    ("dev-libs/foo:3/1.0", CANDIDATES),
    ("dev-libs/foo:9", CANDIDATES),  # slot nothing matches
    ("!dev-libs/foo", CANDIDATES),  # blocker: matches same as non-blocker
    (">=dev-libs/nonexistent-1.0", CANDIDATES),  # no matches at all
    # a candidate with no slot info must still pass a slotted atom, per the
    # real match_from_list's "unknown slot isn't filtered" behavior.
    ("dev-libs/foo:2", ["dev-libs/foo-1.0", "dev-libs/foo-1.0:2"]),
    # slot operators, no explicit slot: match every candidate regardless
    # of its own slot -- matches_slot's early "atom.slot is None" return,
    # same path a plain unslotted atom already takes.
    ("dev-libs/foo:=", CANDIDATES),
    ("dev-libs/foo:*", CANDIDATES),
    # slot operator WITH an explicit slot ("slot=" form): filters by that
    # slot exactly like a plain ":slot" atom would -- the operator itself
    # adds no additional matching constraint.
    ("dev-libs/foo:2=", CANDIDATES),
    ("dev-libs/foo:3=", CANDIDATES),
    ("dev-libs/foo:9=", CANDIDATES),  # explicit slot nothing matches
    # USE deps: parsed but never enforced by matching (see portage-dep's
    # module doc comment) -- verified this isn't an invented divergence:
    # real match_from_list, given these same plain-string candidates
    # (no .use/.iuse attributes), skips its own USE-dep filtering
    # entirely too, so "[bar]" and "[-bar]" must match the identical set.
    ("dev-libs/foo[bar]", CANDIDATES),
    ("dev-libs/foo[-bar]", CANDIDATES),
    ("dev-libs/foo:2[bar?]", CANDIDATES),  # combined with a slot restriction
]

# "=*" glob version operator (PMS 8.3.1): dedicated candidates exercising
# the component-boundary rule (bug 560466: "1*" must not match "10", even
# though "10" literally starts with "1") and the "leading zeros" special
# case real match_from_list's own "=*" branch applies before comparing
# (see portage-dep's normalize_leading_zeros doc comment) -- neither is
# covered by the plain CANDIDATES list above.
GLOB_CANDIDATES = [
    "dev-libs/foo-1.2",
    "dev-libs/foo-1.20",  # digit immediately after the "1.2" prefix: NOT a
                           # real boundary, must not match "=...-1.2*"
    "dev-libs/foo-1.2.3",  # "." after the prefix: a real boundary
    "dev-libs/foo-1.2-r1",  # "-" after the prefix: a real boundary too
    "dev-libs/foo-1.3",  # doesn't share the "1.2" prefix at all
    "dev-libs/foo-1",
    "dev-libs/foo-10",  # bug 560466's own example: "1*" must not match this
    "dev-libs/foo-1b",  # letter suffix right after "1": digit/non-digit
                         # adjacency, a real boundary
]

MATCH_CASES += [
    ("=dev-libs/foo-1.2*", GLOB_CANDIDATES),
    ("=dev-libs/foo-1*", GLOB_CANDIDATES),
    # leading zeros in the atom's own version are normalized before the
    # prefix comparison, so "01*" behaves identically to "1*" above.
    ("=dev-libs/foo-01*", GLOB_CANDIDATES),
    # a glob combined with an explicit revision: the boundary rule applies
    # to the revision digits too, not just the plain version.
    ("=dev-libs/foo-1.2-r1*", ["dev-libs/foo-1.2-r1", "dev-libs/foo-1.2-r10"]),
    # redundant leading zeros in a CANDIDATE's own version are normalized
    # too, not just the atom's -- "00.5" is numerically identical to "0.5".
    ("=dev-libs/foo-0.5*", ["dev-libs/foo-0.5", "dev-libs/foo-0.50", "dev-libs/foo-00.5"]),
]


@pytest.mark.parametrize("atom,candidates", MATCH_CASES)
def test_match_matches_between_implementations(
    atom, candidates, atom_harness_python, atom_harness_rust
):
    python_result = _run(atom_harness_python, "match", atom, *candidates)
    rust_result = _run([str(atom_harness_rust)], "match", atom, *candidates)
    assert rust_result == python_result


def test_match_of_invalid_atom_is_invalid_in_both(
    atom_harness_python, atom_harness_rust
):
    python_result = _run(
        atom_harness_python, "match", "dev-libs/foo::gentoo", *CANDIDATES
    )
    rust_result = _run(
        [str(atom_harness_rust)], "match", "dev-libs/foo::gentoo", *CANDIDATES
    )
    assert python_result == "INVALID"
    assert rust_result == python_result


def test_batch_mode_output_matches(atom_harness_python, atom_harness_rust):
    """Exercises benchmark-shaped batch mode: many operations fed to a
    single process invocation via stdin."""
    # The batch protocol is a single whitespace-delimited line per
    # operation ("parse <atom>"), so an atom containing a literal space
    # (e.g. the "[ bar]" invalid-USE-dep case) can't be represented in it
    # at all -- unrelated to what this test is actually exercising (many
    # operations in one process), so it's excluded here; it's still
    # covered by the single-shot parity test above.
    all_atoms = [a for a in PARSE_VALID_ATOMS + PARSE_INVALID_ATOMS if " " not in a]
    lines = [f"parse {a}" for a in all_atoms]
    lines += [f"match {atom} {' '.join(candidates)}" for atom, candidates in MATCH_CASES]
    stdin_data = "\n".join(lines) + "\n"

    python_out = subprocess.run(
        [*atom_harness_python, "batch"],
        input=stdin_data,
        capture_output=True,
        text=True,
        check=True,
    ).stdout

    rust_out = subprocess.run(
        [str(atom_harness_rust), "batch"],
        input=stdin_data,
        capture_output=True,
        text=True,
        check=True,
    ).stdout

    assert rust_out == python_out
    assert len(rust_out.splitlines()) == len(lines)
