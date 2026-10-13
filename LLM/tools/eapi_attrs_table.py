#!/usr/bin/env python3
"""Generate the S1 oracle TSV from the installed Portage (do not hand-copy).

Re-reads ``portage.eapi._get_eapi_attrs`` live (3.0.82.2) and writes
``crates/portage-dep/src/testdata/eapi-attrs-3.0.82.2.tsv`` with columns
``eapi \\t field \\t verdict \\t value``.

Portage is still the oracle at test-generation time (AGENTS.md rule 4):
every expected value below comes out of ``portage.eapi`` live, never out
of a transcript. The field list itself is read off the live
``_eapi_attrs`` namedtuple so the battery cannot drift from the oracle.

Rows: one ``OK`` row per (eapi, field) for eapi in ``0``..``9``,
``9-pre1`` (the testing spelling, parsed via ``partition("-")[0]``),
``None`` (Python None, the permissive set) and ``99`` (unsupported, same
permissive set); one ``ERR`` row per supported-but-unparseable spelling
(``Eapi.__init__`` raises ``ValueError`` there, eapi.py:224-229), with
``str(exc)`` as the detail — the Rust ``EapiValueError`` Display must
render exactly that text.
"""

import os

import portage
from portage.eapi import Eapi, _get_eapi_attrs

EXPECTED_PORTAGE_VERSION = "3.0.82.2"

OUT = os.path.join(
    os.path.dirname(os.path.abspath(__file__)),
    "..",
    "..",
    "crates",
    "portage-dep",
    "src",
    "testdata",
    "eapi-attrs-3.0.82.2.tsv",
)

# Digits plus the testing spelling; deprecated/whitespace spellings are
# the ERR rows below (SUPPORTED_EAPIS are digits, eapi.rs).
EAPIS = [str(n) for n in range(10)] + ["9-pre1"]
ERR_EAPIS = ["3_pre1", " 8"]


def main():
    assert portage.VERSION == EXPECTED_PORTAGE_VERSION, portage.VERSION
    fields = list(_get_eapi_attrs(None)._fields)
    assert len(fields) == 38, fields
    n_ok = 0
    with open(OUT, "w") as out:
        out.write("eapi\tfield\tverdict\tvalue\n")
        for label, arg in [("None", None)] + [(e, e) for e in EAPIS] + [("99", "99")]:
            attrs = _get_eapi_attrs(arg)
            for f in fields:
                out.write("%s\t%s\tOK\t%s\n" % (label, f, bool(getattr(attrs, f))))
                n_ok += 1
        for bad in ERR_EAPIS:
            try:
                Eapi(bad)
            except ValueError as e:
                out.write("%s\t-\tERR\t%s\n" % (bad, str(e)))
            else:  # pragma: no cover - oracle drift alarm
                raise SystemExit("ORACLE DRIFT: Eapi(%r) did not raise" % bad)
    print(
        "wrote %s (%d OK rows over %d eapis, %d ERR rows, %d fields)"
        % (OUT, n_ok, len(EAPIS) + 2, len(ERR_EAPIS), len(fields))
    )


if __name__ == "__main__":
    main()
