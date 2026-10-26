# crates/portage-profile/src/tests/user_patches.rs
Local user-patch digest pins against the host oracle and the upstream NIST vectors.

## module
- Local user-patch digest, Portage `UserPatches` (`lib/portage/package/ebuild/_config/UserPatches.py`); the shell `eapply_user` writer (`bin/phase-helpers.sh`) agrees on the algorithm (#509). Oracle: host `userPatchDigest` values in `LLM/evidence/2026-10-23-509/`; upstream NIST vectors in `MockUserPatches` (`Patch1`+`Patch2` -> `05f0b2d539792008c12ef9993a5d27ab85837c2b34317e423b0fd9322a9d6918`, `Patch3`+`Patch4` -> `506f0b5b42ef2d861c00fe67aba3f33889a2b433febc0291f9d237acd3e0b4f8`).

## fn stray_files_outside_pkg_dirs_are_ignored
- Portage `7964f58e2` (bug 982999). Oracle: `LLM/evidence/2026-10-23-509/captures/15-stray-k.*`.
