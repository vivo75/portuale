EAPI=8
DESCRIPTION="fixture: slice 2 in-bin ordering -- || ( oranyslotalt:2 oranyslotalt:1 ), both tie at the Installed bin (cp-installed); real's all_installed_slots promotion picks the already-installed slot 1 over the first-listed, higher-version, not-installed slot 2 (avoids an unwanted new-slot merge)"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="|| ( dev-libs/oranyslotalt:2 dev-libs/oranyslotalt:1 )"
