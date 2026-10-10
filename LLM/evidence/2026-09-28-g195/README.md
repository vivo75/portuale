# Evidence — backlog #195 aup0b half (2026-09-28, round 4 / g195d)

Staged copies of the probe scripts/outputs the aup0b pins and the
`violated_parent_flags` docstring cite. The originals lived under
`/tmp/opencode/g195{b,c}/` (ephemeral); the text here is byte-identical
apart from this README.

- `g195b-in-probe.sh` / `g195b-real-probe.txt` — round-2 container probe:
  `emerge --pretend --autounmask =dev-libs/aup0b-1` (rc 1) on the staged
  fixtures, pasted verbatim in the aup0b pin docstring
  (`test_emerge_pretend_contract.py`, `..._fails_like_real_when_the_child_flag_is_masked`).
- `g195c-in-probe.sh` / `g195c-real-probe.txt` — round-3 container probe
  (ruling B20 option (a)): default, `--autounmask-backtrack=y` and
  `--autounmask-use=n` all report the bare miss, rc 1; pasted verbatim in
  the pfgraph pin docstring.
- `g195d-oracle-probe.py` / `g195d-oracle-output.txt` — round-4 host
  oracle: real Portage 3.0.82.2 `Atom.violated_conditionals`
  (`lib/portage/dep/__init__.py:1465`, branches `:1525-1576`) run over
  the full operator x validity x default x parent/child-USE matrix plus
  the concrete-gate cells. Usage: `python3 g195d-oracle-probe.py` (the
  script hard-codes the worktree 3rdparty `lib/` path on `sys.path`).
  The Rust `violated_parent_flags` operator-level unit tests
  (`rust/portage-repo/src/lib.rs`, `mod tests_195d`) pin this table's
  `arm` column cell by cell.
