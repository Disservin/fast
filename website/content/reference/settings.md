# Appearance & language

## Interface palette

Choose a palette under **Settings › General › Appearance**:

- **Dark:** Forest (default), Obsidian, Slate, Plum
- **Light:** Paper, Sand

The palette applies immediately and is remembered across restarts. Board colours are a separate
choice under **Settings › Board & pieces**.

## Language

Choose **Settings › General › Language › Deutsch** for German.

- The choice applies immediately to translated labels and menus, and is saved.
- English is the fallback for text that has no translation yet.
- Configuration files store the language as `en` or `de`.

## Add translations

Translations live in `crates/ui/src/i18n.rs`.

- Wrap interface labels in `ui::i18n::t("English source")`.
- Leave PGN tags, UCI option keys, file formats and user data unchanged.
- To add a language, extend `Language` with its config code and native name, then add it to the
  translation catalog.
