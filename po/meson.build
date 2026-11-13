i18n.gettext(
  meson.project_name(),
  preset: 'glib',
  args: [
    '--from-code=UTF-8',
    '--keyword=gettext_noop',
    '--keyword=gettext_f',
    '--keyword=ngettext_f:1,2',
    '--flag=gettext_f:1:rust-format',
    '--flag=ngettext_f:1:rust-format',
    '--flag=ngettext_f:2:rust-format',
    '--package-version=' + meson.project_version(),
    '--msgid-bugs-address=https://github.com/moooossee/moose/issues',
    '--add-location=file',
  ],
)
