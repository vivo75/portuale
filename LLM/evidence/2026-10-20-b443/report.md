# cast cells report

| cell | argv | rc portage | rc portuale | text equal | chunking equal | max timing ratio |
| --- | --- | --- | --- | --- | --- | --- |
| plain-new | `emerge --pretend dev-libs/newpkg` | 0 | 0 | no | no | 1.0x (0 flagged) |
| blocker | `emerge --pretend dev-libs/blockerpkg` | 0 | 0 | no | no | 1.0x (0 flagged) |
| masked-abort | `emerge --pretend dev-libs/maskedpkg` | 1 | 1 | no | no | 1.0x (0 flagged) |
| autounmask | `emerge --pretend --autounmask dev-libs/autounmaskkeywordpkg` | 1 | 1 | no | no | 1.0x (0 flagged) |
| tree | `emerge --pretend --tree dev-libs/diamond` | 0 | 0 | no | no | 1.0x (0 flagged) |
| verbose-use | `emerge --pretend -v dev-libs/newpkg` | 0 | 0 | no | no | 1.0x (0 flagged) |
| upgrade | `emerge --pretend --update dev-libs/upgradepkg` | 0 | 0 | no | no | 1.0x (0 flagged) |
| triangle-conflict | `emerge --pretend dev-libs/needer dev-libs/othermod` | 1 | 1 | no | no | 1.0x (0 flagged) |
