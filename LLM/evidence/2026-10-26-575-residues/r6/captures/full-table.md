| `aub-y-cba` | fx | `--autounmask-backtrack=y dev-libs/aub0c dev-libs/aub0b dev-libs/aub0a` | 0/20 | F0 +brk | 0/20 | ERR ERR |
| `aub-y-cab` | fx | `--autounmask-backtrack=y dev-libs/aub0c dev-libs/aub0a dev-libs/aub0b` | 2/20 | R2 F+ F0 +best +brk | 2/20 | SC DE ERR ERR |
| `aub-y-bca` | fx | `--autounmask-backtrack=y dev-libs/aub0b dev-libs/aub0c dev-libs/aub0a` | 0/20 | F0 +brk | 0/20 | ERR ERR |
| `aub-y-bac` | fx | `--autounmask-backtrack=y dev-libs/aub0b dev-libs/aub0a dev-libs/aub0c` | 0/20 | F0 +brk | 0/20 | ERR ERR |
| `aub-y-acb` | fx | `--autounmask-backtrack=y dev-libs/aub0a dev-libs/aub0c dev-libs/aub0b` | 2/20 | R2 F+ F0 +best +brk | 2/20 | SC DE ERR ERR |
| `aub-y-abc` | fx | `--autounmask-backtrack=y dev-libs/aub0a dev-libs/aub0b dev-libs/aub0c` | 3/20 | R2 R2 F+ F0 +best +brk | 3/20 | SC DE CF MD ERR SC ERR ERR |
| `aub-y-ba` | fx | `--autounmask-backtrack=y dev-libs/aub0b dev-libs/aub0a` | 0/20 | F0 +brk | 0/20 | ERR ERR |
| `aub-y-ab` | fx | `--autounmask-backtrack=y dev-libs/aub0a dev-libs/aub0b` | 1/20 | R1 F0 +best +brk | 1/20 | CF ERR ERR |
| `aub-n-cba` | fx | `--autounmask-backtrack=n dev-libs/aub0c dev-libs/aub0b dev-libs/aub0a` | 0/20 | B:need_config_change +brk | 0/20 | ERR ERR |
| `aub-n-cab` | fx | `--autounmask-backtrack=n dev-libs/aub0c dev-libs/aub0a dev-libs/aub0b` | 0/20 | B:need_config_change | 0/20 | SC |
| `aub-n-bca` | fx | `--autounmask-backtrack=n dev-libs/aub0b dev-libs/aub0c dev-libs/aub0a` | 0/20 | B:need_config_change +brk | 0/20 | ERR ERR |
| `aub-n-bac` | fx | `--autounmask-backtrack=n dev-libs/aub0b dev-libs/aub0a dev-libs/aub0c` | 0/20 | B:need_config_change +brk | 0/20 | ERR ERR |
| `aub-n-acb` | fx | `--autounmask-backtrack=n dev-libs/aub0a dev-libs/aub0c dev-libs/aub0b` | 0/20 | B:need_config_change | 0/20 | SC |
| `aub-n-abc` | fx | `--autounmask-backtrack=n dev-libs/aub0a dev-libs/aub0b dev-libs/aub0c` | 0/20 | B:need_config_change | 0/20 | SC |
| `aub-n-ba` | fx | `--autounmask-backtrack=n dev-libs/aub0b dev-libs/aub0a` | 0/20 | B:need_config_change +brk | 0/20 | ERR ERR |
| `aub-n-ab` | fx | `--autounmask-backtrack=n dev-libs/aub0a dev-libs/aub0b` | 0/20 | B:need_config_change | 0/20 | OK |
| `fx-slotconflictunsolvable` | fx | `dev-libs/slotconflictunsolvable` | 6/20 | R2 R2 R2 F+ R1 R1 F0 +best | 5/20 | SC MD MD ERR MD MD ERR SC ERR |
| `fx-slotconfgroup` | fx | `dev-libs/slotconfgroup` | 6/20 | R2 R2 R2 F+ R1 R1 F0 +best | 5/20 | SC MD MD ERR MD MD ERR SC ERR |
| `fx-scuseparent` | fx | `dev-libs/scuseparent` | 6/20 | R2 R2 R2 F+ R1 R1 F0 +best | 5/20 | SC MD MD ERR MD MD ERR SC ERR |
| `fx-slotusegroup` | fx | `dev-libs/slotusegroup` | 4/20 | R2 R2 R2 F+ F0 +best | 3/20 | SC DE MD MD ERR SC ERR |
| `fx-ormisstop` | fx | `dev-libs/ormisstop` | 1/20 | R1 B:success | 1/20 | MD OK |
| `fx-ormissbad` | fx | `dev-libs/ormissbad` | 1/20 | R1 F0 +best | 0/20 | MD ERR MD ERR |
| `fx-abort-masked-mid` | fx | `dev-libs/abort-masked-mid` | 1/20 | R1 F0 +best | 0/20 | MD ERR MD ERR |
| `fx-abort-unsat-mid` | fx | `dev-libs/abort-unsat-mid` | 1/20 | R1 F0 +best | 0/20 | MD ERR MD ERR |
| `fx-abort-masked-last` | fx | `dev-libs/abort-masked-last` | 1/20 | R1 F0 +best | 0/20 | MD ERR MD ERR |
| `fx-abort-masked-cycle` | fx | `dev-libs/abort-masked-cycle` | 1/20 | R1 F0 +best | 0/20 | MD ERR MD ERR |
| `fx-r594etop` | fx | `dev-libs/r594etop` | 0/20 | F0 | 0/20 | DE DE ERR |
| `fx-r594gtop` | fx | `dev-libs/r594gtop` | 1/20 | R1 B:success | 1/20 | MD OK |
| `fx-r594qtop` | fx | `dev-libs/r594qtop` | 0/20 | B:need_config_change | 0/20 | MD |
| `fx-r594ctop` | fx | `dev-libs/r594ctop` | 1/20 | R1 B:success | 1/20 | MD OK |
| `fx-r594ftop` | fx | `dev-libs/r594ftop` | 1/20 | R1 F0 +best | 1/20 | MD DE DE ERR |
| `fx-r594jtop` | fx | `dev-libs/r594jtop` | 1/20 | R1 B:need_config_change | 1/20 | MD OK |
| `fx-p569pull` | fx | `dev-libs/p569pull` | 2/20 | R2 R2 B:success | 2/20 | SC MD OK |
| `fx-p563pull` | fx | `dev-libs/p563pull` | 2/20 | R2 R2 B:success | 2/20 | SC MD OK |
| `fx-p589apull` | fx | `dev-libs/p589apull` | 0/20 | B:success | 0/20 | OK OK |
| `fx-p589bpull` | fx | `dev-libs/p589bpull` | 4/20 | R2 R2 R1 R1 B:success | 4/20 | SC SC MD MD OK |
| `fx-p589cpull` | fx | `dev-libs/p589cpull` | 4/20 | R2 R2 R1 R1 B:success | 4/20 | SC SC MD MD OK |
| `fx-t8x` | fx | `dev-libs/t8x` | 1/20 | R1 B:success | 1/20 | MD OK |
| `fx-needer` | fx | `dev-libs/needer` | 0/20 | B:success | 0/20 | OK OK |
| `fx-g0` | fx | `dev-libs/p569pull dev-libs/abort-masked-mid` | 1/20 | R1 F0 +best | 0/20 | MD ERR MD ERR |
| `fx-g0-bt1` | fx | `--backtrack=1 dev-libs/p569pull dev-libs/abort-masked-mid` | 1/1 | R1 B:limit +best | 0/1 | MD ERR MD ERR |
| `fx-c-aun` | fx | `--autounmask=n dev-libs/r594ctop` | 0/20 | F0 | 0/20 | DE DE ERR |
| `fx-q-bt1` | fx | `--backtrack=1 dev-libs/r594qtop` | 0/1 | B:need_config_change | 0/1 | MD |
| `fx-p569pull_t8x` | fx | `dev-libs/p569pull dev-libs/t8x` | 3/20 | R1 R2 R2 B:success | 3/20 | MD SC MD OK |
| `fx-triangle` | fx | `dev-libs/needer dev-libs/othermod` | 4/20 | R2 R2 F+ R1 F0 +best | 3/20 | SC MD ERR MD ERR SC ERR SC MD ERR MD ERR SC ERR |
| `fx-triangle-rev` | fx | `dev-libs/othermod dev-libs/needer` | 4/20 | R2 R2 F+ R1 F0 +best | 3/20 | SC MD ERR MD ERR SC ERR SC MD ERR MD ERR SC ERR |
| `fx-paired-2` | fx | `=dev-libs/paired-2.0` | 0/20 | B:success | 0/20 | OK OK |
| `keeper-arg` | keeper | `=dev-libs/paired-2.0` | 3/20 | R2 R2 F+ F0 +best | 2/20 | OK SC SC ERR ERR OK |
| `keeper-hard` | keeper | `dev-libs/needer` | 4/20 | R2 R2 R1 R1 F0 +best | 3/20 | OK SC SC MD MD ERR OK |
| `keeper-triangle` | keeper | `dev-libs/needer dev-libs/othermod` | 4/20 | R2 R2 F+ R1 F0 +best | 3/20 | SC MD ERR MD ERR SC ERR SC MD ERR MD ERR SC ERR |
| `igw-ge` | igw | `>=dev-libs/igw0x-2` | 3/20 | R2 R2 F+ F0 +best | 2/20 | OK SC SC ERR ERR OK |
| `igw-lt` | igw | `<dev-libs/igw0x-1` | 3/20 | R2 F+ R1 F0 +best | 2/20 | OK SC ERR SC ERR OK |
| `cgpver-ge` | cgpver | `>=dev-libs/cgp1x-2` | 3/20 | R2 R2 F+ F0 +best | 2/20 | OK SC SC ERR ERR OK |
| `cgpver-lt` | cgpver | `<dev-libs/cgp1x-1` | 3/20 | R2 F+ R1 F0 +best | 2/20 | OK SC ERR SC ERR OK |
| `ch-d` | fx | `dev-libs/r6c0` | 4/20 | R1 R1 R1 R1 F0 +best | 3/20 | MD MD MD MD ERR DE ERR |
| `ch-b1` | fx | `--backtrack=1 dev-libs/r6c0` | 1/1 | R1 B:limit +best | 1/1 | MD MD DE ERR |
| `ch-b2` | fx | `--backtrack=2 dev-libs/r6c0` | 2/2 | R1 R0 +best | 1/2 | MD MD DE ERR |
| `ch-b3` | fx | `--backtrack=3 dev-libs/r6c0` | 3/3 | R1 R1 R0 +best | 2/3 | MD MD MD DE ERR |
| `ch-b4` | fx | `--backtrack=4 dev-libs/r6c0` | 3/4 | R1 R1 R0 +best | 2/4 | MD MD MD DE ERR |
| `ch-b5` | fx | `--backtrack=5 dev-libs/r6c0` | 4/5 | R1 R1 R1 R0 +best | 3/5 | MD MD MD MD DE ERR |
| `ch-b6` | fx | `--backtrack=6 dev-libs/r6c0` | 4/6 | R1 R1 R1 R0 +best | 3/6 | MD MD MD MD DE ERR |
| `fx-vv1-y` | fx | `--autounmask-backtrack=y dev-libs/r594vdep dev-libs/r594vtop` | 0/20 | B:need_config_change | 1/20 | CF OK |
| `fx-vv2-y` | fx | `--autounmask-backtrack=y dev-libs/r594vtop dev-libs/r594vdep` | 0/20 | B:need_config_change | 1/20 | CF OK |
| `fx-vv3-y` | fx | `--autounmask-backtrack=y dev-libs/r594vdep dev-libs/r594vm` | 0/20 | B:need_config_change | 1/20 | CF OK |
| `fx-vv4-y` | fx | `--autounmask-backtrack=y dev-libs/r594vm dev-libs/r594vdep` | 0/20 | B:need_config_change | 1/20 | CF OK |
| `fxq-1` | fx | `--autounmask-backtrack=y dev-libs/r594vdep dev-libs/r594qa` | 0/20 | B:need_config_change | 1/20 | CF OK |
| `fxq-2` | fx | `--autounmask-backtrack=y dev-libs/r594qa dev-libs/r594vdep` | 0/20 | B:need_config_change | 1/20 | CF OK |
| `fxq-3` | fx | `dev-libs/r594vdep dev-libs/r594qa` | 0/20 | B:need_config_change | 0/20 | OK |
| `c-q1` | cfx | `--autounmask-backtrack=y dev-libs/r594vdep dev-libs/r594qa` | 0/20 | B:need_config_change | 1/20 | CF OK |
| `w-ab-y` | fx | `--autounmask-backtrack=y dev-libs/r6a dev-libs/r6b` | 0/20 | B:need_config_change | 0/20 | OK |
| `w-ba-y` | fx | `--autounmask-backtrack=y dev-libs/r6b dev-libs/r6a` | 1/20 | R1 B:need_config_change | 1/20 | CF OK |
| `p-ab-y` | fx | `--autounmask-backtrack=y dev-libs/r6pa dev-libs/r6pb` | 0/20 | B:need_config_change | 0/20 | OK |
| `p-ba-y` | fx | `--autounmask-backtrack=y dev-libs/r6pb dev-libs/r6pa` | 0/20 | B:need_config_change | 1/20 | CF OK |
| `c-scu2` | cfx | `--backtrack=2 dev-libs/slotconflictunsolvable` | 2/2 | R2 R1 B:limit +best | 2/2 | SC MD MD SC ERR |
| `c-scu3` | cfx | `--backtrack=3 dev-libs/slotconflictunsolvable` | 3/3 | R2 R2 R1 B:limit +best | 3/3 | SC MD MD MD SC ERR |
| `c-sug2` | cfx | `--backtrack=2 dev-libs/slotusegroup` | 2/2 | R2 R1 B:limit +best | 2/2 | SC DE MD SC ERR |
| `c-p589b2` | cfx | `--backtrack=2 dev-libs/p589bpull` | 2/2 | R2 R1 B:limit +best | 2/2 | SC SC MD OK SC SC MD OK |
