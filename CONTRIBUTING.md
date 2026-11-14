# Contributing to Moose

Thanks for helping make Moose better. Bug reports, translations, small fixes,
clearer wording, and design feedback are all welcome. You do not need to write
code to contribute.

## Translate Moose

You can help translate Moose directly from your browser:

1. Open [Moose on Weblate](https://hosted.weblate.org/projects/moose/moose/) and sign in.
2. Choose your language, or select **Start new translation** to add one.
3. Select **Translate**, enter your translation, and save it.

Use clear, natural wording and follow the terminology used by GNOME applications
in your language. Keep Moose and other product names unchanged. Preserve
placeholders such as `{name}` and `{count}`, formatting markup, and URLs.

Check context and plural forms before submitting. If a string is unclear, leave
a comment on Weblate asking for clarification.

We credit people who help translate or review Moose in the
[README](README.md#translation-credits) and on the
[website](https://moooossee.github.io/docs/contributing/#translation-credits).
Leave a comment on Weblate if you would like us to use a particular name or profile.

Weblate sends translation updates to this repository automatically. They reach
users in a future app release.

## Report a bug or suggest an idea

Check the [existing issues](https://github.com/moooossee/moose/issues) first.
If there is no matching issue, open one.

For a bug, include:

- Your Moose version, Linux distribution, and how you installed the app.
- The steps to reproduce it, what you expected, and what happened instead.
- A screenshot or relevant error message, if it helps.
- The model and Ollama version, if the problem involves a response or connection.

Remove private conversations, documents, credentials, and server details before
sharing screenshots or logs.

For an idea, explain what you are trying to do and how the change would help.
Please discuss large features or interface changes in an issue before starting.
Small fixes can go straight to a pull request.

## Work on a change

Fork the repository, clone your fork, and create a branch from `main`.
Follow the [build instructions](README.md#build-and-run) to run Moose locally.
The Flatpak build provides the desktop dependencies used by the app.

Most code lives in these folders:

- `src/ui/`: the GTK and libadwaita interface.
- `src/ollama/`: communication with Ollama.
- `src/storage/` and `migrations/`: saved data and database changes.
- `data/`: styles, icons, and app metadata.

Keep each change focused on one problem. Follow the surrounding code, use English
for original text, and keep the interface simple and consistent with the rest of Moose.
For database changes, add a migration instead of editing an existing one.
If you change dependencies, keep `Cargo.lock` and `cargo-sources.json` in sync.

## Check your change

For Rust changes, run the same core checks as CI:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```

These commands do not enable the GUI. For interface changes, also build the
Flatpak and try the affected flow. Check light and dark mode and a narrow window.
Add or update tests when they help protect the behavior you changed.
Documentation-only changes do not need a local app build.

## Open a pull request

Send your pull request to `main`. Explain the problem, what changed, and how you
checked it. Include screenshots for visual changes and link any related issue.
If you could not check something, say so.

Keep unrelated cleanup out of the pull request so the change is easier to review.
CI checks formatting, metadata, Rust code, and the Flatpak build. Review its results
and address any failures before asking for a final review.

Be kind, give useful feedback, and ask when something is unclear.

## Flathub generative AI policy

For Flathub submissions, follow the official
[generative AI policy](https://docs.flathub.org/docs/for-app-authors/requirements/#generative-ai-policy)
