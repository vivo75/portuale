EAPI=8
DESCRIPTION="fixture: bug-515584 -- alternative 1's [use]-dep flag is masked (other, never selected), alternative 2's is merely unsatisfied-by-default (unsat_use_non_installed, selectable) -- alt 2 must win with an autounmask flip proposed"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="|| ( dev-libs/unsatusemaskedalt[unsatusemaskedflag] dev-libs/unsatuseordertarget[unsatuseorflag] )"
