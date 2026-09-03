EAPI=8
DESCRIPTION="fixture: IUSE +pf; RDEPEND child[pf=] plus pf? ( pfgraphextra ). Flipping pf off (parent flip) must both satisfy child[pf=] AND drop pfgraphextra -- only a whole-graph re-resolve gets the second part right"
SLOT="0"
KEYWORDS="amd64"
IUSE="+pf"
RDEPEND="dev-libs/pfgraphchild[pf=] pf? ( dev-libs/pfgraphextra )"
