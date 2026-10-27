# Pass 0: P-2 wants =Z-2 while Q wants =Z-1 (version conflict; backtracking masks P-2).
# Only P-1 pulls sci-libs/M, so the upstream case-6 USE conflict on sci-libs/K appears on
# pass 1, where the slot conflict handler can find its USE change.
ebuilds = {
    "dev-libs/Z-1": {"EAPI": "2"},
    "dev-libs/Z-2": {"EAPI": "2"},
    "dev-libs/P-1": {"EAPI": "2", "DEPEND": "=dev-libs/Z-1 sci-libs/M"},
    "dev-libs/P-2": {"EAPI": "2", "DEPEND": "=dev-libs/Z-2"},
    "dev-libs/Q-1": {"EAPI": "2", "DEPEND": "=dev-libs/Z-1"},
    "sci-libs/K-1": {"IUSE": "+foo", "EAPI": 1},
    "sci-libs/L-1": {"DEPEND": "sci-libs/K[-foo]", "EAPI": 2},
    "sci-libs/M-1": {"DEPEND": "sci-libs/K[foo=]", "IUSE": "+foo", "EAPI": 2},
}
installed = {
    "sci-libs/K-1": {"IUSE": "foo", "USE": ""},
    "sci-libs/L-1": {"DEPEND": "sci-libs/K[-foo]"},
}
requests = [
    (["dev-libs/P", "dev-libs/Q", "sci-libs/L"], {}),
    (["dev-libs/P", "dev-libs/Q", "sci-libs/L"], {"--autounmask": "n"}),
]
