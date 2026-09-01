"""Black-box contract suite for the use_reduce(flat=True) pilot (see
PROMPT.md and rust/use-reduce-harness/src/use_reduce.rs).
Drives the Python harness (a thin wrapper around the real
portage.dep.use_reduce) and the Rust harness identically via subprocess,
and asserts their outputs are byte-for-byte the same.
"""

import subprocess

import pytest

# (mode, uselist_arg, tokens)
REDUCE_CASES = [
    ("normal", "-", []),  # empty depstr
    ("normal", "-", ["dev-libs/a", "dev-libs/b"]),  # no conditionals at all
    ("normal", "bar", ["dev-libs/foo", "bar?", "(", "dev-libs/baz", ")", "!bar?", "(", "dev-libs/qux", ")"]),
    ("normal", "-", ["dev-libs/foo", "bar?", "(", "dev-libs/baz", ")", "!bar?", "(", "dev-libs/qux", ")"]),
    ("normal", "-", ["||", "(", "dev-libs/a", "dev-libs/b", ")"]),
    ("normal", "foo", ["foo?", "(", "||", "(", "dev-libs/a", "dev-libs/b", ")", ")"]),
    ("normal", "-", ["foo?", "(", "||", "(", "dev-libs/a", "dev-libs/b", ")", ")"]),
    ("normal", "foo,bar", ["dev-libs/a", "foo?", "(", "dev-libs/b", "bar?", "(", "dev-libs/c", ")", ")"]),
    ("normal", "foo", ["dev-libs/a", "foo?", "(", "dev-libs/b", "bar?", "(", "dev-libs/c", ")", ")"]),
    ("normal", "foo", ["foo?", "(", "dev-libs/a", ")", "foo?", "(", "dev-libs/b", ")"]),  # dup conditional
    ("normal", "-", ["!foo?", "(", "dev-libs/a", ")"]),
    ("normal", "foo", ["!foo?", "(", "dev-libs/a", ")"]),
    ("matchall", "-", ["foo?", "(", "dev-libs/a", ")", "!bar?", "(", "dev-libs/b", ")"]),
    ("matchnone", "-", ["foo?", "(", "dev-libs/a", ")", "!bar?", "(", "dev-libs/b", ")"]),
    ("matchall", "-", ["!foo?", "(", "dev-libs/a", ")"]),  # matchall: negated still active
    # nested parens with no conditional at all: redundant brackets collapse
    ("normal", "-", ["(", "(", "dev-libs/a", ")", ")"]),
    ("normal", "-", ["dev-libs/a", "(", "dev-libs/b", "(", "dev-libs/c", ")", ")"]),
    # a leading digit is a *valid* USE flag name start (surprising but
    # confirmed against the real useflag_re: ^[A-Za-z0-9][A-Za-z0-9+_@-]*$)
    ("normal", "1notaflag", ["1notaflag?", "(", "dev-libs/a", ")"]),
]

REDUCE_ERROR_CASES = [
    ("normal", "-", ["dev-libs/a", "("]),  # unclosed paren
    ("normal", "-", [")"]),  # unmatched close
    ("normal", "-", ["foo?"]),  # dangling conditional, no bracket
    ("normal", "-", ["foo?", "dev-libs/a"]),  # conditional not followed by "("
    ("normal", "-", ["(", ")"]),  # literal empty parens
    ("normal", "-", ["||", "(", ")"]),  # || with literal empty parens
    ("normal", "-", ["foo?", "(", ")"]),  # conditional with literal empty parens
    ("normal", "-", ["||"]),  # dangling ||, no bracket
    ("normal", "-", ["||", "dev-libs/a"]),  # || not followed by "("
    ("normal", "-", ["dev-libs/a", "->", "b.tar.gz"]),  # SRC_URI arrow: out of v1 scope
    ("normal", "-", ["_badflag?", "(", "dev-libs/a", ")"]),  # invalid flag name in conditional (leading "_")
]


def _run(cmd: list[str], *args: str) -> str:
    result = subprocess.run(
        [*cmd, *args], capture_output=True, text=True, check=True
    )
    return result.stdout.strip()


@pytest.mark.parametrize("mode,uselist,tokens", REDUCE_CASES)
def test_reduce_matches_between_implementations(
    mode, uselist, tokens, use_reduce_harness_python, use_reduce_harness_rust
):
    python_result = _run(use_reduce_harness_python, "reduce", mode, uselist, *tokens)
    rust_result = _run([str(use_reduce_harness_rust)], "reduce", mode, uselist, *tokens)
    assert rust_result == python_result
    assert python_result != "ERROR"


@pytest.mark.parametrize("mode,uselist,tokens", REDUCE_ERROR_CASES)
def test_reduce_error_matches_between_implementations(
    mode, uselist, tokens, use_reduce_harness_python, use_reduce_harness_rust
):
    python_result = _run(use_reduce_harness_python, "reduce", mode, uselist, *tokens)
    rust_result = _run([str(use_reduce_harness_rust)], "reduce", mode, uselist, *tokens)
    assert python_result == "ERROR"
    assert rust_result == python_result


def test_batch_mode_output_matches(use_reduce_harness_python, use_reduce_harness_rust):
    """Exercises benchmark-shaped batch mode: many operations fed to a
    single process invocation via stdin."""
    lines = [
        f"reduce {mode} {uselist} {' '.join(tokens)}".rstrip()
        for mode, uselist, tokens in REDUCE_CASES + REDUCE_ERROR_CASES
    ]
    stdin_data = "\n".join(lines) + "\n"

    python_out = subprocess.run(
        [*use_reduce_harness_python, "batch"],
        input=stdin_data,
        capture_output=True,
        text=True,
        check=True,
    ).stdout

    rust_out = subprocess.run(
        [str(use_reduce_harness_rust), "batch"],
        input=stdin_data,
        capture_output=True,
        text=True,
        check=True,
    ).stdout

    assert rust_out == python_out
    assert len(rust_out.splitlines()) == len(lines)
