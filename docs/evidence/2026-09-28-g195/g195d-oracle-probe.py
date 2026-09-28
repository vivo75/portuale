#!/usr/bin/env python3
"""Oracle probe for g195d Important-1: real Atom.violated_conditionals truth table.

Runs real Portage 3.0.82.2 (sibling 3rdparty checkout, lib/ on sys.path) on the
same inputs the Rust `violated_parent_flags` unit tests use, and prints one row
per cell:

  op validity default P C | real_conditional real_enabled real_disabled gate missing_iuse arm port_now match

Columns:
  op         one of ? = != !?  (real token operator)
  validity   valid (foo in child IUSE) | invalid (foo not in child IUSE)
  default    none | plus (+) | minus (-)
  P C        parent-has / child-has, 0/1. Invalid flags are never in the
             IUSE-filtered child USE (real Package.py:728-731), so invalid
             rows only have C=0 -- the same contract the Rust fn documents.
  real_conditional  sorted union of the result's conditional.{enabled,equal,
                    not_equal,disabled} -- real's parent-side partition
                    (depgraph.py:6787-6795); the port's return value models this.
  real_enabled/real_disabled  the concrete sets feeding real's gate
                    (depgraph.py:6784: not (enabled or disabled)).
  gate        PASS (gate passes, parent arm proceeds) | FAIL.
  missing_iuse  required-but-not-in-child-IUSE flags; when non-empty real
                    SKIPS the parent arm before ever calling
                    violated_conditionals (depgraph.py:6721-6728) -- so for
                    defaultless rows the in-isolation conditional hit never
                    reaches the arm end-to-end.
  arm         parent-arm end-to-end outcome: SKIP (missing_iuse) |
              FAIL-gate (concrete violation) | conditional-set-or-empty.
  port_now    the port's CURRENT uniform P/C math (poison on
              invalid-defaultless, concrete gate on valid flags only).
  match       OK | DIVERGE (port_now vs arm).

Gate cells (G1..) append a second, unconditional token for bar to the same
atom to exercise the concrete gate.

Usage: python3 g195d-oracle-probe.py  (run from anywhere; output to stdout)
"""
import sys

sys.path.insert(
    0, "/home/vivo/repo/PORTUALE/wt-195-autounmask-parent-order/pmtest/3rdparty/portage/lib"
)

import portage  # noqa: E402

assert portage.VERSION == "3.0.82.2", portage.VERSION
from portage.dep import Atom  # noqa: E402

EAPI = "8"
CAT = "dev-libs/child"


def token(op, default):
    d = {"none": "", "plus": "(+)", "minus": "(-)"}[default]
    if op == "?":
        return f"foo{d}?"
    if op == "=":
        return f"foo{d}="
    if op == "!=":
        return f"!foo{d}="
    if op == "!?":
        return f"!foo{d}?"
    raise AssertionError(op)


def port_now(op, valid, default, p, c):
    """The port's CURRENT logic (lib.rs violated_parent_flags, pre-g195d)."""
    if not valid and default == "none":
        return "[]POISON"
    if op == "?":
        hit = p and not c
    elif op == "=":
        hit = p != c
    elif op == "!=":
        hit = p == c
    elif op == "!?":
        hit = (not p) and c
    return "[foo]" if hit else "[]"


def probe(atom_str, child_iuse, child_use, parent_use):
    a = Atom(atom_str, eapi=EAPI)
    v = a.violated_conditionals(
        set(child_use), lambda f: f in child_iuse, set(parent_use)
    )
    if v.use is None:
        cond, en, dis = [], [], []
    else:
        cond = sorted(
            set(v.use.conditional.enabled)
            | set(v.use.conditional.equal)
            | set(v.use.conditional.not_equal)
            | set(v.use.conditional.disabled)
        )
        en = sorted(v.use.enabled)
        dis = sorted(v.use.disabled)
    gate = "PASS" if not (en or dis) else "FAIL"
    required = set(a.use.required)
    missing = sorted(required - set(child_iuse))
    if missing:
        arm = "SKIP-missing-iuse"
    elif gate == "FAIL":
        arm = "FAIL-gate"
    else:
        arm = ("[" + ",".join(cond) + "]") if cond else "[]"
    return cond, en, dis, gate, missing, arm


def fmt(s):
    return "[" + ",".join(s) + "]" if s else "[]"


