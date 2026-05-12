project(
  'moose',
  'rust',
  version: '0.1.0',
  meson_version: '>= 1.4.0',
  default_options: ['buildtype=debugoptimized']
)

python = find_program('python3')
cargo_build = files('build-aux/cargo-build.py')
datadir = get_option('datadir')
service_config = configuration_data()
service_config.set('bindir', join_paths(get_option('prefix'), get_option('bindir')))

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
    '@OUTPUT@'
  ],
  build_by_default: true,
  install: true,
  install_dir: get_option('bindir')
)

configure_file(
  input: 'data/io.github.moooossee.Moose.desktop',
  output: 'io.github.moooossee.Moose.desktop',
  copy: true,
  install: true,
  install_dir: join_paths(datadir, 'applications')
)

configure_file(
  input: 'data/io.github.moooossee.Moose.metainfo.xml.in',
  output: 'io.github.moooossee.Moose.metainfo.xml',
  copy: true,
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
