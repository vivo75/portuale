# Pass 0: a version conflict on dev-libs/T (A wants any T, B wants <T-2) next to the
# upstream case-6 USE conflict on sci-libs/K (EAPIs as upstream so USE never goes empty).
ebuilds = {
    "dev-libs/T-1": {"EAPI": "2"},
    "dev-libs/T-2": {"EAPI": "2"},
    "dev-libs/A-1": {"EAPI": "2", "DEPEND": "dev-libs/T"},
    "dev-libs/B-1": {"EAPI": "2", "DEPEND": "<dev-libs/T-2"},
    "sci-libs/K-1": {"IUSE": "+foo", "EAPI": 1},
    "sci-libs/L-1": {"DEPEND": "sci-libs/K[-foo]", "EAPI": 2},
    "sci-libs/M-1": {"DEPEND": "sci-libs/K[foo=]", "IUSE": "+foo", "EAPI": 2},
}
installed = {
    "sci-libs/K-1": {"IUSE": "foo", "USE": ""},
    "sci-libs/L-1": {"DEPEND": "sci-libs/K[-foo]"},
}
requests = [
    (["sci-libs/L", "sci-libs/M"], {}),
    (["dev-libs/A", "dev-libs/B"], {}),
    (["dev-libs/A", "dev-libs/B", "sci-libs/L", "sci-libs/M"], {}),
    (["dev-libs/A", "dev-libs/B", "sci-libs/L", "sci-libs/M"], {"--autounmask": "n"}),
    (["dev-libs/A", "dev-libs/B", "sci-libs/L", "sci-libs/M"], {"--backtrack": 0}),
]
