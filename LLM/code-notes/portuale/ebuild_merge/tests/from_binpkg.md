# crates/portuale/src/ebuild_merge/tests/from_binpkg.rs
Binary-merge hook environment and `user_patch.digests` report pins (#542).

## fn binary_hook_env_recomputes_use_expand_from_the_binpkg
- Expected values from the L1 bash merge on Portage 3.0.83-r1 (`LLM/evidence/2026-10-24-vdb-env/`: `declare -x QEMU_SOFTMMU_TARGETS=""`). Portage: `config.py`, `setcpv`/`environ`.

## fn parse_user_patch_digests_accepts_the_shell_format (#542)
- The `user_patch.digests` member parses exactly like Portage (`strip(b"\0").split(b"\0")`, hex/basename validation, pairwise count). Oracle: the shell-written bytes (`hash\0basename\0`, trailing NUL included).

## fn parse_user_patch_digests_rejects_malformed_members (#542)
- Malformed members are invalid, never a table. Oracle: Portage's `else` arm (`This binary package has invalid user patch digest metadata!!!`).

## fn user_patch_table_matches_the_oracle_on_a_match (#542)
- The table on the matching merge is byte-identical to the oracle (`captures/14-merge-match.portage.out`, `term_columns` 80 off a terminal).

## fn user_patch_table_warns_on_mismatch_and_malformed_members (#542)
- A differing local set warns on stderr (oracle `captures/16-merge-mismatch.portage.{out,err}`); so does a missing local set, and a malformed member warns invalid instead of a table.

## fn user_patch_table_truncates_long_names_like_portage (#542)
- Truncation mirrors Portage's character-count rules (long basenames cut to `max(term_columns - 8 - 6, 25)`, digests to `max(term_columns - len(basename) - 6, 8)`, rows padded to `term_columns - 4`).
