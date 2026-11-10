import sys, os
sys.path.insert(0, "lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
from _emerge import depgraph as dg
patched = os.environ.get("PATCH") == "1"
if patched:
    orig = dg.depgraph._want_installed_pkg
    def want(self, pkg):
        if self._frozen_config.excluded_pkgs.findAtomForPackage(pkg, modified_use=self._pkg_use_enabled(pkg)):
            return True
        arg = False
        try:
            for arg, atom in self._iter_atoms_for_pkg(pkg):
                pass   # force_reinstall arm removed
        except Exception:
            pass
        if "selective" in self._dynamic_config.myparams:
            return True
        return not arg
    dg.depgraph._want_installed_pkg = want

ebuilds = {
 "dev-libs/lib-1.0": {"EAPI":"8","SLOT": "0/1"},
 "dev-libs/lib-2.0": {"EAPI":"8","SLOT": "0/2"},
 "dev-libs/cons-1.0": {"EAPI":"8","RDEPEND": "dev-libs/lib:="},
 "dev-libs/cons-1.1": {"EAPI":"8","RDEPEND": "dev-libs/lib:="},
}
installed = {
 "dev-libs/lib-1.0": {"EAPI":"8","SLOT": "0/1"},
 "dev-libs/cons-1.0": {"EAPI":"8","RDEPEND": "dev-libs/lib:0/1="},
}
binpkgs = {"dev-libs/cons-1.1": {"EAPI":"8","RDEPEND": "dev-libs/lib:0/1="}}
shapes = [
 ("u-d-usepkg cons", ["dev-libs/cons"], {"--update":True,"--deep":True,"--usepkg":True}),
 ("noupdate cons", ["dev-libs/cons"], {}),
 ("emptytree-ish complete cons", ["dev-libs/cons"], {"--complete-graph":True}),
 ("noupdate cons usepkg", ["dev-libs/cons"], {"--usepkg":True}),
 ("noupdate lib", ["dev-libs/lib"], {}),
 ("noupdate lib usepkg", ["dev-libs/lib"], {"--usepkg":True}),
 ("noupdate lib+cons usepkg", ["dev-libs/lib","dev-libs/cons"], {"--usepkg":True}),
 ("noreplace cons usepkg", ["dev-libs/cons"], {"--noreplace":True,"--usepkg":True}),
 ("update cons usepkg", ["dev-libs/cons"], {"--update":True,"--usepkg":True}),
 ("usepkgonly cons", ["dev-libs/cons"], {"--usepkgonly":True}),
]
os.environ.setdefault("PORTAGE_GNUPGHOME", os.environ.get("GH",""))
for name, atoms, opts in shapes:
    o = {"--pretend":True, "--verbose":True}; o.update(opts)
    pg = ResolverPlayground(ebuilds=ebuilds, installed=installed, binpkgs=binpkgs,
        user_config={"make.conf": ('FEATURES="-getbinpkg"',)}, debug=False)
    try:
        r = pg.run(atoms, options=o)
        print("##", name, "ok=", r.success, "list=", r.mergelist)
    finally:
        pg.cleanup()
