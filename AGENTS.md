# AGENTS.md

Fast is a native desktop chess workstation in Rust and [GPUI](https://www.gpui.rs). It is
offline and single-user, and has tabs for Analyse, Database, Train, Play, Tournament, Test
positions and Settings. `README.md` covers running the app. `ARCHITECTURE.md` covers the
crates, threads, engine protocol and data flow. Update `ARCHITECTURE.md` when you change any of these.

## Checks

CI runs these, and every change must pass them:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p fast
```

`cargo run -p fast -- --demo` starts a separate generated profile with every screen filled in and
scripted engine output. Use it to look at UI changes without touching real data.
`--screen analyse|database|train|play|settings[:engines|board|databases|shortcuts]` picks the
first screen.

## Code layout

- `chess_core`, `engine`, `db` and `trainer` have no GPUI dependency and are unit-tested.
  Keep them that way.
- `ui` holds the design tokens (`theme.rs`), the shared widgets (`widgets.rs`) and the
  shared views: board, scoresheet, timeline, eval bar and graph.
- `crates/app` (the package is named `fast`) holds the window, the tabs, the screens and the configuration.
- Never block the UI thread. Engines run on their own threads, and database work runs in
  `cx.background_spawn`.

## Design spec

The design is flat, dense and calm, like a professional tool. Information comes first and
decoration is kept to a minimum.

### Colour

- Take every colour from the active palette through `ui::T` (`rgb(T.fg)` and so on). Never
  hard-code a hex value in a screen. If a new role is needed, add a token to `Theme` and give
  it a value in every palette.
- There are six palettes: Forest (the default), Obsidian, Slate and Plum are dark; Paper and
  Sand are light. Every change must look right in all of them, including the light ones.
  Board colours are a separate setting (`BoardTheme`).
- Token roles:

  | Token | Use |
  | --- | --- |
  | `bg` | App background |
  | `surface` | Panels, headers, side columns, normal buttons |
  | `surface_2` | Table header rows, inset areas |
  | `hover` | Hover background |
  | `selected` | Selected row or item, selected segmented button |
  | `border` | Panel outlines |
  | `border_strong` | Button and input outlines |
  | `divider` | Row separators, inner rules, empty progress tracks |
  | `fg` | Primary text |
  | `fg_2` | Secondary text, values in dense tables, side variations |
  | `muted` | Labels, metadata, section labels |
  | `faint` | Move numbers, tiny column captions, disabled text |
  | `accent`, `accent_hover`, `accent_fg` | What is active or chosen: current move, primary buttons, focus rings, progress fill, checked boxes |
  | `good`, `warn` | Positive status (Known, ready) and caution (Due, leaks, deviations) |
  | `q_best` … `q_blunder` | Move quality for arrows and labels |
  | `arrow` | User-drawn board arrows |
  | `engine_a`, `engine_b`, `chip_*`, `live_*`, `danger_*`, `tour_*` | Tournament tab |

- Text must have a contrast of at least 4.5:1 against its background.

### Type

- Use Geist (`ui::SANS`) for interface text. The body size is 14px, and controls and dense rows
  use 13px.
- Use Geist Mono (`ui::MONO`, the `mono()` widget) for everything a user compares or reads as
  data: moves, PVs, evaluations, depths, node counts, speeds, times, percentages and FENs.
- Section labels use `section_label()`: 11px, semibold, uppercase, tracked and `muted`. Use them for
  panel titles and table column headings.
- Tiny column captions above a value, such as DEPTH or NODES in the engine rows, are 10px
  and `faint`. The value under them is 12px mono in `fg_2`.
- Weights: regular for text, medium for buttons and names, semibold for labels and emphasised
  numbers.

### Shape and spacing

- `panel()`: `surface`, a 1px `border`, a 6px radius and no shadow. Never use gradients or
  shadows anywhere.
- Radii: 6px for panels and buttons, 4px for inputs, badges and key caps, and 3px for checkboxes.
- Panel content has 16px of horizontal padding. A panel header is a 44px row with a
  `section_label` on the left, actions on the right and a 1px bottom border.
- Use a 4px spacing scale: 4 or 6 between controls in a group, 8 to 10 between groups, 14 to 16 between sections.
- Separate rows in lists and tables with 1px `divider` lines, not with gaps or zebra stripes.

### Controls

- `btn()` comes in three sizes: 36px by default (forms and dialogs), `.compact()` at 28px (panel and
  toolbar controls) and `.mini()` at 24px (small toggles inside a panel header). Use the
  smallest size that fits the context.
- Use `.primary()` (an `accent` fill) for at most one main action per area. Most buttons are
  normal: `surface` with a `border_strong` outline.
- Tabs in a panel header use `header_tab()`: full header height, the chosen one in `fg` with a
  2px `accent` bottom line, the others `muted`.
- Build segmented controls (`segmented()`) as rows of buttons. The chosen one is `.selected()`:
  a `selected` background with an `accent` border. Toggle buttons such as Engine, Log and
  Online use `.selected(open)` the same way.
- Use `check()` for checkboxes and `badge()` / `stat()` for small tags. Shortcuts are shown
  with `key_cap()`.
- Number settings use compact one-line steppers: a label, then `−`, a fixed-width value and
  `+`, sized to their content rather than stretched across the panel.
- Everything must be reachable by keyboard and show a visible focus ring: a 2px `accent` border.
- Icons are Lucide SVGs in `assets/icons`, usually at 16px. Engines show `engine_icon()`.

### Tables and data

- A table has a `surface_2` header row of section labels and fixed-width numeric columns, so
  values never move as they update. Numbers are mono and right-aligned or column-aligned.
- A selected row gets the `selected` background. Hovering uses `hover`.
- Values must never wrap inside a fixed-width cell. Use `whitespace_nowrap()`, and move any
  extra state (such as cached or stale) next to the name instead of into the number.
- Progress is a thin 2px bar along the bottom of a row: `divider` track with `accent` fill.
- Show a missing value as an em dash `—`, never as blank space or `0`.
- Show each figure in one place. If a panel that shows it can be closed, show the figure
  somewhere else while that panel is closed. For example, the Engine panel shows nodes, speed
  and time, so Line preview shows only the depth.

### Board

- The board is always square and scales with its column. Coordinates are drawn inside the edge
  squares, and the board has a 1px outline.
- Highlight the last move, show legal-move dots, and highlight a king in check in red.
  Engine arrows are coloured by move quality (`q_*`), and user arrows use `arrow`.

### Layout

- Screens are columns of panels. Side panels can be resized, and their sizes are saved
  (`config.panels`). Give priority to the board and the main table, and keep secondary
  settings folded or compact.
- Make use of space. Remove empty bands, full-width controls holding short values, and
  labels on their own line when the label fits beside its control.
- Window minimum 1100×720. Every screen must work at that size.

### Writing

- Wrap all interface text in `ui::i18n::t()`. English is the key, and other languages
  (currently German) live in `i18n.rs`. `btn`, `check` and `section_label` already translate their labels.
- Use sentence case for labels and buttons ("Replace cached analysis", not "Replace Cached
  Analysis"). The only uppercase text is section labels and tiny column captions.
- Keep text short and plain. Name things the way a chess player would, and write button labels as verbs.
- Do not use middle dots (`·`) as separators, in interface text or anywhere else. Separate
  things with spacing, their own column or label, a comma, or a word. Ratios and
  series use ` / ` (as in `W / D / L`). Settings paths use `›` (as in `Settings › Engines`).

## Code style

- Write a doc comment for each function and type that says what it does for the user, in
  plain sentences. Add comments only where the reason for the code is not obvious from the code.
- Keep views small. A screen is a `Render` struct built from `ui` widgets, with state in
  `Entity<T>` models and changes announced through `cx.notify()` and events.
- Commit subjects are short imperative sentences about what the user sees ("Make Cancel
  download take effect at once"). The body explains what changed and why.
