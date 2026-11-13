project(
  'moose',
  'rust',
  version: '0.8.5',
  meson_version: '>= 1.4.0',
  default_options: ['buildtype=debugoptimized']
)

python = find_program('python3')
gnome = import('gnome')
i18n = import('i18n')
cargo_build = files('build-aux/cargo-build.py')
datadir = get_option('datadir')
styledir = join_paths(datadir, 'moose')
localedir = join_paths(get_option('prefix'), get_option('localedir'))
service_config = configuration_data()
service_config.set('bindir', join_paths(get_option('prefix'), get_option('bindir')))

subdir('data')
subdir('po')

custom_target(
  'moose',
  output: 'moose',
  command: [
    python,
    cargo_build,
    meson.project_source_root(),
    meson.project_build_root(),
    get_option('cargo_profile'),
    get_option('gui').to_string(),
    join_paths(get_option('prefix'), styledir, 'io.github.moooossee.Moose.css'),
    localedir,
    '@OUTPUT@'
  ],
  build_by_default: true,
  build_always_stale: true,
  install: true,
  install_dir: get_option('bindir')
)

i18n.merge_file(
  input: 'data/io.github.moooossee.Moose.desktop',
  output: 'io.github.moooossee.Moose.desktop',
  po_dir: 'po',
  type: 'desktop',
  install: true,
  install_dir: join_paths(datadir, 'applications')
)

i18n.merge_file(
  input: 'data/io.github.moooossee.Moose.metainfo.xml.in',
  output: 'io.github.moooossee.Moose.metainfo.xml',
  po_dir: 'po',
  type: 'xml',
  install: true,
  install_dir: join_paths(datadir, 'metainfo')
)

configure_file(
  input: 'data/dbus-1/services/io.github.moooossee.Moose.service.in',
  output: 'io.github.moooossee.Moose.service',
  configuration: service_config,
  install: true,
  install_dir: join_paths(datadir, 'dbus-1', 'services')
)

install_data(
  'data/icons/hicolor/scalable/apps/io.github.moooossee.Moose.svg',
  install_dir: join_paths(datadir, 'icons', 'hicolor', 'scalable', 'apps')
)

install_data(
  'data/icons/moose-globe-symbolic.svg',
  'data/icons/moose-funnel-symbolic.svg',
  install_dir: join_paths(datadir, 'icons', 'hicolor', 'scalable', 'actions')
)

install_data(
  'data/io.github.moooossee.Moose.css',
  install_dir: styledir
)

gnome.post_install(glib_compile_schemas: true, gtk_update_icon_cache: true)
