#!/usr/bin/env python3
"""Host-side self-test for diff.py's L2 additions.

Builds two synthetic normalised snapshot prefixes (the format
`normalize.py` emits) and pins:

  * default (L1) mode: size/sha differences are hard SIZE/CONTENT;
  * `--tolerate-payload`: same size/sha differences become non-fatal
    PAYLOAD, while a missing path stays a hard MISSING;
  * the allowlist `layer` filter: an `l2` entry applies only with
    `--layer l2`.

Run: python3 TEST/compare/test-diff-tolerance.py   (exit 0 = all good)
"""
from __future__ import annotations

import importlib.util
import io
import sys
import tempfile
from contextlib import redirect_stdout
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("compare_diff", HERE / "diff.py")
assert spec and spec.loader
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

FAILED = 0


def check(label: str, cond: bool, detail: str = "") -> None:
    global FAILED
    if cond:
        print(f"ok   - {label}")
    else:
        FAILED += 1
        print(f"FAIL - {label}{(': ' + detail) if detail else ''}")


def write_side(root: Path, name: str, foo_sha: str, foo_md5: str, extra: bool) -> Path:
    prefix = root / name
    lines = [f"/usr/bin/foo\tf\t0755\t0\t0\t10\t{foo_sha}\t-\t-"]
    mtimes = ["/usr/bin/foo\t1000"]
    if extra:
        lines.append("/usr/share/extra\tf\t0644\t0\t0\t3\tshaX\t-\t-")
        mtimes.append("/usr/share/extra\t1000")
    (root / f"{name}.files.norm.tsv").write_text("\n".join(lines) + "\n")
    (root / f"{name}.mtimes.tsv").write_text("\n".join(mtimes) + "\n")
    vdb = prefix.parent / (prefix.name + ".vdb") / "pkg" / "cat" / "pf"
    vdb.mkdir(parents=True)
    (vdb / "CONTENTS").write_text(
        f"obj /usr/bin/foo {foo_md5} 0\n" "dir /usr\n"
    )
    return prefix


def run(args: list[str]) -> tuple[int, str]:
    buf = io.StringIO()
    with redirect_stdout(buf):
        rc = mod.main(args)
    return rc, buf.getvalue()


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="difftol.") as td:
        root = Path(td)
        a = write_side(root, "a", "a" * 64, "d" * 32, extra=False)
        b = write_side(root, "b", "b" * 64, "e" * 32, extra=False)

        rc, out = run([str(a), str(b)])
        check("default: sha/md5 diff is hard", rc == 1 and "[CONTENT]" in out and "[CONTENTS]" in out, out)

        rc, out = run(["--tolerate-payload", str(a), str(b)])
        check("tolerate: payload diff is non-fatal", rc == 0 and "[PAYLOAD]" in out, out)
        check("tolerate: payload line counted", "payload diffs     : 2" in out, out)

        # a missing path stays hard in tolerated mode
        c = write_side(root, "c", "a" * 64, "d" * 32, extra=True)
        rc, out = run(["--tolerate-payload", str(a), str(c)])
        check("tolerate: missing path is still hard", rc == 1 and "[MISSING]" in out, out)

        # layer filter: l2 entries apply only under --layer l2
        allow = root / "known.yaml"
        allow.write_text(
            "- id: l2-only\n"
            "  layer: l2\n"
            "  category: MISSING\n"
            "  path_glob: /usr/share/extra\n"
            "  reason: test entry\n"
        )
        rc, out = run(["--layer", "l1", "--tolerate-payload", str(a), str(c), str(allow)])
        check("layer l1 ignores an l2 entry", rc == 1, out)
        rc, out = run(["--layer", "l2", "--tolerate-payload", str(a), str(c), str(allow)])
        check("layer l2 applies an l2 entry", rc == 0 and "l2-only" in out, out)

    print(f"\ntest-diff-tolerance: {'OK' if FAILED == 0 else f'{FAILED} failure(s)'}")
    return 1 if FAILED else 0


if __name__ == "__main__":
    raise SystemExit(main())
