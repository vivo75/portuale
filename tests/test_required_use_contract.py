"""Black-box contract suite for the REQUIRED_USE portuale (see
docs/agent-context.md and rust/portage-required-use/src/lib.rs).
Drives the Python harness (a thin wrapper around the real
portage.dep.check_required_use, pinned to eapi="8") and the Rust harness
identically via subprocess, and asserts their outputs are byte-for-byte
the same.
"""

import subprocess

import pytest

# (enabled, iuse, tokens)
CHECK_CASES = [
    ("foo", "foo", ["foo"]),  # plain flag, enabled
    ("-", "foo", ["foo"]),  # plain flag, disabled
    ("-", "foo", ["!foo"]),  # negated flag, disabled -- satisfied
    ("foo", "foo", ["!foo"]),  # negated flag, enabled -- unsatisfied
    ("-", "a,b", ["a", "b"]),  # top-level implicit all-of, both unsatisfied
    ("a,b", "a,b", ["a", "b"]),  # top-level implicit all-of, both satisfied
    ("a", "a,b", ["a", "b"]),  # top-level implicit all-of, one unsatisfied
    ("-", "a,b", ["(", "a", "b", ")"]),  # bare all-of group
    ("a,b", "a,b", ["(", "a", "b", ")"]),
    ("-", "a,b", ["||", "(", "a", "b", ")"]),  # any-of, none enabled
    ("a", "a,b", ["||", "(", "a", "b", ")"]),  # any-of, one enabled
    ("a,b", "a,b", ["||", "(", "a", "b", ")"]),  # any-of, both enabled
    ("-", "a,b", ["^^", "(", "a", "b", ")"]),  # exactly-one-of, zero
    ("a", "a,b", ["^^", "(", "a", "b", ")"]),  # exactly-one-of, one
    ("a,b", "a,b", ["^^", "(", "a", "b", ")"]),  # exactly-one-of, two
    ("-", "a,b", ["??", "(", "a", "b", ")"]),  # at-most-one-of, zero
    ("a", "a,b", ["??", "(", "a", "b", ")"]),  # at-most-one-of, one
    ("a,b", "a,b", ["??", "(", "a", "b", ")"]),  # at-most-one-of, two
    ("-", "foo,bar", ["foo?", "(", "bar", ")"]),  # conditional, flag inactive
    ("foo", "foo,bar", ["foo?", "(", "bar", ")"]),  # conditional active, unsatisfied
    ("foo,bar", "foo,bar", ["foo?", "(", "bar", ")"]),  # conditional active, satisfied
    ("-", "foo,bar", ["!foo?", "(", "bar", ")"]),  # negated conditional active, unsatisfied
    ("foo", "foo,bar", ["!foo?", "(", "bar", ")"]),  # negated conditional inactive
    (
        "foo,a",
        "foo,a,b,c",
        ["foo?", "(", "||", "(", "a", "^^", "(", "b", "c", ")", ")", ")"],
    ),  # nested groups
    ("-", "-", ["||", "(", ")"]),  # empty any-of: EAPI 7+ semantics, unsatisfied
    ("-", "-", ["??", "(", ")"]),  # empty at-most-one-of: vacuously satisfied either way
    (
        "python_targets_python3_11",
        "python_targets_python3_11,python_targets_python3_12",
        [
            "^^",
            "(",
            "python_targets_python3_11",
            "python_targets_python3_12",
            ")",
        ],
    ),  # real-world-shaped: PYTHON_TARGETS-style exactly-one-of
]

_LIBSDL2_IUSE = (
    "X,alsa,dbus,haptic,joystick,opengl,sound,udev,video,wayland,"
    "fcitx,gles1,gles2,ibus,jack,kms,nas,pulseaudio,sndio,test,"
    "static-libs,vulkan,xscreensaver"
)
_LIBSDL2_RU = (
    "alsa? ( sound ) fcitx? ( dbus ) gles1? ( video ) gles2? ( video ) "
    "haptic? ( joystick ) ibus? ( dbus ) jack? ( sound ) "
    "kms? ( || ( gles1 gles2 opengl ) ) nas? ( sound ) opengl? ( video ) "
    "pulseaudio? ( sound ) sndio? ( sound ) test? ( static-libs ) "
    "vulkan? ( video ) wayland? ( gles2 ) xscreensaver? ( X )"
).split()

