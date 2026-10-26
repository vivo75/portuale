"""S0 probe for Task 4 (#578): what Portage 3.0.83 verifies for a binpkg.

Covers `BinpkgVerifier._start` (lib/_emerge/BinpkgVerifier.py:25-89),
`checksum.verify_all` (lib/portage/checksum.py:378),
`_apply_hash_filter` (lib/portage/checksum.py:343) and `_hash_filter
transparent` (lib/_emerge/BinpkgVerifier.py:42-44).

Run: PYTHONPATH=3rdparty/portage/lib python3 LLM/evidence/2026-10-25-review/t4/s0_probe.py
"""
import hashlib
import os
import sys
import tempfile

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "..",
                                "3rdparty", "portage", "lib"))

from portage.checksum import (
    _apply_hash_filter,
    _filter_unaccelarated_hashes,
    _hash_filter,
    get_valid_checksum_keys,
    hashorigin_map,
    perform_multiple_checksums,
    verify_all,
)

print("== host hashlib algorithms_available ==")
print(sorted(hashlib.algorithms_available))
print("== get_valid_checksum_keys ==")
print(sorted(get_valid_checksum_keys()))
print("== hashorigin_map ==")
for k in sorted(get_valid_checksum_keys()):
    from portage.checksum import hashorigin_map as om
    print(f"  {k}: {om.get(k)}")
from portage import checksum as c
print(f"== _whirlpool_unaccelerated: {c._whirlpool_unaccelerated}")

print("== int(SIZE) semantics (bintree._get_digests:2866 does int(metadata['SIZE'])) ==")
for raw in ["4618", " 4618", "4618 ", " 4618 ", "-1", "+5", "x", "",
            "0x10", "46_18", "4618\n", "007"]:
    try:
        print(f"  {raw!r} -> {int(raw)}")
    except ValueError:
        print(f"  {raw!r} -> ValueError (size dropped from digests)")

print("== verify_all against a real file ==")
d = tempfile.mkdtemp()
p = os.path.join(d, "payload.tbz2")
with open(p, "wb") as f:
    f.write(b"0123456789abcdef")
digests = perform_multiple_checksums(p, ["MD5", "SHA1", "SHA256", "SHA512",
                                         "BLAKE2B", "BLAKE2S", "SHA3_256",
                                         "SHA3_512", "RMD160", "WHIRLPOOL"])
for k in sorted(digests):
    print(f"  {k} = {digests[k]}")
size = os.stat(p).st_size
print(f"  size = {size}")

base = {"size": size}
base.update(digests)
ok, reason = verify_all(p, dict(base))
print(f"all correct -> {ok} {reason!r}")

bad = dict(base)
bad["size"] = 9999
print(f"bad size -> {verify_all(p, bad)!r}")

bad = dict(base)
bad["SHA512"] = "0" * 128
print(f"bad SHA512 -> {verify_all(p, bad)!r}")

bad = {"size": size, "SHA512": "0" * 128}
print(f"size + only wrong SHA512 -> {verify_all(p, bad)!r}")

bad = {"size": size, "BLAKE2B": "0" * 128}
print(f"size + only wrong BLAKE2B -> {verify_all(p, bad)!r}")

bad = {"size": size, "FOO": "abc", "BAR": ""}
print(f"size + only unknown keys -> {verify_all(p, bad)!r}")

bad = {"MD5": digests["MD5"]}
print(f"hashes but no size -> {verify_all(p, bad)!r}")

bad = {"size": size}
print(f"size only, no hashes -> {verify_all(p, bad)!r}")

bad = {"size": -1}
bad.update({k: v for k, v in digests.items()})
print(f"size -1 (int('-1')) -> {verify_all(p, bad)!r}")

print("== _hash_filter transparency (BinpkgVerifier.py:42-44) ==")
for fstr in ["", "  ", "-MD5", "-*", "-MD5 -SHA1", "MD5", "*"]:
    hf = _hash_filter(fstr)
    print(f"  {fstr!r} transparent={hf.transparent}")

print("== _apply_hash_filter keeps at least one ==")
names = ["MD5", "SHA1", "SHA512"]
for fstr in ["-MD5", "-MD5 -SHA1", "-MD5 -SHA1 -SHA512", "-*", "SHA512"]:
    hf = _hash_filter(fstr)
    d0 = {"size": 16, "MD5": "m", "SHA1": "s", "SHA512": "h"}
    if not hf.transparent:
        d0 = _apply_hash_filter(d0, hf)
    print(f"  {fstr!r} -> {sorted(d0)}")

print("== _filter_unaccelarated_hashes ==")
d0 = {"size": 16, "MD5": "m", "WHIRLPOOL": "w"}
print(f"  whirlpool+md5 -> {sorted(_filter_unaccelarated_hashes(d0))}")
d0 = {"size": 16, "WHIRLPOOL": "w"}
print(f"  whirlpool alone -> {sorted(_filter_unaccelarated_hashes(d0))}")
