# Pass 0: a version conflict on dev-libs/T (A wants any T, B wants <T-2) plus a USE conflict
# on sci-libs/K (upstream case 6 shape). Question: what happens when the handler can
# only find a solution on a later pass?
ebuilds = {
    "dev-libs/T-1": {"EAPI": "8"},
    "dev-libs/T-2": {"EAPI": "8"},
    "dev-libs/A-1": {"EAPI": "8", "RDEPEND": "dev-libs/T"},
    "dev-libs/B-1": {"EAPI": "8", "RDEPEND": "<dev-libs/T-2"},
    "sci-libs/K-1": {"IUSE": "+foo", "EAPI": "8"},
    "sci-libs/L-1": {"RDEPEND": "sci-libs/K[-foo]", "EAPI": "8"},
    "sci-libs/M-1": {"RDEPEND": "sci-libs/K[foo=]", "IUSE": "+foo", "EAPI": "8"},
}
installed = {
    "sci-libs/K-1": {"IUSE": "foo", "USE": "", "EAPI": "8"},
    "sci-libs/L-1": {"RDEPEND": "sci-libs/K[-foo]", "EAPI": "8"},
}
requests = [
    (["sci-libs/L", "sci-libs/M"], {}),
    (["dev-libs/A", "dev-libs/B"], {}),
    (["dev-libs/A", "dev-libs/B", "sci-libs/L", "sci-libs/M"], {}),
    (["dev-libs/A", "dev-libs/B", "sci-libs/L", "sci-libs/M"], {"--backtrack": 0}),
]
