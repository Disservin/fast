import "./style.css";

const input = document.querySelector("#docs-search");
const items = [...document.querySelectorAll("[data-search-item]")].map(
  (element) => ({
    element,
    text: element.textContent.toLocaleLowerCase().replace(/\s+/g, " "),
  }),
);
const links = [...document.querySelectorAll("[data-topic-link]")];
const status = document.querySelector("#search-status");

/** Find a linked guide or subsection, including encoded URL fragments. */
function linkedTarget() {
  try {
    return document.getElementById(decodeURIComponent(location.hash.slice(1)));
  } catch {
    return null;
  }
}

/** Read the guide containing a linked section, defaulting to installation. */
function currentTopic() {
  return linkedTarget()?.closest("[data-search-item]")?.id ?? "installation";
}

/** Scroll after the linked guide is revealed so subsection links land correctly. */
function scrollToLinkedHeading() {
  requestAnimationFrame(() =>
    linkedTarget()?.scrollIntoView({ block: "start" }),
  );
}

/** Show the selected guide or all topics matching every entered search word. */
function renderDocs() {
  const words = input.value
    .trim()
    .toLocaleLowerCase()
    .split(/\s+/)
    .filter(Boolean);
  const topic = currentTopic();
  let count = 0;
  for (const { element, text } of items) {
    const visible = words.length
      ? words.every((word) => text.includes(word))
      : element.id === topic;
    element.hidden = !visible;
    if (visible) count++;
  }
  for (const link of links) {
    if (!words.length && link.dataset.topicLink === topic)
      link.setAttribute("aria-current", "page");
    else link.removeAttribute("aria-current");
  }
  document.querySelector("#no-results").hidden = count > 0;
  status.textContent = words.length
    ? `${count} ${count === 1 ? "topic" : "topics"} found`
    : "";
  status.hidden = !words.length;
  document.title = words.length
    ? "Search documentation — Fast"
    : `${document.getElementById(topic).querySelector(".heading-text").textContent} — Fast`;
}

/** Navigate to a guide and clear any previous search results. */
function navigateDocs() {
  input.value = "";
  renderDocs();
  scrollToLinkedHeading();
}

input.addEventListener("input", renderDocs);
input.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    event.preventDefault();
    navigateDocs();
    input.blur();
  }
});
links.forEach((link) =>
  link.addEventListener("click", () => {
    input.value = "";
    // Update even when the selected hash is unchanged after a search.
    location.hash = link.dataset.topicLink;
    renderDocs();
  }),
);
window.addEventListener("hashchange", navigateDocs);
document.addEventListener("keydown", (event) => {
  if (
    (event.ctrlKey || event.metaKey) &&
    !event.altKey &&
    event.key.toLowerCase() === "k"
  ) {
    event.preventDefault();
    input.focus();
    input.select();
  }
});
renderDocs();

scrollToLinkedHeading();
