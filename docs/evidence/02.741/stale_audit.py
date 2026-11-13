"""S0 stale-binding audit (plan 02.741 S0.3): throwaway, PMS-grounded.

For every installed package in /var/db/pkg, parse every atom of the five
*DEPEND variables with health_checker.parse_dep_string (written from the
PMS) and list the `:=` (slot-operator equals) atoms whose recorded
sub-slot differs from the installed provider's SLOT sub-slot.
Charter property 1 is the spec: a consumer whose recorded `:=` binding
names a gone sub-slot has an unmatched dependency.
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, "/home/vivo/repo/PORTUALE/pmtest/pytests-contract-suite")
import health_checker as hc

VDB = Path("/var/db/pkg")
DEP_KEYS = ["RDEPEND", "PDEPEND", "DEPEND", "BDEPEND", "IDEPEND"]

# The raw := marker: a slot part ending in `=` (PMS slot-operator `=` /
# `slot=`; stored as `:slot/sub=` between colon and equals sign).
SLOT_EQ = re.compile(r":([^:\[]*)=$")


def installed_providers():
    """(cat, pkg) -> list of (version, slot, subslot)."""
    prov = {}
    for catdir in VDB.iterdir():
        if not catdir.is_dir():
            continue
        for pkgdir in catdir.iterdir():
            if not pkgdir.is_dir() or "-" not in pkgdir.name:
                continue
            slot_f = pkgdir / "SLOT"
            if not slot_f.exists():
                continue
            slot = slot_f.read_text().strip()
            main, _, sub = slot.partition("/")
            sub = sub or main  # PMS: implicit sub-slot equals the slot
            name = pkgdir.name
            # version = trailing -<version>; package = rest
            m = re.match(r"^(.*)-(\d.*)$", name)
            if not m:
                continue
            prov.setdefault((catdir.name, m.group(1)), []).append(
                (m.group(2), main, sub)
            )
    return prov


def main():
    prov = installed_providers()
    stale = []  # (consumer_cpv, key, atom_text, provider_slot)
    for catdir in VDB.iterdir():
        if not catdir.is_dir():
            continue
        for pkgdir in catdir.iterdir():
            if not pkgdir.is_dir():
                continue
            cpv = f"{catdir.name}/{pkgdir.name}"
            texts = []
            for key in DEP_KEYS:
                f = pkgdir / key
                if f.exists():
                    texts.append((key, f.read_text()))
            for key, text in texts:
                try:
                    nodes = hc.parse_dep_string(text)
                except ValueError:
                    continue
                for atom in hc._plain_atoms(nodes):
                    if atom.blocker:
                        continue
                    m = SLOT_EQ.search(atom.text)
                    if not m:
                        continue
                    rec_slot, _, rec_sub = m.group(1).partition("/")
                    if rec_slot == "*" or not rec_sub:
                        continue  # `slot=*` never counts; no sub-slot recorded
                    for ver, main, sub in prov.get(atom.cp, []):
                        if main == rec_slot and sub != rec_sub:
                            stale.append(
                                (cpv, key, atom.text,
                                 f"{atom.cp[0]}/{atom.cp[1]} {main}/{sub}"))
                            break
    stale.sort()
    print(f"installed cps: {len(prov)}")
    print(f"stale := bindings: {len(stale)}")
    for row in stale:
        print("  {} [{}] {} (installed {})".format(*row))
    go = [r for r in stale if "dev-lang/go" in r[2]]
    print(f"go-consumer stale rows: {len(go)}")
    cc = [r for r in stale if "openssl" in r[2] and r[0].startswith("dev-util/cargo-c")]
    print(f"cargo-c stale rows: {len(cc)}")
    for row in cc:
        print("  {} [{}] {} (installed {})".format(*row))


main()
