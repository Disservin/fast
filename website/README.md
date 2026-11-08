# Fast documentation site

A static Vite site for GitHub Pages. Requires Node.js 22.12+ (CI uses Node.js 24).

```sh
cd website
npm ci
npm run dev
```

`npm run build` produces `website/dist`. `npm run preview` serves the production build.
The site imports the existing images from `docs/screenshots`; replace those files to refresh
screenshots. Use the Forest interface palette (`ui_theme = "forest"`) and green Moss
board (`[board]` with `theme = "moss"`) in an isolated demo profile, at 1600×1000.
Gallery content and behavior live in `src/main.js`, the home page in
`index.html`, the docs layout in `docs.html`, and shared styles in `src/style.css`.

## Writing documentation

Guides live in `content/`, grouped into `getting-started/`, `workspace/`, `reference/`
and `project/`. Each guide is a Markdown file beginning with one `# Title`. Use `##`
for subsections, fenced code blocks for commands, and Markdown tables for reference data.

`content/navigation.json` defines category labels, folders and page order. To add a guide:

1. Create `content/<category>/<slug>.md` with its title and instructions.
2. Add the filename without `.md` to that category's `pages` list in `navigation.json`.
3. Run `npm test` and `npm run build`.

The filename is the topic's URL hash; keep it stable to preserve existing links.
Link to another guide using its relative Markdown path, such as
`[Shortcuts](../reference/shortcuts.md)`. Repository references use full GitHub URLs.

Headings get visible `#` permalinks. Guide links keep their existing hashes; section links
use `#<topic>--<heading>`, for example `#installation--linux-build-dependencies`.
Repeated headings get numbered suffixes. In Markdown, use `[Setup](other.md#setup)`
for another guide's section or `[Setup](#setup)` for a section in the current guide.

`build/docs.js` renders Markdown and generates the categorized sidebar at build time
using Marked. It validates titles, unique topic names and local guide links. Vite's dev
server refreshes when content changes. Generated HTML is never edited or checked in.
The browser receives static articles, so all docs remain available without JavaScript.

`src/docs.js` handles topic navigation and local full-text search. Ctrl/Cmd+K focuses
the top-right search field; Escape clears search. Both HTML entry points are built by
Vite; no server routing is required. `npm test` checks the Markdown build and topic links.

## GitHub Pages

In the repository's **Settings › Pages**, choose **GitHub Actions** as the source.
The `Documentation site` workflow builds pull requests and deploys pushes to `master`.
It can also be run manually. The project URL is https://disservin.github.io/fast/.
Relative build paths also support a custom domain without changing the Vite configuration.