# "reduce" cases: the harness returns "-" when satisfied, else the
# human-readable minimal unsatisfied sub-expression (real
# check_required_use(...).tounicode() -> human_readable_required_use).
REDUCE_CASES = [
    ("a", "a,b", ["||", "(", "a", "b", ")"]),  # satisfied -> "-"
    ("-", "a,b", ["a", "b"]),  # top-level all-of, both unsatisfied
    ("a", "a,b,c", ["a", "b", "c"]),  # all-of, one already satisfied -> dropped
    ("-", "a,b", ["||", "(", "a", "b", ")"]),  # any-of, none
    ("-", "a,b", ["^^", "(", "a", "b", ")"]),  # exactly-one-of, zero
    ("a,b", "a,b", ["^^", "(", "a", "b", ")"]),  # exactly-one-of, two
    ("foo", "foo,bar,baz", ["foo?", "(", "bar", ")", "baz"]),  # cond + trailing leaf
    ("-", "foo,bar", ["foo?", "(", "bar", ")"]),  # cond inactive -> "-"
    (
        "x",
        "x,y,z",
        ["x?", "(", "^^", "(", "y", "z", ")", ")", "z"],
    ),  # nested: only x?( ^^ ) unsatisfied, trailing z too
    (
        "X,alsa,dbus,haptic,joystick,opengl,sound,udev,video,wayland",
        _LIBSDL2_IUSE,
        _LIBSDL2_RU,
    ),  # the media-libs/libsdl2 / wine-vanilla shape -> "wayland? ( gles2 )"
]

CHECK_ERROR_CASES = [
    ("-", "-", ["foo"]),  # flag not declared in IUSE at all
    ("-", "a", ["(", "a"]),  # unclosed paren
    ("-", "a", ["a", ")"]),  # unmatched close
    ("-", "a", ["||"]),  # dangling operator, no bracket
    ("-", "a", ["||", "a"]),  # operator not followed by "("
    ("-", "a", ["foo?"]),  # dangling conditional, no bracket
    ("-", "a", ["foo?", "a"]),  # conditional not followed by "("
    ("-", "a", ["^^"]),  # dangling ^^, no bracket
    ("-", "a", ["??"]),  # dangling ??, no bracket
]


def _run(cmd: list[str], *args: str) -> str:
    result = subprocess.run([*cmd, *args], capture_output=True, text=True, check=True)
    return result.stdout.strip()


@pytest.mark.parametrize("enabled,iuse,tokens", CHECK_CASES)
def test_check_matches_between_implementations(
    enabled, iuse, tokens, required_use_harness_python, required_use_harness_rust
):
    python_result = _run(required_use_harness_python, "check", enabled, iuse, *tokens)
    rust_result = _run([str(required_use_harness_rust)], "check", enabled, iuse, *tokens)
    assert rust_result == python_result
    assert python_result != "ERROR"


@pytest.mark.parametrize("enabled,iuse,tokens", CHECK_ERROR_CASES)
def test_check_error_matches_between_implementations(
    enabled, iuse, tokens, required_use_harness_python, required_use_harness_rust
):
    python_result = _run(required_use_harness_python, "check", enabled, iuse, *tokens)
    rust_result = _run([str(required_use_harness_rust)], "check", enabled, iuse, *tokens)
    assert python_result == "ERROR"
    assert rust_result == python_result


@pytest.mark.parametrize("enabled,iuse,tokens", REDUCE_CASES)
def test_reduce_matches_between_implementations(
    enabled, iuse, tokens, required_use_harness_python, required_use_harness_rust
):
    python_result = _run(required_use_harness_python, "reduce", enabled, iuse, *tokens)
    rust_result = _run([str(required_use_harness_rust)], "reduce", enabled, iuse, *tokens)
    assert rust_result == python_result
    assert python_result != "ERROR"


def test_batch_mode_output_matches(required_use_harness_python, required_use_harness_rust):
    """Exercises benchmark-shaped batch mode: many operations fed to a
    single process invocation via stdin."""
    lines = [
        f"check {enabled} {iuse} {' '.join(tokens)}".rstrip()
        for enabled, iuse, tokens in CHECK_CASES + CHECK_ERROR_CASES
    ]
    stdin_data = "\n".join(lines) + "\n"

    python_out = subprocess.run(
        [*required_use_harness_python, "batch"],
        input=stdin_data,
        capture_output=True,
        text=True,
        check=True,
    ).stdout

    rust_out = subprocess.run(
        [str(required_use_harness_rust), "batch"],
        input=stdin_data,
        capture_output=True,
        text=True,
        check=True,
    ).stdout

    assert rust_out == python_out
    assert len(rust_out.splitlines()) == len(lines)
