ebuilds = {
    "sci-libs/K-1": {"IUSE": "+foo", "EAPI": "8"},
    "sci-libs/L-1": {"RDEPEND": "sci-libs/K[-foo]", "EAPI": "8"},
    "sci-libs/M-1": {"RDEPEND": "sci-libs/K[foo=]", "IUSE": "+foo", "EAPI": "8"},
}
installed = {
    "sci-libs/K-1": {"IUSE": "foo", "USE": "", "EAPI": "8"},
    "sci-libs/L-1": {"RDEPEND": "sci-libs/K[-foo]", "EAPI": "8"},
}
requests = [(["sci-libs/L", "sci-libs/M"], {"--backtrack": 0, "--debug": True})]
