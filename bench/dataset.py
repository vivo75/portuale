"""Deterministic synthetic dataset for the versions-comparison benchmark.

PORTING/PROMPT.md calls for benchmark data drawn from "a real, vendored
Gentoo tree snapshot" rather than synthetic stress data -- that's not wired
up yet (no snapshot is vendored into this repo). This generator is the
stand-in until that follow-up lands; see PORTING/README.md.
"""

import random

_SUFFIXES = ["alpha", "beta", "pre", "rc", "p"]
_INVALID_VERSIONS = ["abc", "1.0-r", "1.0_bogus", "-1.0", "1.0-beta", "v1.0"]


def _random_valid_version(rng: random.Random) -> str:
    parts = [rng.randint(0, 20)]
    for _ in range(rng.randint(0, 3)):
        parts.append(rng.randint(0, 999))
    ver = ".".join(str(p) for p in parts)
    if rng.random() < 0.15:
        ver += rng.choice("abcdefghijklmnopqrstuvwxyz")
    for _ in range(rng.randint(0, 2)):
        ver += f"_{rng.choice(_SUFFIXES)}"
        if rng.random() < 0.7:
            ver += str(rng.randint(0, 20))
    if rng.random() < 0.3:
        ver += f"-r{rng.randint(0, 10)}"
    return ver


def _random_version(rng: random.Random, invalid_fraction: float) -> str:
    if rng.random() < invalid_fraction:
        return rng.choice(_INVALID_VERSIONS)
    return _random_valid_version(rng)


def generate_batch_lines(
    n: int,
    seed: int = 0,
    invalid_fraction: float = 0.05,
    ververify_fraction: float = 0.2,
) -> list[str]:
    """Generates `n` harness "batch" input lines: a mix of `vercmp` (most)
    and `ververify` (the rest) operations over randomly generated version
    strings, including a small fraction of intentionally-invalid ones.
    Deterministic for a given seed, so runs are comparable over time.
    """
    rng = random.Random(seed)
    lines = []
    for _ in range(n):
        if rng.random() < ververify_fraction:
            v = _random_version(rng, invalid_fraction)
            lines.append(f"ververify {v}")
        else:
            v1 = _random_version(rng, invalid_fraction)
            v2 = _random_version(rng, invalid_fraction)
            lines.append(f"vercmp {v1} {v2}")
    return lines
