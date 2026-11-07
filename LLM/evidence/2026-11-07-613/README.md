# #613 probes (2026-11-07)

World-B `abk0d`: ROOT staged like pmtest `_abk0_worldb_root` (installed
`abk0a-1` (`RDEPEND=abk0c`), `abk0b-1` (`RDEPEND=<abk0a-3`), `abk0c-1`
(`IUSE=x y z`), world `dev-libs/abk0b`), configroot a copy of the pmtest
fixtures. Same argv on both managers (`probe.sh`):
`--ignore-default-opts --getbinpkg=n --usepkg=n --color=n --pretend <opts> dev-libs/abk0d`.
Portage 3.0.83 on this host. `*.portuale-before.txt` is portuale at
`286b7c15`, `*.portuale.txt` after the fix. `plain-*` is the same shape with
`dev-libs/abk0c x y` in `package.use` on both sides (no USE change needed).

| cell | opts | Portage | before | after |
|---|---|---|---|---|
| def | (none) | 0/20, A-3, `x*`, conflict block | 1/20, A-2, `y*`, skip warning, `x y` | = Portage |
| bt0 | `--backtrack=0` | 0/0, A-3, conflict block | 0/0, A-2, `x y` | = Portage |
| bt1 | `--backtrack=1` | 0/1, as def | 1/1, as before-def | = Portage |
| bt2 | `--backtrack=2` | 0/2, as def | 1/2, as before-def | = Portage |
| aby | `--autounmask-backtrack=y` | 2/20, A-2, `x* y*`, skip warning | 1/20 | = Portage |
| aby-bt1 | `-aby --backtrack=1` | 1/1, A-3, `x*`, block + `--backtrack=30` advice | 1/1, A-2, `x*`, skip warning | = Portage |
| aby-bt2 | `-aby --backtrack=2` | 2/2 | 1/2 | = Portage |
| aby-bt3 | `-aby --backtrack=3` | 2/3 | 1/3 | = Portage |
| plain-def/aby | (none) / `-aby` | 2/20 | 1/20 | = Portage |
| plain-bt1 | `--backtrack=1` | 1/1, A-3, conflict block, rc 1 | 1/1, A-2, rc 0 | = Portage |
| plain-bt2 | `--backtrack=2` | 2/2 | 1/2 | = Portage |
| plain-bt0 | `--backtrack=0` | 0/0, A-3, conflict block | not captured | = Portage |

"= Portage" modulo the known display cut: portuale's slot-conflict block
has no ` for <ROOT>` header suffix and no `to '<ROOT>'` on merge instances
(so the caret line pads shorter); `cmp.sh` normalises those.

Mechanism: the installed `A-1` misses `abk0d`'s `>=dev-libs/abk0a-2`, so
both `A-3` and `A-1` are conflict packages in Portage's
`_process_slot_conflict` and `_slot_confict_backtrack` files two choices;
the `{A-1}` one is popped first. With `--autounmask-backtrack` off the first
pass's `x` change stops the search (`need_config_change`).
