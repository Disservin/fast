import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { compileDocs } from "./docs.js";

/** Verify every real guide is rendered and remains reachable in the sidebar and search. */
test("compile every authored guide into the static page", () => {
  const contentDirectory = fileURLToPath(new URL("../content", import.meta.url));
  const docs = compileDocs(contentDirectory);
  const authoredGuides = readdirSync(contentDirectory, { recursive: true })
    .filter((file) => file.endsWith(".md"))
    .map((file) => join(contentDirectory, file))
    .sort();
  assert.ok(authoredGuides.length > 0);
  assert.deepEqual(
    docs.topics.map((topic) => topic.file).sort(),
    authoredGuides,
  );
  for (const topic of docs.topics) {
    assert.ok(docs.articles.includes(`id="${topic.slug}" data-search-item`));
    assert.ok(docs.navigation.includes(`href="#${topic.slug}"`));
  }
  assert.ok(docs.articles.includes('<div class="docs-table"><table>'));
  assert.ok(docs.articles.includes("Ctrl+Shift+L"));
  assert.ok(docs.articles.includes("UHO_Lichess_4852_v1.epd"));
});

/** Verify Markdown links follow topic routes and broken authoring fails the build. */
test("resolve local Markdown links and reject invalid guides", (t) => {
  const directory = mkdtempSync(join(tmpdir(), "fast-docs-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  mkdirSync(join(directory, "guide"));
  writeFileSync(
    join(directory, "navigation.json"),
    JSON.stringify([
      { title: "Guide", directory: "guide", pages: ["first", "second"] },
    ]),
  );
  writeFileSync(
    join(directory, "guide/first.md"),
    "# First\n\n[Next](second.md)\n\n[Section](second.md#setup)\n",
  );
  writeFileSync(
    join(directory, "guide/second.md"),
    "# Second\n\n## Setup\n\nSome instructions.\n\n## Setup\n\nMore instructions.\n\n[Local](#setup)\n",
  );
  const articles = compileDocs(directory).articles;
  assert.ok(articles.includes('href="#second"'));
  assert.ok(articles.includes('id="second--setup"'));
  assert.ok(articles.includes('id="second--setup-2"'));
  assert.ok(articles.includes('href="#second--setup"'));
  assert.ok(articles.includes('aria-label="Link to Setup"'));
  writeFileSync(
    join(directory, "guide/first.md"),
    "# First\n\n[Missing](missing.md)\n",
  );
  assert.throws(() => compileDocs(directory), /Unknown local docs link/);
  writeFileSync(join(directory, "guide/first.md"), "Missing title\n");
  assert.throws(() => compileDocs(directory), /level-one title/);
});
