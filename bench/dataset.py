"""Benchmark datasets for the versions-comparison pilot.

Two generators, both producing harness "batch" input lines (see
docs/agent-context.md, "Test/benchmark harness architecture"):

- `generate_snapshot_lines`: the default. Draws from a real, vendored
  Gentoo tree snapshot (gentoo_snapshot.json, produced by
  extract_snapshot.py) per docs/agent-context.md's "Benchmark data" decision -- real
  package names and real version strings, mostly paired within the same
  package (the realistic usage pattern: comparing candidate versions of
  one package during dependency resolution).
- `generate_synthetic_lines`: seeded-random version strings. Useful as a
  fallback if the snapshot is ever unavailable, and for stress-testing
  version-grammar edge cases the current tree snapshot doesn't happen to
  contain.
"""

import json
import random
from pathlib import Path

DEFAULT_SNAPSHOT_PATH = Path(__file__).resolve().parent / "gentoo_snapshot.json"

_SUFFIXES = ["alpha", "beta", "pre", "rc", "p"]
_INVALID_VERSIONS = ["abc", "1.0-r", "1.0_bogus", "-1.0", "1.0-beta", "v1.0"]


def load_snapshot(path: Path = DEFAULT_SNAPSHOT_PATH) -> dict[str, list[str]]:
    return json.loads(path.read_text())


def generate_snapshot_lines(
    n: int,
    seed: int = 0,
    ververify_fraction: float = 0.2,
    same_package_fraction: float = 0.8,
    snapshot_path: Path = DEFAULT_SNAPSHOT_PATH,
) -> list[str]:
    """Generates `n` batch lines from the vendored Gentoo tree snapshot: a
    mix of `vercmp` (most, `same_package_fraction` of those comparing two
    versions of the *same* real package) and `ververify` (the rest) over
    real version strings. Deterministic for a given seed.
    """
    packages = load_snapshot(snapshot_path)
    multi_version_pkgs = [cp for cp, vers in packages.items() if len(vers) >= 2]
    all_versions = [v for vers in packages.values() for v in vers]
    if not multi_version_pkgs or not all_versions:
        raise ValueError(f"snapshot at {snapshot_path} has no usable version data")

    rng = random.Random(seed)
    lines = []
    for _ in range(n):
        if rng.random() < ververify_fraction:
            lines.append(f"ververify {rng.choice(all_versions)}")
            continue
        if rng.random() < same_package_fraction:
            cp = rng.choice(multi_version_pkgs)
            v1, v2 = rng.sample(packages[cp], 2)
        else:
            v1 = rng.choice(all_versions)
            v2 = rng.choice(all_versions)
        lines.append(f"vercmp {v1} {v2}")
    return lines


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


def generate_synthetic_lines(
    n: int,
    seed: int = 0,
    invalid_fraction: float = 0.05,
    ververify_fraction: float = 0.2,
) -> list[str]:
    """Generates `n` batch lines: a mix of `vercmp` (most) and `ververify`
    (the rest) operations over randomly generated version strings,
    including a small fraction of intentionally-invalid ones. Deterministic
    for a given seed, so runs are comparable over time.
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
