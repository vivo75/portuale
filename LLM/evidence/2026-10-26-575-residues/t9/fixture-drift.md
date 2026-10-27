# Task 9 fixture drift (full contract suite, scripts/pytest-parallel.sh)

Result: 4552 passed, 2 skipped, 2 xfailed, rc 0. No failing test. Exactly one corpus drift
warning; corpus NOT blessed (contract.json.xz restored byte-identical after a scratch bless used
only to read the diff).

## Drifting case

`pytests-contract-suite/test_emerge_pretend_contract.py::test_pretend_case_exit_code[--color=y -pc: coloured advisory block (WARN ` * `, green backtick commands)]#0` -- stdout changed.

Drift lines (all caused by the six new installed packages, nothing else):

- +3 depclean-listing blocks (after line 310): `dev-libs/p589cvirt`, `p589bvirt`, `p589avirt`, each `selected: 1`, protected/omitted none.
- +3 blocks (after line 715): `dev-libs/p589cprov`, `p589bprov`, `p589aprov`, each `selected: 5.42`.
- `All selected packages:` line gains `=dev-libs/p589aprov-5.42 =dev-libs/p589avirt-1 =dev-libs/p589bprov-5.42 =dev-libs/p589bvirt-1 =dev-libs/p589cprov-5.42 =dev-libs/p589cvirt-1`.
- `Packages installed:   175` -> `181`.
- `Number to remove:     171` -> `177`.

The corpus blob 76cf7f69a8547f921f28 is this stdout. No other case drifted; no drift for any reason other than the new packages.
