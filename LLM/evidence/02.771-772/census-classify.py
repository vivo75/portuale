#!/usr/bin/env python3
"""Classify the CENSUS lines of one fast plan by the plan's 0.3 table.

usage: census-classify.py PLAN.json CENSUS.stderr OUT.txt [LITERAL.txt]

Reads the `CENSUS<TAB>...` lines the census binary printed on stderr (see
`census_dump` in the census patch), appends the class to each, writes them to
OUT.txt and prints one summary line `class=count ...` followed by the late U
and C1b edges.  Rules, in order (runbook 02.771-772 Task 3 Step 5):
N (target is not a row), C1a, C1b, C2, C5, C3, U.  C3 compares the atom with
the literal checker's own reduction of the same row's build classes.
"""
import json
import re
import sys
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent.parent.parent.parent / "pmtest" / "pytests-contract-suite"))
import health_checker as hc  # noqa: E402

FIELDS = ["from", "to", "atom", "vars", "row", "to_rows", "is_row", "inst_ver", "inst_slot",
          "plan_ver", "plan_sub", "met", "other", "scc", "slot_op", "in_depend", "text_has",
          "or_displaced"]


def strip_use(atom: str) -> str:
    return re.sub(r"\[.*\]$", "", atom)


def checker_atoms(entry: dict) -> set:
    """The plain build atoms the literal checker's reduction of this row keeps."""
    pkg = hc._package(entry["category"], entry["package"], entry["version"], entry.get("slot"), entry)
    out = set()
    for cls in hc.BUILD_CLASSES + ("IDEPEND",):
        for a in hc._plain_atoms(hc._reduced(pkg, cls)):
            out.add(strip_use(a.text))
    return out


def installed_sub(inst_slot: str):
    if inst_slot in ("", "-"):
        return None
    return inst_slot.split("/", 1)[1] if "/" in inst_slot else inst_slot


def classify(e: dict, kept_atoms) -> str:
    if e["is_row"] == "0":
        return "N"
    met, slot_op, in_depend = e["met"] == "1", e["slot_op"] == "1", e["in_depend"] == "1"
    inst, plan = installed_sub(e["inst_slot"]), (None if e["plan_sub"] == "-" else e["plan_sub"])
    moved = inst is None or plan is None or inst != plan
    if met and not (slot_op and in_depend and moved):
        return "C1a"
    if met:
        return "C1b"
    if e["other"] == "1":
        return "C2"
    if e["or_displaced"] == "1":
        return "C5"
    if strip_use(e["atom"]) not in kept_atoms(int(e["row"])):
        return "C3"
    return "U"


def main(argv: list) -> int:
    plan_path, census_path, out_path = argv[1:4]
    doc = json.load(open(plan_path))
    by_order = {e["merge_order"]: e for e in doc["entries"]}
    cache: dict = {}

    def kept_atoms(row: int) -> set:
        if row not in cache:
            entry = by_order.get(row)
            cache[row] = checker_atoms(entry) if entry else set()
        return cache[row]

    counts: Counter = Counter()
    needs, rows = [], []
    for line in open(census_path, encoding="utf-8"):
        if line.startswith("CENSUS-NOTARGET\t"):
            counts["N0"] += 1  # atom aimed at no slot (unmatched or masked): left to the checker
            rows.append(line.rstrip("\n").replace("CENSUS-NOTARGET", "#NOTARGET", 1))
            continue
        if not line.startswith("CENSUS\t"):
            continue
        cols = line.rstrip("\n").split("\t")[1:]
        e = dict(zip(FIELDS, cols))
        cls = classify(e, kept_atoms)
        counts[cls] += 1
        rows.append("\t".join(cols + [cls]))
        if cls in ("U", "C1b", "C3"):
            needs.append(f"{cls}\t{e['from']} (row {e['row']}) -> {e['to']} (rows {e['to_rows']}) "
                         f"atom {e['atom']} [{e['vars']}] installed {e['inst_ver']} SLOT {e['inst_slot']} "
                         f"planned {e['plan_ver']}/{e['plan_sub']} text_has_atom={e['text_has']}")
    with open(out_path, "w", encoding="utf-8") as out:
        out.write("# " + "\t".join(FIELDS + ["class"]) + "\n")
        out.write("\n".join(rows) + ("\n" if rows else ""))
    print(" ".join(f"{k}={counts.get(k, 0)}" for k in ("N", "N0", "C1a", "C1b", "C2", "C3", "C4", "C5", "U")),
          f"total={sum(counts.values())}")
    for n in needs:
        print(n)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
