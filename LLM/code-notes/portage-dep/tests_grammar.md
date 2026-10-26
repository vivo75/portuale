# crates/portage-dep/src/tests_grammar.rs
S2 grammar tests: the oracle battery plus focused EAPI/conditional/build-id pins.

## tests: reducer_probe_atoms (moved in #527/#528)
- The old hand-written `reducer_probe_atoms_match_portage_3_0_83` asserted the
  three #510 probe atoms' constructor verdicts. It is gone because the battery
  now carries the same atoms in all nine modes (#528), and the reducer half
  moved to the crates that own the hook (#527): `portage-use-reduce`'s
  `validated_walk_passes_probe_atoms_to_the_hook_whole` /
  `string_path_returns_probe_atoms_unvalidated`, and `portage-repo`'s
  `reducer_rejects_the_510_probe_atom_with_the_visible_wrapper`.
- Facts kept from that block: the oracle is
  `LLM/evidence/2026-10-23-510/probe.py`, run `python3 -I probe.py` on the
  host Portage 3.0.83 and against the 3.0.82.2 tag built with its own C
  parser. `games-arcade/2048` and `dev-util/timidity--` are `OK` in the
  constructor and the reducer; `cat/pkg[a(+),-a]` is `Atom` `ERR
  Invalid use dep: '-a'`, `use_reduce(token_class=Atom)` `InvalidDependString`
  (`invalid use dep token: '-a'`), and `use_reduce` without `token_class` `OK`
  (the plain string comes back; quirk Q8).
- The three C-parser fixes the 3.0.83 entry points share are `450282d84`,
  `8bf578fd7`, `bdce305cd`; their 3.0.82.2 equivalents (`6a64fb2f4`,
  `4f76d9ee5`, `a2206fc32`) are already in that tag, which is why the two
  versions agree (errata `LLM/02.442-port-portage-3.0.83.errata.md` E1).
- Battery provenance and row split: `testdata/atoms-3.0.83.tsv` is generated
  from Portage 3.0.83 by `LLM/tools/atom_oracle_table.py` (oracle probe
  battery, 2026-10-23); its tail rows are the three #510 probe atoms (#528).
  This file pins the constructor verdicts for every mode; the reducer
  tokenization rows live in `portage-use-reduce` (`use_reduce_flat` /
  `use_reduce_flat_validated`, #527) and the wrapper rows in
  `crates/portage-repo/src/tests/dep_validate.rs` (`validate_dep_string`,
  what emerge prints).
