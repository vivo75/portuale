import sys, portage
from _emerge.RootConfig import RootConfig
from _emerge.Package import Package
root = portage.settings['EROOT']
trees = portage.db
rc = RootConfig(portage.settings, trees[root], None)
kind = sys.argv[1]; atom = sys.argv[2]
if kind == 'installed':
    db = trees[root]['vartree'].dbapi
else:
    db = trees[root]['bintree'].dbapi; trees[root]['bintree'].populate()
for cpv in db.match(atom):
    md = dict(zip(Package.metadata_keys, db.aux_get(cpv, Package.metadata_keys)))
    pkg = Package(built=True, cpv=cpv, installed=(kind=='installed'), metadata=md, root_config=rc, type_name=kind)
    print(kind, cpv, "RDEPEND=%r" % md['RDEPEND'], "EAPI=%s" % md['EAPI'])
    print("  invalid:", pkg.invalid)
    print("  visible:", pkg.visible)
