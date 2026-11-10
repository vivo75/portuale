# #666: does the `force_reinstall` arm of `_want_installed_pkg` ever decide anything?

Portage 3.0.83 (`3rdparty/portage`, tag `portage-3.0.83`), 2026-11-10, ResolverPlayground,
`PYTHONHASHSEED=0`, `PORTAGE_GNUPGHOME` = copy of `lib/portage/tests/.gnupg` (chmod 700).
Run from `3rdparty/portage` with `PATH=$PWD/bin:$PATH`:

- `probe_patched_vs_stock.py`: `PATCH=0` stock, `PATCH=1` with the `force_reinstall`
  branch removed from `_want_installed_pkg`. 10 shapes (`-u`, `-uD`, `--noreplace`,
  `--usepkg`, `--usepkgonly`, `--complete-graph`, installed cons built `lib:0/1=`, binary
  cons 1.1). Output `patched_vs_stock_out.txt`: identical merge lists, both runs.
- `probe_arm_fires.py`: the patched function prints `CALL` for every argument it iterates.
  Over 7 more shapes (an intermediate consumer, multi-atom requests, with and without
  `--usepkg`), every iterated argument has `force_reinstall=False`; the branch is never
  taken. Further runs (not kept): the #481 `mbabicons` shape (binary parent, installed
  child) and the `--rebuild-if-*` options, 20+ shapes in all, same result: the replace set
  there holds the built parent, never the installed child.

Conclusion: no plan differs with or without the arm; it was never reached. Only the
internal sets of `_gen_reinstall_sets` carry `force_reinstall`; a command-line atom does
not, and `return not arg` already covers it, which portuale has.
Not probed: `--changed-slot` reinstalls got one probe, `--rebuild-if-*` restarts one.