print(f"# real portage {portage.VERSION}, Atom.violated_conditionals oracle")
print("# op validity default P C | conditional enabled disabled gate missing_iuse arm port_now match")
n_diverge = 0
n_total = 0
for op in ("?", "=", "!=", "!?"):
    for validity in ("valid", "invalid"):
        for default in ("none", "plus", "minus"):
            for p in (0, 1):
                for c in (0, 1):
                    if validity == "invalid" and c == 1:
                        continue  # filtered USE never holds an invalid flag
                    n_total += 1
                    child_iuse = {"foo"} if validity == "valid" else {"other"}
                    child_use = {"foo"} if c and validity == "valid" else set()
                    parent_use = {"foo"} if p else set()
                    t = token(op, default)
                    cond, en, dis, gate, missing, arm = probe(
                        f"{CAT}[{t}]", child_iuse, child_use, parent_use
                    )
                    pn = port_now(op, validity == "valid", default, bool(p), bool(c))
                    arm_norm = arm.replace("SKIP-missing-iuse", "[]SKIP").replace(
                        "FAIL-gate", "[]GATE"
                    )
                    pn_norm = pn.replace("[]POISON", "[]")
                    # match: port return vs arm outcome, treating SKIP/GATE as []
                    if arm in ("SKIP-missing-iuse", "FAIL-gate"):
                        ok = pn_norm == "[]"
                    else:
                        ok = pn == arm
                    if not ok:
                        n_diverge += 1
                    print(
                        f"{op} {validity} {default} {p} {c} | {fmt(cond)} {fmt(en)} "
                        f"{fmt(dis)} {gate} {fmt(missing)} {arm} {pn} "
                        f"{'OK' if ok else 'DIVERGE'}"
                    )

# ---- gate cells: valid parent-active conditional + one unconditional token --
# foo is valid; P=1 C_foo=0. COND/ : '?' P=1 C=0 violated; '=' P=1 C=0 violated.
gate_rows = [
    # (gid, cond_tok, uncond_tok, bar_valid, bar_in_use)
    ("G1", "foo?", "bar", True, False),      # valid bar absent: real gate FAILS
    ("G2", "foo?", "-bar", True, True),      # valid -bar present: real gate FAILS
    ("G3", "foo?", "bar(-)", False, False),  # invalid bar(-): real enabled hit
    ("G4", "foo?", "-bar(+)", False, False),  # invalid -bar(+): real disabled hit
    ("G5", "foo?", "bar(+)", False, False),  # invalid bar(+): real satisfied
    ("G6", "foo?", "-bar(-)", False, False),  # invalid -bar(-): real satisfied
    ("G7", "foo?", "bar", False, False),     # invalid defaultless: real enabled hit
    ("G8", "foo?", "-bar", False, False),    # invalid defaultless: real disabled hit
    ("G9", "foo=", "bar(-)", False, False),
    ("G10", "foo=", "-bar(+)", False, False),
]
print("# gate cells: COND + UNCOND | conditional enabled disabled gate arm port_now match")
for gid, cond_tok, uncond_tok, bar_valid, bar_use in gate_rows:
    n_total += 1
    child_iuse = {"foo"} | ({"bar"} if bar_valid else {"other"})
    child_use = ({"bar"} if (bar_use and bar_valid) else set()) | set()
    parent_use = {"foo"}
    cond, en, dis, gate, missing, arm = probe(
        f"{CAT}[{cond_tok},{uncond_tok}]", child_iuse, child_use, parent_use
    )
    # port current: poison iff any conditional invalid-defaultless (foo valid
    # here: no poison); required_missing iff a defaultless unconditional flag
    # outside child IUSE; concrete gate on valid flags only.
    if not bar_valid and ("(" not in uncond_tok):
        pn = "[]POISON-req"
    elif bar_valid and (
        (uncond_tok == "bar" and "bar" not in child_use)
        or (uncond_tok == "-bar" and "bar" in child_use)
    ):
        pn = "[]GATE"
    else:
        pn = "[foo]"
    if arm in ("SKIP-missing-iuse", "FAIL-gate"):
        ok = pn.startswith("[]")
    else:
        ok = pn == arm
    if not ok:
        n_diverge += 1
    print(
        f"{gid} {cond_tok} {uncond_tok} bar_valid={int(bar_valid)} bar_use={int(bar_use)} | "
        f"{fmt(cond)} {fmt(en)} {fmt(dis)} {gate} {fmt(missing)} {arm} {pn} "
        f"{'OK' if ok else 'DIVERGE'}"
    )

print(f"# total={n_total} diverge={n_diverge}")
