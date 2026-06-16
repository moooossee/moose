# Moose

Moose is a small desktop app for chatting with local AI models through Ollama.
It feels quiet, simple and local: your conversations, providers, profiles and
model picks stay on your computer.

On Flatpak, Moose can set up its own Ollama in app data. No host Ollama dance.

## Features

- Install and run a Moose-managed Ollama
- Download models from the app
- Chat with local Ollama models
- Keep conversation context and configure each chat
- Use built-in profiles or create reusable custom profiles
- Keep conversations saved locally
- Organize older conversations from the history view
- Export conversations when you want a copy outside Moose
- Connect external Ollama providers when you want

## Build

Moose uses Rust, Meson, GTK 4 and libadwaita.

```sh
meson setup builddir -Dgui=true
meson compile -C builddir
```

## Flatpak

The Flathub manifest is `io.github.moooossee.Moose.yml`.
The Flatpak keeps managed Ollama files and downloaded models inside app data.
Remote and manual providers still work from Preferences.

```sh
flatpak run org.flatpak.Builder --force-clean builddir io.github.moooossee.Moose.yml
```

## License

Moose is released under the GPL-3.0-or-later license.
