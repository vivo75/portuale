import sys, os
sys.path.insert(0, "lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
from _emerge import depgraph as dg
patched = os.environ.get("PATCH") == "1"
if True:
    orig = dg.depgraph._want_installed_pkg
    def want(self, pkg):
        print("   ENTER", pkg.cpv, pkg.installed)
        if self._frozen_config.excluded_pkgs.findAtomForPackage(pkg, modified_use=self._pkg_use_enabled(pkg)):
            return True
        arg = False
        try:
            for arg, atom in self._iter_atoms_for_pkg(pkg):
                
                print("   CALL", pkg.cpv, arg.arg, arg.force_reinstall)
                if arg.force_reinstall:
                    print("   ARM-FIRES", pkg.cpv, arg.arg, "selective=", "selective" in self._dynamic_config.myparams)
        except Exception:
            pass
        if "selective" in self._dynamic_config.myparams:
            return True
        return not arg
    dg.depgraph._want_installed_pkg = want

ebuilds = {"dev-libs/lib-1.0": {"EAPI":"8","SLOT":"0/1"}, "dev-libs/lib-2.0": {"EAPI":"8","SLOT":"0/2"},
 "dev-libs/cons-1.0": {"EAPI":"8","RDEPEND":"dev-libs/lib:="}, "dev-libs/mid-1.0": {"EAPI":"8","RDEPEND":"dev-libs/cons"}}
installed = {"dev-libs/lib-1.0": {"EAPI":"8","SLOT":"0/1"}, "dev-libs/cons-1.0": {"EAPI":"8","RDEPEND":"dev-libs/lib:0/1="}}
binpkgs = {"dev-libs/cons-1.0": {"EAPI":"8","RDEPEND":"dev-libs/lib:0/1="}}
shapes = [
 ("A -uD lib usepkg", ["dev-libs/lib"], {"--update":True,"--deep":True,"--usepkg":True}),
 ("B -uD lib cons usepkg", ["dev-libs/lib","dev-libs/cons"], {"--update":True,"--deep":True,"--usepkg":True}),
 ("C lib usepkg", ["dev-libs/lib"], {"--usepkg":True}),
 ("D mid usepkg", ["dev-libs/mid"], {"--usepkg":True}),
 ("E -uD mid usepkg", ["dev-libs/mid"], {"--update":True,"--deep":True,"--usepkg":True}),
 ("F -uD mid", ["dev-libs/mid"], {"--update":True,"--deep":True}),
 ("G lib cons", ["dev-libs/lib","dev-libs/cons"], {}),
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
