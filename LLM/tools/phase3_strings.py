#!/usr/bin/env python3
"""#336 Phase 3.3: portuale's own user-visible wording says "Portage", not "real".

Each edit is an exact (file, old, new) triple; the script refuses to run if an
`old` string is missing or ambiguous. Text Portage itself prints is never in
this list (none of these strings occur in Portage 3.0.82.2's `_emerge/` or
`bin/`).
"""

import sys
from pathlib import Path

P = "crates/portuale/src/"
EDITS = [
    (P + "pretend.rs", 'is a real emerge {kind}, but is not yet', 'is a Portage emerge {kind}, but is not yet'),
    (P + "pretend.rs", "Any real\nemerge option or action", "Any Portage\nemerge option or action"),
    (P + "pretend.rs", "prints the real header only", "prints Portage's header only"),
    (P + "pretend.rs", "Portuale extensions (not real emerge options):", "Portuale extensions (not Portage emerge options):"),
    (P + "ebuild.rs", "Any other real command (clean, digest, manifest, ...)", "Any other Portage command (clean, digest, manifest, ...)"),
    (P + "ebuild.rs", "(real: sets PORTAGE_DEBUG, so bin/ebuild.sh runs set -x)", "(as Portage: sets PORTAGE_DEBUG, so bin/ebuild.sh runs set -x)"),
    (P + "ebuild.rs", "Every other real ebuild option", "Every other Portage ebuild option"),
    (P + "ebuild.rs", "not a real bin/ebuild \\", "not a Portage bin/ebuild \\"),
    (P + "main.rs", "the real emerge option surface via clap", "Portage's emerge option surface via clap"),
    ("crates/portage-repo/src/resolver_trace.rs", "not real's LIFO _create_graph order", "not Portage's LIFO _create_graph order"),
    (P + "binpkg.rs", "which real's writer never emits", "which Portage's writer never emits"),
    (P + "remote.rs", "merge kept (real _postinst_failure)", "merge kept (as Portage's _postinst_failure)"),
    ("musl/smoke_test.sh", "2 'is a real emerge option, but is not yet implemented in portuale'", "2 'is a Portage emerge option, but is not yet implemented in portuale'"),
    ("musl/smoke_test.sh", '"emerge reports a real, unimplemented option by name', '"emerge reports an unimplemented Portage option by name'),
    # test-only messages
    (P + "vdb_rw.rs", "the stamp real appended", "the stamp Portage appended"),
    (P + "vdb_rw.rs", "stays until real removes it", "stays until Portage removes it"),
    (P + "portageq.rs", "to compare with real portageq", "to compare with Portage's portageq"),
]


def main():
    texts = {}
    for path, old, new in EDITS:
        t = texts.setdefault(path, Path(path).read_text())
        n = t.count(old)
        if n != 1:
            sys.exit(f"{path}: {old!r} occurs {n} times, expected 1")
        texts[path] = t.replace(old, new)
    for path, t in texts.items():
        Path(path).write_text(t)
    print(f"{len(EDITS)} edits in {len(texts)} files")


if __name__ == "__main__":
    main()
