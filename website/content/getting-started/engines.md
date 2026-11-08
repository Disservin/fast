# Engines & profiles

## Add an engine

Open **Settings › Engines**, then:

- **Download engine** installs an engine from the catalog. Downloading needs internet access;
  installed engines run offline.
- **Add from file** adds a local UCI executable.

## Run several engines

Several engines can run side by side. To run one binary with two setups (for example Lc0 with
BT2 and Lc0 with Maia), click **Duplicate** and give each entry its own options.

## Options and profiles

- **Options** are read from the engine and laid out in a compact grid. Every UCI option type is
  supported.
- **Profiles** save a whole set of options so you can switch between them in one click.
- The engine's `UCI_EngineAbout` text is shown as its description.

## Engine picture

Click the icon to choose a picture. If you don't, Fast looks next to the binary for an image
named after it, `logo.*` or `icon.*`, and otherwise shows a rook.

## Debug output

**Debug** sends `debug on` and opens a window that follows the engine's raw output. You can also
open that window from:

- **Engine › Engine output window**
- **Window** in the Log panel
