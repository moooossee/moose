# Moose

Moose is a small desktop app for chatting with local AI models through Ollama.
It is meant to feel quiet, simple and local: your conversations, providers and
model choices stay on your computer.

## Features

- Chat with local Ollama models
- Keep conversations saved locally
- Manage providers and models from the app

## Build

Moose uses Rust, Meson, GTK 4 and libadwaita.

```sh
meson setup builddir -Dgui=true
meson compile -C builddir
```

## Flatpak

The Flathub manifest is `io.github.moooossee.Moose.yml`.

```sh
flatpak run org.flatpak.Builder --force-clean builddir io.github.moooossee.Moose.yml
```

## License

Moose is released under the GPL-3.0-or-later license.
