# portuale manual pages. Adapted from Portage's man/ tree: the tool
# pages for actions portuale does not implement (dispatch-conf,
# egencache, emaint, emirrordist, env-update, etc-update, fixpackages,
# glsa-check, quickpkg) were dropped, and the translated pages under
# ru/ were not carried over. What remains is the emerge/ebuild pair
# portuale's multicall binary provides plus the file-format references
# it reads (make.conf, portage, ebuild, color.map, xpak).

conf_data = get_variable('conf_data', configuration_data())
if not conf_data.has('VERSION')
    conf_data.set('VERSION', meson.project_version())
endif

man_pages_out = []
man_pages_in = [
    'color.map.5',
    'ebuild.1',
    'ebuild.5',
    'emerge.1',
    'make.conf.5',
    'portage.5',
    'xpak.5',
]

foreach man_page : man_pages_in
    man_pages_out += configure_file(
        input : man_page,
        output : man_page,
        configuration : conf_data
    )
endforeach

install_man(man_pages_out)
