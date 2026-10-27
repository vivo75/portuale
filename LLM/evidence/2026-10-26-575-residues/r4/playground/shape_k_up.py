# upstream SlotCollisionTestCase case 6 subset, verbatim EAPIs
ebuilds = {
    "sci-libs/K-1": {"IUSE": "+foo", "EAPI": 1},
    "sci-libs/L-1": {"DEPEND": "sci-libs/K[-foo]", "EAPI": 2},
    "sci-libs/M-1": {"DEPEND": "sci-libs/K[foo=]", "IUSE": "+foo", "EAPI": 2},
}
installed = {
    "sci-libs/K-1": {"IUSE": "foo", "USE": ""},
    "sci-libs/L-1": {"DEPEND": "sci-libs/K[-foo]"},
}
requests = [(["sci-libs/L", "sci-libs/M"], {"--backtrack": 0})]
