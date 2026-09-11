EAPI=8
DESCRIPTION="fixture: F4 || group where the cp-installed alternative isn't installed in the SLOT it targets (unsatuseslotalt:2) vs. the alternative installed in its own slot (unsatuseslotother)"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="|| ( dev-libs/unsatuseslotalt:2[unsatuseorflag] dev-libs/unsatuseslotother[unsatuseorflag] )"
