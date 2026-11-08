import { readFileSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";
import { Marked } from "marked";

/** Escape authored titles before inserting them into page markup. */
function escapeHtml(value) {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll('"', "&quot;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}

/** Make stable, readable section fragments from Markdown heading text. */
function headingSlug(text) {
  return (
    text
      .normalize("NFKD")
      .toLowerCase()
      .replace(/[^\p{L}\p{N}\s-]/gu, "")
      .trim()
      .replace(/\s+/g, "-") || "section"
  );
}

/** Add an accessible permalink beside a guide or section title. */
function permalink(id, title) {
  return `<a class="heading-anchor" href="#${id}" aria-label="Link to ${escapeHtml(title)}">#</a>`;
}

/** Compile the categorized Markdown guides into searchable static articles and links. */
export function compileDocs(contentDirectory) {
  const manifest = resolve(contentDirectory, "navigation.json");
  const categories = JSON.parse(readFileSync(manifest, "utf8"));
  const files = [manifest];
  const seen = new Set();
  const topics = categories.flatMap((category) =>
    category.pages.map((slug) => {
      if (!/^[a-z0-9-]+$/.test(slug) || seen.has(slug))
        throw new Error(`Invalid or duplicate docs slug: ${slug}`);
      seen.add(slug);
      const file = resolve(contentDirectory, category.directory, `${slug}.md`);
      if (relative(contentDirectory, file).startsWith(".."))
        throw new Error(`Docs path is outside content: ${file}`);
      files.push(file);
      const source = readFileSync(file, "utf8");
      const heading = /^# (.+)\r?\n/.exec(source);
      if (!heading) throw new Error(`Start ${file} with a level-one title.`);
      return {
        slug,
        title: heading[1],
        category: category.title,
        file,
        body: source.slice(heading[0].length),
      };
    }),
  );
  const targets = new Map(topics.map((topic) => [topic.file, topic.slug]));
  const articles = topics
    .map((topic) => {
      const headings = new Map();
      const markdown = new Marked({
        renderer: {
          heading({ tokens, depth, text }) {
            const base = headingSlug(text);
            const count = (headings.get(base) ?? 0) + 1;
            headings.set(base, count);
            const id = `${topic.slug}--${base}${count > 1 ? `-${count}` : ""}`;
            return `<h${depth} id="${id}">${this.parser.parseInline(tokens)} ${permalink(id, text)}</h${depth}>`;
          },
        },
        gfm: true,
        walkTokens(token) {
          if (token.type !== "link" || /^(?:[a-z]+:|\/)/i.test(token.href))
            return;
          if (token.href.startsWith("#")) {
            const fragment = token.href.slice(1);
            if (!seen.has(fragment) && !fragment.includes("--"))
              token.href = `#${topic.slug}--${fragment}`;
            return;
          }
          const [path, fragment] = token.href.split("#");
          const slug = targets.get(resolve(dirname(topic.file), path));
          if (!slug)
            throw new Error(
              `Unknown local docs link in ${topic.file}: ${token.href}`,
            );
          token.href = `#${slug}${fragment ? `--${fragment}` : ""}`;
        },
      });
      const body = markdown
        .parse(topic.body)
        .replaceAll("<table>", '<div class="docs-table"><table>')
        .replaceAll("</table>", "</table></div>");
      return `<article id="${topic.slug}" data-search-item tabindex="-1"><p class="eyebrow">${escapeHtml(topic.category)}</p><h1><span class="heading-text">${escapeHtml(topic.title)}</span> ${permalink(topic.slug, topic.title)}</h1>${body}</article>`;
    })
    .join("\n");
  const navigation = categories
    .map(
      (category) =>
        `<div class="sidebar-group"><h2>${escapeHtml(category.title)}</h2>${topics
          .filter((topic) => topic.category === category.title)
          .map(
            (topic) =>
              `<a href="#${topic.slug}" data-topic-link="${topic.slug}">${escapeHtml(topic.title)}</a>`,
          )
          .join("")}</div>`,
    )
    .join("\n");
  return {
    articles,
    navigation: `<nav aria-label="Documentation topics">${navigation}</nav>`,
    files,
    topics,
  };
}

/** Render Markdown during builds and refresh the preview when an author edits a guide. */
export function docsPlugin(contentDirectory) {
  return {
    name: "fast-markdown-docs",
    buildStart() {
      for (const file of compileDocs(contentDirectory).files)
        this.addWatchFile(file);
    },
    transformIndexHtml(html, context) {
      if (!context.filename.endsWith("docs.html")) return html;
      const docs = compileDocs(contentDirectory);
      return html
        .replace("<!-- docs-navigation -->", docs.navigation)
        .replace("<!-- docs-content -->", docs.articles);
    },
    configureServer(server) {
      server.watcher.add(contentDirectory);
    },
    handleHotUpdate({ file, server }) {
      if (!relative(contentDirectory, file).startsWith("..")) {
        server.ws.send({ type: "full-reload", path: "*" });
        return [];
      }
    },
  };
}
